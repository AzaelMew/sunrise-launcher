//! Mods › Discover (mock 03): the registry's mods, filtered, sorted, one click to Add.
use super::model::{self, Make, Sort};
use super::ui::*;
use super::*;
use crate::alt::{BG, CARD, LINE, MUTED, TEXT};
use gpui::deferred;
use sunrise_launcher::browser::details::category_label;
use sunrise_launcher::browser::images;

/// How many cards are drawn before "Show more".
const PAGE: usize = 48;

/// A dropdown line: its label, whether it is the current choice, and what picking it does.
type MenuOption = (&'static str, bool, Box<dyn Fn(&mut Launcher)>);

impl Launcher {
    pub(super) fn alt_discover(
        &mut self,
        window: &mut Window,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = self.alt_mods_header(false, focus, window, cx);
        let mut page = div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(24.))
            .text_color(c(TEXT))
            .child(header)
            .children(self.alt.mods.examples.then(example_banner))
            .children(self.alt_strip(cx));

        let examples = self.alt.mods.examples;
        let (cards, categories): (Vec<Card>, Vec<String>) = match (&self.alt.mods.catalog, examples)
        {
            (_, true) => {
                let cards = examples::cards();
                let cats = model::categories(&cards);
                (cards, cats)
            }
            (Catalog::Ready(cards, cats), _) => (cards.clone(), cats.clone()),
            (Catalog::Failed(e), _) => {
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
                                crate::alt::button("alt-mods-retry", tr("Try again")).on_click(
                                    cx.listener(|this, _: &ClickEvent, _, cx| {
                                        this.alt.mods.catalog = Catalog::Idle;
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

        let f = self.alt.mods.filters.clone();
        let shown = model::discover(&cards, &f);
        let total = shown.len();
        let limit = self.alt.mods.limit.max(PAGE);
        let added: HashSet<String> = self
            .alt
            .mods
            .snapshot
            .as_ref()
            .map(|s| s.entries.iter().map(|e| e.project.clone()).collect())
            .unwrap_or_default();
        let adding = self.mods.busy.as_ref().map(|b| b.0.clone());
        let native_allowed = self.settings.native_mods_allowed_at.is_some();

        // Category chips, then the three controls.
        let mut chips = div().flex().flex_wrap().items_center().gap(px(8.)).flex_1();
        chips = chips.child(
            chip("alt-cat-all", tr("All"), f.category.is_none()).on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    this.alt.mods.filters.category = None;
                    cx.notify();
                },
            )),
        );
        for (i, k) in categories.iter().take(8).enumerate() {
            let key = k.clone();
            chips = chips.child(
                chip(
                    ("alt-cat", i),
                    category_label(k),
                    f.category.as_ref() == Some(k),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let filters = &mut this.alt.mods.filters;
                    filters.category =
                        (filters.category.as_ref() != Some(&key)).then(|| key.clone());
                    cx.notify();
                })),
            );
        }
        let fits = div()
            .id("alt-fits")
            .flex()
            .items_center()
            .gap(px(8.))
            .cursor_pointer()
            .text_size(px(13.))
            .child(ui::switch("alt-fits-switch", f.fits_only))
            .child(tr("Fits my build"))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                let f = &mut this.alt.mods.filters;
                f.fits_only = !f.fits_only;
                cx.notify();
            }));
        let open_menu = |which: Menu, this: &Launcher| {
            matches!(
                (&this.alt.mods.menu, &which),
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
                    .rounded(px(10.))
                    .border_1()
                    .border_color(c(LINE))
                    .bg(c(CARD))
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
                                    .px(px(10.))
                                    .py(px(7.))
                                    .rounded(px(6.))
                                    .text_size(px(13.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(c(LINE)))
                                    .when(on, |d| d.font_weight(FontWeight::SEMIBOLD))
                                    .child(label)
                                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                        pick(this);
                                        this.alt.mods.menu = None;
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
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(32.))
                .cursor_pointer()
                .text_size(px(13.))
                .child(label.to_owned())
                .child(div().text_color(c(MUTED)).child("⌄"))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let same = matches!(
                        (&this.alt.mods.menu, &which),
                        (Some(Menu::Make), Menu::Make) | (Some(Menu::Sort), Menu::Sort)
                    );
                    this.alt.mods.menu = (!same).then_some(match which {
                        Menu::Make => Menu::Make,
                        Menu::Sort => Menu::Sort,
                    });
                    cx.notify();
                }))
        };
        let make_options: Vec<MenuOption> = std::iter::once((
            tr("Any kind"),
            f.make.is_none(),
            Box::new(|this: &mut Launcher| this.alt.mods.filters.make = None)
                as Box<dyn Fn(&mut Launcher)>,
        ))
        .chain(Make::ALL.into_iter().map(|m| {
            (
                m.label(),
                f.make == Some(m),
                Box::new(move |this: &mut Launcher| this.alt.mods.filters.make = Some(m))
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
                    Box::new(move |this: &mut Launcher| this.alt.mods.filters.sort = s)
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
        let mut grid = div().grid().grid_cols(3).gap(px(12.));
        for (n, &i) in shown.iter().take(limit).enumerate() {
            let card_data = &cards[i];
            let id = card_data.id.clone();
            let picture = card_data
                .icon
                .clone()
                .filter(|_| !card_data.example)
                .and_then(|url| self.mods_image(&url, images::ICON));
            let is_added = added.contains(&id);
            let working = adding.as_deref() == Some(id.as_str());
            let action: AnyElement = if is_added {
                div()
                    .text_size(px(13.))
                    .text_color(c(0x7fd6a0))
                    .child(tr("✓ Added"))
                    .into_any_element()
            } else if let Some(why) = card_data.fit.reason() {
                muted(why).into_any_element()
            } else {
                let needs_ok = card_data.make == Make::Native && !native_allowed;
                let label = match (
                    working,
                    needs_ok || card_data.builds_locally && card_data.make == Make::Native,
                ) {
                    (true, _) => tr("Adding…"),
                    (_, true) => tr("Add…"),
                    _ => tr("Add"),
                };
                let b = crate::alt::button(("alt-add", n), label);
                match (card_data.example, working) {
                    (true, _) | (_, true) => dim(b).into_any_element(),
                    _ => {
                        let id = id.clone();
                        b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            this.alt_add(id.clone());
                            cx.notify();
                        }))
                        .into_any_element()
                    }
                }
            };
            let open = id.clone();
            grid = grid.child(
                div()
                    .id(("alt-card", n))
                    .flex()
                    .gap(px(12.))
                    .p(px(12.))
                    .min_w_0()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(c(LINE))
                    .bg(c(CARD))
                    .cursor_pointer()
                    .hover(|s| s.border_color(c(MUTED)))
                    .child(tile(picture, &card_data.name, 48.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .truncate()
                                            .child(card_data.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(c(MUTED))
                                            .truncate()
                                            .child(card_data.authors.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(c(MUTED))
                                    .truncate()
                                    .child(card_data.summary.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.))
                                            .child(tag(card_data.make.label()))
                                            .child(muted(model::downloads(card_data.downloads)))
                                            .children(
                                                (card_data.builds_locally && card_data.fit.fits())
                                                    .then(|| muted(tr("· builds locally"))),
                                            ),
                                    )
                                    .child(action),
                            ),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.alt_mods_show_project(open.clone());
                        cx.notify();
                    })),
            );
        }
        let body = if shown.is_empty() {
            div().child(muted(if f.text.is_empty() && f.category.is_none() {
                tr("The registry has no mods for this build yet.")
            } else {
                tr("No mods match. Try a different search, or turn off “Fits my build”.")
            }))
        } else {
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(grid)
                .children((total > limit).then(|| {
                    crate::alt::button(
                        "alt-show-more",
                        trf(
                            "Show more ({count} more)",
                            &[("count", &(total - limit).to_string())],
                        ),
                    )
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.alt.mods.limit = this.alt.mods.limit.max(PAGE) + PAGE;
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
                .child(body),
        )
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if this.alt.mods.menu.take().is_some() {
                    cx.notify();
                }
            }),
        )
        .bg(c(BG))
        .into_any_element()
    }
}
