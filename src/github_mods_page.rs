//! "Add from GitHub" on the Mods page: a link to a repository, release, tag, branch or commit,
//! resolved to an exact release file or commit (`sunrise_launcher::github_mods`), shown with the
//! plain warning and a link to the exact source, and installed only after the player says they
//! trust it. Mods installed that way list where they came from, with "Check for updates".
use super::line_edit::{ LineEdit};
use super::*;
use std::path::{Path, PathBuf};
use sunrise_launcher::github_mods::{self, Packed, Resolved};

enum Event {
    /// A link (or an update check for a mod id) resolved.
    Resolved(Result<Resolved, String>, Option<String>),
    /// The mods of a source, fetched and checked, when there is a choice to make.
    Fetched(Resolved, Result<Vec<Packed>, String>),
    Installed(Result<String, String>),
}

enum Stage {
    Idle,
    Working(String),
    /// The warning and the source link, before anything is downloaded. `only`: the mod an update
    /// is for.
    Review(Resolved, Option<String>),
    Pick(Resolved, Vec<Packed>),
}

pub(super) struct Github {
    text: LineEdit,
    focus: FocusHandle,
    focused: bool,
    stage: Stage,
    message: Option<Result<String, String>>,
    /// The mods installed from GitHub in `home`, and what the last update check found for each.
    home: Option<PathBuf>,
    newer: std::collections::HashMap<String, Resolved>,
    confirm_remove: Option<String>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Github {
    pub(super) fn new(cx: &mut App) -> Self {
        let (tx, rx) = crate::wake::channel();
        Self {
            text: LineEdit::new(String::new(), 300, 40),
            focus: cx.focus_handle().tab_index(0).tab_stop(true),
            focused: false,
            stage: Stage::Idle,
            message: None,
            home: None,
            newer: Default::default(),
            confirm_remove: None,
            tx,
            rx,
        }
    }

    pub(super) fn note_focus(&mut self, window: &Window) {
        self.focused = self.focus.is_focused(window);
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
                Event::Resolved(Err(e), _) => {
                    self.github.stage = Stage::Idle;
                    self.github.message = Some(Err(e));
                }
                Event::Resolved(Ok(found), None) => self.github.stage = Stage::Review(found, None),
                Event::Resolved(Ok(found), Some(id)) => {
                    self.github.stage = Stage::Idle;
                    let have = github_mods::origin(&self.mods.home(), &id);
                    if have
                        .as_ref()
                        .is_some_and(|o| github_mods::is_update(o, &found))
                    {
                        self.github.newer.insert(id, found);
                    } else {
                        self.github.message = Some(Ok(trf("{id} is up to date.", &[("id", &id)])));
                    }
                }
                Event::Fetched(found, Ok(mods)) => self.github.stage = Stage::Pick(found, mods),
                Event::Fetched(_, Err(e)) => {
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
        self.github.stage =
            Stage::Working(trf("Downloading {source}…", &[("source", &found.label())]));
        let tx = self.github.tx.clone();
        let home = self.mods.home();
        std::thread::spawn(move || {
            let fetched = github_mods::fetch(&found);
            let result = match fetched {
                Ok(mods) if mods.len() == 1 || only.is_some() => {
                    let done = finish(&home, &found, &mods, only.as_deref(), false);
                    github_mods::discard(&found);
                    Event::Installed(done)
                }
                other => Event::Fetched(found, other),
            };
            let _ = tx.send(result);
        });
    }

}
