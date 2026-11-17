//! Parts of the launcher's window drawn as views of their own: what an [`Island`] draws is the
//! launcher's, built by the launcher's own method from its own state, but a change inside it (a
//! row's hover, a press) redraws the island alone instead of the whole launcher (a menu's frame
//! went from about 3 ms to well under 1). Any change to the launcher redraws it too: GPUI Fast
//! notes that the island read the launcher while it was built.
use super::*;
use gpui::{Entity, WeakEntity};

/// What an island draws, from the launcher; nothing while it is closed.
type Build = fn(&mut Launcher, &mut Window, &mut Context<Launcher>) -> Option<AnyElement>;

pub(crate) struct Island {
    launcher: WeakEntity<Launcher>,
    build: Build,
}

impl Render for Island {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let build = self.build;
        self.launcher
            .update(cx, |launcher, cx| build(launcher, window, cx))
            .ok()
            .flatten()
            // Closed: out of the flow, so it takes no gap either.
            .unwrap_or_else(|| div().absolute().into_any_element())
    }
}

/// The launcher's islands: its menus.
pub(crate) struct Islands {
    /// The glass sidebar's ⋮ menu (`frame.rs`).
    pub row_menu: Entity<Island>,
    /// The top bar's account menu (`frame.rs`).
    pub account_menu: Entity<Island>,
    /// Settings' language menu (`language_page.rs`).
    pub language_menu: Entity<Island>,
}

impl Islands {
    pub(crate) fn new(cx: &mut Context<Launcher>) -> Self {
        let launcher = cx.weak_entity();
        let mut island = |build: Build| {
            let launcher = launcher.clone();
            cx.new(|_| Island { launcher, build })
        };
        Islands {
            row_menu: island(|l, window, cx| {
                l.theme
                    .glass
                    .then(|| l.row_menu(window, cx))
                    .flatten()
                    .map(IntoElement::into_any_element)
            }),
            account_menu: island(|l, _, cx| {
                l.glass_account_menu(cx).map(IntoElement::into_any_element)
            }),
            language_menu: island(|l, _, cx| {
                (l.language_menu && l.theme.glass)
                    .then(|| l.glass_language_menu(cx).into_any_element())
            }),
        }
    }
}
