//! Mods › Discover (mock 03): the registry's mods, filtered, sorted, one click to Add.
use super::model::{self, Make, Sort};
use super::ui::*;
use super::*;
use crate::paint::palette;
use gpui::deferred;
use sunrise_launcher::browser::details::category_label;
use sunrise_launcher::browser::images;

/// How many cards are drawn before "Show more".
const PAGE: usize = 48;

/// A dropdown line: its label, whether it is the current choice, and what picking it does.
type MenuOption = (&'static str, bool, Box<dyn Fn(&mut Launcher)>);

impl Launcher {
    pub(super) fn discover(
        &mut self,
        window: &mut Window,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = self.mods_header(false, focus, window, cx);
        let mut page = div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(24.))
            .text_color(c(palette().text))
            .child(header)
            .children(self.strip(cx));
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
        let added: HashSet<String> = self
            .ui
            .mods
            .snapshot
            .as_ref()
            .map(|s| s.entries.iter().map(|e| e.project.clone()).collect())
            .unwrap_or_default();
        let adding = self.mods.busy.as_ref().map(|b| b.0.clone());
        let native_allowed = self.settings.native_mods_allowed_at.is_some();

        // Category chips, then the three controls.
        let all: Choose = Rc::new(|this, _| this.ui.mods.filters.category = None);
        let chips = std::iter::once((tr("All").into(), f.category.is_none(), all))
            .chain(categories.iter().take(8).map(|k| {
                let key = k.clone();
                let pick: Choose = Rc::new(move |this, _| {
                    let filters = &mut this.ui.mods.filters;
                    filters.category =
                        (filters.category.as_ref() != Some(&key)).then(|| key.clone());
                });
                (
                    category_label(k).into(),
                    f.category.as_ref() == Some(k),
                    pick,
                )
            }))
            .collect();
        let chips = div()
            .flex()
            .flex_1()
            .child(choices("alt-cat", chips, cx).gap(px(8.)));
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
        let open_menu = |which: Menu, this: &Launcher| {
            matches!(
                (&this.ui.mods.menu, &which),
                (Some(Menu::Make), Menu::Make) | (Some(Menu::Sort), Menu::Sort)
            )
        };
        let make_open = open_menu(Menu::Make, self);
        let sort_open = open_menu(Menu::Sort, self);
        let make_label = f.make.map_or(tr("Any kind"), Make::label);
        let popover = |options: Vec<MenuOption>, cx: &mut Context<Self>| {
            deferred(
                div()
                    .absolute()
                    .top(px(38.))
                    .right(px(0.))
                    .w(px(200.))
                    .p(px(6.))
                    .rounded(px(tokens::R_CONTROL))
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
                                    .rounded(px(tokens::R_SMALL))
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
        let dropdown = |id: &'static str, label: &str, which: Menu, cx: &mut Context<Self>| {
            div()
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
                    let same = matches!(
                        (&this.ui.mods.menu, &which),
                        (Some(Menu::Make), Menu::Make) | (Some(Menu::Sort), Menu::Sort)
                    );
                    this.ui.mods.menu = (!same).then_some(match which {
                        Menu::Make => Menu::Make,
                        Menu::Sort => Menu::Sort,
                    });
                    cx.notify();
                }))
        };
        let make_options: Vec<MenuOption> = std::iter::once((
            tr("Any kind"),
            f.make.is_none(),
            Box::new(|this: &mut Launcher| this.ui.mods.filters.make = None)
                as Box<dyn Fn(&mut Launcher)>,
        ))
        .chain(Make::ALL.into_iter().map(|m| {
            (
                m.label(),
                f.make == Some(m),
                Box::new(move |this: &mut Launcher| this.ui.mods.filters.make = Some(m))
                    as Box<dyn Fn(&mut Launcher)>,
            )
        }))
        .collect();
        let sort_options: Vec<MenuOption> = Sort::ALL
            .into_iter()
            .map(|s| {
                (
                    s.label(),
                    f.sort == s,
                    Box::new(move |this: &mut Launcher| this.ui.mods.filters.sort = s)
                        as Box<dyn Fn(&mut Launcher)>,
                )
            })
            .collect();
        let controls = div()
            .flex()
            .items_center()
            .gap(px(18.))
            .child(fits)
            .child(
                div()
                    .relative()
                    .child(dropdown("alt-make-menu", make_label, Menu::Make, cx))
                    .children(make_open.then(|| popover(make_options, cx))),
            )
            .child(
                div()
                    .relative()
                    .child(dropdown("alt-sort-menu", f.sort.label(), Menu::Sort, cx))
                    .children(sort_open.then(|| popover(sort_options, cx))),
            );
        page = page.child(
            div()
                .flex()
                .items_start()
                .gap(px(16.))
                .child(chips)
                .child(controls),
        );

        // The cards.
        let mut grid = div()
            .grid()
            .grid_cols(columns(self.content_width()))
            .gap(px(12.));
        for (n, &i) in shown.iter().take(limit).enumerate() {
            let card_data = &cards[i];
            let id = card_data.id.clone();
            let picture = card_data
                .icon
                .clone()
                .and_then(|url| self.mods_image(&url, images::ICON));
            let is_added = added.contains(&id);
            let working = adding.as_deref() == Some(id.as_str());
            let note = card_data.fit.reason().filter(|_| !is_added);
            let control = if is_added {
                Some(added_mark())
            } else if note.is_some() {
                None
            } else {
                let needs_ok = card_data.make == Make::Native && !native_allowed;
                let label = match (
                    working,
                    needs_ok || card_data.builds_locally && card_data.make == Make::Native,
                ) {
                    (true, _) => tr("Installing…"),
                    (_, true) => tr("Install…"),
                    _ => tr("Install"),
                };
                let b = crate::frame::button(("alt-add", n), label);
                Some(match working {
                    true => dim(b).into_any_element(),
                    false => {
                        let id = id.clone();
                        let name = card_data.name.clone();
                        b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.add(id.clone(), name.clone());
                            cx.notify();
                        }))
                        .into_any_element()
                    }
                })
            };
            let mut facts = vec![
                tag(card_data.make.label()),
                muted(model::downloads(card_data.downloads)),
            ];
            if card_data.builds_locally && card_data.fit.fits() {
                facts.push(muted(tr("Builds on this computer")));
            }
            let open = id.clone();
            grid = grid.child(
                browse_card(
                    ("alt-card", n),
                    picture,
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
        let body = if shown.is_empty() {
            div().child(muted(if f.text.is_empty() && f.category.is_none() {
                tr("No mods for this build yet.")
            } else {
                tr("No mods match. Try another search, or turn off “Fits my game”.")
            }))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(grid)
                .children((total > limit).then(|| {
                    crate::frame::button(
                        "alt-show-more",
                        trf(
                            "Show more ({count} more)",
                            &[("count", &(total - limit).to_string())],
                        ),
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.ui.mods.limit = this.ui.mods.limit.max(PAGE) + PAGE;
                        cx.notify();
                    }))
                }))
        };
        page.child(
            div()
                .id("alt-discover-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&crate::frame::scrolls("discover"))
                .child(body),
        )
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.ui.mods.menu.take().is_some() {
                    cx.notify();
                }
            }),
        )
        .into_any_element()
    }
}
