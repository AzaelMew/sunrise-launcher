//! Mods › Discover's source catalogue: only card icons load before a player opens a listing.
use super::ui::*;
use super::*;
use crate::frame::button;
use std::time::{Duration, SystemTime};
use sunrise_launcher::browser::sunrise::sources::{self, Catalogue, Source};

mod jobs;
mod project;

pub(super) use jobs::Event;
pub(super) use project::{short_sha, source_error};

const PAGE_SIZE: usize = 48;
const FRESH: Duration = Duration::from_secs(600);

#[derive(Default)]
pub(super) struct State {
    catalogue: Option<Catalogue>,
    reading: bool,
    pub(super) open: Option<jobs::Open>,
    detail_request: u64,
    pub(super) limit: usize,
    pub(super) work: jobs::Work,
}

impl Launcher {
    pub(super) fn discover_profile_line(&self, show_source_warning: bool) -> Div {
        let target = self.discover_target_name();
        div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_x(px(10.))
            .gap_y(px(4.))
            .when(show_source_warning, |line| {
                line.child(muted(tr(
                    "Build directly from GitHub. Listed source code is not reviewed.",
                )))
            })
            .child(div().flex_1())
            .child(muted(trf("Profile: {build}", &[("build", &target)])))
    }

    pub(super) fn discover_target_name(&self) -> String {
        self.selected_instance()
            .and_then(|instance| self.profile_target(instance))
            .map_or_else(
                || tr("No game build selected").into(),
                |tag| crate::sheets::build_name(&tag),
            )
    }

    pub(super) fn read_nightly_catalogue(&mut self, force: bool) {
        let n = &mut self.ui.mods.nightly;
        if n.reading
            || (!force
                && n.catalogue
                    .as_ref()
                    .is_some_and(|c| c.checked_at.elapsed().is_ok_and(|time| time < FRESH)))
        {
            return;
        }
        n.reading = true;
        let tx = self.ui.mods.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(super::Event::Nightly(Event::Catalogue(sources::catalogue(
                force,
            ))));
        });
    }

    pub(super) fn nightly_discover(
        &mut self,
        window: &mut Window,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.read_nightly_catalogue(false);
        let header = self.mods_header(false, focus, window, cx);
        let mut page = div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(24.))
            .text_color(c(palette().text))
            .child(header)
            .child(self.discover_profile_line(true))
            .children(self.strip(cx));
        let Some(catalogue) = &self.ui.mods.nightly.catalogue else {
            return page
                .child(muted(tr("Loading source projects…")))
                .into_any_element();
        };
        if catalogue.error.is_some() {
            let retry = button("nightly-catalogue-retry", tr("Try again"));
            let retry = if self.ui.mods.nightly.reading {
                dim(retry)
            } else {
                retry.on_click(cx.listener(|this, _, _, cx| {
                    this.read_nightly_catalogue(true);
                    cx.notify();
                }))
            };
            let (line, cached) = match catalogue.saved_at {
                Some(time) => (
                    trf(
                        "You're offline · showing the saved list from {date}",
                        &[("date", &source_time(time))],
                    ),
                    true,
                ),
                None => (tr("Source list unavailable offline").to_owned(), false),
            };
            page = page.child(
                card()
                    .p(px(12.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(line))
                    .child(retry),
            );
            if !cached {
                return page.into_any_element();
            }
        }
        let shown = source_matches(&catalogue.sources, &self.ui.mods.filters.text);
        let total = shown.len();
        let allow_icon_fetch = catalogue.error.is_none();
        let limit = self.ui.mods.nightly.limit.max(PAGE_SIZE);
        let build = self.nightly_build_revision();
        let can_start = self.nightly_catalogue_current() && !self.nightly_busy();
        let work = &self.ui.mods.nightly.work;
        let mut grid = div()
            .grid()
            .grid_cols(columns(self.content_width()))
            .gap(px(12.));
        for (n, source) in shown.into_iter().take(limit).enumerate() {
            let pin = work
                .release_pins
                .get(source.id())
                .filter(|pin| pin.source() == source);
            let icon = sources::SourceIcon::new(source, pin, allow_icon_fetch);
            let picture = icon
                .as_ref()
                .and_then(|key| work.icons.get(key))
                .cloned()
                .flatten();
            let icon = icon.filter(|key| !work.icons.contains_key(key));
            let key = (source.clone(), allow_icon_fetch);
            let metadata = work.metadata.get(&key).and_then(Option::as_ref);
            let authors = metadata
                .filter(|metadata| !metadata.authors.is_empty())
                .map_or_else(
                    || source.owner().to_owned(),
                    |metadata| metadata.authors.join(", "),
                );
            let mut facts = vec![tag(model::Make::Native.label())];
            if made_for_build(
                build.as_deref(),
                metadata.and_then(|m| m.build_pin.as_deref()),
            ) {
                facts.push(badge(tr("Made for this build")));
            }
            // Collection membership is another fact, not a separate card layout.
            let metadata_request = (matches!(source.tracker(), sources::Tracker::Branch(_))
                && !work.metadata.contains_key(&key))
            .then_some(key);
            let load = (icon.is_some() || metadata_request.is_some()).then(|| {
                when_visible(cx, move |this| {
                    if let Some(icon) = icon {
                        this.request_nightly_icon(icon);
                    }
                    if let Some(key) = metadata_request {
                        this.request_nightly_metadata(key);
                    }
                })
            });
            let action = button(("nightly-build", n), tr("Build"));
            let action = if can_start {
                let source = source.clone();
                action.on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.check_nightly(source.clone(), false, true);
                    cx.notify();
                }))
            } else {
                dim(action)
            };
            let open = source.clone();
            grid = grid.child(
                browse_card(
                    ("nightly-card", n),
                    picture,
                    source.id(),
                    source.name(),
                    &authors,
                    source.summary(),
                    facts,
                    Some(action.into_any_element()),
                    None,
                )
                .relative()
                .children(load)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.inspect_nightly(open.clone(), false);
                    cx.notify();
                })),
            );
        }
        let more = (total > limit).then(|| {
            button("nightly-more", tr("Show more")).on_click(cx.listener(move |this, _, _, cx| {
                this.ui.mods.nightly.limit = limit + PAGE_SIZE;
                cx.notify();
            }))
        });
        let results = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(grid)
            .when(total == 0, |d| {
                d.child(muted(tr("No Nightly mods match your search.")))
            })
            .children(more);
        page.child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .child(muted(trf(
                    "{count} source projects",
                    &[("count", &total.to_string())],
                )))
                .child(muted(tr("Name A–Z"))),
        )
        .child(
            div()
                .id("nightly-catalogue-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&crate::frame::scrolls("nightly-discover"))
                .child(results),
        )
        .into_any_element()
    }

    fn nightly_icon_tile(
        &self,
        source: &Source,
        commit: Option<&sources::SourceCommit>,
        allow_fetch: bool,
        side: f32,
        cx: &Context<Self>,
    ) -> Div {
        let icon = sources::SourceIcon::new(source, commit, allow_fetch);
        let icons = &self.ui.mods.nightly.work.icons;
        let loaded = icon.as_ref().and_then(|icon| icons.get(icon));
        let tile = tile(loaded.cloned().flatten(), source.id(), source.name(), side);
        let Some(icon) = icon.filter(|_| loaded.is_none()) else {
            return tile;
        };
        tile.relative().child(when_visible(cx, move |this| {
            this.request_nightly_icon(icon)
        }))
    }

    fn nightly_build_revision(&self) -> Option<String> {
        backend::build_revision(&self.profile_target(self.selected_instance()?)?)
    }
}

fn when_visible(
    cx: &Context<Launcher>,
    load: impl FnOnce(&mut Launcher) + 'static,
) -> impl IntoElement + 'static {
    let entity = cx.entity().downgrade();
    gpui::canvas(
        move |bounds, window, cx| {
            if window.content_mask().bounds.intersects(&bounds) {
                cx.defer(move |cx| {
                    entity.update(cx, |this, _| load(this)).ok();
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}

fn made_for_build(profile: Option<&str>, pin: Option<&str>) -> bool {
    // The manifest validated a full pin; the profile returned its full commit or a valid prefix.
    profile
        .zip(pin)
        .is_some_and(|(profile, pin)| pin.starts_with(profile))
}

fn source_matches<'a>(sources: &'a [Source], filter: &str) -> Vec<&'a Source> {
    let words: Vec<_> = filter.split_whitespace().map(str::to_lowercase).collect();
    let mut shown: Vec<_> = sources
        .iter()
        .filter(|source| {
            let text = format!(
                "{} {} {} {}",
                source.name(),
                source.id(),
                source.repo(),
                source.summary()
            )
            .to_lowercase();
            words.iter().all(|word| text.contains(word))
        })
        .map(|source| (source.name().to_lowercase(), source.id(), source))
        .collect();
    shown.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    shown.into_iter().map(|(_, _, source)| source).collect()
}

pub(super) fn installed_source(row: &Row) -> Option<&sunrise_launcher::github_mods::Origin> {
    row.local
        .as_ref()?
        .github
        .as_ref()
        .filter(|origin| origin.source.is_some())
}

pub(super) fn source_provenance(origin: &sunrise_launcher::github_mods::Origin) -> String {
    let repo = format!("{}/{}", origin.owner, origin.repo);
    trf(
        "Built from source · {repo} @ {sha}",
        &[("repo", &repo), ("sha", short_sha(&origin.commit))],
    )
}

fn source_time(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_matching_full_build_or_valid_profile_prefix_gets_the_badge() {
        let pin = "9425651aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let other = "9425651bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        assert!(made_for_build(Some(pin), Some(pin)));
        assert!(made_for_build(Some("9425651"), Some(pin)));
        assert!(!made_for_build(Some(other), Some(pin)));
        assert!(!made_for_build(Some("1234567"), Some(pin)));
        assert!(!made_for_build(Some(pin), None));
        assert!(!made_for_build(None, Some(pin)));
    }
}
