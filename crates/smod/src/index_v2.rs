//! The signed v2 mod registry catalogue (`v2/index.json`, signed by `v2/index.json.minisig`;
//! readers fetch both as one `v2/index.signed`).
//!
//! Readers verify the minisign signature over the exact bytes of `index.json` before parsing.
//! Schema 2 ignores unknown fields for forward compatibility, and refuses any malformed payload
//! whole without salvaging partial entries.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

pub use crate::index::Kind;
use crate::manifest::JarRef;
pub use crate::manifest::valid_mod_id;

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

/// Maximum number of categories a project page may declare (Modrinth `tags.rs:10`).
const MAX_PROJECT_CATEGORIES: usize = 8;

/// The published v2 mod registry catalogue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Index {
    pub schema: u32,
    pub serial: u64,
    pub generated: String,
    #[serde(default)]
    pub builds: Vec<Build>,
    pub categories: Vec<Category>,
    pub projects: Vec<Project>,
    pub revoked: Vec<Revoked>,
    /// Publisher-curated sets of listed projects; an index from before collections has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collections: Vec<Collection>,
}

/// A set of listed projects curated by the one account that owns all of their repositories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    /// Permanent 8-character base62 identity, never a project's.
    pub id: String,
    pub name: String,
    pub summary: String,
    /// Longer text, `""` when the collection has none.
    #[serde(default)]
    pub description: String,
    /// The GitHub account that owns every member's repository.
    pub publisher: String,
    /// Relative path under `pages/<collection>/`.
    pub icon: String,
    /// Listed projects in the collection's own order.
    pub members: Vec<Member>,
}

/// One project in a collection, and whether it is picked when a player opens the collection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub project: String,
    pub default: bool,
}

/// One offered Sunrise game build listed in the v2 index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Build {
    /// Exact Sunrise build ID (`26.3-<semver>` or `<channel>-<YYYY-MM-DD>-<short>`).
    pub id: String,
    /// Full 40-character lowercase Git commit SHA of the game build's SDK.
    pub commit: String,
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
    /// In-game mod ID (`[a-z0-9_]{1,64}`, portable filename). Not unique across projects.
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
    /// Exact Sunrise build ID this release was built for.
    pub sunrise: String,
    /// Full signer workflow reference (`<project.workflow>@refs/tags/release-v<number>`).
    pub workflow: String,
    /// Numeric GitHub ID of the repository this release was signed from, recorded when it was
    /// published, so a project that later moves to another repository keeps its older releases
    /// verifiable. Absent in indexes written before the field existed: the project's repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<u64>,
    /// Exact Sunrise build IDs this release is known to fit.
    pub fits: Vec<String>,
    /// Exact Sunrise build IDs this release is definitively known not to fit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub does_not_fit: Vec<String>,
    /// The original jars this release lists that Modrinth no longer serves, one entry per jar.
    /// A release with any is not installable: its `[[jar]]` files can't be fetched. Left out when
    /// empty. An installed copy whose jar is already cached keeps working.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jar_unavailable: Vec<JarRef>,
    /// The builds where this release fits but a required dependency has no release, one entry
    /// per build and dependency, written by the registry and never worked out by the launcher.
    /// On a listed build the release is not installable. Left out when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependency_unavailable: Vec<DependencyUnavailable>,
    pub file: File,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

/// Three-way compatibility verdict for a release against a specific Sunrise game build.
///
/// Compatibility comes solely from the release's generated `fits` and `does_not_fit` lists;
/// never inferred from `sunrise` or semver.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FitVerdict {
    /// The build ID is listed in `fits`.
    Fits,
    /// The build ID is listed in `does_not_fit`.
    DoesNotFit,
    /// The build ID appears in neither `fits` nor `does_not_fit`.
    Unknown,
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
    /// Target in-game mod slot (`[a-z0-9_]{1,64}`, portable filename).
    #[serde(rename = "mod")]
    pub mod_id: String,
    /// The semver range the provider's release must satisfy, as written in the mod's manifest.
    pub version: semver::VersionReq,
    /// Optional pinned provider project ID (8-char base62).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub required: bool,
}

/// A required dependency of a release that has no release for one Sunrise build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyUnavailable {
    /// Exact Sunrise build ID.
    pub build: String,
    /// In-game mod slot of the required dependency that can't be satisfied on `build`.
    #[serde(rename = "mod")]
    pub mod_id: String,
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
    /// Three-way fit verdict for `build` using only the generated `fits` and `does_not_fit` lists.
    /// Never infers compatibility from `sunrise` or semver.
    pub fn fit_verdict(&self, build: &str) -> FitVerdict {
        if self.fits.iter().any(|entry| entry == build) {
            FitVerdict::Fits
        } else if self.does_not_fit.iter().any(|entry| entry == build) {
            FitVerdict::DoesNotFit
        } else {
            FitVerdict::Unknown
        }
    }

    /// True when the release's computed fit list names `build` exactly.
    pub fn fits_build(&self, build: &str) -> bool {
        self.fit_verdict(build) == FitVerdict::Fits
    }
}

impl Project {
    /// The repository ID `release` was signed from: its own row's, else the project's.
    pub fn release_repository(&self, release: &Release) -> u64 {
        release.repository.unwrap_or(self.repository.id)
    }

    /// Releases whose computed fit list names `build` exactly.
    pub fn releases_fitting<'a>(&'a self, build: &'a str) -> impl Iterator<Item = &'a Release> {
        self.releases.iter().filter(move |r| r.fits_build(build))
    }

    /// Verifies that `bytes` match the SHA-256 embedded in a content-addressed image path for this
    /// project (`pages/<project id>/<sha256>.<png|jpg>`).
    pub fn verify_image_bytes(&self, relative_path: &str, bytes: &[u8]) -> Result<(), String> {
        verify_image_bytes(&self.id, relative_path, bytes)
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

        let mut seen_builds = HashSet::new();
        for build in &self.builds {
            if !valid_build_id(&build.id) {
                return Err(format!("invalid offered build id {:?}", build.id));
            }
            if !crate::manifest::revision(&build.commit) {
                return Err(format!(
                    "build {:?}: invalid commit {:?}",
                    build.id, build.commit
                ));
            }
            if (build.id.starts_with("nightly-") || build.id.starts_with("dev-"))
                && build
                    .id
                    .rsplit_once('-')
                    .is_none_or(|(_, short)| !build.commit.starts_with(short))
            {
                return Err(format!(
                    "build {:?}: commit {:?} does not match build id",
                    build.id, build.commit
                ));
            }
            if !seen_builds.insert(build.id.as_str()) {
                return Err(format!("duplicate offered build id {:?}", build.id));
            }
        }

        let mut declared_categories = HashMap::new();
        for category in &self.categories {
            if !valid_slug(&category.id) {
                return Err(format!("invalid category id {:?}", category.id));
            }
            if declared_categories
                .insert(category.id.as_str(), category)
                .is_some()
            {
                return Err(format!("duplicate category id {:?}", category.id));
            }
            if !valid_slug(&category.icon) {
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
            if project.screenshots.is_empty() {
                return Err(format!(
                    "{}: project must have at least one screenshot",
                    project.id
                ));
            }
            for screenshot in &project.screenshots {
                check_page_path(&project.id, &screenshot.path)
                    .map_err(|e| format!("{}: screenshot: {e}", project.id))?;
            }
            if project.categories.len() > MAX_PROJECT_CATEGORIES {
                return Err(format!(
                    "{}: at most {MAX_PROJECT_CATEGORIES} categories are allowed",
                    project.id
                ));
            }
            let mut project_categories = HashSet::new();
            for category in &project.categories {
                if !project_categories.insert(category.as_str()) {
                    return Err(format!("{}: duplicate category {category:?}", project.id));
                }
                let Some(declared) = declared_categories.get(category.as_str()) else {
                    return Err(format!("{}: undeclared category {category:?}", project.id));
                };
                if !declared.kinds.contains(&project.kind) {
                    return Err(format!(
                        "{}: category {category:?} does not apply to kind {:?}",
                        project.id,
                        project.kind.name()
                    ));
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
            if !valid_token(&project.workflow) || project.workflow.contains('@') {
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
                check_release(&project.id, &project.workflow, release)?;
                if !seen_versions.insert(release.version.as_str()) {
                    return Err(format!(
                        "{}: duplicate release version {:?}",
                        project.id, release.version
                    ));
                }
            }
        }

        let mut seen_collections = HashSet::new();
        for collection in &self.collections {
            let id = &collection.id;
            if !valid_project_id(id) || seen_projects.contains(id.as_str()) {
                return Err(format!("invalid collection id {id:?}"));
            }
            if !seen_collections.insert(id.as_str()) {
                return Err(format!("duplicate collection id {id:?}"));
            }
            if collection.name.trim().is_empty() || collection.summary.trim().is_empty() {
                return Err(format!(
                    "collection {id}: name and summary must be non-empty"
                ));
            }
            check_page_path(id, &collection.icon)
                .map_err(|e| format!("collection {id}: icon: {e}"))?;
            if collection.members.len() < 2 || !collection.members.iter().any(|m| m.default) {
                return Err(format!(
                    "collection {id}: needs at least two members and one picked by default"
                ));
            }
            let mut members = HashSet::new();
            for member in &collection.members {
                let Some(project) = self.project(&member.project) else {
                    return Err(format!(
                        "collection {id}: member {:?} is not a listed project",
                        member.project
                    ));
                };
                if !members.insert(member.project.as_str()) {
                    return Err(format!(
                        "collection {id}: member {:?} is listed twice",
                        member.project
                    ));
                }
                // The owner's rule: only the publisher of every member curates a collection.
                let owner = project
                    .repository
                    .name
                    .split_once('/')
                    .map(|(owner, _)| owner);
                if owner != Some(collection.publisher.as_str()) {
                    return Err(format!(
                        "collection {id}: member {} is not published by {:?}",
                        member.project, collection.publisher
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

    /// Finds a collection by its permanent ID.
    pub fn collection(&self, id: &str) -> Option<&Collection> {
        self.collections.iter().find(|c| c.id == id)
    }

    /// Finds an offered build by its exact build ID.
    pub fn build(&self, id: &str) -> Option<&Build> {
        self.builds.iter().find(|b| b.id == id)
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

    /// Returns any exact-digest takedown row matching `sha256`, regardless of project ownership.
    /// Whole-project takedowns (`sha256: None`) are not returned here because they require proved
    /// project ownership.
    pub fn revoked_by_sha256(&self, sha256: &str) -> Option<&Revoked> {
        self.revoked.iter().find(|row| {
            row.sha256
                .as_deref()
                .is_some_and(|taken| taken.eq_ignore_ascii_case(sha256))
        })
    }

    /// Finds every `(Project, Release)` pair providing `mod_id` whose archive SHA-256 equals
    /// `sha256`. Used to adopt a v1 receipt only when verified digest history uniquely identifies
    /// one v2 project.
    pub fn releases_by_sha256(&self, mod_id: &str, sha256: &str) -> Vec<(&Project, &Release)> {
        self.projects
            .iter()
            .filter(|project| project.mod_id == mod_id)
            .flat_map(|project| {
                project
                    .releases
                    .iter()
                    .filter(|release| release.file.sha256.eq_ignore_ascii_case(sha256))
                    .map(move |release| (project, release))
            })
            .collect()
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

/// Extracts the expected lowercase 64-hex SHA-256 digest from a content-addressed image path
/// (`pages/<project id>/<sha256>.<png|jpg>`).
pub fn content_addressed_image_sha256<'a>(
    project_id: &str,
    relative_path: &'a str,
) -> Result<&'a str, String> {
    if !valid_project_id(project_id) {
        return Err(format!("invalid project id {project_id:?}"));
    }
    let expected_prefix = format!("pages/{project_id}/");
    let Some(file_name) = relative_path.strip_prefix(&expected_prefix) else {
        return Err(format!(
            "expected image path under {expected_prefix:?}, got {relative_path:?}"
        ));
    };
    if file_name.contains('/') || file_name.contains('\\') {
        return Err(format!(
            "expected a flat content-addressed image name under {expected_prefix:?}, got {relative_path:?}"
        ));
    }
    let digest = file_name
        .strip_suffix(".png")
        .or_else(|| file_name.strip_suffix(".jpg"))
        .ok_or_else(|| {
            format!("expected a .png or .jpg content-addressed image path, got {relative_path:?}")
        })?;
    check_hex64(digest).map_err(|e| format!("image path {relative_path:?}: {e}"))?;
    Ok(digest)
}

/// Verifies that fetched `bytes` match the SHA-256 embedded in a content-addressed image path
/// (`pages/<project id>/<sha256>.<png|jpg>`).
pub fn verify_image_bytes(
    project_id: &str,
    relative_path: &str,
    bytes: &[u8],
) -> Result<(), String> {
    let expected = content_addressed_image_sha256(project_id, relative_path)?;
    let actual = crate::index::sha256(bytes);
    if actual != expected {
        return Err(format!(
            "image {relative_path:?} hash mismatch: expected {expected}, got {actual}"
        ));
    }
    Ok(())
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

/// True for an exact Sunrise build ID: a stable tag (`<game>-<semver>`, e.g. `26.3-0.1.0`) or
/// an exact Nightly/Dev build ID (`<channel>-<YYYY-MM-DD>-<7..=40 lowercase hex>`).
/// Rolling channel aliases like `nightly` or `dev` are rejected.
pub fn valid_build_id(id: &str) -> bool {
    is_channel_build(id, "nightly") || is_channel_build(id, "dev") || is_stable_build(id)
}

fn is_channel_build(id: &str, channel: &str) -> bool {
    let Some(rest) = id
        .strip_prefix(channel)
        .and_then(|rest| rest.strip_prefix('-'))
    else {
        return false;
    };
    let bytes = rest.as_bytes();
    if bytes.len() < 12 || bytes[10] != b'-' {
        return false;
    }
    let date = &bytes[..10];
    let commit = &bytes[11..];
    check_date_bytes(date).is_ok()
        && (7..=40).contains(&commit.len())
        && commit
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A stable tag: two or three numeric game-version parts without leading zeros, then a full
/// Sunrise semver with no pre-release or build metadata (the publisher's `stable_tag`).
fn is_stable_build(id: &str) -> bool {
    let Some((game, version)) = id.split_once('-') else {
        return false;
    };
    let parts: Vec<&str> = game.split('.').collect();
    let canonical_number = |part: &&str| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && (*part == "0" || !part.starts_with('0'))
            && part.parse::<u64>().is_ok()
    };
    (2..=3).contains(&parts.len())
        && parts.iter().all(canonical_number)
        && semver::Version::parse(version)
            .is_ok_and(|semver| semver.pre.is_empty() && semver.build.is_empty())
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
    let year = i64::from(parse_ascii_digits(&bytes[0..4])?);
    let month: u32 = parse_ascii_digits(&bytes[5..7])?;
    let day: u32 = parse_ascii_digits(&bytes[8..10])?;
    let hour: u32 = parse_ascii_digits(&bytes[11..13])?;
    let minute: u32 = parse_ascii_digits(&bytes[14..16])?;
    let second: u32 = parse_ascii_digits(&bytes[17..19])?;

    validate_calendar_date(year, month, day)?;
    if hour > 23 || minute > 59 || second > 60 {
        return Err(format!("invalid time in timestamp {text:?}"));
    }

    let mut rest = &bytes[19..];
    if let Some(after_dot) = rest.strip_prefix(b".") {
        let frac_len = after_dot.iter().take_while(|b| b.is_ascii_digit()).count();
        if frac_len == 0 {
            return Err(format!("empty fractional seconds in timestamp {text:?}"));
        }
        rest = &after_dot[frac_len..];
    }

    let offset_secs: i64 = match rest {
        b"Z" | b"z" => 0,
        r if r.len() == 6 && matches!(r[0], b'+' | b'-') && r[3] == b':' => {
            let off_h = i64::from(parse_ascii_digits(&r[1..3])?);
            let off_m = i64::from(parse_ascii_digits(&r[4..6])?);
            if off_h > 23 || off_m > 59 {
                return Err(format!(
                    "out-of-range timezone offset in timestamp {text:?}"
                ));
            }
            let total = off_h * 3600 + off_m * 60;
            if r[0] == b'+' { total } else { -total }
        }
        _ => return Err(format!("invalid timezone offset in timestamp {text:?}")),
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

fn check_release(
    project_id: &str,
    project_workflow: &str,
    release: &Release,
) -> Result<(), String> {
    let at = |msg: String| format!("{project_id} {}: {msg}", release.version);
    semver::Version::parse(&release.version).map_err(|e| {
        format!(
            "{project_id}: invalid release version {:?}: {e}",
            release.version
        )
    })?;
    check_date(&release.published).map_err(|e| at(format!("published: {e}")))?;
    if !valid_build_id(&release.sunrise) {
        return Err(at(format!("invalid sunrise build {:?}", release.sunrise)));
    }
    check_release_workflow(project_workflow, &release.workflow)
        .map_err(|e| at(format!("workflow: {e}")))?;
    if release.repository == Some(0) {
        return Err(at("repository must be positive".into()));
    }

    let mut seen_verdicts = HashSet::new();
    for build in &release.fits {
        if !valid_build_id(build) {
            return Err(at(format!("invalid fits entry {build:?}")));
        }
        if !seen_verdicts.insert(build.as_str()) {
            return Err(at(format!("duplicate fits entry {build:?}")));
        }
    }
    for build in &release.does_not_fit {
        if !valid_build_id(build) {
            return Err(at(format!("invalid does_not_fit entry {build:?}")));
        }
        if !seen_verdicts.insert(build.as_str()) {
            return Err(at(format!("duplicate or conflicting fit entry {build:?}")));
        }
    }

    for entry in &release.dependency_unavailable {
        if !valid_build_id(&entry.build) {
            return Err(at(format!(
                "invalid dependency_unavailable build {:?}",
                entry.build
            )));
        }
        if !valid_mod_id(&entry.mod_id) {
            return Err(at(format!(
                "dependency_unavailable: invalid mod id {:?}",
                entry.mod_id
            )));
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

fn check_release_workflow(project_workflow: &str, release_workflow: &str) -> Result<(), String> {
    let Some((path, reference)) = release_workflow.split_once('@') else {
        return Err(format!(
            "expected {project_workflow:?} at refs/tags/release-v<number>, got {release_workflow:?}"
        ));
    };
    if path != project_workflow {
        return Err(format!(
            "workflow path {path:?} does not match project workflow {project_workflow:?}"
        ));
    }
    let Some(version_num) = reference.strip_prefix("refs/tags/release-v") else {
        return Err(format!(
            "expected refs/tags/release-v<number>, got {reference:?}"
        ));
    };
    if version_num.is_empty() || !version_num.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!(
            "expected numeric release-v<number> tag, got {reference:?}"
        ));
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
    parse_web_url(url, true)
}

fn check_http_url(url: &str) -> Result<(), String> {
    parse_web_url(url, false)
}

fn parse_web_url(text: &str, https_only: bool) -> Result<(), String> {
    if text.contains('\\') || text.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(format!("invalid characters in URL {text:?}"));
    }
    // The web parser forgives an empty authority (`https:///path` reads `path` as the host),
    // an empty user (`https://@host`) and an empty port (`host:`); the written URL has none.
    let authority = text
        .split_once("://")
        .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    if authority.is_empty() {
        return Err(format!("URL {text:?} is missing a host"));
    }
    if authority.contains('@') {
        return Err(format!("URL {text:?} must not contain credentials"));
    }
    if authority.ends_with(':') {
        return Err(format!("URL {text:?} has an invalid port"));
    }
    let url = url::Url::parse(text).map_err(|e| format!("invalid URL {text:?}: {e}"))?;
    match url.scheme() {
        "https" => (),
        "http" if !https_only => (),
        _ if https_only => return Err(format!("expected an https:// URL, got {text:?}")),
        _ => return Err(format!("expected an http(s):// URL, got {text:?}")),
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(format!("URL {text:?} is missing a host"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(format!("URL {text:?} must not contain credentials"));
    }
    Ok(())
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
    check_date_bytes(text.as_bytes()).map_err(|e| format!("{e} in {text:?}"))
}

fn check_date_bytes(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return Err("expected YYYY-MM-DD date".into());
    }
    let year = i64::from(parse_ascii_digits(&bytes[0..4])?);
    let month = parse_ascii_digits(&bytes[5..7])?;
    let day = parse_ascii_digits(&bytes[8..10])?;
    validate_calendar_date(year, month, day)
}

fn parse_ascii_digits(bytes: &[u8]) -> Result<u32, String> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return Err("expected ASCII digits".into());
    }
    let mut value = 0u32;
    for &b in bytes {
        value = value
            .checked_mul(10)
            .and_then(|v| v.checked_add(u32::from(b - b'0')))
            .ok_or_else(|| "integer overflow".to_string())?;
    }
    Ok(value)
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
