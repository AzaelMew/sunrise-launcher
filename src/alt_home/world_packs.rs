//! A world's data packs, an inline view under Home (`Data packs` on a world card), with Back.
//!
//! The list is read-only: `datapacks::list` of the world the player picked, nothing is written
//! on opening or while it shows. Add hands over to Mods > Discover for vanilla data packs,
//! aimed at that world (`alt_discover_for_world`), which validates the source and the install
//! permission; this view installs nothing. The target is always a world the player chose here:
//! never the first one by default. Worlds past the three on Home are reached from the
//! chooser, which lists every world of the setup.
use super::*;
use crate::alt::Page;
use std::path::PathBuf;
use sunrise_launcher::datapacks::{self, DataPack};

/// The view's state: closed, or open on a setup and maybe a chosen world.
#[derive(Default)]
pub(crate) struct WorldPacks {
    open: Option<Open>,
}

struct Open {
    /// The setup the view was opened in; another one selected closes the view.
    setup: PathBuf,
    /// The chosen world's folder under `saves/`, if one was chosen.
    target: Option<Target>,
}

struct Target {
    path: PathBuf,
    name: String,
    packs: Vec<DataPack>,
    /// Add went to Discover: the list is read again when Home shows next.
    away: bool,
}

impl WorldPacks {
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Opens the view for `setup`, on `world` when one was picked (a card), else on the chooser.
    pub(crate) fn open(&mut self, setup: PathBuf, world: Option<&World>) {
        self.open = Some(Open {
            setup,
            target: world.map(Target::new),
        });
    }

    /// Closes the view when the setup changed, or the chosen world is no longer in `worlds`
    /// (`worlds` as last read for `setup`); reads the packs again after a trip to Discover.
    pub(crate) fn sync(&mut self, setup: &std::path::Path, worlds: Option<&[World]>) {
        let Some(open) = &mut self.open else { return };
        if open.setup != setup {
            self.open = None;
            return;
        }
        let Some(target) = &mut open.target else {
            return;
        };
        if let Some(worlds) = worlds {
            match worlds.iter().find(|w| w.path == target.path) {
                None => {
                    open.target = None;
                    return;
                }
                Some(world) => target.name = world.name.clone(),
            }
        }
        if std::mem::take(&mut target.away) {
            target.packs = datapacks::list(&target.path);
        }
    }

    /// The world Add would target, if one was chosen.
    #[cfg(test)]
    pub(crate) fn target(&self) -> Option<&std::path::Path> {
        let target = self.open.as_ref()?.target.as_ref()?;
        Some(&target.path)
    }

    fn choose(&mut self, world: &World) {
        if let Some(open) = &mut self.open {
            open.target = Some(Target::new(world));
        }
    }
}

impl Target {
    fn new(world: &World) -> Self {
        Target {
            path: world.path.clone(),
            name: world.name.clone(),
            packs: datapacks::list(&world.path),
            away: false,
        }
    }
}

impl Launcher {
    pub(super) fn alt_world_packs(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let worlds: Vec<World> = self.worlds.list.clone();
        let back = button("alt-packs-back", tr("Back")).on_click(cx.listener(
            |this, _: &ClickEvent, _, cx| {
                this.alt.home.packs = WorldPacks::default();
                cx.notify();
            },
        ));
        let target = self.alt.home.packs.open.as_ref().and_then(|o| {
            o.target
                .as_ref()
                .map(|t| (t.path.clone(), t.name.clone(), t.packs.clone()))
        });
        let title = match &target {
            Some((_, name, _)) => trf("Data packs in {world}", &[("world", name)]),
            None => tr("Choose a world").to_string(),
        };
        let add = button("alt-packs-add", tr("Add"));
        let add = match &target {
            Some((path, _, _)) => {
                let path = path.clone();
                add.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.alt_discover_for_world(path.clone());
                    if this.alt.page != Page::Home {
                        if let Some(Open {
                            target: Some(t), ..
                        }) = &mut this.alt.home.packs.open
                        {
                            t.away = true;
                        }
                    }
                    cx.notify();
                }))
            }
            None => add.opacity(0.4).cursor_default(),
        };
        let mut chips = div().flex().flex_wrap().gap(px(8.));
        for (i, world) in worlds.iter().enumerate() {
            let chosen = target.as_ref().is_some_and(|(p, _, _)| *p == world.path);
            let world = world.clone();
            let chip = button(format!("alt-packs-world-{i}"), world.name.clone())
                .border_color(rgb(if chosen { GOLD } else { LINE }))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.alt.home.packs.choose(&world);
                    cx.notify();
                }));
            chips = chips.child(chip);
        }
        let body = match &target {
            None => card().p(px(20.)).child(muted(tr(
                "Pick the world these data packs are for. Nothing is chosen for you.",
            ))),
            Some((_, _, packs)) if packs.is_empty() => card()
                .p(px(20.))
                .child(muted(tr("No data packs in this world yet."))),
            Some((_, _, packs)) => {
                let mut list = card().flex().flex_col();
                for (i, pack) in packs.iter().enumerate() {
                    let row = div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(10.))
                        .when(i > 0, |d| d.border_t_1().border_color(rgb(SOFT)))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .child(
                                    div()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(TEXT))
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis()
                                        .child(pack.file.clone()),
                                )
                                .child(muted(pack.description.clone())),
                        )
                        .child(muted(if pack.enabled { tr("On") } else { tr("Off") }));
                    list = list.child(row);
                }
                list
            }
        };
        let note = target.is_some().then(|| {
            muted(tr(
                "The game turns packs on when the world opens, or on /reload.",
            ))
        });
        div()
            .id("alt-world-packs")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(BG))
            .flex()
            .flex_col()
            .gap(px(16.))
            .px(px(32.))
            .py(px(28.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(back)
                    .child(div().flex_1().min_w(px(0.)).child(heading(&title)))
                    .child(add),
            )
            .child(chips)
            .child(body)
            .children(note)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    fn world(folder: &str) -> World {
        World {
            folder: folder.into(),
            path: PathBuf::from("/s/saves").join(folder),
            name: folder.into(),
            mode: "Survival",
            last_played: SystemTime::UNIX_EPOCH,
            version: None,
            size: 0,
            icon: None,
            mods: vec![],
        }
    }

    #[test]
    fn nothing_is_targeted_until_a_world_is_chosen() {
        let mut view = WorldPacks::default();
        let (a, b) = (world("a"), world("b"));
        view.open("/s".into(), None);
        assert!(view.is_open() && view.target().is_none());
        view.choose(&b);
        assert_eq!(view.target(), Some(b.path.as_path()));
        view.open("/s".into(), Some(&a));
        assert_eq!(view.target(), Some(a.path.as_path()));
    }

    #[test]
    fn a_setup_switch_or_a_vanished_world_drops_the_target() {
        let mut view = WorldPacks::default();
        let (a, b) = (world("a"), world("b"));
        view.open("/s".into(), Some(&a));
        view.sync("/s".as_ref(), Some(std::slice::from_ref(&b)));
        assert!(view.is_open() && view.target().is_none());
        view.choose(&b);
        view.sync("/other".as_ref(), Some(&[b]));
        assert!(!view.is_open());
    }

    #[test]
    fn coming_back_from_discover_reads_the_packs_again() {
        let dir = std::env::temp_dir().join(format!("wp-{}", std::process::id()));
        let w = dir.join("saves/w");
        std::fs::create_dir_all(w.join("datapacks/p")).unwrap();
        let mut world = world("w");
        world.path = w.clone();
        let mut view = WorldPacks::default();
        view.open(dir.clone(), Some(&world));
        assert!(view
            .open
            .as_ref()
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .packs
            .is_empty());
        std::fs::write(
            w.join("datapacks/p/pack.mcmeta"),
            r#"{"pack":{"description":"d"}}"#,
        )
        .unwrap();
        view.sync(&dir, Some(&[world.clone()]));
        assert!(view
            .open
            .as_ref()
            .unwrap()
            .target
            .as_ref()
            .unwrap()
            .packs
            .is_empty());
        view.open.as_mut().unwrap().target.as_mut().unwrap().away = true;
        view.sync(&dir, Some(&[world]));
        assert_eq!(
            view.open
                .as_ref()
                .unwrap()
                .target
                .as_ref()
                .unwrap()
                .packs
                .len(),
            1
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
