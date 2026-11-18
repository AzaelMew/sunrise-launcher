//! The selected instance's worlds on Home (`sunrise_launcher::worlds`): each with its picture,
//! mode, version, when it was last played and its size, and Back up, Copy, Delete (to the
//! trash, after a confirmation) and Open folder. Reading sizes and copying run in the background.
use super::*;
use std::path::{ PathBuf};
use sunrise_launcher::worlds::{self, World};

#[derive(Default)]
pub(super) struct Worlds {
    /// The game folder the list is of, and the list as last read: kept on screen while it is
    /// read again, so the page doesn't jump.
    pub(super) home: Option<PathBuf>,
    pub(super) list: Vec<World>,
    /// Which of the list's rows Home's glass Worlds tab has built and measured (`cull`).
    pub(super) cull: cull::Cull,
    fresh: bool,
    reading: bool,
    /// The world being backed up, copied or deleted now.
    pub(super) busy: Option<String>,
    pub(super) confirm_delete: Option<String>,
    /// What the last action did (a backup's zip), or why it failed.
    pub(super) done: Option<Result<(String, Option<PathBuf>), String>>,
}

impl Worlds {

    /// Read again on the next frame: the game may have saved, or an action changed the list.
    pub(super) fn stale(&mut self) {
        self.fresh = false;
    }
}

/// "today", "yesterday", "3 days ago", else the date.
pub(super) fn ago(when: std::time::SystemTime) -> String {
    let when: chrono::DateTime<chrono::Local> = when.into();
    let days = (chrono::Local::now().date_naive() - when.date_naive()).num_days();
    match days {
        ..=0 => tr("today").into(),
        1 => tr("yesterday").into(),
        2..=13 => trf("{days} days ago", &[("days", &days.to_string())]),
        _ => when.format("%Y-%m-%d").to_string(),
    }
}

impl Launcher {
    /// Reads the worlds of `home` in the background when the list is missing or of another
    /// instance.
    pub(super) fn load_worlds(&mut self, home: PathBuf, cx: &mut Context<Self>) {
        if self.worlds.home.as_ref() != Some(&home) {
            self.worlds = Worlds {
                home: Some(home.clone()),
                ..Worlds::default()
            };
        }
        if self.worlds.fresh || self.worlds.reading {
            return;
        }
        self.worlds.reading = true;
        let read = home.clone();
        let task = cx
            .background_executor()
            .spawn(async move { worlds::list(&read) });
        cx.spawn(async move |this, cx| {
            let list = task.await;
            let _ = this.update(cx, |this, cx| {
                // Another instance may have been picked meanwhile.
                if this.worlds.home.as_ref() == Some(&home) {
                    this.worlds.list = list;
                    this.worlds.cull.reset();
                    this.worlds.fresh = true;
                    this.worlds.reading = false;
                }
                cx.notify();
            });
        })
        .detach();
    }

}
