//! The Mods tab's build card and the per-mod route chips (docs/modding/src/how-to/native-mods.md).
//! The card picks the build mode, shows where a build is (a percentage, one pill per stage) and
//! cancels it. A chip on each native mod says how it reaches the game; its menu gives the reason
//! and the actions.
use super::*;
use crate::glass_ui as ui;
use native_mods::{Profile};
use native_status::State;

/// The running or finished native snapshot; serialize readers to avoid racing cache leases.
pub(super) struct RouteMemo {
    key: String,
    result: Option<Result<native_mods::Routes, String>>,
}

/// The update check for one instance and newer game, while it runs (`None`) and once it ended.
pub(super) struct UpdateMemo {
    key: String,
    result: Option<Result<native_mods::UpdateCheck, String>>,
}

/// What a mod in the list is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModKind {
    /// Rust compiled into the game.
    Native,
    /// A sandboxed `mod.wasm`: never needs a build.
    Wasm,
    /// Data and assets only.
    Content,
}

impl ModKind {
    pub(super) fn of(manifest: &sunrise_launcher_smod::manifest::Manifest) -> Self {
        if manifest.native.is_some() {
            Self::Native
        } else if manifest.wasm.is_some() {
            Self::Wasm
        } else {
            Self::Content
        }
    }

    /// What the badge means, for its tooltip.
    pub(super) fn hint(self) -> &'static str {
        match self {
            Self::Native => tr("Has its own program code. It runs inside your game and is built on this computer, so it needs your permission first."),
            Self::Wasm => tr("Has its own program code, run in a protected sandbox. It never needs a build."),
            Self::Content => tr("Only data and pictures, no program code. It never needs a build."),
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Native => tr("Native"),
            Self::Wasm => tr("WebAssembly"),
            Self::Content => tr("Content"),
        }
    }
}

/// The colours a card or chip draws with, in either look.
#[derive(Clone, Copy)]
struct Pal {
    ink: Hsla,
    muted: Hsla,
    faint: Hsla,
    accent: Hsla,
    success: Hsla,
    warn: Hsla,
    danger: Hsla,
}

impl Pal {
    fn of(t: &Theme) -> Self {
        if t.glass {
            Self {
                ink: ui::white(1.),
                muted: ui::white(0.6),
                faint: ui::white(0.07),
                accent: t.accent,
                success: ui::live(),
                warn: ui::warn(),
                danger: ui::danger(),
            }
        } else {
            Self {
                ink: t.ink,
                muted: t.muted,
                faint: t.well,
                accent: t.accent,
                success: t.success,
                warn: t.warn,
                danger: t.danger,
            }
        }
    }
}

/// What a native mod's chip and menu say about how it reaches the game.
pub(super) struct RouteFacts {
    glyph: &'static str,
    color: Hsla,
    headline: SharedString,
    reason: String,
}

impl RouteFacts {
    pub(super) fn color(&self) -> Hsla {
        self.color
    }
}

impl Launcher {

    /// A build stopped for worlds at stake: the choice is shown, and the mods that failed to
    /// compile stay marked past it.
    pub(super) fn keep_stop(&mut self, stake: &native_mods::AtStake) {
        self.native_worlds = Some(stake.clone());
        self.mods.unbuilt = self
            .native_for
            .clone()
            .map(|instance| (instance, stake.unbuilt.clone()));
    }

    /// The mods the selected instance's native build left out, as the build recorded them.
    pub(super) fn native_left_out(&self) -> Vec<native_mods::LeftOut> {
        if self.mod_preview.active {
            return self.mod_preview.left_out.clone();
        }
        let Some(instance) = self.selected_instance() else {
            return Vec::new();
        };
        let Some(version) = self.version_of(instance) else {
            return Vec::new();
        };
        match self.native_build_of(version) {
            Some(build) => build.left_out,
            // A game that needed no build because every mod with native code was left out.
            None => native_mods::kept_left_out(&instance.home, version),
        }
    }

    /// Where the instance's native mods stand, read again only when what it depends on changes.
    pub(super) fn native_state_of(&self, instance: &instances::Instance) -> State {
        if let Some(state) = self.mod_preview.state.filter(|_| self.mod_preview.active) {
            return state;
        }
        let version = self.version_of(instance).cloned();
        let home = instance.home.clone();
        let profile = Profile::Optimized;
        let key = format!(
            "{}{profile:?}",
            native_status::fingerprint(&home, version.as_deref())
        );
        let mut memo = self.mods.native_state.borrow_mut();
        match memo.as_ref() {
            Some((k, state, _)) if *k == key => *state,
            _ => {
                let built = version.as_deref().and_then(native_status::built);
                let enabled = native_status::enabled(&home);
                let state = native_status::in_mode(
                    native_status::state(&enabled, built.as_deref()),
                    version.as_deref().and_then(native_status::built_profile),
                    profile,
                );
                // A build in another mode rebuilds every mod; otherwise only the changed ones.
                let unbuilt = match state {
                    native_status::State::NotBuilt => enabled.into_iter().map(|m| m.0).collect(),
                    _ => native_status::unbuilt(&enabled, built.as_deref()),
                };
                *memo = Some((key, state, unbuilt));
                state
            }
        }
    }

    /// Whether native mod `id` of the selected instance waits for a build ([`Self::native_state_of`]).
    pub(super) fn native_mod_unbuilt(&self, id: &str) -> bool {
        if let Some(instance) = self.selected_instance() {
            self.native_state_of(instance);
        }
        self.mods
            .native_state
            .borrow()
            .as_ref()
            .is_some_and(|(_, _, unbuilt)| unbuilt.iter().any(|u| u == id))
    }

    /// A bordered panel, as the build card and its warning sit on the page.
    pub(super) fn panel(&self) -> Div {
        let t = self.theme;
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(16.))
            .when(t.glass, |d| {
                d.rounded(px(ui::rad(sunrise_launcher_theme::glass::radius::LARGE)))
                    .bg(ui::white(0.04))
            })
            .when(!t.glass, |d| {
                d.bg(t.well).border_2().border_color(t.outline)
            })
    }

}

/// Text at `size` px in `color`, in the look's face.
fn text_in(size: f32, color: Hsla) -> Div {
    div()
        .font_family(sunrise_launcher_theme::glass::FONT)
        .text_size(px(size))
        .line_height(px((size * 1.4).round()))
        .text_color(color)
}
