//! Home's Console tab: the game's output of this run as it comes, with a search, Follow (keep the
//! newest line in view), Copy, and Share log (`sunrise_launcher::game_log`): an upload to mclo.gs
//! that happens only after the player confirms, without their home folder or names.

use super::*;

/// Keeps a received line, dropping the oldest past `LOG_LINES`, and counts it in `revision`.
pub(super) fn keep_line(log: &mut VecDeque<String>, revision: &mut u64, line: String) {
    log.push_back(line);
    *revision += 1;
    if log.len() > LOG_LINES {
        log.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_sees_new_lines_after_the_buffer_is_full() {
        let mut launcher_revision = 0u64;
        let mut log = VecDeque::new();
        let mut last = (launcher_revision, log.len());
        for n in 0..LOG_LINES + 3 {
            keep_line(&mut log, &mut launcher_revision, n.to_string());
            let now = (launcher_revision, log.len());
            assert_ne!(now, last, "line {n} must change what follow compares");
            last = now;
        }
        assert_eq!(log.len(), LOG_LINES);
    }
}
