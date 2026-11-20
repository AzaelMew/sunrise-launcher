//! What a mod's card says about its install beyond fit and "Installed": typed, so the card
//! decides how to show it.
use sunrise_launcher::native_mods::BuildFailure;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallState {
    /// Another install runs; this one starts when it ends.
    Waiting,
    /// The mod failed to compile on this computer: it was removed again.
    BuildFailed,
}

/// The state of mod `id`: `waiting` is whether its install waits its turn, `failed` the mods
/// the last install's build removed because they didn't compile.
pub(crate) fn of(id: &str, waiting: bool, failed: &[BuildFailure]) -> Option<InstallState> {
    if waiting {
        Some(InstallState::Waiting)
    } else {
        failed
            .iter()
            .any(|f| f.id == id)
            .then_some(InstallState::BuildFailed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mod_removed_for_not_compiling_reads_so_and_waiting_comes_first() {
        let failed = [BuildFailure {
            id: "broken".into(),
            reason: "r".into(),
            log: "l".into(),
        }];
        assert_eq!(
            of("broken", false, &failed),
            Some(InstallState::BuildFailed)
        );
        assert_eq!(of("broken", true, &failed), Some(InstallState::Waiting));
        assert_eq!(of("fine", false, &failed), None);
        assert_eq!(of("fine", false, &[]), None);
    }
}
