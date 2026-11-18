//! "A specific version…": the sheet that picks one build to keep, however many there are.
//!
//! One list, drawn only where it is on screen: what is installed first (the build kept or played
//! leading), then every build that can be downloaded, by channel and month, newest first. The
//! field narrows it by date, name or commit. A row is only chosen; "Keep this build" is what
//! changes the setup (`pin_alt_build`), and Play installs a build that isn't here yet.
use super::*;
use crate::line_edit::{self, LineEdit};
use std::rc::Rc;
use sunrise_launcher::backend::Release;

/// The height of every line of the list, headings included.
const LINE_H: f32 = 40.;

#[derive(Default)]
pub(crate) struct State {
    find: LineEdit,
    focus: Option<FocusHandle>,
    /// Whether the field has the keyboard, read where the window is at hand.
    focused: bool,
    /// The build chosen in the list, before it is kept.
    pick: Option<String>,
}

/// A line of the list.
#[derive(Clone, Debug, PartialEq)]
enum Line {
    Head(String),
    Build(Build),
}

#[derive(Clone, Debug, PartialEq)]
struct Build {
    tag: String,
    /// "Nov 17", or a release's own name.
    name: String,
    /// "Nightly · 0c02067", or a release's day.
    about: String,
    installed: bool,
}

/// "November 2026" for `2026-11`.
fn month(year_month: &str) -> String {
    let (year, month) = year_month.split_once('-').unwrap_or((year_month, ""));
    let name = match month {
        "01" => tr("January"),
        "02" => tr("February"),
        "03" => tr("March"),
        "04" => tr("April"),
        "05" => tr("May"),
        "06" => tr("June"),
        "07" => tr("July"),
        "08" => tr("August"),
        "09" => tr("September"),
        "10" => tr("October"),
        "11" => tr("November"),
        "12" => tr("December"),
        _ => return year_month.to_owned(),
    };
    format!("{name} {year}")
}

fn build(tag: &str, day: &str, installed: bool) -> Build {
    let nightly = sunrise_launcher::nightly::date_of(tag).is_some();
    Build {
        tag: tag.to_owned(),
        name: build_name(tag),
        about: match sunrise_launcher::nightly::label(tag) {
            Some(label) if nightly => label,
            _ => day.to_owned(),
        },
        installed,
    }
}

/// The list for `query`: the installed builds (`current` first), then the builds of `releases`
/// that can be downloaded and aren't installed, under a heading per channel and month.
/// `releases` is newest first, as it is listed. Every word of `query` has to be in a build's
/// version, day or name.
fn lines(
    releases: &[Release],
    installed: &[String],
    current: Option<&str>,
    query: &str,
) -> Vec<Line> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let day_of = |tag: &str| {
        sunrise_launcher::nightly::date_of(tag)
            .map(str::to_owned)
            .or_else(|| {
                releases
                    .iter()
                    .find(|r| r.tag == tag)
                    .map(|r| r.date.clone())
            })
            .unwrap_or_default()
    };
    let wanted = |b: &Build, day: &str| {
        let hay = format!("{} {} {} {day}", b.tag, b.name, b.about).to_lowercase();
        words.iter().all(|w| hay.contains(w.as_str()))
    };
    // Launcher releases come from their own repository and are never installed here, and `v2026.10.13` is also valid vX.Y.Z: only native builds are left out.
    let game = |tag: &str| !native_mods::is_native(tag);
    let mut out = Vec::new();
    let mut have: Vec<&String> = installed.iter().filter(|t| game(t)).collect();
    have.sort_by(|a, b| {
        (Some(b.as_str()) == current)
            .cmp(&(Some(a.as_str()) == current))
            .then_with(|| sunrise_launcher::backend::compare_versions(b, a))
    });
    let have: Vec<Line> = have
        .into_iter()
        .map(|tag| (build(tag, &day_of(tag), true), day_of(tag)))
        .filter(|(b, day)| wanted(b, day))
        .map(|(b, _)| Line::Build(b))
        .collect();
    if !have.is_empty() {
        out.push(Line::Head(tr("Installed").to_string()));
        out.extend(have);
    }
    // A Nightly can be listed under two names that install as one version: it is one build.
    let mut seen = std::collections::HashSet::new();
    for nightly in [true, false] {
        let mut heading = String::new();
        for r in releases.iter().filter(|r| {
            r.asset.is_some()
                && r.nightly.is_some() == nightly
                && game(&r.tag)
                && !installed.contains(&r.tag)
                && seen.insert(r.tag.as_str())
        }) {
            let day = day_of(&r.tag);
            let b = build(&r.tag, &day, false);
            if !wanted(&b, &day) {
                continue;
            }
            let channel = if nightly { tr("Nightly") } else { tr("Stable") };
            let under = format!("{channel} · {}", month(day.get(..7).unwrap_or(&day)));
            if under != heading {
                out.push(Line::Head(under.clone()));
                heading = under;
            }
            out.push(Line::Build(b));
        }
    }
    out
}

impl Launcher {
    /// Opens the sheet on the build the setup keeps now.
    pub(super) fn open_alt_versions(&mut self) {
        self.open_alt_sheet(Sheet::Versions);
        let pinned = self.selected_instance().and_then(|i| i.version.clone());
        let v = &mut self.alt.sheets.versions;
        v.find.set(String::new());
        v.pick = pinned;
    }

    /// Reads whether the field has the keyboard, before the sheet is drawn.
    pub(super) fn alt_versions_focus(&mut self, window: &Window) {
        let v = &mut self.alt.sheets.versions;
        v.focused = v.focus.as_ref().is_some_and(|f| f.is_focused(window));
    }

    pub(super) fn versions_sheet(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let Some(instance) = self.selected_instance() else {
            return div().id("alt-versions");
        };
        let (pinned, channel) = (instance.version.clone(), instance.channel);
        let plays = self.version_of(instance).cloned();
        // Another build under native mods isn't kept from here: they are checked first.
        let has_native = !native_status::enabled(&instance.home).is_empty();
        let releases: &[Release] = match &self.releases {
            Load::Ready(rows) => rows,
            _ => &[],
        };
        let current = pinned.as_deref().or(plays.as_deref());
        let query = self.alt.sheets.versions.find.text().to_owned();
        let list = Rc::new(lines(releases, &self.installed, current, &query));
        let pick = self.alt.sheets.versions.pick.clone();

        let focus = self
            .alt
            .sheets
            .versions
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        let focused = self.alt.sheets.versions.focused;
        let field = div()
            .id("alt-versions-find")
            .track_focus(&focus)
            .h(px(38.))
            .px(px(12.))
            .flex()
            .items_center()
            .overflow_hidden()
            .rounded(px(8.))
            .bg(rgb(BG))
            .border_1()
            .border_color(rgb(if focused { GOLD } else { LINE }))
            .text_size(px(14.))
            .cursor_text()
            .child(line_edit::content(
                &self.alt.sheets.versions.find,
                &self.theme,
                focused,
                false,
                false,
                "Search by date or commit",
            ))
            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                window.focus(&focus, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                match line_edit::handle(&mut this.alt.sheets.versions.find, ev, cx) {
                    // Escape leaves the field first; the sheet closes on the next one.
                    line_edit::Key::Escape => window.blur(cx),
                    line_edit::Key::Pass => return,
                    _ => {}
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let field = line_edit::mouse(field, cx, |this| &mut this.alt.sheets.versions.find);

        let shown = Rc::clone(&list);
        let (now, chosen) = (plays.clone(), pick.clone());
        let rows = gpui::uniform_list(
            "alt-versions-list",
            list.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|n| match &shown[n] {
                        Line::Head(text) => div()
                            .id(("alt-version", n))
                            .h(px(LINE_H))
                            .px(px(14.))
                            .flex()
                            .items_end()
                            .pb(px(6.))
                            .text_size(px(11.))
                            .text_color(rgb(MUTED))
                            .child(text.to_uppercase()),
                        Line::Build(b) => {
                            let on = chosen.as_deref() == Some(b.tag.as_str());
                            let playing = now.as_deref() == Some(b.tag.as_str());
                            let blocked = has_native && !playing;
                            let tag = b.tag.clone();
                            div()
                                .id(("alt-version", n))
                                .w_full()
                                .h(px(LINE_H))
                                .px(px(14.))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .rounded(px(8.))
                                .when(on, |d| d.bg(rgb(BG)))
                                .child(
                                    div()
                                        .size(px(14.))
                                        .flex_none()
                                        .rounded_full()
                                        .border_1()
                                        .border_color(rgb(if on { TEXT } else { MUTED }))
                                        .when(on, |d| d.bg(rgb(TEXT))),
                                )
                                .child(div().flex_none().text_size(px(14.)).child(b.name.clone()))
                                .child(
                                    muted(b.about.clone())
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.)),
                                )
                                .children(playing.then(|| {
                                    muted(tr("Playing now")).flex_none().text_size(px(12.))
                                }))
                                .children(
                                    (b.installed && !playing).then(|| {
                                        muted(tr("Installed")).flex_none().text_size(px(12.))
                                    }),
                                )
                                .when(blocked, |d| d.opacity(0.45))
                                .when(!blocked, |d| {
                                    d.cursor_pointer().hover(|s| s.bg(rgb(BG))).on_click(
                                        cx.listener(move |this, _: &ClickEvent, _, cx| {
                                            this.alt.sheets.versions.pick = Some(tag.clone());
                                            cx.notify();
                                        }),
                                    )
                                })
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        // As tall as what it lists, up to the room the window has; past that it scrolls.
        .h(px((list.len() as f32 * LINE_H)
            .min((self.window_height - 330.).clamp(LINE_H * 4., LINE_H * 9.))));
        let nothing = list.is_empty().then(|| {
            muted(match (&self.releases, query.trim().is_empty()) {
                (Load::Loading, _) => tr("Reading the list of builds…"),
                (_, false) => tr("No build matches your search."),
                (_, true) => tr("No builds to choose from yet."),
            })
            .px(px(14.))
            .py(px(12.))
        });

        let cancel = button("alt-versions-cancel", tr("Cancel")).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.close_alt_sheet();
                cx.notify();
            },
        ));
        let keep = button("alt-versions-keep", tr("Keep this build"))
            .bg(rgb(TEXT))
            .border_color(rgb(TEXT))
            .text_color(rgb(BG))
            .hover(|s| s.bg(rgb(0xffffff)));
        let keep = match pick.filter(|p| Some(p) != pinned.as_ref()) {
            Some(tag) => keep.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.pin_alt_build(Some(tag.clone()), channel);
                if this.alt.sheets.error.is_none() {
                    this.close_alt_sheet();
                }
                cx.notify();
            })),
            None => keep.opacity(0.45).cursor_default(),
        };
        // Said once, and only when it stops a choice: with the check that lifts it.
        let native = has_native.then(|| {
            div()
                .flex()
                .flex_col()
                .child(muted(tr(
                    "Your native mods are made for the build you play. Another build needs them checked first.",
                ))
                .text_size(px(12.)))
                .children(self.native_check_block(cx))
        });
        let body = div()
            .flex()
            .flex_col()
            .child(title(tr("Choose a version").into()))
            .child(field)
            .children(native)
            .child(
                div()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .p(px(4.))
                    .child(rows)
                    .children(nothing),
            )
            .children(
                self.alt
                    .sheets
                    .error
                    .clone()
                    .map(|e| div().text_color(rgb(BAD)).text_size(px(12.)).child(e)),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(cancel)
                    .child(keep),
            );
        frame("alt-versions", 560., body, cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, date: &str, nightly: bool) -> Release {
        Release {
            tag: tag.into(),
            name: tag.into(),
            body: String::new(),
            date: date.into(),
            time: String::new(),
            asset: Some((String::new(), 1)),
            prerelease: false,
            nightly: nightly.then(|| "nightly".to_owned()),
        }
    }

    fn shape(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| match line {
                Line::Head(text) => format!("# {text}"),
                Line::Build(b) => b.tag.clone(),
            })
            .collect()
    }

    #[test]
    fn installed_builds_lead_then_the_rest_by_channel_and_month() {
        let releases = [
            release("nightly-2026-11-17-aaaaaaa", "2026-11-17", true),
            release("nightly-2026-11-16-bbbbbbb", "2026-11-16", true),
            release("nightly-2026-10-30-ccccccc", "2026-10-30", true),
            // Listed a second time under its dated name.
            release("nightly-2026-10-30-ccccccc", "2026-10-30", true),
            release("v1.0.0", "2026-10-01", false),
        ];
        let installed = [
            "nightly-2026-11-17-aaaaaaa".to_owned(),
            "nightly-2026-11-16-bbbbbbb".to_owned(),
        ];
        let all = lines(
            &releases,
            &installed,
            Some("nightly-2026-11-16-bbbbbbb"),
            "",
        );
        assert_eq!(
            shape(&all),
            [
                "# Installed",
                // The build kept or played leads, whatever its date.
                "nightly-2026-11-16-bbbbbbb",
                "nightly-2026-11-17-aaaaaaa",
                "# Nightly · October 2026",
                "nightly-2026-10-30-ccccccc",
                "# Stable · October 2026",
                "v1.0.0",
            ]
        );
        // A commit or a day finds its build; a heading with nothing under it is gone.
        let found = lines(&releases, &installed, None, "ccccccc");
        assert_eq!(
            shape(&found),
            ["# Nightly · October 2026", "nightly-2026-10-30-ccccccc"]
        );
        assert_eq!(
            shape(&lines(&releases, &installed, None, "nov 17")).len(),
            2
        );
    }
}
