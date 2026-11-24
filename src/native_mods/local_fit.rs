//! Fit worked out on this computer for registry releases the signed index gives no verdict for
//! on a build, the index listing that build or not: the build's own `native-check` reads each
//! native release's `.smod`, all in one run, and a content mod fits when its content API line is
//! the build's, as the registry judges it. The answers are kept beside that build's SDK
//! (`sdk/<commit>/fits.json`, by the `.smod`'s SHA-256). These only fill gaps: a verdict in
//! the index always wins over one kept here.
use super::planning::check_projects;
use super::{bundle_in, fit, sdk_cache, Bundle, Cancellation};
use crate::browser::sunrise::fetch_release_bytes;
use crate::i18n::{tr, trf};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use sunrise_launcher_smod::index::Kind;
use sunrise_launcher_smod::index_v2::{FitVerdict, Index, Release};

/// What this computer's check said of one release on one build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Local {
    Fits,
    DoesNotFit,
    /// The build's tool can't read the release's uses list (it is in a newer schema): only a
    /// newer build's tool can tell.
    Unknown,
}

/// The kept answers for one build, by `.smod` SHA-256 in lowercase.
pub type Found = BTreeMap<String, Local>;

/// The file keeping build `commit`'s answers.
const FILE: &str = "fits.json";

/// What this computer has found for build `commit` so far; nothing when it never checked one.
pub fn kept(commit: &str) -> Found {
    if !super::is_revision(commit) {
        return Found::new();
    }
    fs::read(sdk_cache::build_folder(commit).join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The build id of game build `build`: the index's when it lists the build, else the one this
/// computer kept when it read the build's own release.
pub fn commit_of(index: &Index, build: &str) -> Option<String> {
    index.build(build).map_or_else(
        || sdk_cache::kept_build_of(build),
        |listed| Some(listed.commit.clone()),
    )
}

/// The releases a check of `build` would read: for each live mod project (native or content)
/// with no release the index lists as fitting `build`, its newest live release when the index
/// says nothing of it.
pub fn gaps<'a>(index: &'a Index, build: &str) -> Vec<&'a Release> {
    index
        .projects
        .iter()
        .filter(|p| {
            matches!(p.kind, Kind::NativeMod | Kind::Mod) && index.revoked(&p.id, None).is_none()
        })
        .filter_map(|p| {
            let live: Vec<&Release> = p
                .releases
                .iter()
                .filter(|r| index.revoked(&p.id, Some(&r.file.sha256)).is_none())
                .collect();
            if live
                .iter()
                .any(|r| r.fit_verdict(build) == FitVerdict::Fits)
            {
                return None;
            }
            let newest = live.into_iter().max_by(|a, b| newest_last(a, b))?;
            (newest.fit_verdict(build) == FitVerdict::Unknown).then_some(newest)
        })
        .collect()
}

/// Release order by version, then publication date, the way Discover orders them.
fn newest_last(a: &Release, b: &Release) -> std::cmp::Ordering {
    let version = |r: &Release| semver::Version::parse(&r.version).ok();
    version(a)
        .cmp(&version(b))
        .then_with(|| a.published.cmp(&b.published))
        .then_with(|| a.version.cmp(&b.version))
}

/// `index` with this computer's answers written into each release's `fits` or `does_not_fit`,
/// only where the index says nothing of the build: for every build the index lists, and every
/// build it doesn't whose build id this computer kept.
pub fn filled(index: &Index) -> Index {
    fill(index, sdk_cache::kept_builds(), kept)
}

/// [`filled`], with the game builds whose build id this computer kept in `kept_builds` and a
/// build id's answers read by `found`.
fn fill(index: &Index, kept_builds: Vec<(String, String)>, found: impl Fn(&str) -> Found) -> Index {
    let listed = index
        .builds
        .iter()
        .map(|b| (b.id.clone(), b.commit.clone()));
    let unlisted = kept_builds
        .into_iter()
        .filter(|(version, _)| index.build(version).is_none());
    let mut out = index.clone();
    for (build, commit) in listed.chain(unlisted) {
        let found = found(&commit);
        if found.is_empty() {
            continue;
        }
        for release in out.projects.iter_mut().flat_map(|p| p.releases.iter_mut()) {
            if release.fit_verdict(&build) != FitVerdict::Unknown {
                continue;
            }
            match found.get(&release.file.sha256.to_ascii_lowercase()) {
                Some(Local::Fits) => release.fits.push(build.clone()),
                Some(Local::DoesNotFit) => release.does_not_fit.push(build.clone()),
                Some(Local::Unknown) | None => {}
            }
        }
    }
    out
}

/// What the tool's verdict means for a card.
fn local(verdict: &fit::Verdict) -> Local {
    match verdict {
        fit::Verdict::MadeFor | fit::Verdict::Fits { .. } => Local::Fits,
        fit::Verdict::NoList {
            why: Some(fit::NoListReason::NewerSchema),
            ..
        } => Local::Unknown,
        _ => Local::DoesNotFit,
    }
}

/// How many `.smod` files are downloaded at once.
const PARALLEL: usize = 4;

/// Check every release in [`gaps`] of `index` on `build` that this computer hasn't answered yet,
/// keeping each answer it gets. A build the index doesn't list is known by the build id its own
/// signed release names. Fails, with what the player reads, when the build's SDK or a release
/// can't be read; every other release is still checked and its answer kept.
pub fn check(index: &Index, build: &str) -> Result<(), String> {
    let base = sdk_cache::Base::from_env();
    let keys = sunrise_launcher_release_verify::sdk_keys_for(crate::backend::GAME_REPO);
    let commit = &match index.build(build) {
        Some(listed) => listed.commit.clone(),
        None => sdk_cache::build_of(build, &base, &keys).map_err(|r| r.to_string())?,
    };
    let known = kept(commit);
    let wanted: Vec<&Release> = gaps(index, build)
        .into_iter()
        .filter(|r| !known.contains_key(&r.file.sha256.to_ascii_lowercase()))
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let scratch = tempfile::Builder::new()
        .prefix("sunrise-fit-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    // Each release unpacked into a folder of its own: one run reads them all, and a release
    // that stops the run is then checked alone.
    let mut unpacked: Vec<(String, Bundle)> = Vec::new();
    let mut content: Vec<(String, String)> = Vec::new();
    let mut failed = None;
    for (n, chunk) in wanted.chunks(PARALLEL).enumerate() {
        let fetched: Vec<Result<Vec<u8>, String>> = std::thread::scope(|scope| {
            let jobs: Vec<_> = chunk
                .iter()
                .map(|release| scope.spawn(move || download(release)))
                .collect();
            jobs.into_iter()
                .map(|job| {
                    job.join()
                        .unwrap_or_else(|_| Err(tr("The download stopped.").into()))
                })
                .collect()
        });
        for (m, (release, bytes)) in chunk.iter().zip(fetched).enumerate() {
            let dir = scratch.path().join(format!("{}", n * PARALLEL + m));
            let sha256 = release.file.sha256.to_ascii_lowercase();
            match bytes.and_then(|bytes| unpack(&bytes, &dir)) {
                Ok(Unpacked::Native(bundle)) => unpacked.push((sha256, *bundle)),
                Ok(Unpacked::Content { api }) => content.push((sha256, api)),
                Err(error) => {
                    failed.get_or_insert(error);
                }
            }
        }
    }
    if unpacked.is_empty() && content.is_empty() {
        return failed.map_or(Ok(()), Err);
    }

    let cancellation = Cancellation::default();
    let _lease = super::build_lease(&cancellation, &|| {})?.map_err(super::cache_error)?;
    let sdk = sdk_cache::index(&base, commit, &keys).map_err(|r| r.to_string())?;
    let mut found = kept(commit);
    let save = |found: &Found| {
        let bytes = serde_json::to_vec_pretty(found).map_err(|e| e.to_string())?;
        sdk_cache::write_whole(&sdk_cache::build_folder(commit).join(FILE), &bytes)
            .map_err(|r| r.to_string())
    };
    // A content mod has no code for the build's tool to read: the registry's rule judges it, by
    // its content API line alone. Kept before the tool is fetched, which may fail.
    for (sha256, api) in content {
        let said = if api == sdk.index.content_api {
            Local::Fits
        } else {
            Local::DoesNotFit
        };
        found.insert(sha256, said);
    }
    if unpacked.is_empty() {
        save(&found)?;
        return failed.map_or(Ok(()), Err);
    }
    // The tool and the build's surface first: a run that fails after this is about the mods.
    let tool = sdk_cache::tool(&sdk, &base, &|_| {})
        .and_then(|_| sdk_cache::surface_file(&sdk, &base))
        .map_err(|r| r.to_string());
    if let Err(error) = tool {
        save(&found)?;
        return Err(error);
    }

    let mut answer = |sha256: &str, said: Local| {
        found.insert(sha256.to_owned(), said);
    };
    // One run for every mod with an id no other here has; a shared id is checked alone.
    let mut ids = BTreeSet::new();
    let (batch, alone): (Vec<_>, Vec<_>) = unpacked
        .iter()
        .partition(|(_, bundle)| ids.insert(bundle.manifest.id.clone()));
    let together = scratch.path().join("all");
    for (_, bundle) in &batch {
        super::copy_bundle(bundle, &together.join(&bundle.manifest.id))?;
    }
    let bundles = super::scan_bundles(&together)?;
    let alone = match check_projects(&together, &bundles, &sdk, &base, &cancellation) {
        Ok(checks) => {
            for (sha256, bundle) in &batch {
                answer(sha256, said_of(&checks, &bundle.manifest.id));
            }
            alone
        }
        Err(_) => unpacked.iter().collect(),
    };
    for (sha256, bundle) in alone {
        let only = std::slice::from_ref(bundle);
        match check_projects(&bundle.dir, only, &sdk, &base, &cancellation) {
            Ok(checks) => answer(sha256, said_of(&checks, &bundle.manifest.id)),
            // The tool stopped on this mod without a verdict: it stays unanswered until Try
            // again.
            Err(refusal @ sdk_cache::Refusal::Refused { .. }) => {
                failed.get_or_insert(refusal.to_string());
            }
            Err(refusal) => {
                failed.get_or_insert(refusal.to_string());
                break;
            }
        }
    }
    save(&found)?;
    failed.map_or(Ok(()), Err)
}

/// The answer `checks` gives for mod `id`: a refusal means no build of it works here.
fn said_of(checks: &super::Checks, id: &str) -> Local {
    checks
        .verdicts
        .iter()
        .find(|(mod_id, _)| mod_id == id)
        .map_or(Local::DoesNotFit, |(_, verdict)| local(verdict))
}

/// The release's `.smod`, checked against the signed index's size and SHA-256.
fn download(release: &Release) -> Result<Vec<u8>, String> {
    let bytes = fetch_release_bytes(&release.file.url, release.file.size)?;
    if bytes.len() as u64 != release.file.size
        || !sunrise_launcher_smod::index::sha256(&bytes).eq_ignore_ascii_case(&release.file.sha256)
    {
        return Err(trf(
            "{file} doesn't match the registry's checksum.",
            &[("file", &release.file.url)],
        ));
    }
    Ok(bytes)
}

/// A downloaded release, as its check needs it.
enum Unpacked {
    /// A native package, its files written out for the build's tool.
    Native(Box<Bundle>),
    /// A content mod's content API line.
    Content { api: String },
}

/// `bytes` read as a package: a native one's files written under `dir`.
fn unpack(bytes: &[u8], dir: &Path) -> Result<Unpacked, String> {
    let file = dir.with_extension("smod");
    fs::write(&file, bytes).map_err(|e| e.to_string())?;
    let package = sunrise_launcher_smod::Package::read(&file)?;
    if !package.is_native() {
        return Ok(Unpacked::Content {
            api: package.manifest.api,
        });
    }
    let package = sunrise_launcher_smod::index::native::Bundle::read(&file)?;
    for (name, bytes) in &package.files {
        let to = dir.join(name);
        fs::create_dir_all(to.parent().unwrap_or(dir)).map_err(|e| e.to_string())?;
        fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    bundle_in(dir)?
        .map(|bundle| Unpacked::Native(Box::new(bundle)))
        .ok_or_else(|| {
            trf(
                "{mod} holds no native mod.",
                &[("mod", &package.manifest.id)],
            )
        })
}

#[cfg(test)]
#[path = "local_fit_tests.rs"]
mod tests;
