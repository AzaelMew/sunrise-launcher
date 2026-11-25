use super::*;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use sunrise_launcher::{
    browser::sunrise::{
        install_target, save_build_choice, switch_profile_build_v2, switch_provider_v2,
        BuildChoice, InstallError, InstallHooks, InstallProgress, ProviderSwitchPreview,
        SwitchDecision, SwitchHooks, SwitchQuestion, Switched, V2Hooks, V2InstallOptions,
        V2ProviderSwitchHooks, V2SwitchHooks,
    },
    managed_play::{self, Active, FirstBuild, Profile, ReceiptHint},
    profile_txn::Identity,
};

mod build_switch;
mod provider_switch;
mod receipt;

/// A switch the player can start from another port's row: the in-game ID, and both publishers
/// as the player reads them. The backend re-reads the release from the signed index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PortSwitch {
    pub(crate) mod_id: String,
    pub(crate) mod_name: String,
    pub(crate) from_repo: String,
    pub(crate) from_version: String,
    pub(crate) to_project_id: String,
    pub(crate) to_repo: String,
    pub(crate) to_version: String,
}

#[derive(Default)]
pub(crate) struct State {
    pub(super) cache_download: Option<String>,
    running: Option<Running>,
    provider: Option<ProviderRunning>,
    ack: bool,
    receipt: Option<ReceiptRead>,
}

struct ProviderRunning {
    switch: PortSwitch,
    profile_name: String,
    build_label: String,
    progress: InstallProgress,
    eta: crate::eta::StageEta,
    stop: native_mods::Cancellation,
    question: Option<(Box<ProviderSwitchPreview>, Sender<bool>)>,
}

struct ReceiptRead {
    home: PathBuf,
    hint: Result<Option<ReceiptHint>, String>,
    ticket: Arc<()>,
    status: Load<Option<(Identity, Active)>>,
}

struct Running {
    tag: String,
    /// The profile and the build it played when the switch began, shown until the switch ends.
    from: (PathBuf, Option<String>),
    back: Option<Sheet>,
    pending: Option<Pending>,
    progress: InstallProgress,
    eta: crate::eta::StageEta,
    stop: native_mods::Cancellation,
    question: Option<(Box<SwitchQuestion>, Sender<SwitchDecision>)>,
}

/// The selected profile and build survive download and preparation, not a new read of Home.
struct Pending {
    instance: instances::Instance,
    identity: Identity,
    choice: Option<BuildChoice>,
}

#[cfg(test)]
mod tests;
