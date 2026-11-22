//! "A specific version…": the sheet that picks one build to keep, however many there are.
//!
//! One list, drawn only where it is on screen: what is installed first (the build kept or played
//! leading), then every build that can be downloaded, by channel and month, newest first. The
//! field narrows it by date, name or commit. A row is only chosen; "Keep this build" is what
//! starts the staged Switch. Download and Prepare do not change the playing build.
use super::*;
use crate::line_edit::{self, LineEdit};
use crate::paint::palette;
use std::{
    path::PathBuf,
    rc::Rc,
    sync::{Arc, OnceLock},
};
use sunrise_launcher::{backend::Release, instances::Channel};

/// The height of every line of the list, headings included.
const LINE_H: f32 = 40.;

struct FitPreview {
    home: PathBuf,
    tag: String,
    checked: Arc<OnceLock<bool>>,
}

#[derive(Default)]
pub(crate) struct State {
    find: LineEdit,
    focus: Option<FocusHandle>,
    /// Whether the field has the keyboard, read where the window is at hand.
    focused: bool,
    /// The build chosen in the list, before it is kept.
    pick: Option<String>,
    preview: Option<FitPreview>,
    /// The list's scroll, which the arrow keys move to the row they land on.
    scroll: gpui::UniformListScrollHandle,
    /// The keyboard focus of each row, by line: a row that is not drawn has no element, so the
    /// handles live here for the arrow keys to land on a row before it is on screen.
    rows: std::collections::HashMap<usize, FocusHandle>,
}

/// The line the Up or Down key lands on from line `from` (None: no row has the keyboard):
/// the nearest build in that direction that can be chosen, or none at the end of the list.
fn next_row(
    lines: &[Line],
    from: Option<usize>,
    down: bool,
    usable: impl Fn(&Build) -> bool,
) -> Option<usize> {
    let ok = |n: &usize| matches!(&lines[*n], Line::Build(b) if usable(b));
    match (from, down) {
        (None, true) => (0..lines.len()).find(ok),
        (None, false) => (0..lines.len()).rev().find(ok),
        (Some(n), true) => (n + 1..lines.len()).find(ok),
        (Some(n), false) => (0..n.min(lines.len())).rev().find(ok),
    }
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

#[derive(Debug, PartialEq, Eq)]
enum VersionAction {
    Download,
    Prepare,
    Keep,
}

fn version_action(
    installed: bool,
    prepared: impl FnOnce() -> bool,
    can_switch: bool,
) -> Option<VersionAction> {
    if !installed {
        Some(VersionAction::Download)
    } else if !prepared() {
        Some(VersionAction::Prepare)
    } else {
        can_switch.then_some(VersionAction::Keep)
    }
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
    Build {
        tag: tag.to_owned(),
        name: build_name(tag),
        about: sunrise_launcher::nightly::date_of(tag)
            .or_else(|| backend::dev_date(tag))
            .and_then(|_| tag.splitn(5, '-').nth(4))
            .map_or_else(|| day.to_owned(), sunrise_launcher::nightly::short_commit),
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
    channel: Channel,
    query: &str,
) -> Vec<Line> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let day_of = |tag: &str| {
        sunrise_launcher::nightly::date_of(tag)
            .or_else(|| backend::dev_date(tag))
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
    // Launcher releases are separate. Dev is opt-in, but keep a profile's exact current build
    // visible even if it has another saved channel (for example, a copy of a Dev profile).
    let game = |tag: &str| {
        !native_mods::is_native(tag)
            && (channel == Channel::Dev || Some(tag) == current || backend::dev_date(tag).is_none())
    };
    let mut out = Vec::new();
    let mut have: Vec<&String> = installed.iter().filter(|t| game(t)).collect();
    have.sort_by(|a, b| {
        (Some(b.as_str()) == current)
            .cmp(&(Some(a.as_str()) == current))
            .then_with(|| sunrise_launcher::backend::compare_game_versions(b, a))
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
    for (listed, title) in [
        (Channel::Nightly, tr("Nightly")),
        (Channel::Stable, tr("Stable")),
        (Channel::Dev, tr("Dev")),
    ] {
        if listed == Channel::Dev && channel != Channel::Dev {
            continue;
        }
        let mut heading = String::new();
        for r in releases.iter().filter(|r| {
            r.asset.is_some()
                && Channel::of_release(r) == Some(listed)
                && !installed.contains(&r.tag)
        }) {
            let day = day_of(&r.tag);
            let b = build(&r.tag, &day, false);
            if !wanted(&b, &day) {
                continue;
            }
            let under = trf(
                "{channel} · {month}",
                &[
                    ("channel", title),
                    ("month", &month(day.get(..7).unwrap_or(&day))),
                ],
            );
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
    pub(crate) fn open_versions(&mut self) {
        self.open_sheet(Sheet::Versions);
        self.reset_versions();
    }

    pub(crate) fn reset_versions(&mut self) {
        self.ui.sheets.error = None;
        let pinned = self
            .selected_instance()
            .and_then(|i| self.profile_target(i));
        let v = &mut self.ui.sheets.versions;
        v.find.set(String::new());
        v.pick = pinned;
        v.preview = None;
        v.rows.clear();
    }

    /// Reads whether the field has the keyboard, before the sheet is drawn.
    pub(crate) fn versions_focus(&mut self, window: &Window) {
        let v = &mut self.ui.sheets.versions;
        v.focused = v.focus.as_ref().is_some_and(|f| f.is_focused(window));
    }

    pub(super) fn versions_sheet(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let body = self.versions_content(cx);
        frame("alt-versions", 560., body, cx)
    }

    pub(crate) fn open_update_picker(&mut self, tag: String) {
        self.open_versions();
        self.ui.sheets.versions.pick = Some(tag);
    }

    fn architecture_repair(&self, tag: &str) -> Option<&Release> {
        let Load::Ready(releases) = &self.releases else {
            return None;
        };
        backend::architecture_reinstall(releases, tag, self.settings.game_architecture)
    }

    pub(crate) fn can_repair_now(&self, tag: &str) -> bool {
        !self.launch_busy() && self.client.is_none() && !self.removing.iter().any(|t| t == tag)
    }

    fn versions_content(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(instance) = self.selected_instance() else {
            return div().child(tr("No profile selected."));
        };
        let plays = self.profile_target(instance);
        let is_pack = instance.kind == instances::Kind::Pack;
        let home = instance.home.clone();
        let releases: &[Release] = match &self.releases {
            Load::Ready(rows) => rows,
            _ => &[],
        };
        let current = plays.as_deref();
        let query = self.ui.sheets.versions.find.text().to_owned();
        let list = Rc::new(lines(
            releases,
            &self.installed,
            current,
            instance.channel,
            &query,
        ));
        let pick = self.ui.sheets.versions.pick.clone();

        let focus = self
            .ui
            .sheets
            .versions
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        let focused = self.ui.sheets.versions.focused;
        let field = crate::frame::input(focused)
            .id("alt-versions-find")
            .track_focus(&focus)
            .seen()
            .h(px(tokens::FIELD))
            .px(px(12.))
            .text_size(px(tokens::BODY))
            .cursor_text()
            .child(line_edit::content(
                &self.ui.sheets.versions.find,
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
                match line_edit::handle(&mut this.ui.sheets.versions.find, ev, cx) {
                    // Escape leaves the field first; the sheet closes on the next one.
                    line_edit::Key::Escape => window.blur(cx),
                    line_edit::Key::Pass => return,
                    _ => {}
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let field = line_edit::mouse(field, cx, |this| &mut this.ui.sheets.versions.find);

        let shown = Rc::clone(&list);
        let (now, chosen) = (plays.clone(), pick.clone());
        let scroll = self.ui.sheets.versions.scroll.clone();
        let rows = gpui::uniform_list(
            "alt-versions-list",
            list.len(),
            cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|n| match &shown[n] {
                        Line::Head(text) => div()
                            .id(("alt-version", n))
                            .h(px(LINE_H))
                            .px(px(14.))
                            .flex()
                            .items_end()
                            .pb(px(6.))
                            .text_size(px(tokens::CAPTION))
                            .text_color(rgb(palette().muted))
                            .child(text.to_uppercase()),
                        Line::Build(b) => {
                            let on = chosen.as_deref() == Some(b.tag.as_str());
                            let playing = now.as_deref() == Some(b.tag.as_str());
                            let tag = b.tag.clone();
                            let handle = this
                                .ui
                                .sheets
                                .versions
                                .rows
                                .entry(n)
                                .or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
                                .clone();
                            div()
                                .id(("alt-version", n))
                                .w_full()
                                .h(px(LINE_H))
                                .px(px(14.))
                                .flex()
                                .items_center()
                                .gap(px(12.))
                                .rounded(crate::tokens::r(crate::tokens::R_CONTROL))
                                .when(on, |d| d.bg(rgb(palette().selected)))
                                .child(
                                    div()
                                        .size(px(14.))
                                        .flex_none()
                                        .rounded_full()
                                        .border_1()
                                        .border_color(rgb(if on {
                                            palette().text
                                        } else {
                                            palette().muted
                                        }))
                                        .when(on, |d| d.bg(rgb(palette().text))),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(px(tokens::BODY))
                                        .child(b.name.clone()),
                                )
                                .child(
                                    muted(b.about.clone())
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(tokens::SMALL)),
                                )
                                .children(playing.then(|| {
                                    muted(tr("Playing now"))
                                        .flex_none()
                                        .text_size(px(tokens::SMALL))
                                }))
                                .children((b.installed && !playing).then(|| {
                                    muted(tr("Installed"))
                                        .flex_none()
                                        .text_size(px(tokens::SMALL))
                                }))
                                .track_focus(&handle)
                                .reach()
                                .cursor_pointer()
                                .hover(|s| s.bg(rgb(palette().hover)))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    let v = &mut this.ui.sheets.versions;
                                    if v.pick.as_ref() != Some(&tag) {
                                        v.preview = None;
                                        v.pick = Some(tag.clone());
                                    }
                                    cx.notify();
                                }))
                        }
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&scroll)
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
                this.close_sheet();
                cx.notify();
            },
        ));
        let reinstall = pick.as_ref().and_then(|tag| {
            self.architecture_repair(tag)?;
            let tag = tag.clone();
            let label = match &self.installing {
                Some((started, progress)) if started == &tag => trf(
                    "Installing · {percent}%",
                    &[("percent", &format!("{:.0}", progress * 100.))],
                ),
                _ => tr("Reinstall for this architecture").to_owned(),
            };
            let button = button("alt-versions-reinstall", label);
            Some(if self.can_repair_now(&tag) {
                button.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    // Recheck the exact offer and the active jobs at click.
                    if this.can_repair_now(&tag) {
                        if let Some(release) = this.architecture_repair(&tag).cloned() {
                            this.architecture_reinstall = Some(tag.clone());
                            this.install(&release);
                        }
                    }
                    cx.notify();
                }))
            } else {
                button.opacity(0.45).cursor_default().tab_stop(false)
            })
        });
        let keep = crate::frame::button_base("alt-versions-keep", tr("Keep this build"))
            .bg(rgb(palette().text))
            .border_color(rgb(palette().text))
            .text_color(rgb(palette().background))
            .hover(|s| s.bg(rgb(palette().accent_on)));
        let idle = !self.launch_busy() && self.client.is_none();
        let mut cache_action = None;
        let mut plan = None;
        let mut keep_tag = None;
        if let Some(tag) = pick.as_ref() {
            match version_action(
                self.installed.contains(tag),
                || backend::is_prepared(tag),
                !is_pack && Some(tag.as_str()) != current,
            ) {
                Some(VersionAction::Keep) => {
                    if idle {
                        keep_tag = Some(tag.clone());
                    }
                    let request = &mut self.ui.sheets.versions.preview;
                    if request
                        .as_ref()
                        .is_none_or(|preview| preview.home != home || preview.tag != *tag)
                    {
                        let checked = Arc::new(OnceLock::new());
                        *request = Some(FitPreview {
                            home: home.clone(),
                            tag: tag.clone(),
                            checked: checked.clone(),
                        });
                        let target = tag.clone();
                        let work = cx.background_executor().spawn(async move {
                            sunrise_launcher::browser::sunrise::checked_switch_fit(&home, &target)
                        });
                        cx.spawn(async move |this, cx| {
                            let _ = checked.set(work.await);
                            let _ = this.update(cx, |_, cx| cx.notify());
                        })
                        .detach();
                    }
                    plan = Some(
                        if request.as_ref().and_then(|p| p.checked.get()) == Some(&true) {
                            tr("The installed mod files fit this build.")
                        } else {
                            tr("The mod changes aren't known until Sunrise checks this build.")
                        },
                    );
                }
                Some(VersionAction::Prepare) => {
                    plan = Some(tr("Prepare this build before switching. Your profile stays on its current build."));
                    let target = tag.clone();
                    let b = button("alt-versions-prepare", tr("Prepare"));
                    cache_action = Some(if idle {
                        b.on_click(cx.listener(move |this, _, _, cx| {
                            this.prepare_version(target.clone(), backend::MinecraftFrom::Install);
                            cx.notify();
                        }))
                    } else {
                        b.opacity(0.45).tab_stop(false)
                    });
                }
                Some(VersionAction::Download) => {
                    if let Some(release) = self.release_to_install(tag) {
                        plan = Some(tr(
                            "Download this build first. Your profile stays on its current build.",
                        ));
                        let b = button("alt-versions-download", tr("Download"));
                        cache_action = Some(if idle {
                            b.on_click(cx.listener(move |this, _, _, cx| {
                                this.download_for_switch(&release);
                                cx.notify();
                            }))
                        } else {
                            b.opacity(0.45).tab_stop(false)
                        });
                    }
                }
                None => (),
            }
        }
        let keep = match keep_tag {
            Some(tag) => keep.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.begin_switch(tag.clone());
                cx.notify();
            })),
            None => keep.opacity(0.45).cursor_default().tab_stop(false),
        };
        let all = Rc::clone(&list);
        let body = div()
            .flex()
            .flex_col()
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                let down = match ev.keystroke.key.as_str() {
                    "down" => true,
                    "up" => false,
                    _ => return,
                };
                let v = &mut this.ui.sheets.versions;
                let from = v
                    .rows
                    .iter()
                    .find(|(_, h)| h.is_focused(window))
                    .map(|(n, _)| *n);
                if let Some(to) = next_row(&all, from, down, |_| true) {
                    v.scroll.scroll_to_item(to, gpui::ScrollStrategy::Nearest);
                    let row = v
                        .rows
                        .entry(to)
                        .or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
                        .clone();
                    window.focus(&row, cx);
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .child(title(tr("Choose a version").into()))
            .child(field)
            .child(
                div()
                    .rounded(crate::tokens::r(crate::tokens::R_CARD))
                    .border_1()
                    .border_color(rgb(palette().line))
                    .p(px(4.))
                    .child(rows)
                    .children(nothing),
            )
            .children(plan.map(muted))
            .children(self.prepare_error.as_ref().map(|e| muted(e.clone())))
            .children(
                self.ui
                    .sheets
                    .error
                    .clone()
                    .or_else(|| {
                        let (tag, error) = self.install_error.as_ref()?;
                        (self.ui.sheets.versions.pick.as_ref() == Some(tag)).then(|| error.clone())
                    })
                    .map(|e| {
                        div()
                            .text_color(rgb(palette().danger))
                            .text_size(px(tokens::SMALL))
                            .child(e)
                    }),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .flex_wrap()
                    .children(reinstall)
                    .child(cancel)
                    .children(cache_action)
                    .child(keep),
            );
        body
    }
}

/// The build list's scroll, which `sunrise-launcher benchmark` scrolls.
fn bench_scroll_target(sheets: &super::SheetsState) -> gpui::ScrollHandle {
    sheets.versions.scroll.0.borrow().base_handle.clone()
}

/// `sunrise-launcher benchmark`: after each frame, reads where the build list was left and
/// sends one wheel step over it through the window's ordinary input path, so the list scrolls,
/// caches and redraws as it does under a player's wheel. Nothing is read inside a frame.
///
/// With no profile there is no list, and no frames are asked for. A list with nothing to scroll
/// is given up on only once the releases have loaded and its place has held for a few frames,
/// so one that grows when the releases arrive is still scrolled.
pub(crate) fn bench_wheel(launcher: gpui::WeakEntity<Launcher>, window: &mut Window) {
    wheel_after_frame(launcher, window, None, 0);
}

/// Frames a loaded list's place must hold before it counts as settled.
const SETTLED: u32 = 10;

/// The wheel event a benchmark sends: `step` pixels down (negative) or up, at the middle of
/// `bounds`.
pub(crate) fn bench_wheel_event(
    bounds: gpui::Bounds<gpui::Pixels>,
    step: f32,
) -> gpui::PlatformInput {
    gpui::PlatformInput::Scroll(gpui::ScrollEvent {
        position: bounds.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(step))),
        ..Default::default()
    })
}

fn wheel_after_frame(
    launcher: gpui::WeakEntity<Launcher>,
    window: &mut Window,
    last: Option<gpui::Bounds<gpui::Pixels>>,
    held: u32,
) {
    window.on_next_frame(move |window, cx| {
        let Some(entity) = launcher.upgrade() else {
            return;
        };
        let this = entity.read(cx);
        if !this
            .bench
            .as_ref()
            .is_some_and(crate::perf::Bench::scrolling)
            || this.selected_instance().is_none()
        {
            return;
        }
        let loaded = !matches!(this.releases, Load::Loading);
        let handle = bench_scroll_target(&this.ui.sheets);
        let bounds = handle.bounds();
        let laid_out = bounds.size.height > px(0.);
        let held = if loaded && laid_out && Some(bounds) == last {
            held + 1
        } else {
            0
        };
        if laid_out {
            let (offset, max) = (
                f32::from(handle.offset().y),
                f32::from(handle.max_offset().y),
            );
            match entity.update(cx, |l, _| l.bench.as_mut()?.after_frame(offset, max)) {
                Some(step) => {
                    window.dispatch_event(bench_wheel_event(bounds, step), cx);
                }
                // Loaded, laid out, settled and nothing to scroll: no more frames for it.
                None if held >= SETTLED => return,
                None => {}
            }
        }
        wheel_after_frame(launcher, window, Some(bounds), held);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long list in a short window, scrolled by the benchmark's own wheel events through GPUI's
    /// input path, as `bench_wheel` sends them between frames.
    struct Rows {
        scroll: gpui::UniformListScrollHandle,
    }

    impl Render for Rows {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                gpui::uniform_list(
                    "bench-rows",
                    200,
                    cx.processor(|_, range: std::ops::Range<usize>, _, _| {
                        range
                            .map(|n| div().id(("row", n)).h(px(40.)).w_full())
                            .collect::<Vec<_>>()
                    }),
                )
                .size_full()
                .track_scroll(&self.scroll),
            )
        }
    }

    #[test]
    fn the_benchmarks_wheel_moves_a_populated_list_and_counts_the_frame() {
        let mut app = gpui::TestAppContext::single();
        let (rows, cx) = app.add_window_view(|_, _| Rows {
            scroll: gpui::UniformListScrollHandle::new(),
        });
        let handle = rows.read_with(cx, |rows, _| rows.scroll.0.borrow().base_handle.clone());
        cx.run_until_parked();
        let mut bench = crate::perf::Bench::scrolling_for_test();
        let read = |handle: &gpui::ScrollHandle| {
            (
                f32::from(handle.offset().y),
                f32::from(handle.max_offset().y),
            )
        };
        let (top, max) = read(&handle);
        assert!(max > 1000., "a populated list overflows: {max}");
        assert_eq!(top, 0.);
        // After the first frame: nothing moved yet, the first step goes in.
        bench.frame(
            std::time::Instant::now(),
            Some(std::time::Duration::from_millis(16)),
        );
        let step = bench.after_frame(top, max).expect("a list that can move");
        assert!(step < 0., "down first");
        cx.update(|window, cx| {
            window.dispatch_event(bench_wheel_event(handle.bounds(), step), cx);
        });
        cx.run_until_parked();
        // The next frame drew the list moved: that frame is a scroll sample.
        bench.frame(
            std::time::Instant::now(),
            Some(std::time::Duration::from_millis(17)),
        );
        let (moved, _) = read(&handle);
        assert_eq!(moved, step, "the wheel moved the list by its step");
        bench.after_frame(moved, max);
        assert_eq!(bench.scroll_samples(), [17.]);
    }

    #[test]
    fn the_current_unready_build_can_be_downloaded_or_prepared_but_never_kept() {
        assert_eq!(
            version_action(false, || unreachable!(), false),
            Some(VersionAction::Download)
        );
        assert_eq!(
            version_action(true, || false, false),
            Some(VersionAction::Prepare)
        );
        assert_eq!(version_action(true, || true, false), None);
        assert_eq!(
            version_action(true, || true, true),
            Some(VersionAction::Keep)
        );
    }

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

    fn build(tag: &str) -> Line {
        Line::Build(Build {
            tag: tag.into(),
            name: tag.into(),
            about: String::new(),
            installed: false,
        })
    }

    #[test]
    fn arrow_keys_step_over_headings_and_unusable_rows_and_stop_at_the_ends() {
        let lines = [
            Line::Head("November".into()),
            build("a"),
            build("b"),
            Line::Head("October".into()),
            build("c"),
            build("d"),
        ];
        let all = |_: &Build| true;
        assert_eq!(next_row(&lines, None, true, all), Some(1));
        assert_eq!(next_row(&lines, None, false, all), Some(5));
        assert_eq!(next_row(&lines, Some(2), true, all), Some(4));
        assert_eq!(next_row(&lines, Some(4), false, all), Some(2));
        assert_eq!(next_row(&lines, Some(5), true, all), None);
        assert_eq!(next_row(&lines, Some(1), false, all), None);
        let not_c = |b: &Build| b.tag != "c";
        assert_eq!(next_row(&lines, Some(2), true, not_c), Some(5));
        assert_eq!(next_row(&[], None, true, all), None);
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
    fn only_current_nightly_and_stable_builds_have_published_rows() {
        let mut releases = vec![
            release("nightly-2026-11-17-aaaaaaa", "2026-11-17", true),
            Release {
                prerelease: true,
                ..release("26.3-0.1.1", "2026-11-16", false)
            },
        ];
        assert_eq!(
            shape(&lines(&releases, &[], None, Channel::Nightly, "")),
            ["# Nightly · November 2026", "nightly-2026-11-17-aaaaaaa",]
        );
        releases.push(release("26.3-0.1.0", "2026-10-01", false));
        assert_eq!(
            shape(&lines(&releases, &[], None, Channel::Nightly, "26.3-0.1.0")),
            ["# Stable · October 2026", "26.3-0.1.0"]
        );
    }

    #[test]
    fn dev_versions_are_listed_only_after_opt_in() {
        let tag = "dev-2026-11-21-aaaaaaa";
        let pruned = "dev-2026-11-20-bbbbbbb";
        let releases = [Release {
            prerelease: true,
            nightly: Some(tag.into()),
            ..release(tag, "2026-11-22", false)
        }];
        let installed = [pruned.to_owned()];
        assert!(lines(&releases, &installed, None, Channel::Stable, "").is_empty());
        assert_eq!(
            shape(&lines(&releases, &installed, None, Channel::Dev, "")),
            ["# Installed", pruned, "# Dev · November 2026", tag]
        );
    }

    #[test]
    fn installed_builds_lead_then_the_rest_by_channel_and_month() {
        let releases = [
            release("nightly-2026-10-30-ccccccc", "2026-10-30", true),
            release("v2026.10.13", "2026-10-13", false),
            release("26.3-0.1.0", "2026-10-01", false),
        ];
        let installed = [
            "nightly-2026-11-17-aaaaaaa".to_owned(),
            "nightly-2026-11-16-bbbbbbb".to_owned(),
        ];
        let all = lines(
            &releases,
            &installed,
            Some("nightly-2026-11-16-bbbbbbb"),
            Channel::Nightly,
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
                "26.3-0.1.0",
            ]
        );
        // A commit or a day finds its build; a heading with nothing under it is gone.
        let found = lines(&releases, &installed, None, Channel::Nightly, "ccccccc");
        assert_eq!(
            shape(&found),
            ["# Nightly · October 2026", "nightly-2026-10-30-ccccccc"]
        );
        assert_eq!(
            shape(&lines(
                &releases,
                &installed,
                None,
                Channel::Nightly,
                "nov 17"
            ))
            .len(),
            2
        );
    }
}
