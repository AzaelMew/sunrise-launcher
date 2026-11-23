//! The signed v2 mod registry catalogue (`v2/index.json` + `v2/index.json.minisig`).
//!
//! Readers verify the minisign signature over the exact bytes of `index.json` before parsing.
//! Schema 2 ignores unknown fields for forward compatibility, and refuses any malformed payload
//! whole without salvaging partial entries.
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

pub use crate::index::Kind;

/// The schema number accepted by this reader. Breaking changes raise `schema` and move to a new
/// address.
pub const SCHEMA: u32 = 2;

/// Maximum size of a fetched v2 registry index in bytes (same bound as v1).
pub const MAX_INDEX: u64 = 16 << 20;

/// Maximum size of a fetched minisign `.minisig` signature file in bytes.
pub const MAX_SIGNATURE: u64 = 16 << 10;

/// Age after which an accepted index's `generated` timestamp produces a non-blocking warning
/// (7 days). `generated` is informational only and never expires or rejects an index.
pub const STALE_AFTER_SECS: i64 = 7 * 24 * 60 * 60;

/// The published v2 mod registry catalogue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    pub serial: u64,
    pub generated: String,
    pub categories: Vec<Category>,
    pub projects: Vec<Project>,
    pub revoked: Vec<Revoked>,
}

/// One registry-owned category row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub id: String,
    pub icon: String,
    pub kinds: Vec<Kind>,
}

/// One registered project with at least one published release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    /// Permanent 8-character base62 project identity.
    pub id: String,
    /// Unique URL slug (`[a-z0-9_-]+`).
    pub slug: String,
    /// In-game mod ID (`[a-z0-9_]+`). Not unique across projects.
    #[serde(rename = "mod")]
    pub mod_id: String,
    pub kind: Kind,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub authors: Vec<String>,
    pub owners: Vec<String>,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issues: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Relative path under `pages/<project>/`.
    pub icon: String,
    pub screenshots: Vec<Screenshot>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub repository: Repository,
    /// Expected signer workflow path (without `@ref`).
    pub workflow: String,
    pub releases: Vec<Release>,
}

/// One gallery screenshot entry on a project's page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Screenshot {
    /// Relative path under `pages/<project>/`.
    pub path: String,
    #[serde(default)]
    pub caption: String,
}

/// The GitHub repository that owns a registered project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    /// Permanent positive numeric GitHub repository ID.
    pub id: u64,
    /// Current `owner/repo` display name.
    pub name: String,
    /// Package folder inside the repository (`.` for root).
    pub path: String,
}

/// One published release of a project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    /// Publication date (`YYYY-MM-DD`).
    pub published: String,
    /// The Sunrise build ID this release was built for.
    pub sunrise: String,
    /// Full signer workflow reference (e.g.
    /// `sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    /// Exact Sunrise build IDs this release fits.
    pub fits: Vec<String>,
    pub file: File,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

/// The downloadable `.smod` artifact and its Sigstore attestation bundle URL.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub attestation: String,
}

/// One declared dependency of a release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// Target in-game mod slot (`[a-z0-9_]+`).
    #[serde(rename = "mod")]
    pub mod_id: String,
    /// Optional pinned provider project ID (8-char base62).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub required: bool,
}

/// A takedown row: without `sha256`, the whole project is taken down; otherwise only that digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revoked {
    pub project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    pub reason: String,
    pub date: String,
}

/// Why verifying or parsing a signed v2 catalogue failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VerifyError {
    /// No trusted registry public key is configured in this build.
    KeyNotConfigured,
    /// The minisign signature could not be decoded or did not verify against any trusted key.
    BadSignature(String),
    /// The verified payload is not a valid schema-2 index (refused whole, no salvage).
    Malformed(String),
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::KeyNotConfigured => f.write_str("registry key not configured"),
            VerifyError::BadSignature(reason) => {
                write!(f, "bad registry index signature: {reason}")
            }
            VerifyError::Malformed(reason) => write!(f, "malformed registry index: {reason}"),
        }
    }
}

impl std::error::Error for VerifyError {}

impl Release {
    /// True when the release's computed fit list names `build` exactly.
    pub fn fits_build(&self, build: &str) -> bool {
        self.fits.iter().any(|entry| entry == build)
    }
}

impl Project {
    /// Releases whose computed fit list names `build` exactly.
    pub fn releases_fitting<'a>(&'a self, build: &'a str) -> impl Iterator<Item = &'a Release> {
        self.releases.iter().filter(move |r| r.fits_build(build))
    }
}

impl Index {
    /// Parses exact UTF-8 bytes of a schema-2 `index.json` strictly. Refuses malformed payloads
    /// whole without salvaging partial projects, releases, or takedowns.
    pub fn parse_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_INDEX {
            return Err(format!("registry index exceeds {MAX_INDEX} bytes"));
        }
        let text = std::str::from_utf8(bytes)
            .map_err(|e| format!("registry index is not valid UTF-8: {e}"))?;
        let index: Index =
            serde_json::from_str(text).map_err(|e| format!("invalid registry v2 JSON: {e}"))?;
        index.check()?;
        Ok(index)
    }

    /// Parses a schema-2 `index.json` string strictly.
    pub fn parse(text: &str) -> Result<Self, String> {
        Self::parse_bytes(text.as_bytes())
    }

    /// Validates every schema-2 structural invariant. Any violation rejects the whole index.
    pub fn check(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!(
                "unsupported registry index schema {}, expected {SCHEMA}",
                self.schema
            ));
        }
        parse_rfc3339_utc(&self.generated).map_err(|e| format!("generated: {e}"))?;

        let mut seen_categories = HashSet::new();
        for category in &self.categories {
            if !valid_token(&category.id) {
                return Err(format!("invalid category id {:?}", category.id));
            }
            if !seen_categories.insert(category.id.as_str()) {
                return Err(format!("duplicate category id {:?}", category.id));
            }
            if !valid_token(&category.icon) {
                return Err(format!(
                    "category {:?}: invalid icon {:?}",
                    category.id, category.icon
                ));
            }
        }

        let mut seen_projects = HashSet::new();
        let mut seen_slugs = HashSet::new();
        let mut seen_repo_paths = HashSet::new();
        for project in &self.projects {
            if !valid_project_id(&project.id) {
                return Err(format!("invalid project id {:?}", project.id));
            }
            if !seen_projects.insert(project.id.as_str()) {
                return Err(format!("duplicate project id {:?}", project.id));
            }
            if !valid_slug(&project.slug) {
                return Err(format!("{}: invalid slug {:?}", project.id, project.slug));
            }
            if !seen_slugs.insert(project.slug.as_str()) {
                return Err(format!("duplicate project slug {:?}", project.slug));
            }
            if !valid_mod_id(&project.mod_id) {
                return Err(format!(
                    "{}: invalid mod id {:?}",
                    project.id, project.mod_id
                ));
            }
            if project.name.trim().is_empty()
                || project.summary.trim().is_empty()
                || project.description.trim().is_empty()
                || project.license.trim().is_empty()
            {
                return Err(format!(
                    "{}: name, summary, description, and license must be non-empty",
                    project.id
                ));
            }
            for (field, url) in [
                ("homepage", &project.homepage),
                ("issues", &project.issues),
                ("source", &project.source),
            ] {
                if let Some(url) = url {
                    check_http_url(url).map_err(|e| format!("{}: {field}: {e}", project.id))?;
                }
            }
            check_page_path(&project.id, &project.icon)
                .map_err(|e| format!("{}: icon: {e}", project.id))?;
            for screenshot in &project.screenshots {
                check_page_path(&project.id, &screenshot.path)
                    .map_err(|e| format!("{}: screenshot: {e}", project.id))?;
            }
            let mut project_categories = HashSet::new();
            for category in &project.categories {
                if !valid_token(category) {
                    return Err(format!("{}: invalid category {category:?}", project.id));
                }
                if !project_categories.insert(category.as_str()) {
                    return Err(format!("{}: duplicate category {category:?}", project.id));
                }
            }
            if project.repository.id == 0 {
                return Err(format!("{}: repository.id must be positive", project.id));
            }
            check_repo_name(&project.repository.name)
                .map_err(|e| format!("{}: repository.name: {e}", project.id))?;
            check_package_path(&project.repository.path)
                .map_err(|e| format!("{}: repository.path: {e}", project.id))?;
            if !seen_repo_paths.insert((project.repository.id, project.repository.path.as_str())) {
                return Err(format!(
                    "{}: duplicate repository ({}, {:?})",
                    project.id, project.repository.id, project.repository.path
                ));
            }
            if !valid_token(&project.workflow) {
                return Err(format!(
                    "{}: invalid workflow {:?}",
                    project.id, project.workflow
                ));
            }
            if project.releases.is_empty() {
                return Err(format!(
                    "{}: project must have at least one release",
                    project.id
                ));
            }
            let mut seen_versions = HashSet::new();
            for release in &project.releases {
                check_release(&project.id, release)?;
                if !seen_versions.insert(release.version.as_str()) {
                    return Err(format!(
                        "{}: duplicate release version {:?}",
                        project.id, release.version
                    ));
                }
            }
        }

        let mut seen_revoked = HashSet::new();
        for row in &self.revoked {
            if !valid_project_id(&row.project) {
                return Err(format!("revoked: invalid project id {:?}", row.project));
            }
            if let Some(hash) = &row.sha256 {
                check_hex64(hash).map_err(|e| format!("revoked {}: sha256: {e}", row.project))?;
            }
            if row.reason.trim().is_empty() {
                return Err(format!("revoked {}: reason must be non-empty", row.project));
            }
            check_date(&row.date).map_err(|e| format!("revoked {}: date: {e}", row.project))?;
            if !seen_revoked.insert((row.project.as_str(), row.sha256.as_deref())) {
                return Err(format!("revoked {}: duplicate takedown row", row.project));
            }
        }

        Ok(())
    }

    /// Finds a project by its permanent 8-character project ID.
    pub fn project(&self, id: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// Finds a project by its unique page slug.
    pub fn project_by_slug(&self, slug: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.slug == slug)
    }

    /// Lists all projects providing the given in-game `mod` slot. Order carries no preference.
    pub fn projects_for_mod<'a>(&'a self, mod_id: &'a str) -> impl Iterator<Item = &'a Project> {
        self.projects.iter().filter(move |p| p.mod_id == mod_id)
    }

    /// Returns the matching takedown row for `project` (whole-project takedown if `sha256` is
    /// `None`, or whole-project / exact-digest takedown if `sha256` is `Some`).
    pub fn revoked(&self, project: &str, sha256: Option<&str>) -> Option<&Revoked> {
        self.revoked.iter().find(|row| {
            row.project == project
                && match (&row.sha256, sha256) {
                    (None, _) => true,
                    (Some(taken), Some(hash)) => taken.eq_ignore_ascii_case(hash),
                    (Some(_), None) => false,
                }
        })
    }

    /// Parses `self.generated` as UTC Unix seconds.
    pub fn generated_unix_seconds(&self) -> Result<i64, String> {
        parse_rfc3339_utc(&self.generated)
    }

    /// True when `self.generated` is at least [`STALE_AFTER_SECS`] older than `now_unix_seconds`.
    pub fn is_stale_at(&self, now_unix_seconds: i64) -> bool {
        self.generated_unix_seconds()
            .is_ok_and(|generated| now_unix_seconds.saturating_sub(generated) >= STALE_AFTER_SECS)
    }
}

/// Verifies a minisign `.minisig` signature over the exact bytes of `index.json` against a list of
/// trusted registry public keys.
///
/// If `keys` contains no non-empty key, returns [`VerifyError::KeyNotConfigured`] and trusts
/// nothing.
pub fn verify_signature(bytes: &[u8], signature: &str, keys: &[&str]) -> Result<(), VerifyError> {
    let active_keys: Vec<&str> = keys
        .iter()
        .copied()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .collect();
    if active_keys.is_empty() {
        return Err(VerifyError::KeyNotConfigured);
    }
    let decoded = minisign_verify::Signature::decode(signature)
        .map_err(|e| VerifyError::BadSignature(format!("invalid minisign signature: {e}")))?;
    for key_text in active_keys {
        let Ok(public_key) = parse_public_key(key_text) else {
            continue;
        };
        if public_key.verify(bytes, &decoded, false).is_ok() {
            return Ok(());
        }
    }
    Err(VerifyError::BadSignature(
        "signature does not match any trusted registry key".into(),
    ))
}

/// Verifies `signature` over the exact `bytes` first, then strictly parses and validates the
/// schema-2 index without salvaging malformed data.
pub fn verify_and_parse(
    bytes: &[u8],
    signature: &str,
    keys: &[&str],
) -> Result<Index, VerifyError> {
    verify_signature(bytes, signature, keys)?;
    Index::parse_bytes(bytes).map_err(VerifyError::Malformed)
}

fn parse_public_key(text: &str) -> Result<minisign_verify::PublicKey, minisign_verify::Error> {
    if text.contains('\n') {
        minisign_verify::PublicKey::decode(text)
    } else {
        minisign_verify::PublicKey::from_base64(text)
    }
}

/// Resolves a relative image path (`pages/<project>/...`) against the directory of `index_url`.
pub fn image_url(index_url: &str, relative_path: &str) -> Option<String> {
    if !is_safe_relative_url_path(relative_path) || !relative_path.starts_with("pages/") {
        return None;
    }
    let without_query = index_url.split(['?', '#']).next()?;
    let (base, _) = without_query.rsplit_once('/')?;
    Some(format!("{base}/{relative_path}"))
}

/// True for an 8-character ASCII base62 project ID (`0-9`, `A-Z`, `a-z`).
pub fn valid_project_id(id: &str) -> bool {
    id.len() == 8 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// True for a valid project slug (`a-z`, `0-9`, `-`, `_`, non-empty).
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// True for a valid in-game mod ID (`a-z`, `0-9`, `_`, non-empty).
pub fn valid_mod_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Parses an RFC 3339 timestamp into UTC Unix seconds.
pub fn parse_rfc3339_utc(text: &str) -> Result<i64, String> {
    let bytes = text.as_bytes();
    if bytes.len() < 20 {
        return Err(format!("invalid RFC 3339 timestamp {text:?}"));
    }
    if bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return Err(format!("invalid RFC 3339 timestamp {text:?}"));
    }
    let year = i64::from(parse_digits(&text[0..4])?);
    let month: u32 = parse_digits(&text[5..7])?;
    let day: u32 = parse_digits(&text[8..10])?;
    let hour: u32 = parse_digits(&text[11..13])?;
    let minute: u32 = parse_digits(&text[14..16])?;
    let second: u32 = parse_digits(&text[17..19])?;

    validate_calendar_date(year, month, day)?;
    if hour > 23 || minute > 59 || second > 60 {
        return Err(format!("invalid time in timestamp {text:?}"));
    }

    let mut rest = &text[19..];
    if let Some(after_dot) = rest.strip_prefix('.') {
        let frac_len = after_dot.bytes().take_while(|b| b.is_ascii_digit()).count();
        if frac_len == 0 {
            return Err(format!("empty fractional seconds in timestamp {text:?}"));
        }
        rest = &after_dot[frac_len..];
    }

    let offset_secs: i64 = match rest {
        "Z" | "z" => 0,
        _ => {
            let r = rest.as_bytes();
            if r.len() != 6 || !matches!(r[0], b'+' | b'-') || r[3] != b':' {
                return Err(format!("invalid timezone offset in timestamp {text:?}"));
            }
            let off_h = i64::from(parse_digits(&rest[1..3])?);
            let off_m = i64::from(parse_digits(&rest[4..6])?);
            if off_h > 23 || off_m > 59 {
                return Err(format!(
                    "out-of-range timezone offset in timestamp {text:?}"
                ));
            }
            let total = off_h * 3600 + off_m * 60;
            if r[0] == b'+' { total } else { -total }
        }
    };

    let days = days_from_civil(year, month, day);
    let secs = days
        .checked_mul(86_400)
        .and_then(|d| {
            d.checked_add(
                i64::from(hour) * 3600 + i64::from(minute) * 60 + i64::from(second.min(59)),
            )
        })
        .and_then(|t| t.checked_sub(offset_secs))
        .ok_or_else(|| format!("timestamp out of range: {text:?}"))?;
    Ok(secs)
}

fn check_release(project_id: &str, release: &Release) -> Result<(), String> {
    let at = |msg: String| format!("{project_id} {}: {msg}", release.version);
    if release.version.trim().is_empty()
        || release.version.trim() != release.version
        || release.version.bytes().any(|b| b.is_ascii_control())
    {
        return Err(format!(
            "{project_id}: invalid release version {:?}",
            release.version
        ));
    }
    check_date(&release.published).map_err(|e| at(format!("published: {e}")))?;
    if release.sunrise.trim().is_empty()
        || release.sunrise.trim() != release.sunrise
        || release.sunrise.bytes().any(|b| b.is_ascii_control())
    {
        return Err(at(format!("invalid sunrise build {:?}", release.sunrise)));
    }
    if let Some(workflow) = &release.workflow
        && !valid_token(workflow)
    {
        return Err(at(format!("invalid release workflow {workflow:?}")));
    }
    let mut seen_fits = HashSet::new();
    for build in &release.fits {
        if build.trim().is_empty()
            || build.trim() != build
            || build.bytes().any(|b| b.is_ascii_control())
        {
            return Err(at(format!("invalid fits entry {build:?}")));
        }
        if !seen_fits.insert(build.as_str()) {
            return Err(at(format!("duplicate fits entry {build:?}")));
        }
    }
    check_https_url(&release.file.url).map_err(|e| at(format!("file.url: {e}")))?;
    if release.file.size == 0 {
        return Err(at("file.size must be positive".into()));
    }
    check_hex64(&release.file.sha256).map_err(|e| at(format!("file.sha256: {e}")))?;
    check_https_url(&release.file.attestation).map_err(|e| at(format!("file.attestation: {e}")))?;
    for dep in &release.dependencies {
        if !valid_mod_id(&dep.mod_id) {
            return Err(at(format!("dependency: invalid mod id {:?}", dep.mod_id)));
        }
        if let Some(provider) = &dep.project
            && !valid_project_id(provider)
        {
            return Err(at(format!("dependency: invalid project id {provider:?}")));
        }
    }
    Ok(())
}

fn valid_token(s: &str) -> bool {
    !s.is_empty()
        && !s
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
}

fn check_https_url(url: &str) -> Result<(), String> {
    match url.strip_prefix("https://") {
        Some(rest)
            if !rest.is_empty()
                && !url
                    .bytes()
                    .any(|b| b.is_ascii_control() || b.is_ascii_whitespace()) =>
        {
            Ok(())
        }
        _ => Err(format!("expected an https:// URL, got {url:?}")),
    }
}

fn check_http_url(url: &str) -> Result<(), String> {
    let valid_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    match valid_scheme {
        Some(rest)
            if !rest.is_empty()
                && !url
                    .bytes()
                    .any(|b| b.is_ascii_control() || b.is_ascii_whitespace()) =>
        {
            Ok(())
        }
        _ => Err(format!("expected an http(s):// URL, got {url:?}")),
    }
}

fn check_hex64(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!(
            "expected 64 lowercase hex characters, got {value:?}"
        ));
    }
    Ok(())
}

fn check_repo_name(name: &str) -> Result<(), String> {
    let Some((owner, repo)) = name.split_once('/') else {
        return Err(format!("expected owner/repo, got {name:?}"));
    };
    if owner.is_empty()
        || repo.is_empty()
        || repo.contains('/')
        || owner == "."
        || owner == ".."
        || repo == "."
        || repo == ".."
        || !valid_token(name)
    {
        return Err(format!("invalid owner/repo {name:?}"));
    }
    Ok(())
}

fn check_package_path(path: &str) -> Result<(), String> {
    if path == "." {
        return Ok(());
    }
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.bytes().any(|b| b.is_ascii_control())
    {
        return Err(format!("invalid package path {path:?}"));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(format!("invalid package path {path:?}"));
        }
    }
    Ok(())
}

fn is_safe_relative_url_path(path: &str) -> bool {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('?')
        || path.contains('#')
        || path.contains("://")
        || path
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
    {
        return false;
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return false;
        }
        let lower = segment.to_ascii_lowercase();
        if lower.contains("%2f")
            || lower.contains("%5c")
            || matches!(lower.as_str(), "%2e" | "%2e%2e" | ".%2e" | "%2e.")
        {
            return false;
        }
    }
    true
}

fn check_page_path(project_id: &str, path: &str) -> Result<(), String> {
    let expected_prefix = format!("pages/{project_id}/");
    if !path.starts_with(&expected_prefix) || !is_safe_relative_url_path(path) {
        return Err(format!(
            "expected a relative path under {expected_prefix:?}, got {path:?}"
        ));
    }
    Ok(())
}

fn check_date(text: &str) -> Result<(), String> {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err(format!("expected YYYY-MM-DD date, got {text:?}"));
    }
    let year = i64::from(parse_digits(&text[0..4])?);
    let month = parse_digits(&text[5..7])?;
    let day = parse_digits(&text[8..10])?;
    validate_calendar_date(year, month, day)
}

fn parse_digits(s: &str) -> Result<u32, String> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("expected ASCII digits, got {s:?}"));
    }
    s.parse::<u32>()
        .map_err(|_| format!("invalid integer {s:?}"))
}

fn validate_calendar_date(year: i64, month: u32, day: u32) -> Result<(), String> {
    if !(1..=12).contains(&month) {
        return Err(format!("month {month} out of range"));
    }
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => unreachable!(),
    };
    if day == 0 || day > max_day {
        return Err(format!("day {day} out of range for {year:04}-{month:02}"));
    }
    Ok(())
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = u64::from(month);
    let d = u64::from(day);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}

#[cfg(test)]
mod tests;
