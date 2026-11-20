//! "Add from GitHub" on the Mods page: a link to a repository, release, tag, branch or commit,
//! resolved to an exact release file or commit (`sunrise_launcher::github_mods`), shown with the
//! plain warning and a link to the exact source, and installed only after the player says they
//! trust it. Mods installed that way list where they came from, with "Check for updates".
use super::line_edit::LineEdit;
use super::*;
use std::path::{Path, PathBuf};
use sunrise_launcher::github_mods::{self, Packed, Prepared, Resolved};

enum Event {
    /// The mods of a source, fetched and checked, when there is a choice to make.
    Fetched(Result<Prepared, String>),
    Installed(Result<String, String>),
}

enum Stage {
    Idle,
    Working,
    /// The source holds several mods and nothing was asked for by name.
    Pick,
}

pub(super) struct Github {
    text: LineEdit,
    stage: Stage,
    message: Option<Result<String, String>>,
    /// The mods installed from GitHub in `home`, and what the last update check found for each.
    home: Option<PathBuf>,
    newer: std::collections::HashMap<String, Resolved>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Github {
    pub(super) fn busy(&self) -> bool {
        matches!(self.stage, Stage::Working)
    }

    pub(super) fn new() -> Self {
        let (tx, rx) = crate::wake::channel();
        Self {
            text: LineEdit::new(String::new(), 300, 40),
            stage: Stage::Idle,
            message: None,
            home: None,
            newer: Default::default(),
            tx,
            rx,
        }
    }

    fn reread(&mut self, home: &Path) {
        self.home = Some(home.to_owned());
    }
}

/// Installs the chosen mods of `mods` and says what was installed.
fn finish(
    home: &Path,
    found: &Resolved,
    mods: &[Packed],
    only: Option<&str>,
    all: bool,
) -> Result<String, String> {
    let done = github_mods::choose(mods, only, all)?
        .into_iter()
        .map(|p| github_mods::install(home, found, p))
        .collect::<Result<Vec<_>, _>>()?;
    let names: Vec<String> = done
        .iter()
        .map(|o| format!("{} {}", o.id, o.version))
        .collect();
    Ok(trf(
        "Installed {mods} from GitHub ({source}). Build it on the Mods page.",
        &[("mods", &names.join(", ")), ("source", &found.label())],
    ))
}

impl Launcher {
    pub(super) fn github_tick(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.github.rx.try_recv() {
            changed = true;
            match event {
                Event::Fetched(Ok(_)) => self.github.stage = Stage::Pick,
                Event::Fetched(Err(e)) => {
                    self.github.stage = Stage::Idle;
                    self.github.message = Some(Err(e));
                }
                Event::Installed(result) => {
                    self.github.stage = Stage::Idle;
                    if result.is_ok() {
                        self.github.text.set(String::new());
                        self.github.newer.clear();
                    }
                    self.instance_mods_changed();
                    self.github.message = Some(result);
                    let home = self.mods.home();
                    self.github.reread(&home);
                }
            }
        }
        changed
    }

    /// Downloads and checks what the player agreed to; installs it unless there is a choice.
    pub(super) fn github_install(&mut self, found: Resolved, only: Option<String>) {
        if self.settings.native_mods_allowed_at.is_none() {
            self.pending_consent = Some(PendingConsent::Github(found, only));
            return;
        }
        self.github.stage = Stage::Working;
        let tx = self.github.tx.clone();
        let home = self.mods.home();
        std::thread::spawn(move || {
            let result = match github_mods::fetch(found) {
                Ok(prepared) if prepared.mods().len() == 1 || only.is_some() => {
                    Event::Installed(finish(
                        &home,
                        prepared.source(),
                        prepared.mods(),
                        only.as_deref(),
                        false,
                    ))
                }
                other => Event::Fetched(other.map_err(String::from)),
            };
            let _ = tx.send(result);
        });
    }
}
