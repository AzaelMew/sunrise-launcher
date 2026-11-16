//! The right sidebar, the left one's mirror: the player's side of the top bar in its top cell,
//! then the selected instance's sections (installed mods, worlds, saved servers, packs, shaders,
//! screenshots, the log) as rows with their counts, and how many play at its foot. A row slides
//! its section in at Home's right edge; the same row or the panel's close puts it away. Narrow
//! windows keep the sections as an icon column and the player's side in the top bar.
use super::*;
use crate::glass_ui::{self as ui, mono, white};
use home_tabs::HomeTab;
use sunrise_launcher::packs::Kind as PackKind;
use sunrise_theme::glass::{ink, radius};

/// From this window width the right sidebar is as wide as the left; narrower, it is an icon
/// column [`COMPACT`] wide.
const FULL_FROM: f32 = 1180.;
pub(crate) const COMPACT: f32 = 64.;
/// The open section's panel is this wide where the page has the room, and never narrower
/// than [`PANEL_MIN`].
const PANEL: f32 = 520.;
const PANEL_MIN: f32 = 320.;
/// How far the panel slides in from.
const SLIDE: f32 = 16.;

impl Launcher {
    /// Whether the window is wide enough for both sidebars at full width (the right one then
    /// holds the player's side of the top bar); narrower, both fold to icon columns.
    pub(super) fn outline_wide(&self) -> bool {
        self.window_width >= FULL_FROM
    }

    /// How wide each sidebar is.
    pub(super) fn outline_side_width(&self) -> f32 {
        if self.outline_wide() {
            frame::SIDEBAR
        } else {
            COMPACT
        }
    }

    /// The right sidebar, `full` or folded to its icon column.
    pub(super) fn outline_right_sidebar(&mut self, full: bool, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let tabs: Vec<_> = self
            .home_tab_list(cx)
            .into_iter()
            .filter(|(tab, _)| *tab != HomeTab::Overview)
            .collect();
        let open = (self.page == Page::Play && self.home_panel).then_some(self.home_tab);
        let rows = tabs.into_iter().map(|(tab, count)| {
            let (name, icon) = section(tab);
            let on = open == Some(tab);
            let ink_of = white(if on { ink::FULL } else { ink::BODY });
            let row = ui::row(format!("side-section-{tab:?}"), on, 8., &t)
                .h(px(40.))
                .child(ui::icon(icon, 18., ink_of).relative());
            let row = if full {
                row.child(
                    ui::text(
                        14.,
                        FontWeight::MEDIUM,
                        if on { ink::FULL } else { ink::BODY },
                    )
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(name),
                )
                .children(
                    count
                        .filter(|n| *n > 0)
                        .map(|n| mono(12., ink::DIM).relative().child(n.to_string())),
                )
            } else {
                row.justify_center().tooltip(ui::tip(name))
            };
            row.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                let again = this.page == Page::Play && this.home_panel && this.home_tab == tab;
                if this.page != Page::Play {
                    this.go(Page::Play);
                }
                this.home_tab = tab;
                this.home_panel = !again;
                cx.notify();
            }))
        });
        let rows: Vec<_> = rows.collect();
        let line = |at_top: bool| {
            let d = div().absolute().left_0().right_0().h(px(1.));
            if at_top {
                d.top(px(70.)).bg(white(0.08))
            } else {
                d.bottom(px(24.)).bg(white(0.10))
            }
        };
        let width = if full { frame::SIDEBAR } else { COMPACT };
        // On a wide window the top cell holds the account alone: "Sign in with Microsoft" where it
        // fits (mono 12, uppercase, about 7.4 px a letter, beside the logo and padding), else
        // "Sign in". Narrower, the account stays in the top bar and Settings takes the cell.
        let label = tr("Sign in with Microsoft").chars().count() as f32;
        let short = 48. + 7.4 * label > width - 24. - 12.;
        let wide = self.outline_wide();
        let in_cell = if wide {
            self.outline_account(false, short, false, cx)
                .into_any_element()
        } else {
            self.outline_settings(cx).into_any_element()
        };
        let cell = div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(70.))
            .pt(px(14.))
            .flex()
            .items_center()
            .when(full, |d| d.pr(px(24.)).justify_end())
            .when(!full, |d| d.justify_center())
            .child(in_cell);
        // Folded, the label keeps its room unseen, so the rows don't jump as the sidebar opens.
        let label = div()
            .p(px(8.))
            .when(!full, |d| d.invisible())
            .child(mono(12., ink::QUIET).child(tr("This instance").to_uppercase()));
        let foot = full
            .then(|| self.outline_playing())
            .flatten()
            .map(|p| div().px(px(8.)).child(p));
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .gap(px(4.))
            .w(px(width))
            .h_full()
            .pt(px(82.))
            .px(px(if full { 12. } else { 10. }))
            .pb(px(32.))
            .border_l_1()
            .border_color(white(0.08))
            .child(ui::grain(
                ui::color(t.night.side),
                ui::TEXTURE_QUIET,
                width,
                self.window_height,
                self.scale,
            ))
            .child(line(true))
            .child(line(false))
            .child(cell)
            .child(label)
            .child(
                div()
                    .id("side-sections")
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows),
            )
            .child(div().flex_1())
            .children(foot)
    }

    /// The open section's panel at Home's right edge, while one is open.
    pub(super) fn home_section_panel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let open = self.home_panel.then_some(self.home_tab)?;
        (open != HomeTab::Overview).then(|| self.home_panel_view(open, 0., cx))
    }

    /// The open section at the page's right edge, `right` px in: its name and a close over its
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
        let room = self.outline_page_width() - right - 2. * theme::PAGE_GUTTER;
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

/// A section's name and its icon.
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
