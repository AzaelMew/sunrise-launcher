//! What Play says and does, decided once: the profile header on Home and the status strip on
//! every other page both draw [`View`], so neither can show a state the other lacks.
//!
//! [`Facts`] is what the launcher knows (a download under way, a build's error, a held update),
//! read by [`Launcher::launch_facts`]; [`Facts::view`] is the whole table, with nothing drawn and
//! nothing read, so every row of it is tested. The launch itself is `play_game`'s, which owns
//! the sign-in, install, prepare, build and trust gates.
use super::*;
use crate::sheets::{Sheet, WayOut};

/// What the launcher knows that Play depends on.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Facts {
    /// A game build downloading, and how far it is (0 to 1).
    pub installing: Option<f32>,
    pub preparing: bool,
    /// Native mods are being built.
    pub building: bool,
    pub starting: bool,
    /// Mods are being added, removed or updated.
    pub mutating: bool,
    /// The game runs.
    pub running: bool,
    /// An account is selected, or the developer's offline Play is on.
    pub signed_in: bool,
    /// The selected profile has an installed build to play.
    pub installed: bool,
    /// The launcher's data is where it should be (`location_error` is none).
    pub located: bool,
    pub client_error: Option<String>,
    /// The game's own files were not found.
    pub prepare_error: Option<String>,
    pub native_error: Option<String>,
    pub install_error: Option<String>,
    pub crash: Option<String>,
    /// The build the profile stays on can't be built for any more (`build_gone`).
    pub build_gone: Option<instances::Channel>,
    /// The name of the build a held update would move to.
    pub held: Option<String>,
    /// The name of a newer build, for a profile that follows the newest Nightly.
    pub update: Option<String>,
    /// How many mods the game was made without.
    pub mods_off: usize,
}

/// Play itself: what it says, in the order the states win.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Main {
    Installing(f32),
    Preparing,
    Building,
    Starting,
    ChangingMods,
    /// The game runs: the button stops it.
    Stop,
    SignIn,
    Install,
    Play,
}

/// The one action beside Play: one at most, and one name for each.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Act {
    /// Stops the build of native mods.
    StopBuild,
    /// Opens the problem's whole text: "Fix…" with a way out of it, "Details…" with only more
    /// to read.
    Notice {
        text: String,
        way_out: Option<WayOut>,
    },
    /// "Fix…" for a held update: choose what to do with it.
    Resolve,
    /// "Update game".
    Update,
}

/// What the strip's one button does, off Home.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum StripAct {
    /// One of Play's own actions, under its own name.
    Act(Act),
    StopGame,
    /// Nothing can be done from here: Home has Play and the whole story.
    Home,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct View {
    pub main: Main,
    /// Whether Play can be pressed.
    pub enabled: bool,
    /// One sentence beside Play: a problem, a held update, a newer build.
    pub said: Option<String>,
    /// That sentence is a problem.
    pub failed: bool,
    pub act: Option<Act>,
    /// "2 mods not on this build": said only when nothing else is.
    pub mods_off: Option<usize>,
}

/// The first sentence of `text`, or its first line when that is shorter: what there is room
/// to say beside Play.
fn first_sentence(text: &str) -> &str {
    let line = text.trim().lines().next().unwrap_or_default();
    match line.find(". ") {
        Some(end) => &line[..=end],
        None => line,
    }
}

impl Facts {
    /// Something is under way that Play waits for (`Launcher::launch_busy`).
    pub(crate) fn busy(&self) -> bool {
        self.mutating
            || self.installing.is_some()
            || self.preparing
            || self.building
            || self.starting
    }

    pub(crate) fn view(&self) -> View {
        let busy = self.busy();
        let main = if let Some(progress) = self.installing {
            Main::Installing(progress)
        } else if self.preparing {
            Main::Preparing
        } else if self.building {
            Main::Building
        } else if self.starting {
            Main::Starting
        } else if self.mutating {
            Main::ChangingMods
        } else if self.running {
            Main::Stop
        } else if !self.signed_in {
            Main::SignIn
        } else if !self.installed {
            Main::Install
        } else {
            Main::Play
        };
        // A held update says so itself and its sheet carries the build's words: only then is
        // the build's error left to it.
        let problem = self
            .client_error
            .clone()
            .or_else(|| self.prepare_error.clone())
            .or_else(|| self.native_error.clone().filter(|_| self.held.is_none()))
            .or_else(|| self.install_error.clone())
            .or_else(|| self.crash.clone());
        // A problem is said in one sentence; the rest opens as a sheet.
        let brief = problem.as_deref().map(first_sentence);
        let more =
            brief.is_some_and(|b| b.len() < problem.as_deref().unwrap_or_default().trim().len());
        let hold = self
            .held
            .as_ref()
            .map(|to| trf("The update to {build} is on hold", &[("build", to)]));
        let update = self
            .update
            .as_ref()
            .filter(|_| self.installed && !busy && !self.running);
        let failed = brief.is_some() && !self.building;
        let said = brief
            .map(str::to_owned)
            .filter(|_| !self.building)
            .or_else(|| hold.clone())
            .or_else(|| update.map(|build| trf("{build} is available", &[("build", build)])));
        let way_out = if problem.is_none() {
            None
        } else if self.prepare_error.is_some() {
            Some(WayOut::MinecraftFolder)
        } else {
            self.build_gone.filter(|_| !busy).map(WayOut::Newest)
        };
        let act = if self.building {
            Some(Act::StopBuild)
        } else if let Some(text) = problem.filter(|_| way_out.is_some() || more) {
            Some(Act::Notice { text, way_out })
        } else if hold.is_some() && !busy {
            Some(Act::Resolve)
        } else {
            update.map(|_| Act::Update)
        };
        // Mods the game was made without are no problem to raise: one quiet line, at most.
        let mods_off =
            (said.is_none() && !self.building && self.mods_off > 0).then_some(self.mods_off);
        View {
            main,
            enabled: !busy && self.located,
            said,
            failed,
            act,
            mods_off,
        }
    }
}

impl Main {
    pub(crate) fn label(self) -> SharedString {
        match self {
            Main::Installing(progress) => trf(
                "Installing · {percent}%",
                &[("percent", &format!("{:.0}", 100. * progress))],
            )
            .into(),
            Main::Preparing => tr("Preparing…").into(),
            Main::Building => tr("Building mods…").into(),
            Main::Starting => tr("Starting…").into(),
            Main::ChangingMods => tr("Changing mods…").into(),
            Main::Stop => tr("Stop game").into(),
            Main::SignIn => tr("Sign in with Microsoft").into(),
            Main::Install => tr("Install and play").into(),
            Main::Play => tr("Play").into(),
        }
    }

    /// Something Play waits for, as the strip says it off Home.
    fn under_way(self) -> bool {
        matches!(
            self,
            Main::Installing(_)
                | Main::Preparing
                | Main::Building
                | Main::Starting
                | Main::ChangingMods
        )
    }
}

impl Act {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            Act::StopBuild => tr("Stop"),
            Act::Notice {
                way_out: Some(_), ..
            }
            | Act::Resolve => tr("Fix…"),
            Act::Notice { way_out: None, .. } => tr("Details…"),
            Act::Update => tr("Update game"),
        }
    }
}

impl StripAct {
    pub(crate) fn label(&self) -> &'static str {
        match self {
            StripAct::Act(act) => act.label(),
            StripAct::StopGame => tr("Stop"),
            StripAct::Home => tr("Go to Home"),
        }
    }
}

impl View {
    /// What the strip shows on a page other than Home, and its one action: work under way
    /// first, then what failed, then a held update, then a running game. None when there is
    /// nothing to tell (a newer build and mods left out wait on Home).
    pub(crate) fn strip(&self) -> Option<(SharedString, StripAct)> {
        if self.main.under_way() {
            let act = match &self.act {
                Some(Act::StopBuild) => StripAct::Act(Act::StopBuild),
                _ => StripAct::Home,
            };
            return Some((self.main.label(), act));
        }
        match (&self.said, &self.act) {
            (Some(said), Some(act @ Act::Notice { .. })) if self.failed => {
                Some((said.clone().into(), StripAct::Act(act.clone())))
            }
            (Some(said), _) if self.failed => Some((said.clone().into(), StripAct::Home)),
            (Some(said), Some(Act::Resolve)) => {
                Some((said.clone().into(), StripAct::Act(Act::Resolve)))
            }
            _ if self.main == Main::Stop => {
                Some((tr("The game is running").into(), StripAct::StopGame))
            }
            _ => None,
        }
    }
}

impl Launcher {
    /// The selected profile's channel, when its last build failed because the build it stays on
    /// has no mod tools any more and another build is the way on (`sdk_cache::way_on`).
    fn build_gone(&self) -> Option<instances::Channel> {
        let error = self.native_error.as_ref()?;
        let instance = self.selected_instance()?;
        let mine = self.native_for.as_deref() == Some(instance.id.as_str());
        (mine
            && instance.version.is_some()
            && native_mods::sdk_cache::way_on(error)
                == Some(native_mods::sdk_cache::WayOn::OtherBuild))
        .then_some(instance.channel)
    }

    pub(crate) fn launch_facts(&self) -> Facts {
        let active = self.selected_instance();
        let follows_nightly =
            active.is_some_and(|i| i.version.is_none() && i.channel == instances::Channel::Nightly);
        Facts {
            installing: self.installing.as_ref().map(|(_, progress)| *progress),
            preparing: self.preparing,
            building: self.native_step.is_some(),
            starting: self.starting_client.is_some(),
            mutating: self.mutating(),
            running: self.client.is_some(),
            signed_in: self.accounts.selected().is_some()
                || sunrise_launcher::developer_offline::selected(),
            installed: active.and_then(|i| self.version_of(i)).is_some(),
            located: self.location_error.is_none(),
            client_error: self.client_error.clone(),
            prepare_error: self.prepare_error.clone(),
            native_error: self.native_error.clone(),
            install_error: self.install_error.as_ref().map(|(_, e)| e.clone()),
            crash: self.crash.as_ref().map(|c| c.summary.clone()),
            build_gone: self.build_gone(),
            held: self.held_update().map(|h| h.to),
            update: self
                .updates
                .sunrise
                .as_ref()
                .filter(|_| follows_nightly)
                .map(|r| crate::sheets::build_name(&r.tag)),
            mods_off: self.mods_not_on_build(),
        }
    }

    /// Play was pressed: it stops a running game, else starts the selected profile.
    pub(crate) fn press_play(&mut self) {
        if let Some(client) = &self.client {
            client.kill();
        } else {
            self.quick_play = None;
            self.joining = None;
            self.play_game();
        }
    }

    pub(crate) fn launch_act(&mut self, act: &Act) {
        match act {
            Act::StopBuild => native_mods::cancel(),
            Act::Notice { text, way_out } => self.open_notice(text.clone(), *way_out),
            Act::Resolve => self.open_sheet(Sheet::Resolve),
            Act::Update => {
                let (Some(release), Some(instance)) =
                    (self.updates.sunrise.clone(), self.selected_instance())
                else {
                    return;
                };
                if self.native_state_of(instance) != native_status::State::NoMods {
                    // Never overwrite a working native build while staging its candidate.
                    self.instance_install = Some(release.tag.clone());
                    self.install(&release);
                } else {
                    self.install_update();
                }
            }
        }
    }

    pub(crate) fn strip_act(&mut self, act: &StripAct) {
        match act {
            StripAct::Act(act) => self.launch_act(act),
            StripAct::StopGame => {
                if let Some(client) = &self.client {
                    client.kill();
                }
            }
            StripAct::Home => self.navigate(crate::frame::Page::Home),
        }
    }
}

#[cfg(test)]
#[path = "launch_state/tests.rs"]
mod tests;
