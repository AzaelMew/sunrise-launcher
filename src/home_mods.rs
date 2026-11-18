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
    const ALL: [Kind; 3] = [Kind::Mod, Kind::Resource, Kind::Shader];

    pub(crate) fn of(pack: PackKind) -> Kind {
        match pack {
            PackKind::Resource => Kind::Resource,
            PackKind::Shader => Kind::Shader,
        }
    }

    /// Where the ones of this kind that are turned off are listed.
    pub(crate) fn turned_off(self) -> crate::mods::Installed {
        use crate::mods::Installed;
        match self {
            Kind::Mod => Installed::ModsOff,
            Kind::Resource => Installed::Shelf(PackKind::Resource),
            Kind::Shader => Installed::Shelf(PackKind::Shader),
        }
    }

    /// "2 mods", for the line that counts what is turned off.
    fn counted(self, count: usize) -> String {
        let (one, many) = match self {
            Kind::Mod => ("1 mod", "{count} mods"),
            Kind::Resource => ("1 resource pack", "{count} resource packs"),
            Kind::Shader => ("1 shader pack", "{count} shader packs"),
        };
        match count {
            1 => tr(one).to_string(),
            _ => trf(many, &[("count", &count.to_string())]),
        }
    }

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
    /// A mod the registry doesn't know: Mods › Installed, at the mod with this ID.
    Installed(String),
    /// A pack: its shelf, at the pack that is this file.
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

/// What the column lists: the items that are on, in the order given, and how many of each
/// kind are off.
pub(crate) struct Column {
    pub shown: Vec<Item>,
    /// By kind, as [`Kind::ALL`] orders them.
    pub off: [usize; 3],
}

/// The column for a profile with `items` installed; `None` when it has nothing at all.
pub(crate) fn column(items: Vec<Item>) -> Option<Column> {
    if items.is_empty() {
        return None;
    }
    Some(Column {
        off: Kind::ALL.map(|kind| items.iter().filter(|i| !i.on && i.kind == kind).count()),
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
    // The files put there by hand, native and not: the game plays with them too.
    let by_hand = native_status::untracked(home)
        .into_iter()
        .chain(native_status::untracked_wasm(home));
    mods.extend(by_hand.map(|file| {
        Item {
            on: mod_list::is_enabled(home, &file.id),
            name: file.name,
            kind: Kind::Mod,
            picture: Picture::Image(
                file.icon
                    .ok()
                    .flatten()
                    .map(|icon| image(icon.bytes().to_vec())),
            ),
            open: Open::Installed(file.id.clone()),
            id: file.id,
        }
    }));
    mods.sort_by_key(|item| item.name.to_lowercase());
    let pack = |pack: packs::Pack| Item {
        kind: Kind::of(pack.kind),
        picture: Picture::Image(pack.icon.map(image)),
        open: Open::Pack(pack.kind, pack.file.clone()),
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

/// A mod's or a pack's picture, `side` square: it fills the square edge to edge, cropped
/// rather than boxed. Without one (it has none, it has not arrived, or it is not a picture)
/// the first letter of `name` stands on a quiet colour of the theme that `id` always gets. It
/// takes what the launcher's shared tile for these takes, so that one replaces this call for
/// call.
fn tile(picture: Option<impl Into<ImageSource>>, id: &str, name: &str, side: f32) -> Div {
    let p = palette();
    let fills = [p.selected, p.hover, p.line, p.chrome_raised];
    let fill = fills[id
        .bytes()
        .fold(0usize, |sum, b| sum.wrapping_mul(31) + b as usize)
        % 4];
    let initial = crate::join::initial(name);
    let letter = move || {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(fill))
            .text_size(px(side * 0.45))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(palette().muted))
            .child(initial.clone())
    };
    let square = div()
        .flex_none()
        .size(px(side))
        .rounded(px(tokens::R_CONTROL))
        .overflow_hidden();
    match picture {
        Some(picture) => square.child(
            gpui::img(picture)
                .size_full()
                .object_fit(gpui::ObjectFit::Cover)
                .with_fallback(move || letter().into_any_element()),
        ),
        None => square.child(letter()),
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
            Open::Installed(id) => self.open_installed(crate::mods::Installed::Mod(id)),
            Open::Pack(kind, file) => self.open_installed(crate::mods::Installed::Pack(kind, file)),
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
                .child(
                    link("alt-home-browse-mods", tr("Browse mods").to_string())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(palette().text))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            // Discover itself, wherever Mods was last left.
                            this.show_discover();
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
                for item in column.shown.iter().take(built.end).skip(built.start) {
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
                            // By what it is, not where it stands: when the list changes
                            // under the keyboard, the focus does not pass to a neighbour.
                            .id(SharedString::from(format!(
                                "alt-home-mod:{}:{:?}:{}",
                                home.display(),
                                item.kind,
                                item.id
                            )))
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
                // What is off is counted by kind, each count leading to where those are listed.
                if column.off.iter().any(|&count| count > 0) {
                    let mut line = div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_x(px(10.))
                        .gap_y(px(4.))
                        .mt(px(4.))
                        .px(px(6.))
                        .child(muted(tr("Turned off:")));
                    for (kind, count) in Kind::ALL.into_iter().zip(column.off) {
                        if count == 0 {
                            continue;
                        }
                        let id = match kind {
                            Kind::Mod => "alt-home-off-mods",
                            Kind::Resource => "alt-home-off-resource",
                            Kind::Shader => "alt-home-off-shader",
                        };
                        line = line.child(
                            link(id, kind.counted(count))
                                .text_color(rgb(palette().muted))
                                .hover(|s| s.text_color(rgb(palette().text)))
                                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.open_installed(kind.turned_off());
                                    cx.notify();
                                })),
                        );
                    }
                    rows = rows.child(line);
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
        assert_eq!(column.off, [1, 1, 1], "mods, resource packs, shader packs");
    }

    #[test]
    fn each_kind_that_is_off_is_counted_and_leads_to_its_own_list() {
        use crate::mods::Installed;
        let all_on = column(vec![
            item("Lanternlight", Kind::Mod, true),
            item("Soft Edges", Kind::Resource, true),
        ])
        .unwrap();
        assert_eq!((all_on.shown.len(), all_on.off), (2, [0, 0, 0]));
        // Only a shader pack is off: it is counted as one, not as a mod.
        let shader = column(vec![
            item("Lanternlight", Kind::Mod, true),
            item("Evening Glow", Kind::Shader, false),
        ])
        .unwrap();
        assert_eq!(shader.off, [0, 0, 1]);
        assert!(Kind::Mod.turned_off() == Installed::ModsOff);
        assert!(Kind::Resource.turned_off() == Installed::Shelf(PackKind::Resource));
        assert!(Kind::Shader.turned_off() == Installed::Shelf(PackKind::Shader));
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
        assert_eq!(all_off.off, [1, 1, 0]);
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
        assert_eq!(column(shown.to_vec()).unwrap().off, [0, 0, 0]);
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
        assert_eq!((column.shown.len(), column.off), (1, [0, 1, 0]));

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
