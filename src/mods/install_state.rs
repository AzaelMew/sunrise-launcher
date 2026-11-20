//! What a mod's card says about its install beyond fit and "Installed": typed, so the card
//! decides how to show it.
use sunrise_launcher::native_mods::BuildFailure;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallState {
    /// Another install runs; this one starts when it ends.
    Waiting,
    /// The mod failed to compile on this computer: it was removed again.
    BuildFailed,
    /// The game runs, so nothing installs: "Close the game to install".
    CloseGame,
}

/// The state of mod `id`: `waiting` is whether its install waits its turn, `failed` the mods
/// the last install's build removed because they didn't compile, `game_running` whether the
/// game runs.
pub(crate) fn of(
    id: &str,
    waiting: bool,
    failed: &[BuildFailure],
    game_running: bool,
) -> Option<InstallState> {
    if waiting {
        Some(InstallState::Waiting)
    } else if failed.iter().any(|f| f.id == id) {
        Some(InstallState::BuildFailed)
    } else {
        game_running.then_some(InstallState::CloseGame)
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
            of("broken", false, &failed, false),
            Some(InstallState::BuildFailed)
        );
        assert_eq!(
            of("broken", true, &failed, false),
            Some(InstallState::Waiting)
        );
        assert_eq!(of("fine", false, &failed, false), None);
        assert_eq!(of("fine", false, &[], false), None);
    }

    #[test]
    fn a_running_game_says_to_close_it_for_a_mod_with_nothing_else_to_say() {
        assert_eq!(of("a", false, &[], true), Some(InstallState::CloseGame));
        assert_eq!(of("a", true, &[], true), Some(InstallState::Waiting));
    }
}
