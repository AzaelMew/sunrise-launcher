//! Offline Sigstore attestation policy and per-computer publisher pinning for v2 registry mods.
//!
//! Cryptographic verification lives in `sunrise-attestation` (`crates/attestation`). This module
//! applies Sunrise's release policy:
//! - the attested repository ID equals the repository the release's own row says it was signed
//!   from (the project's `repository.id` when the row names none);
//! - the attested signer workflow equals the release's `workflow` field exactly;
//! - that workflow is `sunrise-org/sunrise-mods/.github/workflows/release.yml` at a
//!   `refs/tags/release-v<number>` tag.
//!
//! At a project's first install on this computer, the launcher remembers its publisher in
//! `publisher-pins.json` under the launcher data folder (outside disposable caches): the owner
//! account ID of the repository it was signed from and the workflow path. A newer
//! `release-v<number>` tag on the same workflow path, or a release from another repository of the
//! same owner (a project moved within one account), is accepted; a different owner or workflow
//! path returns [`Error::PublisherChanged`] so the UI can ask the player before accepting the new
//! publisher. A pin written before owner IDs were remembered gains its owner from the next release
//! verified from its own repository; until then a move asks once.

use crate::backend;
use crate::i18n::trf;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

pub use sunrise_attestation::{Attested, PUBLIC_GOOD_TRUSTED_ROOT};

mod error;
mod pins;
mod policy;
pub use error::*;
pub use pins::*;
pub use policy::*;

#[cfg(test)]
mod tests;
