//! The Settings page in the glass look: the mockup's `SettingsView`. A list of the sections on
//! the left, and the sections as glass cards in a pane that scrolls on its own under the title;
//! the list follows the scroll and takes the pane to a section. The rows inside are the mockup's
//! `SettingRow`s ([`glass_ui::flat_rows`]).
//!
//! Not in the mockup, kept in its patterns: the appearance, the Home background, the data and
//! logs folders and the applications menu (About's rows), the AI clients and the hidden developer options. The mockup's About tiles for Sunrise and
//! the launcher open Updates here, beside the switches that decide how updates install. The mockup's Native mods switch is the
//! native mods' build profile here: they are always on.

use super::*;
use crate::glass_ui::{self as ui, white};
use sunrise_launcher_theme::glass::{fill, ink, radius};

/// The list's entries: name and icon. The settings sections, in the mockup's order.
/// The space between the pane's sections.
const SECTION_GAP: f32 = 16.;

/// Settings' pane draws only the sections near its view in full: laying out every card of every
/// section was most of an open Settings frame's work (~900 layout nodes, ~1,500 text
/// measurements). The others are the space they took when last drawn, so the scrollbar, the
/// sidebar's jumps and its marked section stay where they were. A section not yet measured at
/// this width is drawn out of view once, one a frame, until all are; meanwhile it takes
/// [`Culled::ESTIMATE`].
#[derive(Clone, Copy, Default)]
pub(super) struct Culled {
    /// The pane's width the sections were measured at.
    width: f32,
    /// One bit per section: measured at that width, drawn in full on the last frame, all.
    measured: u32,
    drawn: u32,
    all: u32,
}

impl Culled {
    const ESTIMATE: f32 = 360.;

    /// The sections to draw in full on the next frame, and the height to leave for each:
    /// `last` is what each took on the last frame, in full or as space.
    fn next(self, last: &[Option<Pixels>], pane: Pane, everything: bool) -> (Culled, Vec<Pixels>) {
        let mut culled = if pane.width == self.width {
            self
        } else {
            Culled {
                width: pane.width,
                ..Culled::default()
            }
        };
        let (mut top, mut drawn, mut measuring_one) = (px(0.), 0, false);
        let mut heights = Vec::with_capacity(last.len());
        for (i, last) in last.iter().enumerate() {
            let bit = 1 << i;
            let height = match last {
                Some(h) if culled.measured & bit != 0 => *h,
                _ => px(Self::ESTIMATE),
            };
            let near = top + height >= pane.scrolled - pane.margin
                && top <= pane.scrolled + pane.view + pane.margin;
            if everything || near {
                drawn |= bit;
            } else if culled.measured & bit == 0 && !measuring_one {
                // One section out of view is drawn to be measured, each frame until all are.
                measuring_one = true;
                drawn |= bit;
            }
            heights.push(height);
            top += height + px(SECTION_GAP);
        }
        culled.all = (1 << last.len()) - 1;
        culled.drawn = drawn;
        culled.measured |= drawn;
        (culled, heights)
    }
}

/// Where the Settings pane is: its width, the height it shows, how far beyond that sections are
/// drawn, and how far it is scrolled.
#[derive(Clone, Copy)]
struct Pane {
    width: f32,
    view: Pixels,
    margin: Pixels,
    scrolled: Pixels,
}

const SECTIONS: [(&str, &str); 10] = [
    ("General", "sliders-horizontal"),
    ("Appearance", "color-picker"),
    ("Accounts", "user"),
    ("Updates", "refresh"),
    ("Game", "game-controller-01"),
    ("Network", "globe-02"),
    ("Storage", "package-01"),
    ("Data & privacy", "view"),
    ("AI", "ai-chat-02"),
    ("About", "information-circle"),
];


/// What the search finds in each section, in [`SECTIONS`]' order: its rows' names and words a
/// player may look for them by. English; a query matches them and their translation.
const INDEX: [&[&str]; 10] = [
    &[
        "Language",
        "While playing",
        "Show the Sunrise logo at startup",
    ],
    &[
        "Theme",
        "Accent",
        "Glass",
        "Blur",
        "Performance mode",
        "Motion",
        "Corners",
        "Home background",
        "Reset the launcher's look",
        "Dark",
        "Light",
    ],
    &[
        "Microsoft account",
        "Sign in with Microsoft",
        "Sign out",
        "Play as",
        "Skin",
    ],
    &[
        "Install new versions automatically",
        "Delete old versions automatically",
        "Check now",
        "Nightly",
    ],
    &[
        "Game architecture",
        "Arguments",
        "Update mods automatically",
        "Memory",
    ],
    &["Proxy"],
    &["Storage", "Disk space", "Cache"],
    &["Data collection", "Telemetry", "Crash reports"],
    &["No AI", "Agents", "MCP"],
    &[
        "UI made by Xmilon",
        "Credits",
        "GitHub",
        "Version",
        "Data folder",
        "Logs",
        "Applications menu",
    ],
];

/// The settings `query` finds: (section, the row's name, or `None` when the section's own name
/// matched). Every word of the query must be in the name, in English or translated.
fn search(query: &str) -> Vec<(usize, Option<&'static str>)> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let finds = |en: &str| {
        let hay = format!("{en} {}", tr(en)).to_lowercase();
        words.iter().all(|w| hay.contains(w.as_str()))
    };
    let mut hits = Vec::new();
    for (i, &(label, _)) in SECTIONS.iter().enumerate() {
        if finds(label) {
            hits.push((i, None));
        }
        hits.extend(INDEX[i].iter().filter(|e| finds(e)).map(|&e| (i, Some(e))));
    }
    hits
}

/// A section's card (`hzl-glass rounded-[20px] p-2`).
fn card() -> Div {
    crate::glass::plate(radius::PANEL)
        .flex()
        .flex_col()
        .gap(px(2.))
        .p(px(6.))
}

/// Section `i`'s card, under its heading: the section's icon in the accent and its name.
fn section(i: usize) -> Div {
    let (label, icon) = SECTIONS[i];
    card().child(
        div()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(14.))
            .pt(px(12.))
            .pb(px(4.))
            .child(ui::icon(icon, 18., ui::color(ui::look().accent)))
            .child(
                ui::text(17., FontWeight::SEMIBOLD, ink::FULL)
                    .font_family(sunrise_launcher_theme::glass::DISPLAY_FONT)
                    .child(tr(label).to_string()),
            ),
    )
}

/// A tile (the mockup's `AboutTile`, `rounded-[14px] bg-white/[0.045] p-5`): its lead, name, a
/// line under it and what it does.
fn tile(lead: AnyElement, title: String, note: String, action: AnyElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(140.))
        .rounded(px(crate::glass_ui::rad(radius::LARGE)))
        .bg(white(0.045))
        .border_1()
        .border_color(white(ui::edge::REST))
        .p(px(20.))
        .child(div().child(lead))
        .child(
            ui::text(15., FontWeight::SEMIBOLD, ink::FULL)
                .mt(px(16.))
                .child(title),
        )
        .child(
            ui::text(13., FontWeight::NORMAL, 0.6)
                .mt(px(2.))
                .truncate()
                .child(note),
        )
        .child(div().mt(px(16.)).flex().items_start().child(action))
}

/// The scroll spy: which section is being read, given each section's top from the pane's top
/// (`tops`, in order), the pane's `height`, how far it is `scrolled` and how far it can be (`max`).
/// The last section whose top passed a reading line, which sits at 30 % of the pane (the mockup's)
/// and slides down to the pane's bottom over the last 70 % of a pane of scroll: the short last
/// sections, which never reach 30 %, each come in turn as the end nears, instead of the mockup's
/// jump from wherever it was to the last one at the very end.
fn spy(tops: &[f32], height: f32, scrolled: f32, max: f32) -> usize {
    let rest = height * 0.3;
    let travel = height - rest;
    let tail = travel.min(max);
    let line = if tail > 0. {
        rest + travel * ((scrolled - (max - tail)) / tail).clamp(0., 1.)
    } else {
        rest
    };
    tops.iter().rposition(|&top| top <= line + 0.5).unwrap_or(0)
}



#[cfg(test)]
mod search_tests {
    use super::{search, SECTIONS};

    #[test]
    fn search_finds_rows_and_sections_by_every_word() {
        let look = SECTIONS.iter().position(|s| s.0 == "Appearance").unwrap();
        assert!(search("blur").contains(&(look, Some("Blur"))));
        assert!(search("APPEAR").contains(&(look, None)));
        assert!(search("old delete")
            .iter()
            .any(|h| h.1 == Some("Delete old versions automatically")));
        assert!(search("zzzz nothing").is_empty());
        assert!(search("   ").is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::spy;

    /// A 600 px pane over the eight settings cards: tall ones, then About, too short for its
    /// top ever to reach 30 % of the pane.
    const TOPS: [f32; 8] = [0., 420., 700., 1000., 1300., 1600., 1900., 2200.];
    const HEIGHT: f32 = 600.;
    const MAX: f32 = 2400. - HEIGHT;

    fn at(scrolled: f32) -> usize {
        let tops: Vec<f32> = TOPS.iter().map(|t| t - scrolled).collect();
        spy(&tops, HEIGHT, scrolled, MAX)
    }

    #[test]
    fn the_list_follows_the_section_read_at_30_percent() {
        assert_eq!(at(0.), 0);
        assert_eq!(
            at(300.),
            1,
            "Updates' top is 120 px down: in the upper 30 %"
        );
        assert_eq!(at(600.), 2);
        assert_eq!(
            at(1000.),
            3,
            "Network's top at the pane's top, Game's 300 px down"
        );
        assert_eq!(at(1150.), 4, "Game's top 150 px down: in the upper 30 %");
    }

    #[test]
    fn near_the_end_the_short_last_section_is_marked_for_a_stretch() {
        let seen: Vec<usize> = (0..=MAX as usize)
            .step_by(10)
            .map(|s| at(s as f32))
            .collect();
        // Every section is marked on the way down, in order, never skipped.
        let mut order = seen.clone();
        order.dedup();
        assert_eq!(order, (0..8).collect::<Vec<_>>());
        // About is marked for a stretch of scroll before the end, not for one step.
        let about = seen.iter().filter(|&&s| s == 7).count();
        assert!(about >= 8, "About marked for {about} steps of 10 px");
        assert_eq!(at(MAX), 7);
    }

    #[test]
    fn a_pane_that_does_not_scroll_marks_the_first() {
        assert_eq!(spy(&[0., 200.], 600., 0., 0.), 0);
        assert_eq!(spy(&[], 600., 0., 0.), 0);
    }
}

#[cfg(test)]
mod picture_tests {
    

}

#[cfg(test)]
mod cull_tests {
    use super::{Culled, Pane};
    use gpui::{px, Pixels};

}
