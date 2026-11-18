//! Home's Mods column: what the selected profile plays with, to read. Its mods and its
//! resource and shader packs that are on, each a row that opens the item's own page; what is
//! off is one quiet line that leads to Mods, where things are turned on and off.
//!
//! What the profile has is read in the background ([`read`]) and kept ([`Inventory`]): drawing
//! Home opens no file of a mod or a pack. It is read again when the files it was read from
//! change ([`stamp`]).
use super::*;
use crate::frame::{button, Page, Reach as _};
use crate::home::{card, heading, muted};
use crate::paint::palette;
use gpui::{rgb, ImageSource};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use sunrise_launcher::browser::{installed, mod_list, sunrise};
use sunrise_launcher::packs::{self, Kind as PackKind};

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
    /// A registry mod's icon, by the mod's project there: asked for when its row is drawn.
    Project(String),
    /// The item's own picture, read with it.
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

/// Everything the profile in `home` has installed: its registry mods and the mod files put
/// there by hand, by name, then its resource packs as the game stacks them, then its shader
/// packs. Each is on or off; only a pack that is on has its picture read. Reads files: never
/// called while drawing.
pub(crate) fn read(home: &Path) -> Vec<Item> {
    let image = |png: Vec<u8>| Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png));
    let mut mods: Vec<Item> = installed::Installed::load(home)
        .entries
        .iter()
        .filter(|e| {
            e.source == sunrise_launcher::browser::SourceId::Sunrise
                && e.kind == sunrise_launcher::browser::Kind::Mod
        })
        .filter_map(|e| {
            let id = sunrise::mod_id(e)?;
            Some(Item {
                id: id.to_owned(),
                name: e.title.clone(),
                kind: Kind::Mod,
                picture: Picture::Project(e.project.clone()),
                open: Open::Project(e.project.clone()),
                on: mod_list::is_enabled(home, id),
            })
        })
        .collect();
    mods.extend(native_status::untracked(home).into_iter().map(|file| Item {
        on: mod_list::is_enabled(home, &file.id),
        name: file.id.clone(),
        kind: Kind::Mod,
        picture: Picture::Image(None),
        open: Open::Installed(file.id.clone()),
        id: file.id,
    }));
    mods.sort_by_key(|item| item.name.to_lowercase());
    let pack = |pack: packs::Pack| Item {
        kind: match pack.kind {
            PackKind::Resource => Kind::Resource,
            PackKind::Shader => Kind::Shader,
        },
        picture: Picture::Image(pack.icon.map(image)),
        open: Open::Pack(pack.kind, pack.title.clone()),
        on: pack.enabled,
        id: pack.file,
        name: pack.title,
    };
    let mut resource = packs::list_with(home, PackKind::Resource, packs::Pictures::On);
    let order = packs::resource_order(home);
    // Stable: the packs that are off stay by name after the stack.
    resource.sort_by_key(|p| {
        order
            .iter()
            .position(|file| *file == p.file)
            .unwrap_or(usize::MAX)
    });
    let shader = packs::list_with(home, PackKind::Shader, packs::Pictures::On);
    mods.into_iter()
        .chain(resource.into_iter().map(pack))
        .chain(shader.into_iter().map(pack))
        .collect()
}

/// What [`read`] depends on, from the files' names, sizes and times and the saved choices.
pub(crate) fn stamp(home: &Path) -> String {
    native_status::fingerprint(home, None) + "\n" + &packs::stamp(home)
}

/// What the selected profile has installed, as last read.
#[derive(Default)]
pub(crate) struct Inventory {
    home: Option<PathBuf>,
    /// None until the first read ends: Home says nothing about mods before it knows.
    items: Option<Arc<Vec<Item>>>,
    /// [`stamp`] when `items` were read.
    stamp: String,
    reading: bool,
    /// Something may have changed: check the stamp, in the background, when Home next draws.
    stale: bool,
    /// When Home last drew the column.
    drawn: Option<Instant>,
    watched: bool,
}

/// Home drawn again after this long checks the files once more: the player was elsewhere.
const AWAY: Duration = Duration::from_secs(1);
/// How often the files are checked while Home stays open.
const WATCH: Duration = Duration::from_secs(2);

impl Inventory {
    /// The column is being drawn for `home` at `now`: what to show, and the stamp to read
    /// against when a read should start.
    fn draw(&mut self, home: &Path, now: Instant) -> (Option<Arc<Vec<Item>>>, Option<String>) {
        if self.home.as_deref() != Some(home) {
            // Another profile: nothing of the last one is shown as this one's.
            *self = Inventory {
                home: Some(home.to_path_buf()),
                watched: self.watched,
                ..Inventory::default()
            };
        }
        let away = self
            .drawn
            .is_none_or(|last| now.duration_since(last) > AWAY);
        self.drawn = Some(now);
        let due = !self.reading && (self.items.is_none() || self.stale || away);
        if due {
            self.reading = true;
            self.stale = false;
        }
        (self.items.clone(), due.then(|| self.stamp.clone()))
    }

    /// A read for `home` ended: `None` when its files were as the kept list knows them.
    fn took(&mut self, home: &Path, read: Option<(String, Vec<Item>)>) {
        if self.home.as_deref() != Some(home) {
            return;
        }
        self.reading = false;
        match read {
            Some((stamp, items)) => {
                self.stamp = stamp;
                self.items = Some(Arc::new(items));
            }
            // Nothing there and nothing known is still an answer: an empty profile.
            None => {
                self.items.get_or_insert_default();
            }
        }
    }
}

/// [`read`], unless the files are as `known` says.
fn read_changed(home: &Path, known: &str) -> Option<(String, Vec<Item>)> {
    let stamp = stamp(home);
    (stamp != known).then(|| (stamp, read(home)))
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

    /// What the selected profile has, as last read; reads it in the background when it is not
    /// known yet or may have changed.
    fn home_inventory(&mut self, home: &Path, cx: &mut Context<Self>) -> Option<Arc<Vec<Item>>> {
        let home = home.to_path_buf();
        let (items, read) = self.ui.home.mods.draw(&home, Instant::now());
        if let Some(known) = read {
            let at = home.clone();
            let task = cx
                .background_executor()
                .spawn(async move { read_changed(&at, &known) });
            cx.spawn(async move |this, cx| {
                let read = task.await;
                let _ = this.update(cx, |this, cx| {
                    let changed = read.is_some();
                    this.ui.home.mods.took(&home, read);
                    if changed {
                        cx.notify();
                    }
                });
            })
            .detach();
        }
        if !std::mem::replace(&mut self.ui.home.mods.watched, true) {
            // While Home shows, its files are looked at now and then, never while drawing: a
            // change made elsewhere (the game, the command line) reaches the column.
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(WATCH).await;
                let Ok(shown) = this.update(cx, |this, _| {
                    let mods = &this.ui.home.mods;
                    (this.ui.page == Page::Home && !mods.reading)
                        .then(|| mods.home.clone().zip(Some(mods.stamp.clone())))
                        .flatten()
                }) else {
                    break;
                };
                let Some((home, known)) = shown else {
                    continue;
                };
                let now = cx
                    .background_executor()
                    .spawn(async move { stamp(&home) })
                    .await;
                if now != known {
                    let _ = this.update(cx, |this, cx| {
                        this.ui.home.mods.stale = true;
                        cx.notify();
                    });
                }
            })
            .detach();
        }
        items
    }

    /// Home's Mods column for the profile whose game folder is `home`.
    pub(crate) fn home_mods(&mut self, home: &Path, cx: &mut Context<Self>) -> Div {
        let items = self.home_inventory(home, cx);
        let known = items.is_some();
        let column = column(items.map(|items| items.to_vec()).unwrap_or_default());
        let index = sunrise::known_index();
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
            // Still being read: no "none yet" until that is known.
            None if !known => div(),
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
                let (built, place) = self.ui.home.mods_at.clone().list(
                    column.shown.len(),
                    1,
                    crate::home::ROW,
                    crate::home::ROW_GAP,
                );
                let mut rows = div().flex().flex_col().gap(px(crate::home::ROW_GAP));
                for (n, item) in column
                    .shown
                    .iter()
                    .enumerate()
                    .take(built.end)
                    .skip(built.start)
                {
                    let picture = match &item.picture {
                        Picture::Project(project) => tile(
                            index
                                .as_ref()
                                .and_then(|index| index.project(project)?.icon.clone())
                                .and_then(|url| {
                                    self.mods_image(&url, sunrise_launcher::browser::images::ROW)
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
                            .min_h(px(crate::home::ROW))
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
                let mut rows = div().flex().flex_col().gap(px(4.)).child(place(rows));
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
            picture: Picture::Image(None),
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
    fn the_profiles_list_is_read_once_and_again_only_when_asked() {
        let (a, b) = (Path::new("/profiles/a"), Path::new("/profiles/b"));
        let start = Instant::now();
        let mut kept = Inventory::default();
        // Nothing known: nothing shown, and a read starts against no stamp.
        let (shown, read) = kept.draw(a, start);
        assert!(shown.is_none());
        assert_eq!(read.as_deref(), Some(""));
        // While it runs, drawing starts no other.
        assert!(kept.draw(a, start).1.is_none());
        kept.took(
            a,
            Some(("one".into(), vec![item("Lanternlight", Kind::Mod, true)])),
        );
        let soon = start + Duration::from_millis(100);
        let (shown, read) = kept.draw(a, soon);
        assert_eq!(shown.unwrap().len(), 1);
        assert!(read.is_none(), "a redraw reads nothing");
        // Told that the files moved, the next draw checks against what it knows.
        kept.stale = true;
        assert_eq!(kept.draw(a, soon).1.as_deref(), Some("one"));
        kept.took(a, None);
        assert_eq!(kept.draw(a, soon).0.unwrap().len(), 1, "unchanged: kept");
        // Coming back to Home after a while checks once.
        let later = soon + AWAY + Duration::from_millis(1);
        assert_eq!(kept.draw(a, later).1.as_deref(), Some("one"));
        kept.took(a, None);
        // Another profile shows nothing of this one, and an answer for the old one is dropped.
        let (shown, read) = kept.draw(b, later);
        assert!(shown.is_none());
        assert_eq!(read.as_deref(), Some(""));
        kept.took(a, Some(("late".into(), vec![item("Old", Kind::Mod, true)])));
        assert!(kept.draw(b, later).0.is_none());
        // A profile with nothing is known to be empty, not still unread.
        kept.took(b, None);
        assert_eq!(kept.draw(b, later).0.unwrap().len(), 0);
    }

    /// A profile's game folder with one resource pack and one registry mod, both on.
    fn profile(name: &str) -> PathBuf {
        use std::io::Write as _;
        let home = std::env::temp_dir().join(format!(
            "sunrise-launcher-home-mods-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("resourcepacks")).unwrap();
        let file = std::fs::File::create(home.join("resourcepacks/Soft Edges.zip")).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("pack.mcmeta", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"pack":{"pack_format":99,"description":"made for the test"}}"#)
            .unwrap();
        zip.finish().unwrap();
        std::fs::write(
            home.join("options.txt"),
            "resourcePacks:[\"vanilla\",\"file/Soft Edges.zip\"]\n",
        )
        .unwrap();
        std::fs::create_dir_all(home.join("sunrise-native-mods")).unwrap();
        std::fs::write(
            home.join("sunrise-native-mods/lanternlight.smod"),
            b"placeholder",
        )
        .unwrap();
        std::fs::write(home.join("sunrise-mods.txt"), "mod.lanternlight:true\n").unwrap();
        std::fs::write(
            home.join("mod-browser.json"),
            serde_json::json!({"entries": [{
                "source": "sunrise", "project": "lanternlight", "slug": "lanternlight",
                "title": "Lanternlight", "kind": "mod", "version": "1.0.0", "number": "1.0.0",
                "file": "sunrise-native-mods/lanternlight.smod",
                "checksum": format!("SHA-256:{}", "0".repeat(64)), "page": "", "dependency": false,
            }]})
            .to_string(),
        )
        .unwrap();
        home
    }

    fn on(items: &[Item]) -> Vec<(&str, bool)> {
        items.iter().map(|i| (i.name.as_str(), i.on)).collect()
    }

    #[test]
    fn a_change_made_from_the_command_line_reaches_the_open_profile() {
        let home = profile("cli");
        let mut kept = Inventory::default();
        let now = Instant::now();
        // Home opens: the first read.
        let known = kept.draw(&home, now).1.unwrap();
        kept.took(&home, read_changed(&home, &known));
        let shown = kept.draw(&home, now).0.unwrap();
        assert_eq!(
            on(&shown),
            [("Lanternlight", true), ("Soft Edges.zip", true)]
        );
        assert_eq!(column(shown.to_vec()).unwrap().off(), 0);
        // Nothing moved: the watch sees the stamp it knows, and a check reads nothing.
        assert_eq!(stamp(&home), kept.stamp);
        assert!(read_changed(&home, &kept.stamp).is_none());

        // `sunrise-launcher packs off` on the same profile (`cli::set_pack` calls these).
        let pack = packs::find(&home, PackKind::Resource, "Soft Edges.zip").unwrap();
        packs::set_enabled(&home, &pack, false).unwrap();
        // The watch sees another stamp and says so; the next draw reads again.
        assert_ne!(stamp(&home), kept.stamp);
        kept.stale = true;
        let known = kept.draw(&home, now).1.unwrap();
        kept.took(&home, read_changed(&home, &known));
        let shown = kept.draw(&home, now).0.unwrap();
        assert_eq!(
            on(&shown),
            [("Lanternlight", true), ("Soft Edges.zip", false)]
        );
        let column = column(shown.to_vec()).unwrap();
        assert_eq!((column.shown.len(), column.off()), (1, 1));

        // `sunrise-launcher mods disable`, the same way.
        mod_list::set_enabled(&home, "lanternlight", false).unwrap();
        assert_ne!(stamp(&home), kept.stamp);
        kept.took(&home, read_changed(&home, &kept.stamp));
        let shown = kept.draw(&home, now).0.unwrap();
        assert_eq!(
            on(&shown),
            [("Lanternlight", false), ("Soft Edges.zip", false)]
        );
        std::fs::remove_dir_all(&home).unwrap();
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
