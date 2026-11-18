//! Home in the night look (docs/design-system.md, "Night"): the selected instance's name over the
//! sky with what it plays and whether it is ready, Play and the actions beside it, and a card with
//! what's new and joining a friend.
use super::*;






/// `SUNRISE_LAUNCHER_PAGE=getting-ready` opens Home as if Play had been pressed on a version that
/// downloads from Mojang, a third of the way through: for screenshots, nothing runs.
pub(super) fn getting_ready_preview() -> bool {
    static PREVIEW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PREVIEW
        .get_or_init(|| std::env::var("SUNRISE_LAUNCHER_PAGE").as_deref() == Ok("getting-ready"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_run_in_their_order() {
        use backend::PrepareStep::*;
        assert_eq!(backend::PrepareStep::ALL, [Files, Java, Sounds, Prepare]);
        for s in backend::PrepareStep::ALL {
            assert_eq!(backend::PrepareStep::ALL[s as usize], s);
        }
    }
}
