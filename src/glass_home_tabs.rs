//! Home's sections (installed mods, worlds, saved servers, packs, shaders, screenshots, the log)
//! as a rail of buttons at the first screen's right edge. A button slides its section in beside
//! the rail, over Home, so nothing is a scroll away; the same button or the panel's close puts it
//! away again.
use super::*;
use crate::glass_ui::{self as ui, mono};
use home_tabs::HomeTab;
use sunrise_launcher::packs::Kind as PackKind;
use sunrise_theme::glass::{ink, radius};

/// The rail's buttons are this tall and wide.
const BUTTON: f32 = 44.;
/// The open section's panel is this wide where the window has the room, and never narrower
/// than [`PANEL_MIN`].
const PANEL: f32 = 520.;
const PANEL_MIN: f32 = 320.;
/// How far the panel slides in from.
const SLIDE: f32 = 16.;

impl Launcher {
    /// The rail and, while a section is open, its panel, laid over Home's first screen. The rail
    /// sits in the middle of the right column `column` px wide, or near the edge when there is
    /// no column.
    pub(super) fn home_rail(&mut self, column: f32, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let tabs: Vec<_> = self
            .home_tab_list(cx)
            .into_iter()
            .filter(|(tab, _)| *tab != HomeTab::Overview)
            .collect();
        let open = self
            .home_panel
            .then_some(self.home_tab)
            .filter(|tab| tabs.iter().any(|(t, _)| t == tab));
        let edge = if column > 0. {
            (column - BUTTON) / 2.
        } else {
            12.
        };
        let buttons = tabs.into_iter().map(|(tab, count)| {
            let (name, icon) = section(tab);
            let button = ui::icon_button(
                format!("home-rail-{tab:?}"),
                icon,
                BUTTON,
                20.,
                radius::CONTROL,
                open == Some(tab),
                &t,
            )
            .tooltip(ui::tip(name))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                let again = this.home_panel && this.home_tab == tab;
                this.home_tab = tab;
                this.home_panel = !again;
                cx.notify();
            }));
            let badge = count.filter(|n| *n > 0).map(|n| {
                mono(10., ink::DIM)
                    .absolute()
                    .right(px(3.))
                    .bottom(px(2.))
                    .child(n.to_string())
            });
            div().relative().child(button).children(badge)
        });
        let rail = div()
            .absolute()
            .top_0()
            .bottom_0()
            .right(px(edge))
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(4.))
            .children(buttons);
        let panel = open.map(|tab| self.home_panel_view(tab, 2. * edge + BUTTON, cx));
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(-theme::PAGE_GUTTER))
            .right(px(-theme::PAGE_GUTTER))
            .child(rail)
            .children(panel)
    }

    /// The open section beside the rail, `right` px from the edge: its name and a close over its
    /// content, which scrolls on its own. It slides and fades in.
    fn home_panel_view(&mut self, tab: HomeTab, right: f32, cx: &mut Context<Self>) -> AnyElement {
        let t = self.theme;
        let (name, _) = section(tab);
        let close = ui::icon_button(
            "home-panel-close",
            "cancel-01",
            32.,
            16.,
            radius::CONTROL,
            false,
            &t,
        )
        .tooltip(ui::tip(tr("Close")))
        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
            this.home_panel = false;
            cx.notify();
        }));
        let room = self.window_width - frame::SIDEBAR - right - 2. * theme::PAGE_GUTTER;
        let width = PANEL.min(room).max(PANEL_MIN);
        let body = self.section_body(tab, cx);
        crate::glass::float(0.)
            .absolute()
            .top(px(16.))
            .bottom(px(16.))
            .right(px(right))
            .w(px(width))
            .border_1()
            .border_color(ui::line())
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .h(px(52.))
                    .pl(px(16.))
                    .pr(px(10.))
                    .border_b_1()
                    .border_color(ui::line())
                    .child(ui::text(15., FontWeight::SEMIBOLD, ink::FULL).child(name))
                    .child(close),
            )
            .child(
                div()
                    .id("home-panel-body")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap(px(12.))
                    .p(px(16.))
                    .overflow_y_scroll()
                    .child(body),
            )
            .with_animation(
                ElementId::Name(format!("home-panel-{tab:?}").into()),
                crate::motion::eased(crate::motion::POP),
                move |d, p| d.opacity(p).right(px(right - SLIDE * (1. - p))),
            )
            .into_any_element()
    }

    /// A section's content, or one line where it has none yet.
    fn section_body(&mut self, tab: HomeTab, cx: &mut Context<Self>) -> AnyElement {
        let content = match tab {
            HomeTab::Overview => None,
            HomeTab::Mods => Some(self.glass_instance_mods(cx).into_any_element()),
            HomeTab::Worlds => self.glass_worlds(cx).map(IntoElement::into_any_element),
            HomeTab::Multiplayer => self
                .glass_multiplayer(cx)
                .map(IntoElement::into_any_element),
            HomeTab::Packs => self
                .glass_packs(PackKind::Resource, cx)
                .map(IntoElement::into_any_element),
            HomeTab::Shaders => self
                .glass_packs(PackKind::Shader, cx)
                .map(IntoElement::into_any_element),
            HomeTab::Screenshots => self
                .glass_screenshots(cx)
                .map(IntoElement::into_any_element),
            HomeTab::Console => self.glass_console(cx).map(IntoElement::into_any_element),
        };
        content.unwrap_or_else(|| {
            empty_tab(match tab {
                HomeTab::Screenshots => tr("No screenshots yet. Press F2 in the game."),
                _ => tr("Nothing here yet."),
            })
        })
    }
}

/// A section's name and its rail icon.
fn section(tab: HomeTab) -> (&'static str, &'static str) {
    match tab {
        HomeTab::Overview => (tr("Overview"), "dashboard-square-01"),
        HomeTab::Mods => (tr("Installed mods"), "puzzle"),
        HomeTab::Packs => (tr("Resource packs"), "image-01"),
        HomeTab::Shaders => (tr("Shaders"), "flash"),
        HomeTab::Worlds => (tr("Worlds"), "earth"),
        HomeTab::Multiplayer => (tr("Saved servers"), "user-multiple-02"),
        HomeTab::Screenshots => (tr("Screenshots"), "view"),
        HomeTab::Console => (tr("Log"), "computer-terminal-01"),
    }
}

/// A release's changes: each section's name and its lines.
pub(super) type Sections = Vec<(String, Vec<String>)>;

/// An empty tab's one line.
fn empty_tab(line: &str) -> AnyElement {
    ui::empty().child(line.to_string()).into_any_element()
}

/// What a release tells a player. A release body that has a `### For players` section is
/// written for them: only that section's bullets show. Any other body is the maintainers' list
/// of changes: it shows as a one-line summary with the list behind "Technical details".
pub(super) enum PatchNotes {
    ForPlayers(Vec<String>),
    Technical(Sections),
}

pub(super) fn patch_notes(body: &str) -> Option<PatchNotes> {
    let mut sections = patch_sections(body);
    if let Some(i) = sections
        .iter()
        .position(|(name, _)| name.eq_ignore_ascii_case("For players"))
    {
        return Some(PatchNotes::ForPlayers(sections.swap_remove(i).1));
    }
    (!sections.is_empty()).then_some(PatchNotes::Technical(sections))
}

/// A release's changes by section ("### New", "### Fixed"), each line without its pull request
/// numbers; the contributors' thanks and what follows the notes are left out.
pub(super) fn patch_sections(body: &str) -> Sections {
    let mut out: Sections = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("### ") {
            out.push((name.trim().to_string(), Vec::new()));
        } else if line == "---" || line.starts_with("Your launcher updates itself") {
            break;
        } else if let (Some(item), Some((section, items))) =
            (line.strip_prefix("- "), out.last_mut())
        {
            if section.as_str() != "Contributors" {
                items.push(strip_refs(item));
            }
        }
    }
    out.retain(|(name, items)| name != "Contributors" && !items.is_empty());
    out
}

/// "Fix the picker (#6389, fixes #6342)" → "Fix the picker".
fn strip_refs(item: &str) -> String {
    let mut s = item.trim();
    while let Some(open) = s.rfind(" (") {
        let tail = &s[open..];
        if tail.ends_with(')') && tail.contains('#') {
            s = s[..open].trim_end();
        } else {
            break;
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_notes_win_over_the_technical_list() {
        let body = "Intro.\n\n### For players\n- Mods load faster (#1)\n- Fixed a crash on the title screen\n\n### Fixed\n- Rollup 3: #4, #5 stages (#6)\n\nThanks to @a.";
        match patch_notes(body) {
            Some(PatchNotes::ForPlayers(items)) => assert_eq!(
                items,
                vec!["Mods load faster", "Fixed a crash on the title screen"]
            ),
            _ => panic!("expected the player section"),
        }
    }

    #[test]
    fn a_body_without_player_notes_keeps_its_list_as_technical() {
        let body = "A long intro paragraph.\n\n### Fixed\n- Sync upstream to 7b42678bcd (#2922)\n- Make CI failures readable (#2993)\n\nThanks to @arismoko.\n\n---\nLegal.";
        match patch_notes(body) {
            Some(PatchNotes::Technical(sections)) => {
                assert_eq!(sections.len(), 1);
                assert_eq!(sections[0].1.len(), 2);
            }
            _ => panic!("expected a technical list"),
        }
        assert!(patch_notes("Only an intro.").is_none());
    }

    #[test]
    fn a_release_body_becomes_its_sections_without_refs_or_thanks() {
        let body = "### New\n- Native mods: export a headless engine (#6462)\n\n### Fixed\n- Play with friends: any port (#6363, fixes #6342)\n- Keep (this) aside\n\n### Contributors\nThanks to @a.\n\nYour launcher updates itself. New here?\n\n---\nNot an official Minecraft product.";
        assert_eq!(
            patch_sections(body),
            vec![
                (
                    "New".to_string(),
                    vec!["Native mods: export a headless engine".to_string()]
                ),
                (
                    "Fixed".to_string(),
                    vec![
                        "Play with friends: any port".to_string(),
                        "Keep (this) aside".to_string()
                    ]
                ),
            ]
        );
    }
}
