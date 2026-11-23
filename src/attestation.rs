//! Offline Sigstore attestation policy and per-computer publisher pinning for v2 registry mods.
//!
//! Cryptographic verification lives in `sunrise-attestation` (`crates/attestation`). This module
//! applies Sunrise's release policy:
//! - the attested repository ID equals the project's `repository.id`;
//! - the attested signer workflow equals the release's `workflow` field exactly;
//! - that workflow is `sunrise-org/sunrise-mods/.github/workflows/release.yml` at a
//!   `refs/tags/release-v<number>` tag.
//!
//! At a project's first install on this computer, the launcher remembers its repository ID and
//! workflow path in `publisher-pins.json` under the launcher data folder (outside disposable
//! caches). A newer `release-v<number>` tag on the same workflow path is accepted; a different
//! repository ID or workflow path returns [`Error::PublisherChanged`] so the UI can ask the player
//! before accepting the new publisher.

use crate::backend;
use crate::i18n::trf;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::Write as _;
use std::path::Path;

pub use sunrise_attestation::{Attested, PUBLIC_GOOD_TRUSTED_ROOT};

/// The shared Sunrise release workflow path trusted for registry releases.
pub const RELEASE_WORKFLOW_PATH: &str = "sunrise-org/sunrise-mods/.github/workflows/release.yml";
/// Required git ref prefix on [`RELEASE_WORKFLOW_PATH`] (`refs/tags/release-v<number>`).
pub const RELEASE_WORKFLOW_REF_PREFIX: &str = "refs/tags/release-v";
/// Per-computer publisher pin file stored in [`backend::data_dir`].
pub const PINS_FILE: &str = "publisher-pins.json";

const PINS_LOCK: &str = ".publisher-pins.lock";
const PIN_SCHEMA: u32 = 1;

/// What a v2 registry release claims its attestation proves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedRelease<'a> {
    /// Stable numeric GitHub repository ID (`project.repository.id`).
    pub repository_id: u64,
    /// Full signer workflow ref recorded on the release (`release.workflow`),
    /// e.g. `sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1`.
    pub workflow: &'a str,
}

/// A release attestation that passed offline Sigstore verification and Sunrise's workflow policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRelease {
    pub attested: Attested,
    /// Workflow path without `@<ref>`.
    pub workflow_path: String,
    /// Workflow git ref after `@` (`refs/tags/release-v<number>`).
    pub workflow_ref: String,
}

impl VerifiedRelease {
    /// The publisher identity remembered per project on this computer.
    pub fn pin(&self) -> PublisherPin {
        PublisherPin {
            repository_id: self.attested.repository_id,
            repository_name: self.attested.repository_name.clone(),
            workflow_path: self.workflow_path.clone(),
        }
    }
}

/// The repository ID and workflow path remembered for a registry project on this computer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublisherPin {
    pub repository_id: u64,
    pub repository_name: String,
    pub workflow_path: String,
}

impl PublisherPin {
    /// True when `other` keeps the same pinned repository ID and workflow path.
    /// A repository rename (`repository_name`) or a newer `release-v<number>` tag on the same
    /// workflow path is not a publisher change.
    pub fn matches(&self, other: &PublisherPin) -> bool {
        self.repository_id == other.repository_id && self.workflow_path == other.workflow_path
    }
}

/// Details of a verified release whose repository ID or workflow path differs from the computer's
/// remembered pin for the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublisherChange {
    pub project_id: String,
    pub pinned: PublisherPin,
    pub incoming: PublisherPin,
}

impl PublisherChange {
    pub fn repository_changed(&self) -> bool {
        self.pinned.repository_id != self.incoming.repository_id
    }

    pub fn workflow_changed(&self) -> bool {
        self.pinned.workflow_path != self.incoming.workflow_path
    }
}

/// Outcome of checking a verified release against the computer's remembered publisher pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinStatus {
    /// First install of this project on this computer (no prior pin existed).
    FirstInstall,
    /// Matches the repository ID and workflow path remembered at first install.
    Matched,
}

#[derive(Debug)]
pub enum Error {
    /// Offline Sigstore bundle verification failed.
    Verify(Box<sunrise_attestation::Error>),
    /// The attested repository ID did not match the project's `repository.id`.
    WrongRepository {
        expected: u64,
        actual: u64,
        actual_name: String,
    },
    /// The attested signer workflow did not equal the release's `workflow` field.
    WorkflowMismatch { expected: String, actual: String },
    /// The attested signer workflow is not Sunrise's shared release workflow at a
    /// `refs/tags/release-v<number>` tag.
    UntrustedWorkflow { workflow: String },
    /// The release's repository ID or workflow path differs from what this computer remembered for
    /// this project at first install. The UI turns this into a confirmation prompt.
    PublisherChanged(Box<PublisherChange>),
    /// Reading or saving the computer's publisher pin file failed.
    PinStore(String),
}

impl Error {
    /// Returns the publisher-change details when a release needs confirmation from the player.
    pub fn publisher_change(&self) -> Option<&PublisherChange> {
        match self {
            Self::PublisherChanged(change) => Some(change),
            _ => None,
        }
    }
}

impl From<sunrise_attestation::Error> for Error {
    fn from(error: sunrise_attestation::Error) -> Self {
        Self::Verify(Box::new(error))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verify(error) => f.write_str(&trf(
                "This release's build attestation could not be verified: {detail}",
                &[("detail", &error.to_string())],
            )),
            Self::WrongRepository {
                expected,
                actual,
                actual_name,
            } => f.write_str(&trf(
                "This release was built by {repo} (repository {actual}), not the expected repository ({expected}).",
                &[
                    ("repo", actual_name),
                    ("actual", &actual.to_string()),
                    ("expected", &expected.to_string()),
                ],
            )),
            Self::WorkflowMismatch { expected, actual } => f.write_str(&trf(
                "This release was built by {actual}, not the listed workflow {expected}.",
                &[("actual", actual), ("expected", expected)],
            )),
            Self::UntrustedWorkflow { workflow } => f.write_str(&trf(
                "This release was not built by Sunrise's release workflow ({workflow}).",
                &[("workflow", workflow)],
            )),
            Self::PublisherChanged(change) => f.write_str(&trf(
                "The publisher for this mod changed from {old_repo} ({old_workflow}) to {new_repo} ({new_workflow}).",
                &[
                    ("old_repo", &change.pinned.repository_name),
                    ("old_workflow", &change.pinned.workflow_path),
                    ("new_repo", &change.incoming.repository_name),
                    ("new_workflow", &change.incoming.workflow_path),
                ],
            )),
            Self::PinStore(detail) => f.write_str(&trf(
                "Could not read or save remembered mod publishers: {detail}",
                &[("detail", detail)],
            )),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Verify(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

/// Decode a 64-character hexadecimal SHA-256 string into raw bytes.
pub fn parse_sha256_hex(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
        let hi = hex_nibble(pair[0])?;
        let lo = hex_nibble(pair[1])?;
        *slot = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Split `signer_workflow_ref` (`<path>@<ref>`) and verify it names Sunrise's shared release
/// workflow at a `refs/tags/release-v<number>` tag.
pub fn parse_release_workflow(signer_workflow_ref: &str) -> Option<(&str, &str)> {
    let (path, git_ref) = signer_workflow_ref.split_once('@')?;
    if path != RELEASE_WORKFLOW_PATH {
        return None;
    }
    let number = git_ref.strip_prefix(RELEASE_WORKFLOW_REF_PREFIX)?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((path, git_ref))
}

/// Enforce Sunrise's release attestation policy on verified certificate claims.
pub fn check_release_policy(
    attested: Attested,
    expected: &ExpectedRelease<'_>,
) -> Result<VerifiedRelease, Error> {
    if attested.repository_id != expected.repository_id {
        return Err(Error::WrongRepository {
            expected: expected.repository_id,
            actual: attested.repository_id,
            actual_name: attested.repository_name,
        });
    }
    let (workflow_path, workflow_ref) = match parse_release_workflow(&attested.signer_workflow_ref)
    {
        Some((path, git_ref)) => (path.to_owned(), git_ref.to_owned()),
        None => {
            return Err(Error::UntrustedWorkflow {
                workflow: attested.signer_workflow_ref,
            });
        }
    };
    if attested.signer_workflow_ref != expected.workflow {
        return Err(Error::WorkflowMismatch {
            expected: expected.workflow.to_owned(),
            actual: attested.signer_workflow_ref,
        });
    }
    Ok(VerifiedRelease {
        attested,
        workflow_path,
        workflow_ref,
    })
}

/// Verify a registry release's `.sigstore.json` bundle offline against the downloaded `.smod`
/// SHA-256 and Sunrise's release policy. Does not consult or modify publisher pins.
pub fn verify_release(
    bundle_json: &[u8],
    artifact_sha256: [u8; 32],
    expected: &ExpectedRelease<'_>,
) -> Result<VerifiedRelease, Error> {
    let attested =
        sunrise_attestation::verify(bundle_json, artifact_sha256, PUBLIC_GOOD_TRUSTED_ROOT)?;
    check_release_policy(attested, expected)
}

/// Verify a local `.smod`'s `.sigstore.json` bundle offline against its SHA-256 without requiring
/// Sunrise's registry workflow (for unreviewed local files in "Add from source…").
pub fn verify_local(
    bundle_json: &[u8],
    artifact_sha256: [u8; 32],
) -> Result<Attested, sunrise_attestation::Error> {
    sunrise_attestation::verify(bundle_json, artifact_sha256, PUBLIC_GOOD_TRUSTED_ROOT)
}

/// Verify a registry release's attestation and check or record its publisher pin under
/// [`backend::data_dir`].
pub fn verify_release_and_pin(
    project_id: &str,
    bundle_json: &[u8],
    artifact_sha256: [u8; 32],
    expected: &ExpectedRelease<'_>,
) -> Result<(VerifiedRelease, PinStatus), Error> {
    verify_release_and_pin_in(
        &backend::data_dir(),
        project_id,
        bundle_json,
        artifact_sha256,
        expected,
    )
}

/// Verify a registry release's attestation and check or record its publisher pin in `data_dir`.
pub fn verify_release_and_pin_in(
    data_dir: &Path,
    project_id: &str,
    bundle_json: &[u8],
    artifact_sha256: [u8; 32],
    expected: &ExpectedRelease<'_>,
) -> Result<(VerifiedRelease, PinStatus), Error> {
    let verified = verify_release(bundle_json, artifact_sha256, expected)?;
    let status = pin_or_check_in(data_dir, project_id, &verified.pin())?;
    Ok((verified, status))
}

#[derive(Debug, Serialize, Deserialize)]
struct PinFile {
    schema: u32,
    projects: BTreeMap<String, PublisherPin>,
}

/// Load all remembered project publisher pins from [`backend::data_dir`].
pub fn load_pins() -> Result<BTreeMap<String, PublisherPin>, Error> {
    load_pins_in(&backend::data_dir())
}

/// Load all remembered project publisher pins from `data_dir`.
pub fn load_pins_in(data_dir: &Path) -> Result<BTreeMap<String, PublisherPin>, Error> {
    Ok(load_file(data_dir)?.projects)
}

/// Check `incoming` against the computer's remembered pin for `project_id` without modifying disk.
pub fn check_pin(project_id: &str, incoming: &PublisherPin) -> Result<PinStatus, Error> {
    check_pin_in(&backend::data_dir(), project_id, incoming)
}

/// Check `incoming` against the remembered pin for `project_id` in `data_dir` without modifying disk.
pub fn check_pin_in(
    data_dir: &Path,
    project_id: &str,
    incoming: &PublisherPin,
) -> Result<PinStatus, Error> {
    let file = load_file(data_dir)?;
    match file.projects.get(project_id) {
        None => Ok(PinStatus::FirstInstall),
        Some(pinned) if pinned.matches(incoming) => Ok(PinStatus::Matched),
        Some(pinned) => Err(Error::PublisherChanged(Box::new(PublisherChange {
            project_id: project_id.to_owned(),
            pinned: pinned.clone(),
            incoming: incoming.clone(),
        }))),
    }
}

/// Check `incoming` against this computer's remembered pin for `project_id`, recording the pin on
/// first install. Returns [`Error::PublisherChanged`] without modifying disk when the repository ID
/// or workflow path differs.
pub fn pin_or_check(project_id: &str, incoming: &PublisherPin) -> Result<PinStatus, Error> {
    pin_or_check_in(&backend::data_dir(), project_id, incoming)
}

/// Check `incoming` against the remembered pin for `project_id` in `data_dir`, recording the pin on
/// first install. Returns [`Error::PublisherChanged`] without modifying disk when the repository ID
/// or workflow path differs.
pub fn pin_or_check_in(
    data_dir: &Path,
    project_id: &str,
    incoming: &PublisherPin,
) -> Result<PinStatus, Error> {
    under_lock(data_dir, || {
        let mut file = load_file(data_dir)?;
        match file.projects.get_mut(project_id) {
            None => {
                file.projects
                    .insert(project_id.to_owned(), incoming.clone());
                save_file(data_dir, &file)?;
                Ok(PinStatus::FirstInstall)
            }
            Some(pinned) if pinned.matches(incoming) => {
                if pinned.repository_name != incoming.repository_name {
                    pinned.repository_name = incoming.repository_name.clone();
                    save_file(data_dir, &file)?;
                }
                Ok(PinStatus::Matched)
            }
            Some(pinned) => Err(Error::PublisherChanged(Box::new(PublisherChange {
                project_id: project_id.to_owned(),
                pinned: pinned.clone(),
                incoming: incoming.clone(),
            }))),
        }
    })
}

/// Replace the remembered pin for `project_id` on this computer after the player explicitly accepts
/// a publisher change.
pub fn accept_pin(project_id: &str, incoming: &PublisherPin) -> Result<(), Error> {
    accept_pin_in(&backend::data_dir(), project_id, incoming)
}

/// Replace the remembered pin for `project_id` in `data_dir` after the player explicitly accepts a
/// publisher change.
pub fn accept_pin_in(
    data_dir: &Path,
    project_id: &str,
    incoming: &PublisherPin,
) -> Result<(), Error> {
    under_lock(data_dir, || {
        let mut file = load_file(data_dir)?;
        file.projects
            .insert(project_id.to_owned(), incoming.clone());
        save_file(data_dir, &file)
    })
}

fn under_lock<T>(data_dir: &Path, run: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    let err = |e: std::io::Error| Error::PinStore(format!("{PINS_FILE}: {e}"));
    fs::create_dir_all(data_dir).map_err(err)?;
    let gate = data_dir.join(PINS_LOCK);
    match fs::symlink_metadata(&gate) {
        Ok(meta) if meta.is_file() => (),
        Ok(_) => return Err(Error::PinStore(format!("{PINS_FILE}: invalid lock path"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(err(e)),
    }
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&gate)
        .map_err(err)?;
    lock.lock().map_err(err)?;
    let result = run();
    let _ = lock.unlock();
    result
}

fn load_file(data_dir: &Path) -> Result<PinFile, Error> {
    let path = data_dir.join(PINS_FILE);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => (),
        Ok(_) => return Err(Error::PinStore(format!("{PINS_FILE}: not a regular file"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PinFile {
                schema: PIN_SCHEMA,
                projects: BTreeMap::new(),
            });
        }
        Err(error) => return Err(Error::PinStore(format!("{PINS_FILE}: {error}"))),
    }
    let bytes = fs::read(&path).map_err(|e| Error::PinStore(format!("{PINS_FILE}: {e}")))?;
    let file: PinFile =
        serde_json::from_slice(&bytes).map_err(|e| Error::PinStore(format!("{PINS_FILE}: {e}")))?;
    if file.schema != PIN_SCHEMA {
        return Err(Error::PinStore(format!(
            "{PINS_FILE}: unsupported schema {}",
            file.schema
        )));
    }
    Ok(file)
}

fn save_file(data_dir: &Path, file: &PinFile) -> Result<(), Error> {
    let err = |e: std::io::Error| Error::PinStore(format!("{PINS_FILE}: {e}"));
    fs::create_dir_all(data_dir).map_err(err)?;
    let mut bytes = serde_json::to_vec_pretty(file)
        .map_err(|e| Error::PinStore(format!("{PINS_FILE}: {e}")))?;
    bytes.push(b'\n');
    let mut part = tempfile::NamedTempFile::new_in(data_dir).map_err(err)?;
    part.write_all(&bytes).map_err(err)?;
    part.as_file().sync_all().map_err(err)?;
    part.persist(data_dir.join(PINS_FILE))
        .map(|_| ())
        .map_err(|e| err(e.error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const RUFF_BUNDLE: &[u8] =
        include_bytes!("../crates/attestation/tests/fixtures/ruff-source.sigstore.json");
    const RUFF_DIGEST: [u8; 32] = [
        0x03, 0x78, 0xef, 0x60, 0x35, 0x6c, 0xaa, 0xad, 0x7b, 0x84, 0x65, 0x36, 0x91, 0xa5, 0xd5,
        0x7a, 0x1d, 0x8b, 0x57, 0xfd, 0x59, 0xd1, 0x73, 0xcf, 0xc2, 0x7b, 0x0f, 0xd4, 0xa9, 0x1a,
        0x13, 0xf3,
    ];
    const RUFF_DIGEST_HEX: &str =
        "0378ef60356caaad7b84653691a5d57a1d8b57fd59d173cfc27b0fd4a91a13f3";
    const RUFF_REPO_ID: u64 = 523_043_277;
    const RUFF_WORKFLOW: &str = "astral-sh/ruff/.github/workflows/release.yml@refs/heads/main";
    const SUNRISE_WORKFLOW_V1: &str =
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1";
    const SUNRISE_WORKFLOW_V2: &str =
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v2";

    fn sample_attested(repository_id: u64, repository_name: &str, workflow_ref: &str) -> Attested {
        Attested {
            repository_id,
            repository_name: repository_name.to_owned(),
            signer_workflow_ref: workflow_ref.to_owned(),
            source_ref: "refs/tags/v1.0.0".to_owned(),
            source_commit: "a7ba68b4919e56b6af949a42b2da38fe4eec67a9".to_owned(),
            predicate_type: "https://slsa.dev/provenance/v1".to_owned(),
        }
    }

    #[test]
    fn good_bundle_verifies_offline_and_decodes_hex_digest() {
        assert_eq!(parse_sha256_hex(RUFF_DIGEST_HEX), Some(RUFF_DIGEST));
        assert_eq!(
            parse_sha256_hex(&RUFF_DIGEST_HEX.to_ascii_uppercase()),
            Some(RUFF_DIGEST)
        );
        assert_eq!(parse_sha256_hex("deadbeef"), None);
        assert_eq!(parse_sha256_hex(&"z".repeat(64)), None);

        let attested = verify_local(RUFF_BUNDLE, RUFF_DIGEST).unwrap();
        assert_eq!(attested.repository_id, RUFF_REPO_ID);
        assert_eq!(attested.repository_name, "astral-sh/ruff");
        assert_eq!(attested.signer_workflow_ref, RUFF_WORKFLOW);
        assert_eq!(attested.source_ref, "refs/heads/main");
        assert_eq!(
            attested.source_commit,
            "a7ba68b4919e56b6af949a42b2da38fe4eec67a9"
        );
        assert_eq!(attested.predicate_type, "https://slsa.dev/provenance/v1");
    }

    #[test]
    fn wrong_digest_is_rejected() {
        let expected = ExpectedRelease {
            repository_id: RUFF_REPO_ID,
            workflow: SUNRISE_WORKFLOW_V1,
        };
        let err = verify_release(RUFF_BUNDLE, [0u8; 32], &expected).unwrap_err();
        assert!(
            matches!(err, Error::Verify(ref inner) if matches!(inner.as_ref(), sunrise_attestation::Error::Unverified(_))),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn tampered_bundle_is_rejected() {
        let expected = ExpectedRelease {
            repository_id: RUFF_REPO_ID,
            workflow: SUNRISE_WORKFLOW_V1,
        };

        // Tampered DSSE signature
        let mut bundle: Value = serde_json::from_slice(RUFF_BUNDLE).unwrap();
        let sig = &mut bundle["dsseEnvelope"]["signatures"][0]["sig"];
        let mut changed = sig.as_str().unwrap().to_owned();
        let i = changed.len() - 8;
        changed.replace_range(i..i + 1, if &changed[i..i + 1] == "A" { "B" } else { "A" });
        *sig = changed.into();
        let tampered_sig = serde_json::to_vec(&bundle).unwrap();
        assert!(matches!(
            verify_release(&tampered_sig, RUFF_DIGEST, &expected),
            Err(Error::Verify(_))
        ));

        // Tampered Rekor inclusion proof
        let mut bundle: Value = serde_json::from_slice(RUFF_BUNDLE).unwrap();
        bundle["verificationMaterial"]["tlogEntries"][0]["inclusionProof"]["hashes"][0] =
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".into();
        let tampered_log = serde_json::to_vec(&bundle).unwrap();
        assert!(matches!(
            verify_release(&tampered_log, RUFF_DIGEST, &expected),
            Err(Error::Verify(_))
        ));

        // Malformed JSON
        assert!(matches!(
            verify_release(b"{not valid json", RUFF_DIGEST, &expected),
            Err(Error::Verify(_))
        ));
    }

    #[test]
    fn wrong_repository_is_rejected() {
        let expected = ExpectedRelease {
            repository_id: RUFF_REPO_ID + 1,
            workflow: RUFF_WORKFLOW,
        };
        let err = verify_release(RUFF_BUNDLE, RUFF_DIGEST, &expected).unwrap_err();
        assert!(
            matches!(
                err,
                Error::WrongRepository {
                    expected,
                    actual,
                    ref actual_name,
                } if expected == RUFF_REPO_ID + 1
                    && actual == RUFF_REPO_ID
                    && actual_name == "astral-sh/ruff"
            ),
            "unexpected error: {err:?}"
        );
        assert!(err.to_string().contains("astral-sh/ruff"));
    }

    #[test]
    fn wrong_workflow_and_ref_are_rejected() {
        // Real bundle signed by astral-sh/ruff fails Sunrise's workflow policy even when expected matches it.
        let ruff_expected = ExpectedRelease {
            repository_id: RUFF_REPO_ID,
            workflow: RUFF_WORKFLOW,
        };
        assert!(matches!(
            verify_release(RUFF_BUNDLE, RUFF_DIGEST, &ruff_expected),
            Err(Error::UntrustedWorkflow { .. })
        ));

        // Valid Sunrise release workflow tags pass when matching expected.workflow.
        for workflow in [
            SUNRISE_WORKFLOW_V1,
            SUNRISE_WORKFLOW_V2,
            "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v42",
        ] {
            let attested = sample_attested(1234, "alice/mod-a", workflow);
            let verified = check_release_policy(
                attested,
                &ExpectedRelease {
                    repository_id: 1234,
                    workflow,
                },
            )
            .unwrap();
            assert_eq!(verified.workflow_path, RELEASE_WORKFLOW_PATH);
            assert!(verified
                .workflow_ref
                .starts_with(RELEASE_WORKFLOW_REF_PREFIX));
        }

        // Valid Sunrise workflow that does not match the release's listed workflow field is rejected.
        let attested = sample_attested(1234, "alice/mod-a", SUNRISE_WORKFLOW_V1);
        let err = check_release_policy(
            attested,
            &ExpectedRelease {
                repository_id: 1234,
                workflow: SUNRISE_WORKFLOW_V2,
            },
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                Error::WorkflowMismatch {
                    ref expected,
                    ref actual,
                } if expected == SUNRISE_WORKFLOW_V2 && actual == SUNRISE_WORKFLOW_V1
            ),
            "unexpected error: {err:?}"
        );

        // Wrong paths, branches, non-numeric tags, prefixes, or missing '@' are all rejected.
        for bad in [
            "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/heads/main",
            "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/v1",
            "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v",
            "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1-rc1",
            "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1/extra",
            "sunrise-org/sunrise-mods/.github/workflows/other.yml@refs/tags/release-v1",
            "other-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1",
            "https://github.com/sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1",
            "sunrise-org/sunrise-mods/.github/workflows/release.yml",
        ] {
            let attested = sample_attested(1234, "alice/mod-a", bad);
            let err = check_release_policy(
                attested,
                &ExpectedRelease {
                    repository_id: 1234,
                    workflow: bad,
                },
            )
            .unwrap_err();
            assert!(
                matches!(err, Error::UntrustedWorkflow { .. }),
                "expected UntrustedWorkflow for {bad}, got {err:?}"
            );
        }
    }

    #[test]
    fn pin_allows_same_repo_and_workflow_path_across_release_tags_and_renames() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = check_release_policy(
            sample_attested(1001, "alice/mod-a", SUNRISE_WORKFLOW_V1),
            &ExpectedRelease {
                repository_id: 1001,
                workflow: SUNRISE_WORKFLOW_V1,
            },
        )
        .unwrap();
        assert_eq!(
            check_pin_in(dir.path(), "Ab3dE9xQ", &v1.pin()).unwrap(),
            PinStatus::FirstInstall
        );
        assert_eq!(
            pin_or_check_in(dir.path(), "Ab3dE9xQ", &v1.pin()).unwrap(),
            PinStatus::FirstInstall
        );

        // Newer release-v2 tag on the same workflow path is not a publisher change.
        let v2 = check_release_policy(
            sample_attested(1001, "alice/mod-a", SUNRISE_WORKFLOW_V2),
            &ExpectedRelease {
                repository_id: 1001,
                workflow: SUNRISE_WORKFLOW_V2,
            },
        )
        .unwrap();
        assert_eq!(
            check_pin_in(dir.path(), "Ab3dE9xQ", &v2.pin()).unwrap(),
            PinStatus::Matched
        );
        assert_eq!(
            pin_or_check_in(dir.path(), "Ab3dE9xQ", &v2.pin()).unwrap(),
            PinStatus::Matched
        );

        // Repository rename with the same numeric repository_id is not a publisher change.
        let renamed = check_release_policy(
            sample_attested(1001, "alice/mod-a-renamed", SUNRISE_WORKFLOW_V2),
            &ExpectedRelease {
                repository_id: 1001,
                workflow: SUNRISE_WORKFLOW_V2,
            },
        )
        .unwrap();
        assert_eq!(
            pin_or_check_in(dir.path(), "Ab3dE9xQ", &renamed.pin()).unwrap(),
            PinStatus::Matched
        );
        let pins = load_pins_in(dir.path()).unwrap();
        assert_eq!(pins["Ab3dE9xQ"].repository_name, "alice/mod-a-renamed");

        // Another project is pinned independently.
        let other = PublisherPin {
            repository_id: 2002,
            repository_name: "bob/mod-b".to_owned(),
            workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
        };
        assert_eq!(
            pin_or_check_in(dir.path(), "Zz91kLmN", &other).unwrap(),
            PinStatus::FirstInstall
        );
        assert_eq!(load_pins_in(dir.path()).unwrap().len(), 2);
    }

    #[test]
    fn pin_rejects_changed_repository_or_workflow_path_until_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let initial = PublisherPin {
            repository_id: 1001,
            repository_name: "alice/mod-a".to_owned(),
            workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
        };
        assert_eq!(
            pin_or_check_in(dir.path(), "Ab3dE9xQ", &initial).unwrap(),
            PinStatus::FirstInstall
        );

        // Different repository ID triggers PublisherChanged without overwriting disk.
        let moved_repo = PublisherPin {
            repository_id: 2002,
            repository_name: "bob/mod-a-fork".to_owned(),
            workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
        };
        let err = pin_or_check_in(dir.path(), "Ab3dE9xQ", &moved_repo).unwrap_err();
        let change = err
            .publisher_change()
            .expect("must return PublisherChanged");
        assert_eq!(change.project_id, "Ab3dE9xQ");
        assert!(change.repository_changed());
        assert!(!change.workflow_changed());
        assert_eq!(change.pinned, initial);
        assert_eq!(change.incoming, moved_repo);
        assert!(err.to_string().contains("alice/mod-a"));
        assert!(err.to_string().contains("bob/mod-a-fork"));
        assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], initial);

        // Different workflow path triggers PublisherChanged without overwriting disk.
        let moved_workflow = PublisherPin {
            repository_id: 1001,
            repository_name: "alice/mod-a".to_owned(),
            workflow_path: "sunrise-org/sunrise-mods/.github/workflows/release-v2.yml".to_owned(),
        };
        let err = check_pin_in(dir.path(), "Ab3dE9xQ", &moved_workflow).unwrap_err();
        let change = err
            .publisher_change()
            .expect("must return PublisherChanged");
        assert!(!change.repository_changed());
        assert!(change.workflow_changed());
        assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], initial);

        // Explicit acceptance updates the pin; subsequent installs match it.
        accept_pin_in(dir.path(), "Ab3dE9xQ", &moved_repo).unwrap();
        assert_eq!(
            pin_or_check_in(dir.path(), "Ab3dE9xQ", &moved_repo).unwrap(),
            PinStatus::Matched
        );
        assert_eq!(load_pins_in(dir.path()).unwrap()["Ab3dE9xQ"], moved_repo);
    }

    #[test]
    fn corrupt_or_unsupported_pin_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let pin = PublisherPin {
            repository_id: 1001,
            repository_name: "alice/mod-a".to_owned(),
            workflow_path: RELEASE_WORKFLOW_PATH.to_owned(),
        };

        fs::write(dir.path().join(PINS_FILE), b"{corrupt").unwrap();
        assert!(matches!(
            check_pin_in(dir.path(), "Ab3dE9xQ", &pin),
            Err(Error::PinStore(_))
        ));
        assert!(matches!(
            pin_or_check_in(dir.path(), "Ab3dE9xQ", &pin),
            Err(Error::PinStore(_))
        ));

        fs::write(
            dir.path().join(PINS_FILE),
            br#"{"schema": 99, "projects": {}}"#,
        )
        .unwrap();
        assert!(matches!(load_pins_in(dir.path()), Err(Error::PinStore(_))));
    }
}
