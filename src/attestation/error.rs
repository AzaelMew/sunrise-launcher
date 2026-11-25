//! What can go wrong verifying a release or keeping its publisher pin.

use super::*;

#[derive(Debug)]
pub enum Error {
    /// Offline Sigstore bundle verification failed.
    Verify(Box<sunrise_attestation::Error>),
    /// The attested repository ID did not match the repository the release was signed from.
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
    /// The release's owner (or, for an old pin, repository) or workflow path differs from what
    /// this computer remembered for this project. The UI turns this into a confirmation prompt.
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
                "The publisher for this mod changed from {old_repo} (repository {old_id}, {old_workflow}) to {new_repo} (repository {new_id}, {new_workflow}).",
                &[
                    ("old_repo", &change.pinned.repository_name),
                    ("old_id", &change.pinned.repository_id.to_string()),
                    ("old_workflow", &change.pinned.workflow_path),
                    ("new_repo", &change.incoming.repository_name),
                    ("new_id", &change.incoming.repository_id.to_string()),
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
