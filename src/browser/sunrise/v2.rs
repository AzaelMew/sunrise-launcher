//! Signed v2 mod registry catalogue reader (`v2/index.json` + `v2/index.json.minisig`).
//!
//! Verifies the minisign signature over the exact fetched (or cached) bytes against the built-in
//! registry key list before parsing, persists the last verified copy in its own cache namespace
//! (`mod-browser/registry-v2/`), refuses any lower `serial` across both network and cached copies,
//! warns without failing on a stale `generated` timestamp, and rejects malformed payloads whole
//! with no salvage.
use crate::i18n::{tr, trf};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sunrise_launcher_smod::index_v2::{self, Index, VerifyError};

/// Where the signed v2 registry publishes its index (GitHub Pages of sunrise-org/sunrise-mods).
pub const INDEX_URL: &str = sunrise_launcher_addresses::MOD_INDEX_V2;

/// Environment variable overriding [`INDEX_URL`] (HTTPS URL, local path, or debug loopback URL).
pub const ENV_OVERRIDE: &str = "SUNRISE_MOD_INDEX_V2";

/// How long an in-memory verified v2 index is reused before fetching again.
const FRESH: Duration = Duration::from_secs(600);

/// Dedicated v2 cache directory under the game home (isolated from v1's `sunrise-index.json`).
const CACHE_DIR: &str = "mod-browser/registry-v2";
const CACHE_INDEX: &str = "index.json";
const CACHE_SIG: &str = "index.json.minisig";
const STATE_FILE: &str = "state.json";

/// Built-in minisign public keys trusted to sign the v2 mod registry catalogue.
///
/// No production key is configured yet: non-test builds keep this empty so the reader reports
/// `"registry key not configured"` and trusts nothing.
#[cfg(not(test))]
pub const REGISTRY_KEYS: &[&str] = &[];

/// Disposable test-only minisign public key; never compiled into non-test builds.
#[cfg(test)]
pub const REGISTRY_KEYS: &[&str] = &["RWSwC2FU6jlWFepUk56a61uccuXxBaMuB35XctszNB8XOUNzfUDxPror"];

/// A verified v2 registry catalogue loaded from the network or from the verified disk cache.
#[derive(Clone, Debug)]
pub struct Catalog {
    pub index: Arc<Index>,
    /// True when loaded from the verified last-good disk cache because the live fetch failed.
    pub from_cache: bool,
    /// Non-blocking warning when `index.generated` is stale; never prevents using the catalogue.
    pub stale_warning: Option<String>,
}

/// Why loading the signed v2 registry catalogue failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadError {
    /// No registry public key is configured in this build.
    KeyNotConfigured,
    /// Fetching `index.json` or `index.json.minisig` failed and no valid cached copy could be used.
    Fetch(String),
    /// The minisign signature over `index.json` failed verification.
    BadSignature(String),
    /// The signed `index.json` payload was malformed or violated schema-2 rules (no salvage).
    Malformed(String),
    /// The verified index carried a lower `serial` than the highest previously accepted serial.
    Rollback { got: u64, highest: u64 },
    /// The persisted rollback state file exists on disk but could not be read or parsed.
    CorruptState(String),
}

impl From<VerifyError> for LoadError {
    fn from(error: VerifyError) -> Self {
        match error {
            VerifyError::KeyNotConfigured => Self::KeyNotConfigured,
            VerifyError::BadSignature(reason) => Self::BadSignature(reason),
            VerifyError::Malformed(reason) => Self::Malformed(reason),
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KeyNotConfigured => f.write_str(tr("Sunrise mod registry key not configured.")),
            Self::Fetch(reason) => f.write_str(reason),
            Self::BadSignature(_) => {
                f.write_str(tr("The mod registry index signature could not be verified."))
            }
            Self::Malformed(reason) => f.write_str(&trf(
                "The mod registry index is malformed: {reason}",
                &[("reason", reason)],
            )),
            Self::Rollback { got, highest } => f.write_str(&trf(
                "The mod registry returned an older index (serial {got}, expected at least {highest}).",
                &[("got", &got.to_string()), ("highest", &highest.to_string())],
            )),
            Self::CorruptState(reason) => f.write_str(&trf(
                "The mod registry state file could not be read: {reason}",
                &[("reason", reason)],
            )),
        }
    }
}

impl std::error::Error for LoadError {}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedState {
    highest_serial: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    active: Option<ActiveCache>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ActiveCache {
    index: String,
    signature: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PublishStep {
    Index,
    Signature,
    State,
}

struct LoadedState {
    at: Instant,
    source: String,
    catalog: Catalog,
    highest_serial: u64,
}

struct RefreshContext<'a> {
    cache_dir: &'a Path,
    keys: &'a [&'a str],
    source: &'a str,
    force_refresh: bool,
    now_unix: i64,
}

static LOADED: Mutex<Option<LoadedState>> = Mutex::new(None);
/// Serializes on-disk serial-floor checks and cache publication across stores.
/// Lock order is always `LOADED` (when held) before `COMMIT_LOCK`.
static COMMIT_LOCK: Mutex<()> = Mutex::new(());

/// Where the v2 index comes from: `SUNRISE_MOD_INDEX_V2` when set and non-empty, else
/// [`INDEX_URL`].
pub fn location() -> String {
    std::env::var(ENV_OVERRIDE)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| INDEX_URL.to_owned())
}

/// Derives the `.minisig` address or file path beside `index_location`.
pub fn signature_location(index_location: &str) -> String {
    if let Some(mut parsed) = url::Url::parse(index_location)
        .ok()
        .filter(|parsed| matches!(parsed.scheme(), "https" | "http"))
    {
        let next_path = format!("{}.minisig", parsed.path());
        parsed.set_path(&next_path);
        return parsed.to_string();
    }
    format!("{index_location}.minisig")
}

/// The v2 catalogue cache folder under the game home.
pub fn cache_dir() -> PathBuf {
    crate::backend::game_home().join(CACHE_DIR)
}

/// Returns the last verified v2 catalogue held in memory without network or disk I/O.
pub fn known_index() -> Option<Catalog> {
    LOADED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map(|state| state.catalog.clone())
}

/// Returns the verified v2 catalogue, reusing an in-memory copy from the last ten minutes for the
/// same endpoint, otherwise fetching and verifying against [`REGISTRY_KEYS`].
pub fn index() -> Result<Catalog, LoadError> {
    load_process_index(false)
}

/// Forces a fresh fetch and verification of the v2 catalogue against [`REGISTRY_KEYS`].
pub fn refreshed_index() -> Result<Catalog, LoadError> {
    load_process_index(true)
}

fn load_process_index(force_refresh: bool) -> Result<Catalog, LoadError> {
    let source = location();
    let cache = cache_dir();
    let ctx = RefreshContext {
        cache_dir: &cache,
        keys: REGISTRY_KEYS,
        source: &source,
        force_refresh,
        now_unix: now_unix_seconds(),
    };
    refresh_in(&LOADED, &ctx, fetch_raw)
}

fn refresh_in<F>(
    loaded: &Mutex<Option<LoadedState>>,
    ctx: &RefreshContext<'_>,
    fetch: F,
) -> Result<Catalog, LoadError>
where
    F: FnOnce(&str) -> Result<(Vec<u8>, Vec<u8>), String>,
{
    if !has_configured_key(ctx.keys) {
        return Err(LoadError::KeyNotConfigured);
    }
    {
        let guard = loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(state) = guard.as_ref() {
            if !ctx.force_refresh && state.source == ctx.source && state.at.elapsed() < FRESH {
                return Ok(state.catalog.clone());
            }
        }
    }

    // Keep network and file I/O outside the lock so concurrent refreshes overlap freely.
    let fetched = fetch(ctx.source);

    // Serialize the serial-floor check, disk commit, and in-memory update together so a slower
    // response can never overwrite a newer catalogue on disk.
    let mut guard = loaded.lock().unwrap_or_else(|e| e.into_inner());
    let prev_highest = guard.as_ref().map(|state| state.highest_serial);
    let mut store = Store::with_highest_serial(ctx.cache_dir.to_path_buf(), ctx.keys, prev_highest);
    let catalog = store.load_from_fetch(fetched, ctx.now_unix)?;
    let highest_serial = guard.as_ref().map_or(catalog.index.serial, |current| {
        current.highest_serial.max(catalog.index.serial)
    });
    *guard = Some(LoadedState {
        at: Instant::now(),
        source: ctx.source.to_owned(),
        catalog: catalog.clone(),
        highest_serial,
    });
    Ok(catalog)
}

/// Self-contained v2 catalogue verifier, cache, and rollback-state store.
///
/// Owns a dedicated cache directory and trusted key list, making verification and rollback checks
/// independent of process-global state.
pub struct Store {
    cache_dir: PathBuf,
    keys: Vec<String>,
    highest_serial: Option<u64>,
}

impl Store {
    /// Creates a store rooted at `cache_dir` trusting `keys`.
    pub fn new(cache_dir: PathBuf, keys: &[&str]) -> Self {
        Self::with_highest_serial(cache_dir, keys, None)
    }

    /// Creates a store seeded with an in-memory `highest_serial` floor.
    pub fn with_highest_serial(
        cache_dir: PathBuf,
        keys: &[&str],
        highest_serial: Option<u64>,
    ) -> Self {
        Self {
            cache_dir,
            keys: keys.iter().map(|key| (*key).to_owned()).collect(),
            highest_serial,
        }
    }

    /// Returns the highest accepted serial tracked in memory or on disk.
    pub fn highest_serial(&self) -> Result<Option<u64>, LoadError> {
        let disk_serial = self.read_state()?.map(|state| state.highest_serial);
        Ok(combine_serial(self.highest_serial, disk_serial))
    }

    /// Fetches `index.json` and `index.json.minisig` from `source`, verifies the signature and
    /// serial, caches the verified copy, and falls back to the verified cache only on fetch error.
    pub fn fetch_and_load(&mut self, source: &str, now_unix: i64) -> Result<Catalog, LoadError> {
        self.require_keys()?;
        let fetched = fetch_raw(source);
        self.load_from_fetch(fetched, now_unix)
    }

    /// Applies a completed fetch result against this store's keys, rollback state, and cache.
    ///
    /// Verification, invalid signature encoding, malformed-payload, and rollback errors on a
    /// fetched response are returned directly and never masked by a cached copy. Only a fetch
    /// failure falls back to the verified last-good cache, which is still signature- and
    /// serial-checked.
    pub fn load_from_fetch(
        &mut self,
        fetched: Result<(Vec<u8>, Vec<u8>), String>,
        now_unix: i64,
    ) -> Result<Catalog, LoadError> {
        self.require_keys()?;
        match fetched {
            Ok((bytes, sig_bytes)) => self.accept_raw_bytes(&bytes, &sig_bytes, now_unix),
            Err(fetch_error) => match self.load_cached(now_unix) {
                Ok(catalog) => Ok(catalog),
                Err(
                    error @ (LoadError::KeyNotConfigured
                    | LoadError::Rollback { .. }
                    | LoadError::CorruptState(_)),
                ) => Err(error),
                Err(_) => Err(LoadError::Fetch(fetch_error)),
            },
        }
    }

    /// Verifies exact `bytes` and raw `signature_bytes` (returning [`LoadError::BadSignature`] if
    /// the signature is not valid UTF-8), enforces monotonic `serial`, writes the verified copy
    /// and updated serial atomically to the v2 cache namespace, and returns the accepted catalogue.
    pub fn accept_raw_bytes(
        &mut self,
        bytes: &[u8],
        signature_bytes: &[u8],
        now_unix: i64,
    ) -> Result<Catalog, LoadError> {
        self.require_keys()?;
        let signature = std::str::from_utf8(signature_bytes)
            .map_err(|e| LoadError::BadSignature(format!("invalid UTF-8 signature: {e}")))?;
        self.accept_bytes(bytes, signature, now_unix)
    }

    /// Verifies exact `bytes` and `signature`, enforces monotonic `serial`, publishes the verified
    /// index, signature, and serial together in the v2 cache namespace, and returns the accepted
    /// catalogue.
    pub fn accept_bytes(
        &mut self,
        bytes: &[u8],
        signature: &str,
        now_unix: i64,
    ) -> Result<Catalog, LoadError> {
        self.accept_bytes_inner(bytes, signature, now_unix, |_, path, data| {
            write_atomic(path, data)
        })
    }

    #[cfg(test)]
    fn accept_bytes_with_writer<W>(
        &mut self,
        bytes: &[u8],
        signature: &str,
        now_unix: i64,
        write_file: W,
    ) -> Result<Catalog, LoadError>
    where
        W: FnMut(PublishStep, &Path, &[u8]) -> Result<(), String>,
    {
        self.accept_bytes_inner(bytes, signature, now_unix, write_file)
    }

    fn accept_bytes_inner<W>(
        &mut self,
        bytes: &[u8],
        signature: &str,
        now_unix: i64,
        write_file: W,
    ) -> Result<Catalog, LoadError>
    where
        W: FnMut(PublishStep, &Path, &[u8]) -> Result<(), String>,
    {
        self.require_keys()?;
        let key_refs: Vec<&str> = self.keys.iter().map(String::as_str).collect();
        let index = index_v2::verify_and_parse(bytes, signature, &key_refs)?;

        let _commit = COMMIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let state = self.read_state()?;
        let highest = combine_serial(
            self.highest_serial,
            state.as_ref().map(|s| s.highest_serial),
        );
        if let Some(prev) = highest.filter(|&prev| index.serial < prev) {
            return Err(LoadError::Rollback {
                got: index.serial,
                highest: prev,
            });
        }
        let next_serial = highest.map_or(index.serial, |prev| prev.max(index.serial));
        let previous_active = state.and_then(|s| s.active);
        self.persist_verified_with(
            bytes,
            signature,
            next_serial,
            previous_active.as_ref(),
            write_file,
        )?;
        self.highest_serial = Some(next_serial);
        let stale_warning = stale_warning_at(&index, now_unix);
        Ok(Catalog {
            index: Arc::new(index),
            from_cache: false,
            stale_warning,
        })
    }

    /// Loads and re-verifies the cached v2 copy from this store's namespace, enforcing both
    /// signature verification and the highest accepted `serial`.
    pub fn load_cached(&mut self, now_unix: i64) -> Result<Catalog, LoadError> {
        self.require_keys()?;
        let _commit = COMMIT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let state = self.read_state()?;
        let (index_path, sig_path) = match state.as_ref().and_then(|s| s.active.as_ref()) {
            Some(active) => {
                if !plain_cache_name(&active.index) || !plain_cache_name(&active.signature) {
                    return Err(LoadError::CorruptState(
                        "invalid cached snapshot file name".into(),
                    ));
                }
                (
                    self.cache_dir.join(&active.index),
                    self.cache_dir.join(&active.signature),
                )
            }
            None => (
                self.cache_dir.join(CACHE_INDEX),
                self.cache_dir.join(CACHE_SIG),
            ),
        };
        let bytes = read_bounded_file(
            &index_path,
            index_v2::MAX_INDEX,
            tr("The mod registry index is too large.").into(),
        )
        .map_err(LoadError::Fetch)?;
        let sig_bytes = read_bounded_file(
            &sig_path,
            index_v2::MAX_SIGNATURE,
            tr("The mod registry signature is too large.").into(),
        )
        .map_err(LoadError::Fetch)?;
        let signature = String::from_utf8(sig_bytes)
            .map_err(|e| LoadError::BadSignature(format!("invalid UTF-8 signature: {e}")))?;

        let key_refs: Vec<&str> = self.keys.iter().map(String::as_str).collect();
        let index = index_v2::verify_and_parse(&bytes, &signature, &key_refs)?;
        let highest = combine_serial(
            self.highest_serial,
            state.as_ref().map(|s| s.highest_serial),
        );
        if let Some(prev) = highest.filter(|&prev| index.serial < prev) {
            return Err(LoadError::Rollback {
                got: index.serial,
                highest: prev,
            });
        }
        self.highest_serial = Some(highest.map_or(index.serial, |prev| prev.max(index.serial)));
        let stale_warning = stale_warning_at(&index, now_unix);
        Ok(Catalog {
            index: Arc::new(index),
            from_cache: true,
            stale_warning,
        })
    }

    #[cfg(test)]
    fn active_cache_paths(&self) -> Result<Option<(PathBuf, PathBuf)>, LoadError> {
        let Some(state) = self.read_state()? else {
            return Ok(None);
        };
        let Some(active) = state.active else {
            return Ok(None);
        };
        Ok(Some((
            self.cache_dir.join(active.index),
            self.cache_dir.join(active.signature),
        )))
    }

    fn require_keys(&self) -> Result<(), LoadError> {
        if !self.keys.iter().any(|key| !key.trim().is_empty()) {
            return Err(LoadError::KeyNotConfigured);
        }
        Ok(())
    }

    fn read_state(&self) -> Result<Option<PersistedState>, LoadError> {
        let state_path = self.cache_dir.join(STATE_FILE);
        match fs::read_to_string(&state_path) {
            Ok(text) => {
                let parsed: PersistedState = serde_json::from_str(&text)
                    .map_err(|e| LoadError::CorruptState(e.to_string()))?;
                Ok(Some(parsed))
            }
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(LoadError::CorruptState(error.to_string())),
        }
    }

    fn persist_verified_with<W>(
        &self,
        bytes: &[u8],
        signature: &str,
        serial: u64,
        previous_active: Option<&ActiveCache>,
        mut write_file: W,
    ) -> Result<(), LoadError>
    where
        W: FnMut(PublishStep, &Path, &[u8]) -> Result<(), String>,
    {
        fs::create_dir_all(&self.cache_dir)
            .map_err(|e| LoadError::Fetch(format!("{}: {e}", self.cache_dir.display())))?;
        let prefix = next_snapshot_prefix(serial).map_err(LoadError::Fetch)?;
        let index_name = format!("{prefix}.json");
        let sig_name = format!("{prefix}.json.minisig");
        let index_path = self.cache_dir.join(&index_name);
        let sig_path = self.cache_dir.join(&sig_name);
        let state_path = self.cache_dir.join(STATE_FILE);

        if let Err(err) = write_file(PublishStep::Index, &index_path, bytes) {
            let _ = fs::remove_file(&index_path);
            return Err(LoadError::Fetch(err));
        }
        if let Err(err) = write_file(PublishStep::Signature, &sig_path, signature.as_bytes()) {
            let _ = fs::remove_file(&index_path);
            let _ = fs::remove_file(&sig_path);
            return Err(LoadError::Fetch(err));
        }
        let state_json = serde_json::to_vec(&PersistedState {
            highest_serial: serial,
            active: Some(ActiveCache {
                index: index_name.clone(),
                signature: sig_name.clone(),
            }),
        })
        .map_err(|e| LoadError::CorruptState(e.to_string()))?;
        if let Err(err) = write_file(PublishStep::State, &state_path, &state_json) {
            let _ = fs::remove_file(&index_path);
            let _ = fs::remove_file(&sig_path);
            return Err(LoadError::Fetch(err));
        }

        // Remove the superseded snapshot only after state.json atomically published the new one.
        if let Some(old) = previous_active {
            if plain_cache_name(&old.index) && old.index != index_name {
                let _ = fs::remove_file(self.cache_dir.join(&old.index));
            }
            if plain_cache_name(&old.signature) && old.signature != sig_name {
                let _ = fs::remove_file(self.cache_dir.join(&old.signature));
            }
        }
        Ok(())
    }
}

fn combine_serial(memory: Option<u64>, disk: Option<u64>) -> Option<u64> {
    match (memory, disk) {
        (Some(mem), Some(disk)) => Some(mem.max(disk)),
        (Some(mem), None) => Some(mem),
        (None, Some(disk)) => Some(disk),
        (None, None) => None,
    }
}

fn plain_cache_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':', '\0'])
}

fn next_snapshot_prefix(serial: u64) -> Result<String, String> {
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|e| format!("random snapshot id: {e}"))?;
    let hex: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("snapshot-{serial}-{hex}"))
}

fn has_configured_key(keys: &[&str]) -> bool {
    keys.iter().any(|key| !key.trim().is_empty())
}

fn stale_warning_at(index: &Index, now_unix: i64) -> Option<String> {
    index.is_stale_at(now_unix).then(|| {
        trf(
            "The mod registry index was last updated on {date} and may be out of date.",
            &[("date", &index.generated)],
        )
    })
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

fn fetch_raw(source: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let sig_source = signature_location(source);
    let loopback = is_debug_loopback(source);
    if source.starts_with("http://") && !loopback {
        return Err(tr("The mod registry requires HTTPS.").into());
    }
    if source.starts_with("https://") || loopback {
        let index_bytes = fetch_http_bytes(
            source,
            loopback,
            source == INDEX_URL,
            index_v2::MAX_INDEX,
            tr("The mod registry index is too large.").into(),
        )?;
        let sig_bytes = fetch_http_bytes(
            &sig_source,
            loopback,
            source == INDEX_URL,
            index_v2::MAX_SIGNATURE,
            tr("The mod registry signature is too large.").into(),
        )?;
        return Ok((index_bytes, sig_bytes));
    }

    let index_bytes = read_bounded_file(
        Path::new(source),
        index_v2::MAX_INDEX,
        tr("The mod registry index is too large.").into(),
    )?;
    let sig_bytes = read_bounded_file(
        Path::new(&sig_source),
        index_v2::MAX_SIGNATURE,
        tr("The mod registry signature is too large.").into(),
    )?;
    Ok((index_bytes, sig_bytes))
}

fn is_debug_loopback(source: &str) -> bool {
    cfg!(any(test, debug_assertions))
        && url::Url::parse(source).is_ok_and(|parsed| {
            parsed.scheme() == "http"
                && parsed.host_str() == Some("127.0.0.1")
                && parsed.port().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none()
        })
}

fn fetch_http_bytes(
    url: &str,
    loopback: bool,
    direct: bool,
    max_bytes: u64,
    too_large: String,
) -> Result<Vec<u8>, String> {
    let agent = if loopback {
        ureq::AgentBuilder::new()
            .try_proxy_from_env(false)
            .redirects(0)
            .timeout(Duration::from_secs(2))
            .build()
    } else {
        let slot = if direct {
            "sunrise-index-v2-direct"
        } else {
            "sunrise-index-v2"
        };
        crate::proxy::agent(slot, url, |builder| {
            let builder = builder.https_only(true).timeout(Duration::from_secs(10));
            if direct {
                builder.redirects(0)
            } else {
                builder
            }
        })?
    };

    let response = agent
        .get(url)
        .set("User-Agent", crate::proxy::USER_AGENT)
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(code, _) => trf(
                "The mod registry answered {code}.",
                &[("code", &code.to_string())],
            ),
            ureq::Error::Transport(transport) => trf(
                "The mod registry can't be reached: {error}",
                &[("error", &transport.to_string())],
            ),
        })?;
    if response.status() != 200 || response.header("Content-Range").is_some() {
        return Err(tr("The mod registry didn't send the full index.").into());
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max_bytes {
        return Err(too_large);
    }
    Ok(bytes)
}

fn read_bounded_file(path: &Path, max_bytes: u64, too_large: String) -> Result<Vec<u8>, String> {
    let file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > max_bytes {
        return Err(too_large);
    }
    Ok(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut temp = tempfile::Builder::new()
        .prefix(".sunrise-v2-")
        .tempfile_in(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    temp.write_all(bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    temp.persist(path)
        .map_err(|e| format!("{}: {}", path.display(), e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests;
