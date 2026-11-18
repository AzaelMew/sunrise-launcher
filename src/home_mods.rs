//! Home's Mods column: what the selected profile plays with, to read. Its mods and its
//! resource and shader packs that are on, each a row that opens the item's own page; what is
//! off is one quiet line that leads to Mods, where things are turned on and off.
use super::*;
use crate::frame::{button, Page, Reach as _};
use crate::home::{card, heading, muted};
use crate::paint::palette;
use gpui::{rgb, ImageSource};
use std::sync::Arc;
use sunrise_launcher::packs::Kind as PackKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Mod,
    Resource,
    Shader,
}

impl Kind {
    /// What a row says under the name.
    fn label(self) -> &'static str {
        match self {
            Kind::Mod => "Mod",
            Kind::Resource => "Resource pack",
            Kind::Shader => "Shader pack",
        }
    }
}

/// Where a row's picture comes from.
#[derive(Clone)]
pub(crate) enum Picture {
    /// A mod's icon, by its address in the registry.
    Address(Option<String>),
    /// A pack's `pack.png`.
    Image(Option<Arc<gpui::Image>>),
}

/// What a press on a row opens.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Open {
    /// A registry mod's page, by its id.
    Project(String),
    /// A mod the registry doesn't know: Mods › Installed, showing it by its name.
    Installed(String),
    /// A pack: its shelf, showing it by its name.
    Pack(PackKind, String),
}

/// One installed mod or pack of the profile.
#[derive(Clone)]
pub(crate) struct Item {
    pub id: String,
    pub name: String,
    pub kind: Kind,
    pub picture: Picture,
    pub open: Open,
    pub on: bool,
}

/// What the column lists: the items that are on, in the order given, and how many are off.
pub(crate) struct Column {
    pub shown: Vec<Item>,
    pub mods_off: usize,
    pub packs_off: usize,
}

impl Column {
    pub(crate) fn off(&self) -> usize {
        self.mods_off + self.packs_off
    }

    /// Where "N turned off" leads: the mods that are off, or the packs when only packs are.
    pub(crate) fn off_page(&self) -> Page {
        match self.mods_off {
            0 => Page::Packs,
            _ => Page::Installed,
        }
    }
}

/// The column for a profile with `items` installed; `None` when it has nothing at all.
pub(crate) fn column(items: Vec<Item>) -> Option<Column> {
    if items.is_empty() {
        return None;
    }
    let off = |mods: bool| {
        items
            .iter()
            .filter(|i| !i.on && (i.kind == Kind::Mod) == mods)
            .count()
    };
    Some(Column {
        mods_off: off(true),
        packs_off: off(false),
        shown: items.into_iter().filter(|i| i.on).collect(),
    })
}

/// Home's two columns stand side by side from this window width; under it they stack.
const SIDE_BY_SIDE_FROM: f32 = 1000.;

pub(crate) fn side_by_side(window_width: f32) -> bool {
    window_width >= SIDE_BY_SIDE_FROM
}

const PICTURE: f32 = 40.;

/// A mod's or a pack's picture, `side` square. It takes what the launcher's shared tile for
/// these takes (the picture, the item's id and name, the side), so that one replaces this call
/// for call; until then an item without a picture gets the package glyph.
fn tile(picture: Option<impl Into<ImageSource>>, _id: &str, _name: &str, side: f32) -> Div {
    let square = div()
        .flex_none()
        .size(px(side))
        .rounded(px(tokens::R_CONTROL))
        .overflow_hidden()
        .flex()
        .items_center()
        .justify_center();
    match picture {
        Some(picture) => square.child(gpui::img(picture).size(px(side))),
        None => square.bg(rgb(palette().selected)).child(blocks::icon(
            "package",
            side * 0.45,
            rgb(palette().muted).into(),
        )),
    }
}

/// A line of text that is pressed like a link: a Tab stop with the ring.
fn link(id: &'static str, label: String) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .cursor_pointer()
        .tab_index(0)
        .rounded(px(tokens::R_SMALL))
        .text_size(px(tokens::SMALL))
        .focus_visible(|s| s.shadow(crate::frame::focus_ring(palette().focus)))
        .seen()
        .child(label)
}

impl Launcher {
    fn open_home_item(&mut self, open: Open) {
        match open {
            Open::Project(id) => self.mods_show_project(id),
            Open::Installed(name) => {
                self.navigate(Page::Installed);
                self.show_installed_mod(&name);
            }
            Open::Pack(kind, name) => {
                self.navigate(Page::Packs);
                self.show_installed_pack(kind, &name);
            }
        }
    }

    /// Home's Mods column for the selected profile.
    pub(crate) fn home_mods(&mut self, cx: &mut Context<Self>) -> Div {
        let column = column(self.mods_for_home());
        let head = div()
            .h(px(tokens::CONTROL))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .child(heading(tr("Mods")))
            .child(
                button("alt-home-manage-mods", tr("Manage mods")).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.navigate(Page::Discover);
                        cx.notify();
                    },
                )),
            );
        let body = match column {
            None => div()
                .flex()
                .items_center()
                .gap(px(8.))
                .py(px(8.))
                .child(muted(tr("No mods yet")))
                .child(muted("·"))
                .child(
                    link("alt-home-browse-mods", tr("Browse mods").to_string())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(palette().text))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.navigate(Page::Discover);
                            cx.notify();
                        })),
                ),
            Some(column) => {
                let mut rows = div().flex().flex_col().gap(px(4.));
                for (n, item) in column.shown.iter().enumerate() {
                    let picture = match &item.picture {
                        Picture::Address(address) => tile(
                            address.as_ref().and_then(|url| {
                                self.mods_image(url, sunrise_launcher::browser::images::ROW)
                            }),
                            &item.id,
                            &item.name,
                            PICTURE,
                        ),
                        Picture::Image(image) => tile(image.clone(), &item.id, &item.name, PICTURE),
                    };
                    let open = item.open.clone();
                    rows = rows.child(
                        card()
                            .id(("alt-home-mod", n))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .p(px(5.))
                            .pr(px(10.))
                            .cursor_pointer()
                            .tab_index(0)
                            .hover(|s| s.bg(rgb(palette().hover)))
                            .focus_visible(|s| s.border_color(rgb(palette().focus)))
                            .seen()
                            .child(picture)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .text_size(px(tokens::LABEL))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(palette().text))
                                            .truncate()
                                            .child(item.name.clone()),
                                    )
                                    .child(muted(tr(item.kind.label()))),
                            )
                            .child(blocks::icon(
                                "chevron-right",
                                14.,
                                rgb(palette().muted).into(),
                            ))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.open_home_item(open.clone());
                                cx.notify();
                            })),
                    );
                }
                let off = column.off();
                if off > 0 {
                    let page = column.off_page();
                    let label = match off {
                        1 => tr("1 turned off").to_string(),
                        _ => trf("{count} turned off", &[("count", &off.to_string())]),
                    };
                    rows = rows.child(
                        div().flex().mt(px(4.)).px(px(6.)).child(
                            link("alt-home-mods-off", label)
                                .text_color(rgb(palette().muted))
                                .hover(|s| s.text_color(rgb(palette().text)))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.navigate(page);
                                    if page == Page::Installed {
                                        this.show_mods_turned_off();
                                    }
                                    cx.notify();
                                })),
                        ),
                    );
                }
                rows
            }
        };
        div().flex().flex_col().gap(px(10.)).child(head).child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str, kind: Kind, on: bool) -> Item {
        Item {
            id: name.to_lowercase(),
            name: name.into(),
            kind,
            picture: Picture::Address(None),
            open: Open::Installed(name.into()),
            on,
        }
    }

    fn names(column: &Column) -> Vec<&str> {
        column.shown.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn the_column_lists_only_what_is_on_and_counts_what_is_off() {
        let column = column(vec![
            item("Lanternlight", Kind::Mod, true),
            item("Trailmarks", Kind::Mod, false),
            item("Quiet Rain", Kind::Mod, true),
            item("Soft Edges", Kind::Resource, true),
            item("Old Paper", Kind::Resource, false),
            item("Evening Glow", Kind::Shader, false),
        ])
        .unwrap();
        assert_eq!(names(&column), ["Lanternlight", "Quiet Rain", "Soft Edges"]);
        assert_eq!((column.mods_off, column.packs_off, column.off()), (1, 2, 3));
        // The line leads to the mods that are off when any mod is.
        assert!(column.off_page() == Page::Installed);
    }

    #[test]
    fn nothing_off_says_nothing_and_only_packs_off_leads_to_packs() {
        let all_on = column(vec![
            item("Lanternlight", Kind::Mod, true),
            item("Soft Edges", Kind::Resource, true),
        ])
        .unwrap();
        assert_eq!((all_on.shown.len(), all_on.off()), (2, 0));
        let packs = column(vec![
            item("Lanternlight", Kind::Mod, true),
            item("Evening Glow", Kind::Shader, false),
        ])
        .unwrap();
        assert_eq!(packs.off(), 1);
        assert!(packs.off_page() == Page::Packs);
    }

    #[test]
    fn a_profile_with_nothing_is_empty_and_one_with_everything_off_is_not() {
        assert!(column(Vec::new()).is_none());
        let all_off = column(vec![
            item("Trailmarks", Kind::Mod, false),
            item("Old Paper", Kind::Resource, false),
        ])
        .unwrap();
        assert!(all_off.shown.is_empty());
        assert_eq!(all_off.off(), 2);
    }

    #[test]
    fn every_kind_names_itself() {
        assert_eq!(
            [Kind::Mod, Kind::Resource, Kind::Shader].map(Kind::label),
            ["Mod", "Resource pack", "Shader pack"]
        );
    }

    #[test]
    fn the_columns_stack_on_a_narrow_window() {
        assert!(side_by_side(1280.));
        assert!(side_by_side(1000.));
        assert!(!side_by_side(999.));
        assert!(!side_by_side(800.));
    }
}
