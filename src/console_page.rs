//! Home's Console tab: the game's output of this run as it comes, with a search, Follow (keep the
//! newest line in view), Copy, and Share log (`sunrise_launcher::game_log`): an upload to mclo.gs
//! that happens only after the player confirms, without their home folder or names.

use super::*;


use sunrise_launcher::game_log::{ Shared};

pub(super) struct Console {
    follow: bool,
    /// Which lines show: every one, warnings and errors, or errors only.
    level: Level,
    /// The last run's `latest.log`, shown while this run has no lines.
    file: Option<FileLog>,
    /// A read of `file` is running off the UI thread.
    reading: bool,
    /// When `file` was last checked for a change.
    checked: Option<std::time::Instant>,
    scroll: Scrollbar,
    /// Revision, lines shown and search at the last draw: a change means scroll down when
    /// following.
    seen: (u64, usize, String),
    confirm_share: bool,
    sharing: bool,
    shared: Option<Result<Shared, String>>,
}

impl Default for Console {
    fn default() -> Self {
        Console {
            follow: true,
            level: Level::All,
            file: None,
            reading: false,
            checked: None,
            scroll: Scrollbar::default(),
            seen: (0, 0, String::new()),
            confirm_share: false,
            sharing: false,
            shared: None,
        }
    }
}

/// A file's length and modification time: a change in either means it is read again.
type Stamp = (u64, Option<std::time::SystemTime>);

/// What the last run's log file holds, as far as it is known.
#[derive(Debug, PartialEq)]
enum Tail {
    Loading,
    /// No file, or an empty one: the game has not logged yet.
    Absent,
    /// The file is there and could not be read, with the reason in plain words.
    Failed(String),
    Lines(Vec<String>),
}

struct FileLog {
    path: std::path::PathBuf,
    stamp: Option<Stamp>,
    tail: Tail,
}

/// The log's level filter (the mockup's All, Warnings, Errors).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Level {
    All,
    Warnings,
    Errors,
}

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
