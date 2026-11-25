//! What a mod-browser install or switch acts on.
//!
//! The browser plans, stages and publishes inside a profile, but it does not read the
//! launcher's instance list or its profile transaction bookkeeping. The UI captures one
//! [`Target`] when the player asks for work and passes it down; the target carries the exact
//! filesystem identity that was admitted, so every later step can prove it is still acting on
//! the same folder. Choices the player made about an instance (its channel, its build) are
//! written back through the target instead of the browser editing `instances.json` itself.
use super::CommittedBuildChoice;
use crate::{
    i18n::trf,
    instances::{self, Channel, Instance},
    native_mods,
    profile_txn::Identity,
};
use std::path::{Path, PathBuf};

/// Why the profile a browser operation was queued for can no longer be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TargetError(String);

impl std::fmt::Display for TargetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for TargetError {}

impl From<String> for TargetError {
    fn from(detail: String) -> Self {
        Self(detail)
    }
}

impl From<TargetError> for String {
    fn from(error: TargetError) -> Self {
        error.0
    }
}

/// One exact, admitted profile, as captured before a queued operation started.
#[derive(Clone, Debug)]
pub struct Target {
    id: String,
    home: PathBuf,
    identity: Identity,
    native_profile: native_mods::Profile,
    is_pack: bool,
    channel: Channel,
}

impl Target {
    /// Capture the exact profile an operation is for. The initial default profile may not have a
    /// game directory until its first operation; never recreate a named profile here.
    pub fn of(instance: &Instance) -> Result<Self, TargetError> {
        Ok(Self::with_identity(instance, instance.admit()?))
    }

    fn with_identity(instance: &Instance, identity: Identity) -> Self {
        Self {
            id: instance.id.clone(),
            home: instance.home.clone(),
            identity,
            native_profile: instance.native_profile,
            is_pack: instance.kind == instances::Kind::Pack,
            channel: instance.channel,
        }
    }

    /// A target for a profile whose exact identity was admitted earlier, when the request was
    /// queued; no new admission happens here.
    pub fn resumed(instance: &Instance, identity: Identity) -> Self {
        Self::with_identity(instance, identity)
    }

    /// `sunrise`, an own instance's folder name, `port:<pack>` or `pack:<id>`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The game folder the profile plays in: saves, options.txt and packs.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The filesystem identity the profile was admitted at.
    pub fn identity(&self) -> Identity {
        self.identity
    }

    /// Its saved native build choice, or Optimized before the player chooses one.
    pub fn native_profile(&self) -> native_mods::Profile {
        self.native_profile
    }

    /// A pack controls which mods it installs; the browser never edits one directly.
    pub fn is_pack(&self) -> bool {
        self.is_pack
    }

    /// Whether the profile follows stable releases, which fit checks treat as the safest.
    pub fn is_stable(&self) -> bool {
        self.channel == Channel::Stable
    }

    /// Persist the picker's channel/pin choice to `instances.json` when the active build itself
    /// does not need to change.
    pub fn save_build_choice(&self, tag: &str, choice: BuildChoice) -> Result<(), TargetError> {
        instances::edit(&self.id, |meta| match choice {
            BuildChoice::Follow(channel) => {
                meta.version = None;
                meta.channel = Some(channel);
            }
            BuildChoice::Keep => {
                meta.version = Some(tag.into());
                if let Some(channel) = Channel::of_build(tag) {
                    meta.channel = Some(channel);
                }
            }
        })
        .map(|_| ())
        .map_err(|error| {
            TargetError(trf(
                "The build is ready, but the profile choice could not be saved: {error}",
                &[("error", &error)],
            ))
        })
    }

    /// Write a switch's already-committed channel or pin choice back to `instances.json`.
    pub fn sync_build_choice(&self, committed: &CommittedBuildChoice) {
        if let Err(error) = instances::sync_build_choice(&self.id, &self.home, committed) {
            // The choice is already durable in active-build.json; listing or editing the
            // profile recovers instances.json on the next write.
            eprintln!("Sunrise: instances.json is not saved: {error}");
        }
    }
}

/// What the picker is asking the profile to follow after the switch.
#[derive(Clone, Copy)]
pub enum BuildChoice {
    Follow(Channel),
    Keep,
}

impl BuildChoice {
    /// The durable channel or pin this choice records once the switch publishes `tag`.
    pub fn commit(self, tag: &str) -> Result<CommittedBuildChoice, TargetError> {
        let (channel, version) = match self {
            Self::Follow(channel) => (Some(channel), None),
            Self::Keep => (Channel::of_build(tag), Some(tag.into())),
        };
        CommittedBuildChoice::new(channel, version).map_err(TargetError::from)
    }
}
