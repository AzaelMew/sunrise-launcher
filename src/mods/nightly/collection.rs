//! A source collection's page: its mods to pick from, one Build. Each picked mod builds from its
//! own repository through the same source flow as a single mod, with one consent for all of them.
use super::*;
use crate::mods_page::install::{stage_label, SourceTarget};
use std::collections::BTreeSet;
use sunrise_launcher::browser::sunrise::{PreparedSource, SourceError};
use sunrise_launcher::native_mods::Cancellation;
use sunrise_launcher_smod::index::Collection;

/// The open collection and the mods ticked on its page.
pub(in crate::mods) struct CollectionOpen {
    id: String,
    ticked: BTreeSet<String>,
    /// The listings a Build click is checking; its result applies only while they still match.
    pending: Option<Vec<Source>>,
}

/// What preparing a collection's picks produced: the builds to queue, and why any pick can't.
pub(in crate::mods) struct PreparedCollection {
    id: String,
    picks: Vec<Source>,
    target: Option<SourceTarget>,
    prepared: Vec<PreparedSource>,
    failed: Vec<(String, String)>,
}

impl Launcher {
    pub(in crate::mods) fn open_source_collection(&mut self, collection: &Collection) {
        self.ui.page = Page::Project;
        self.ui.mods.open = None;
        let state = &mut self.ui.mods.nightly;
        state.open = None;
        state.collection = Some(CollectionOpen {
            id: collection.id.clone(),
            ticked: collection
                .members
                .iter()
                .filter(|member| member.default)
                .map(|member| member.id.clone())
                .collect(),
            pending: None,
        });
    }

    /// Source mods already built into this profile, by id.
    fn built_sources(&self) -> BTreeSet<String> {
        self.ui
            .mods
            .snapshot
            .as_ref()
            .into_iter()
            .flat_map(|snapshot| &snapshot.rows.local)
            .filter_map(|local| local.github.as_ref())
            .map(|origin| origin.id.clone())
            .collect()
    }

    /// Resolve and check each ticked mod that isn't built yet, then queue them as one batch: the
    /// installer asks consent once, listing every repository.
    fn build_source_collection(&mut self) {
        if !self.nightly_catalogue_current() || !self.guard() {
            return;
        }
        let built = self.built_sources();
        let target = self.source_target();
        let state = &mut self.ui.mods.nightly;
        let (Some(open), Some(catalogue)) = (state.collection.as_mut(), &state.catalogue) else {
            return;
        };
        let picks = collection_picks(catalogue, &open.id, &open.ticked, &built);
        if picks.is_empty() || open.pending.is_some() {
            return;
        }
        open.pending = Some(picks.clone());
        let id = open.id.clone();
        let tx = self.ui.mods.tx.clone();
        std::thread::spawn(move || {
            let mut done = PreparedCollection {
                id,
                picks: picks.clone(),
                target: None,
                prepared: Vec::new(),
                failed: Vec::new(),
            };
            match target {
                Ok(target) => {
                    let cancellation = Cancellation::default();
                    let mut packed = Vec::new();
                    for (source, pin) in picks.iter().zip(sources::resolve_batch(&picks)) {
                        match pin
                            .map_err(SourceError::Github)
                            .and_then(|pin| sunrise::pack_source(&pin, &cancellation))
                        {
                            Ok(package) => packed.push(package),
                            Err(error) => done.failed.push(failure(&error, Some(source))),
                        }
                    }
                    // One check over every pick, so a ticked library supplies its ticked
                    // dependents, as a pasted repository's packages are checked together.
                    if !packed.is_empty() {
                        match sunrise::prepare_pasted_sources(
                            target.instance(),
                            target.identity(),
                            target.base(),
                            packed,
                            &cancellation,
                        ) {
                            Ok(prepared) => done.prepared = prepared,
                            Err(error) => done.failed.push(failure(&error, None)),
                        }
                    }
                    done.target = Some(target);
                }
                Err(error) => done.failed.push((String::new(), error)),
            }
            let _ = tx.send(super::super::Event::Nightly(Event::Collection(Box::new(
                done,
            ))));
        });
    }

    pub(super) fn collection_prepared(&mut self, done: PreparedCollection) {
        let state = &mut self.ui.mods.nightly;
        let Some(open) = state
            .collection
            .as_mut()
            .filter(|open| open.id == done.id && open.pending.as_ref() == Some(&done.picks))
        else {
            return;
        };
        open.pending = None;
        // Queue only what the current list still offers for this collection, as a single
        // source's Build does.
        let built = BTreeSet::new();
        let ticked = done.picks.iter().map(|s| s.id().to_owned()).collect();
        let listed = state.catalogue.as_ref().is_some_and(|catalogue| {
            collection_picks(catalogue, &done.id, &ticked, &built) == done.picks
        });
        if !listed || !self.nightly_catalogue_current() {
            self.ui.mods.notice = Some(Err(tr(
                "This source listing changed. Open it again from Discover.",
            )
            .into()));
            return;
        }
        let failed = done
            .failed
            .iter()
            .map(|(name, why)| match name.is_empty() {
                true => why.clone(),
                false => format!("{name}: {why}"),
            })
            .collect::<Vec<_>>();
        if let Some(target) = done.target.filter(|_| !done.prepared.is_empty()) {
            if let Err(error) = self.queue_sources(target, done.prepared) {
                self.ui.mods.notice = Some(Err(error));
                return;
            }
        }
        if !failed.is_empty() {
            self.ui.mods.notice = Some(Err(trf(
                "These mods can't be built: {mods}",
                &[("mods", &failed.join("; "))],
            )));
        }
    }

    pub(in crate::mods) fn source_collection(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let activity: Option<(Vec<String>, sunrise::InstallProgress)> =
            self.source_activity().map(|a| (a.ids.to_vec(), a.progress));
        let built = self.built_sources();
        let can_start = self.nightly_catalogue_current() && !self.nightly_busy();
        let state = &self.ui.mods.nightly;
        let back = super::super::project::back(tr("Nightly mods")).on_click(cx.listener(
            |this, _, _, cx| {
                this.ui.mods.nightly.collection = None;
                this.choose_view(Page::Discover);
                cx.notify();
            },
        ));
        let page = div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .p(px(24.))
            .gap(px(12.))
            .text_color(c(palette().text))
            .children(self.strip(cx));
        let found = state.collection.as_ref().and_then(|open| {
            let catalogue = state.catalogue.as_ref()?;
            let collection = catalogue
                .collections
                .iter()
                .find(|collection| collection.id == open.id)?
                .clone();
            let members: Vec<Source> = collection
                .members
                .iter()
                .filter_map(|member| {
                    catalogue
                        .sources
                        .iter()
                        .find(|source| source.id() == member.id)
                        .cloned()
                })
                .collect();
            Some((
                collection,
                members,
                open.ticked.clone(),
                open.pending.is_some(),
                catalogue.error.is_none(),
            ))
        });
        let Some((collection, members, ticked, preparing, allow_fetch)) = found else {
            return page
                .child(back)
                .child(muted(tr("This collection can't be shown.")))
                .into_any_element();
        };
        let icon = sources::SourceIcon::pages(&collection.icon, allow_fetch);
        let picture = state.work.icons.get(&icon).cloned().flatten();
        let load = (!state.work.icons.contains_key(&icon))
            .then(|| when_visible(cx, move |this| this.request_nightly_icon(icon)));
        let owners = collection.owners.join(", ");
        let head = div()
            .relative()
            .flex()
            .items_center()
            .gap(px(16.))
            .child(tile(picture.clone(), &collection.id, &collection.name, 64.))
            .children(load)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(tokens::DISPLAY))
                            .font_weight(FontWeight::BOLD)
                            .child(crate::selectable::selectable(collection.name.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_x(px(8.))
                            .gap_y(px(4.))
                            .child(note(trf("by {authors}", &[("authors", &owners)])))
                            .child(badge(tr("Collection"))),
                    ),
            );

        // The list: a header that picks all, then one row per mod.
        let pickable: Vec<&Source> = members
            .iter()
            .filter(|source| !built.contains(source.id()))
            .collect();
        let picked = pickable
            .iter()
            .filter(|source| ticked.contains(source.id()))
            .count();
        let all_state = model::collection::ticks(picked, pickable.len());
        let all_ids: Vec<String> = pickable.iter().map(|s| s.id().to_owned()).collect();
        let list_head = div()
            .id("nightly-collection-all")
            .reach()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(10.))
            .cursor_pointer()
            .text_size(px(tokens::LABEL))
            .text_color(c(palette().muted))
            .child(checkbox_of_set("nightly-collection-all-box", all_state).tab_stop(false))
            .child(trf(
                "{picked} of {all} selected",
                &[
                    ("picked", &picked.to_string()),
                    ("all", &pickable.len().to_string()),
                ],
            ))
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(open) = this.ui.mods.nightly.collection.as_mut() {
                    if all_ids.iter().all(|id| open.ticked.contains(id)) {
                        open.ticked.clear();
                    } else {
                        open.ticked.extend(all_ids.iter().cloned());
                    }
                }
                cx.notify();
            }));
        let mut rows = card().child(list_head);
        for (i, source) in members.iter().enumerate() {
            let installed = built.contains(source.id());
            let on = installed || ticked.contains(source.id());
            let open = source.clone();
            let name = div()
                .id(("nightly-collection-name", i))
                .reach()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(tokens::LABEL))
                .cursor_pointer()
                .hover(|s| s.text_color(c(palette().accent_ink)))
                .child(source.name().to_owned())
                // The row's own press ticks it: this one only opens the page.
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.ui.mods.nightly.collection = None;
                    this.inspect_nightly(open.clone(), false);
                    cx.notify();
                }));
            // The installer's own result for this mod, as its page shows it.
            let running = activity
                .as_ref()
                .filter(|(ids, _)| ids.iter().any(|id| id == source.id()));
            let failed = self
                .source_outcome(source.id())
                .and_then(|outcome| outcome.as_ref().err())
                .filter(|_| !installed && running.is_none());
            let right = if installed {
                Some(status("check", tr("Installed"), palette().success))
            } else if let Some((_, progress)) = running {
                Some(status("hammer", stage_label(*progress), palette().muted))
            } else {
                failed.map(|_| status("information-circle", tr("Build failed"), palette().warning))
            };
            let why = failed.map(|error| {
                let log = match error {
                    SourceError::Build { log: Some(log), .. } => Some(log.clone()),
                    _ => None,
                };
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(6.))
                    .pt(px(4.))
                    .child(note(
                        source_error(error, Some(source)).unwrap_or_else(|| error.to_string()),
                    ))
                    .children(log.map(|log| self.nightly_log_button(source.name(), log, cx)))
            });
            let tick = checkbox(("nightly-collection-box", i), on).tab_stop(false);
            let member = source.id().to_owned();
            rows = rows.child(
                div()
                    .id(("nightly-collection-row", i))
                    .reach()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(14.))
                    .py(px(10.))
                    .border_t_1()
                    .border_color(c(palette().divider))
                    .when(!installed, |d| d.cursor_pointer())
                    .child(match installed {
                        true => dim(tick),
                        false => tick,
                    })
                    .child(self.nightly_icon_tile(source, None, allow_fetch, 36., cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(name)
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(tokens::SMALL))
                                    .text_color(c(palette().muted))
                                    .child(source.summary().to_owned()),
                            )
                            .children(why),
                    )
                    .children(right)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if installed {
                            return;
                        }
                        if let Some(open) = this.ui.mods.nightly.collection.as_mut() {
                            if !open.ticked.remove(&member) {
                                open.ticked.insert(member.clone());
                            }
                        }
                        cx.notify();
                    })),
            );
        }

        // The side panel: the one Build, and what it does.
        let label = match (preparing, picked) {
            (true, _) => tr("Checking the sources…").to_string(),
            (false, 0) if pickable.is_empty() => tr("Installed").to_string(),
            (false, n) => trf("Build {count} mods", &[("count", &n.to_string())]),
        };
        let action = crate::frame::primary("nightly-collection-build", label)
            .w_full()
            .h(px(tokens::ACTION));
        let action = if can_start && !preparing && picked > 0 {
            action.on_click(cx.listener(|this, _, _, cx| {
                this.build_source_collection();
                cx.notify();
            }))
        } else {
            dim(action)
        };
        let right = card()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(tile(picture, &collection.id, &collection.name, 40.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::BOLD)
                                    .child(collection.name.clone()),
                            )
                            .child(muted(trf(
                                "Collection · {count} mods",
                                &[("count", &collection.members.len().to_string())],
                            ))),
                    ),
            )
            .child(action)
            .child(muted(tr(
                "Each mod builds from its own repository on this computer. You're asked once, for all of them.",
            )))
            .child(muted(tr(
                "Build directly from GitHub. Listed source code is not reviewed.",
            )));
        let (above, beside) = match self.content_width() < 860. {
            true => (Some(right), None),
            false => (None, Some(right.w(px(360.)).flex_none())),
        };
        let about_text = match collection.description.trim().is_empty() {
            true => collection.summary.clone(),
            false => collection.description.clone(),
        };
        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(head)
            .children(above)
            .child(super::super::project::prose(self.theme, &about_text))
            .child(
                div()
                    .text_size(px(tokens::HEADING))
                    .font_weight(FontWeight::BOLD)
                    .child(tr("Mods")),
            )
            .child(rows);
        page.child(
            div()
                .id("nightly-collection-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&crate::frame::scrolls("nightly-collection"))
                .child(
                    div().flex().flex_col().gap(px(14.)).child(back).child(
                        div()
                            .flex()
                            .items_start()
                            .gap(px(24.))
                            .child(left)
                            .children(beside),
                    ),
                ),
        )
        .into_any_element()
    }
}

/// The ticked listings of the current list's collection `id`, in its order, leaving out mods
/// already built and anything no longer a member.
fn collection_picks(
    catalogue: &sources::Catalogue,
    id: &str,
    ticked: &BTreeSet<String>,
    built: &BTreeSet<String>,
) -> Vec<Source> {
    catalogue
        .collections
        .iter()
        .find(|collection| collection.id == id)
        .into_iter()
        .flat_map(|collection| &collection.members)
        .filter(|member| ticked.contains(&member.id) && !built.contains(&member.id))
        .filter_map(|member| catalogue.sources.iter().find(|s| s.id() == member.id))
        .cloned()
        .collect()
}

fn failure(error: &SourceError, source: Option<&Source>) -> (String, String) {
    eprintln!("Nightly collection preflight: {error}");
    let why = source_error(error, source).unwrap_or_else(|| error.to_string());
    (
        source.map_or_else(String::new, |s| s.name().to_owned()),
        why,
    )
}
