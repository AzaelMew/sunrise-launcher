//! Mods › Discover (mock 03): the registry's mods, filtered, sorted, one click to Add.
use super::model::{self, Make, Sort};
use super::ui::*;
use super::*;
use crate::paint::palette;
use gpui::deferred;
use sunrise_launcher::browser::details::category_label;
use sunrise_launcher::browser::images;

/// How many cards are drawn before "Show more".
pub(super) const PAGE: usize = 48;

/// A dropdown line: its label, whether it is the current choice, and what picking it does.
pub(super) type MenuOption = (SharedString, bool, Box<dyn Fn(&mut Launcher)>);

impl Launcher {
    pub(super) fn discover(
        &mut self,
        window: &mut Window,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let page = self.discover_shell(false, window, focus, cx);
        let (cards, categories): (Vec<Card>, Vec<String>) = match &self.ui.mods.catalog {
            Catalog::Ready(cards, cats) => (cards.clone(), cats.clone()),
            Catalog::Failed(e) => {
                let e = e.clone();
                return page
                    .child(
                        card()
                            .p(px(20.))
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(tr("The Sunrise registry can't be reached")),
                            )
                            .child(muted(e))
                            .child(
                                crate::frame::button("alt-mods-retry", tr("Try again")).on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.ui.mods.catalog = Catalog::Idle;
                                        cx.notify();
                                    }),
                                ),
                            ),
                    )
                    .into_any_element();
            }
            _ => {
                return page
                    .child(muted(tr("Loading the registry…")))
                    .into_any_element();
            }
        };

        let f = self.ui.mods.filters.clone();
        let shown = model::discover(&cards, &f);
        let total = shown.len();
        let limit = self.ui.mods.limit.max(PAGE);
        let index = sunrise::known_index();

        // The filters: category and kind as dropdowns, fits my game, and the order.
        let fits = div()
            .id("alt-fits")
            .reach()
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .text_size(px(tokens::LABEL))
            // The line is the Tab stop, not the switch in it.
            .child(ui::switch("alt-fits-switch", f.fits_only).tab_stop(false))
            .child(tr("Fits my game"))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                let f = &mut this.ui.mods.filters;
                f.fits_only = !f.fits_only;
                cx.notify();
            }));
        let category_now = f.category.as_deref().map_or_else(
            || tr("All categories").to_string(),
            |k| category_label(k).to_string(),
        );
        let make_label = f.make.map_or(tr("Any kind"), Make::label);
        let category_options = || {
            std::iter::once((
                tr("All categories").into(),
                f.category.is_none(),
                Box::new(|this: &mut Launcher| this.ui.mods.filters.category = None)
                    as Box<dyn Fn(&mut Launcher)>,
            ))
            .chain(categories.iter().map(|k| {
                let key = k.clone();
                (
                    category_label(k).into(),
                    f.category.as_ref() == Some(k),
                    Box::new(move |this: &mut Launcher| {
                        this.ui.mods.filters.category = Some(key.clone())
                    }) as Box<dyn Fn(&mut Launcher)>,
                )
            }))
            .collect()
        };
        let make_options = || {
            std::iter::once((
                tr("Any kind").into(),
                f.make.is_none(),
                Box::new(|this: &mut Launcher| this.ui.mods.filters.make = None)
                    as Box<dyn Fn(&mut Launcher)>,
            ))
            .chain(Make::ALL.into_iter().map(|m| {
                (
                    m.label().into(),
                    f.make == Some(m),
                    Box::new(move |this: &mut Launcher| this.ui.mods.filters.make = Some(m))
                        as Box<dyn Fn(&mut Launcher)>,
                )
            }))
            .collect()
        };
        let sort_options = || {
            Sort::ALL
                .into_iter()
                .map(|s| {
                    (
                        s.label().into(),
                        f.sort == s,
                        Box::new(move |this: &mut Launcher| this.ui.mods.filters.sort = s)
                            as Box<dyn Fn(&mut Launcher)>,
                    )
                })
                .collect()
        };
        // What narrows the list on the left, what orders it on the right.
        let page = page.child(discover_filters(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap_x(px(18.))
                .child(self.discover_menu(&category_now, Menu::Category, category_options, cx))
                .child(self.discover_menu(make_label, Menu::Make, make_options, cx))
                .child(fits),
            self.discover_menu(f.sort.label(), Menu::Sort, sort_options, cx),
        ));

        // Read readiness once for this frame, separately from the catalogue filter.
        let game = self.mods_game();
        let readiness = self.selected_instance().map(|instance| {
            let base = self.version_of(instance).map(String::as_str);
            sunrise::NativeInstallReadiness::for_profile(&instance.home, base, &game)
        });
        // The cards.
        let mut grid = self.discover_grid();
        for (n, &i) in shown.iter().take(limit).enumerate() {
            let card_data = &cards[i];
            let id = card_data.id.clone();
            let picture = card_data
                .icon
                .clone()
                .and_then(|url| self.mods_image(&url, images::ICON));
            let installed = self.discover_installed();
            let collection = index.as_ref().and_then(|index| index.collection(&id));
            let entry = installed.and_then(|rows| rows.registry_entry(&id));
            let version = entry.map(|entry| entry.number.as_str());
            let package = installed
                .zip(entry)
                .and_then(|(rows, entry)| rows.archives.get(&entry.file)?.icon.clone());
            let is_added = collection.map_or(version.is_some(), |collection| {
                model::collection::installed(collection, &cards, |id| {
                    installed.is_some_and(|rows| rows.registry_version(id).is_some())
                })
            });
            let working = self.registry_card_control(&id, |label| {
                dim(crate::frame::button(("alt-add", n), label)).into_any_element()
            });
            let issue = self.registry_card_note(&id);
            let unavailable = (!is_added && card_data.fit.fits() && card_data.make == Make::Native)
                .then(|| readiness.as_ref()?.block(index.as_deref()?, &id, &game))
                .flatten();
            let note = unavailable
                .clone()
                .or_else(|| issue.clone())
                .or_else(|| card_data.fit.reason().filter(|_| !is_added));
            let control = if working.is_some() {
                working
            } else if is_added || !card_data.fit.fits() {
                None
            } else if unavailable.is_some() {
                Some(dim(crate::frame::button(("alt-add", n), tr("Install"))).into_any_element())
            } else {
                let label = if card_data.members.is_some() {
                    tr("Choose mods").into()
                } else {
                    self.registry_install_label()
                };
                let b = crate::frame::button(("alt-add", n), label);
                let id = id.clone();
                let collection = card_data.members.is_some();
                Some(
                    b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        if collection {
                            this.mods_show_project(id.clone());
                        } else {
                            this.add(id.clone());
                        }
                        cx.notify();
                    }))
                    .into_any_element(),
                )
            };
            let mut facts = match card_data.members {
                Some(count) => vec![
                    badge(tr("Collection")),
                    muted(trf("{count} mods", &[("count", &count.to_string())])),
                    muted(model::downloads(card_data.downloads)),
                ],
                None => vec![
                    tag(card_data.make.label()),
                    muted(model::downloads(card_data.downloads)),
                ],
            };
            if card_data.library {
                facts.insert(1, badge(tr("Library")));
            }
            if index
                .as_ref()
                .and_then(|index| index.project(&id))
                .is_some_and(|project| project.key.is_none())
            {
                facts.insert(1, badge(tr("Unsigned")));
            }
            if let Some(name) = &card_data.part_of {
                facts.push(muted(trf("Part of {collection}", &[("collection", name)])));
            }
            if card_data.builds_locally && card_data.fit.fits() && unavailable.is_none() {
                facts.push(muted(tr("Builds on this computer")));
            }
            if let Some(rows) = installed {
                facts.extend(match collection {
                    Some(collection) => collection_install_fact(
                        collection,
                        |id| rows.registry_version(id).is_some(),
                        |id| rows.registry_pending(id),
                    ),
                    None => {
                        version.map(|version| install_fact(version, rows.registry_pending(&id)))
                    }
                });
            }
            let open = id.clone();
            grid = grid.child(
                browse_card(
                    ("alt-card", n),
                    mod_artwork(picture, package),
                    &id,
                    &card_data.name,
                    &card_data.authors,
                    &card_data.summary,
                    facts,
                    control,
                    note,
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.mods_show_project(open.clone());
                    cx.notify();
                })),
            );
        }
        let empty = if f.text.is_empty() && f.category.is_none() {
            tr("No mods for this build yet.")
        } else {
            tr("No mods match. Try another search, or turn off “Fits my game”.")
        };
        page.child(self.discover_results(grid, total, "discover", empty, cx))
            .into_any_element()
    }

    pub(super) fn discover_shell(
        &mut self,
        sources: bool,
        window: &mut Window,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> Div {
        let header = self.mods_header(false, focus, window, cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(24.))
            .text_color(c(palette().text))
            .child(header)
            .child(self.discover_profile_line(sources))
            .children(self.strip(cx))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.ui.mods.menu.take().is_some() {
                        cx.notify();
                    }
                }),
            )
    }

    pub(super) fn discover_grid(&self) -> Div {
        div()
            .grid()
            .grid_cols(columns(self.content_width()))
            .gap(px(12.))
    }

    pub(super) fn discover_results(
        &self,
        grid: Div,
        total: usize,
        scroll: &'static str,
        empty: &'static str,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let limit = self.ui.mods.limit.max(PAGE);
        div()
            .id("mods-discover-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&crate::frame::scrolls(scroll))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .when(total == 0, |body| body.child(muted(empty)))
                    .when(total > 0, |body| body.child(grid))
                    .children((total > limit).then(|| {
                        crate::frame::button(
                            "mods-discover-more",
                            trf(
                                "Show more ({count} more)",
                                &[("count", &(total - limit).to_string())],
                            ),
                        )
                        .on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.ui.mods.limit = limit + PAGE;
                                cx.notify();
                            },
                        ))
                    })),
            )
    }

    pub(super) fn discover_menu(
        &self,
        label: &str,
        which: Menu,
        options: impl FnOnce() -> Vec<MenuOption>,
        cx: &mut Context<Self>,
    ) -> Div {
        // A menu opens under its dropdown, from the side the dropdown sits on.
        let popover = |options: Vec<MenuOption>, from_left: bool, cx: &mut Context<Self>| {
            deferred(
                div()
                    .absolute()
                    .top(px(38.))
                    .when(from_left, |d| d.left(px(0.)))
                    .when(!from_left, |d| d.right(px(0.)))
                    .w(px(200.))
                    .p(px(6.))
                    .rounded(crate::tokens::r(crate::tokens::R_CONTROL))
                    .border_1()
                    .border_color(c(palette().line))
                    .bg(c(palette().raised))
                    .shadow_lg()
                    .occlude()
                    .flex()
                    .flex_col()
                    .children(
                        options
                            .into_iter()
                            .enumerate()
                            .map(|(i, (label, on, pick))| {
                                div()
                                    .id(("alt-menu-option", i))
                                    .reach()
                                    .px(px(10.))
                                    .py(px(7.))
                                    .rounded(crate::tokens::r(crate::tokens::R_SMALL))
                                    .text_size(px(tokens::LABEL))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(c(palette().hover)))
                                    .when(on, |d| d.font_weight(FontWeight::SEMIBOLD))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        pick(this);
                                        this.ui.mods.menu = None;
                                        cx.notify();
                                    }))
                            }),
                    ),
            )
            .with_priority(10)
        };
        let id = match which {
            Menu::Category => "alt-category-menu",
            Menu::Make => "alt-make-menu",
            Menu::Sort => "alt-sort-menu",
        };
        let trigger = div()
            .id(id)
            .reach()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(tokens::CONTROL))
            .cursor_pointer()
            .text_size(px(tokens::LABEL))
            .child(label.to_owned())
            .child(blocks::icon("chevron-down", 13., c(palette().muted)))
            // A press here is the dropdown's: the page closing any open menu on a press would
            // shut this one just before its click opened it again.
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                let same = this.ui.mods.menu == Some(which);
                this.ui.mods.menu = (!same).then_some(which);
                cx.notify();
            }));
        div().relative().child(trigger).children(
            (self.ui.mods.menu == Some(which)).then(|| popover(options(), which != Menu::Sort, cx)),
        )
    }

    /// Never label cards from another profile or from an incomplete installed read.
    pub(super) fn discover_installed(&self) -> Option<&InstalledRows> {
        self.ui
            .mods
            .snapshot
            .as_ref()
            .filter(|snapshot| snapshot.ready() && snapshot.home == self.mods_home())
            .map(|snapshot| &snapshot.rows)
    }
}

impl InstalledRows {
    fn registry_entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| {
            entry.source == SourceId::Sunrise && entry.kind == Kind::Mod && entry.project == id
        })
    }

    fn registry_version(&self, id: &str) -> Option<&str> {
        self.registry_entry(id).map(|entry| entry.number.as_str())
    }

    pub(super) fn registry_pending(&self, id: &str) -> bool {
        self.registry_entry(id)
            .and_then(sunrise::mod_id)
            .is_some_and(|id| self.unbuilt.contains(id))
    }
}

pub(super) fn install_fact(version: &str, pending: bool) -> Div {
    if pending {
        status("clock", tr(ADDED_PENDING), palette().warning)
    } else {
        status(
            "tick-02",
            trf("Installed · {version}", &[("version", version)]),
            palette().success,
        )
    }
}

pub(super) fn collection_install_fact(
    collection: &sunrise_launcher_smod::index::Collection,
    installed: impl Fn(&str) -> bool,
    pending: impl Fn(&str) -> bool,
) -> Option<Div> {
    let count = collection
        .members
        .iter()
        .filter(|member| installed(&member.id))
        .count();
    (count > 0).then(|| {
        let waiting = collection
            .members
            .iter()
            .any(|member| installed(&member.id) && pending(&member.id));
        div()
            .flex()
            .flex_wrap()
            .gap(px(8.))
            .child(muted(trf(
                "{added} of {total} added",
                &[
                    ("added", &count.to_string()),
                    ("total", &collection.members.len().to_string()),
                ],
            )))
            .children(waiting.then(|| status("clock", tr(ADDED_PENDING), palette().warning)))
    })
}

pub(super) fn discover_filters(filters: Div, order: Div) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap_x(px(18.))
        .child(filters)
        .child(order)
}
