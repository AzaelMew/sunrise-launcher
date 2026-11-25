//! Sunrise's release policy: which workflow may sign a registry release, and the offline check of its attestation.

use super::*;

/// The shared Sunrise release workflow path trusted for registry releases.
pub const RELEASE_WORKFLOW_PATH: &str = "sunrise-org/sunrise-mods/.github/workflows/release.yml";
/// Required git ref prefix on [`RELEASE_WORKFLOW_PATH`] (`refs/tags/release-v<number>`).
pub const RELEASE_WORKFLOW_REF_PREFIX: &str = "refs/tags/release-v";

/// What a v2 registry release claims its attestation proves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpectedRelease<'a> {
    /// Stable numeric GitHub repository ID the release was signed from
    /// (`Project::release_repository`).
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
            owner_id: Some(self.attested.owner_id),
            repository_id: self.attested.repository_id,
            repository_name: self.attested.repository_name.clone(),
            workflow_path: self.workflow_path.clone(),
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
    for (slot, pair) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
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
