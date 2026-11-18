//! The redesign's Home (`alt/README.md`, mocks 01, 10, 11a and 12b): the player's worlds as
//! pictures, one box for servers (an address or a friend's invite code, Save, Join) with the
//! saved list, and the registry's most downloaded mods.
//!
//! Everything shown is read from the player's real launcher data, and nothing is written until
//! the player acts (Save, Remove, Add, Play, Join). Play and Join set `quick_play` or `joining`
//! and call `alt_play`, which owns the sign-in, install, prepare, build and trust gates:
//! - worlds: `worlds::list` of the selected setup's `saves/` (`level.dat` and `icon.png`, read
//!   only), Play starts the game straight into one (`quick_play`);
//! - a world without `icon.png` is drawn from a block texture of the player's own prepared
//!   game files (`backend::resources`, nothing is shipped), else a plain tile with its initial;
//! - servers: the game's own `servers.dat` (`server_list`) with the status ping the Multiplayer
//!   screen sends, Join, Save and Remove; an invite code joins through `join_friend`;
//! - featured servers: [`featured`] has no source yet, so none is shown;
//! - popular mods: the registry index's most downloaded mods installable on this game, Add is
//!   the Mods page's own install (`mods_install`, with its consent), a row opens the project.
use super::*;
use crate::alt::{button, BG, CARD, GOLD, LINE, MUTED, TEXT};
use crate::worlds_page;
use gpui::{rgb, rgba};
use sunrise_launcher::browser::{self, Kind, Project, Query, SourceId};
use sunrise_launcher::worlds::World;

/// Worlds shown across the top.
const WORLDS: usize = 3;
/// Popular mods listed.
const POPULAR: usize = 6;
/// The picture band of a world card, in points.
const BAND: f32 = 150.;
/// An icon is drawn at a whole multiple of its pixels up to about this wide, then cropped.
const BAND_ART: f32 = 480.;
/// One texture tile, in points (16 pixels at 8×).
const TILE: f32 = 128.;
/// The game's block textures a world without a picture may take, one by its folder's name.
const BLOCKS: [&str; 6] = [
    "dirt",
    "stone",
    "cobblestone",
    "oak_planks",
    "sand",
    "spruce_planks",
];
/// The icons of a world or a server are small: anything larger is not one.
const MAX_PICTURE: u64 = 4 << 20;

#[derive(Default)]
pub(crate) enum Popular {
    #[default]
    Unasked,
    Asking,
    Ready(String, Vec<Project>),
    Failed(String),
}

#[derive(Default)]
pub(crate) struct AltHomeState {
    popular: Popular,
    /// The saved server whose Remove button is showing, and whether it was asked once.
    menu: Option<(usize, bool)>,
}

/// A server Sunrise features. There is no public list to read them from yet: none is shown
/// until there is one, and none is made up.
pub(crate) struct Featured {
    pub name: String,
    pub address: String,
    pub note: String,
}

pub(crate) fn featured() -> Vec<Featured> {
    Vec::new()
}

/// What the address box holds.
#[derive(Debug, PartialEq)]
pub(crate) enum Entry {
    Empty,
    Invite,
    Address(String),
    /// Spaces or the like inside: neither.
    Unusable,
}

pub(crate) fn entry(text: &str) -> Entry {
    let text = text.trim();
    if text.is_empty() {
        Entry::Empty
    } else if sunrise_launcher_friends::invite::parse(text).is_some() {
        Entry::Invite
    } else if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        Entry::Unusable
    } else {
        Entry::Address(text.to_string())
    }
}

/// The block texture a world takes, the same one every time for the same folder.
pub(crate) fn block_for(folder: &str) -> &'static str {
    let sum = folder
        .bytes()
        .fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize));
    BLOCKS[sum % BLOCKS.len()]
}

/// The first letter of `name`, upper case, for a plain tile.
pub(crate) fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".into())
}

/// The popular mods to list: those that run on this game, not already installed, `limit` of them
/// in the order given (the registry's most downloaded first).
pub(crate) fn popular_rows(
    found: &[Project],
    game: &str,
    installed: impl Fn(&Project) -> bool,
    limit: usize,
) -> Vec<Project> {
    found
        .iter()
        .filter(|p| p.kind == Kind::Mod && p.support(game).installable() && !installed(p))
        .take(limit)
        .cloned()
        .collect()
}

fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn card() -> Div {
    div()
        .bg(rgb(CARD))
        .border_1()
        .border_color(rgb(LINE))
        .rounded(px(12.))
        .overflow_hidden()
}

fn heading(label: &str) -> Div {
    div()
        .text_size(px(17.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(TEXT))
        .child(label.to_string())
}

fn muted(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.5))
        .text_color(rgb(MUTED))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text.into())
}

fn tile(label: &str, side: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded(px(8.))
        .bg(rgb(LINE))
        .text_size(px(side * 0.45))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(MUTED))
        .child(initial(label))
}

impl Launcher {
    pub(crate) fn alt_home(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(home) = self.selected_instance().map(|i| i.home.clone()) else {
            return div()
                .p(px(32.))
                .text_color(rgb(MUTED))
                .child(tr("Getting ready…"))
                .into_any_element();
        };
        self.load_worlds(home.clone(), cx);
        self.load_servers(home);
        self.ping_servers(cx);
        self.mods_load();
        self.alt_popular_load(cx);
        let worlds = self.alt_worlds(cx);
        let servers = self.alt_servers(cx);
        let popular = self.alt_popular(cx);
        div()
            .id("alt-home")
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(BG))
            .flex()
            .flex_col()
            .gap(px(28.))
            .px(px(32.))
            .py(px(28.))
            .child(worlds)
            .child(
                div()
                    .flex()
                    .gap(px(28.))
                    .items_start()
                    .child(div().flex_1().min_w(px(0.)).child(servers))
                    .child(div().flex_1().min_w(px(0.)).child(popular)),
            )
            .into_any_element()
    }

    fn alt_worlds(&mut self, cx: &mut Context<Self>) -> Div {
        let list: Vec<World> = self.worlds.list.iter().take(WORLDS).cloned().collect();
        let mut row = div().flex().gap(px(16.));
        for world in list.iter() {
            let art = self.alt_world_art(world);
            row = row.child(self.alt_world_card(world, art, cx));
        }
        let body = if list.is_empty() {
            let saves = self.worlds.home.clone().map(|h| h.join("saves"));
            card()
                .p(px(20.))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(muted(tr(
                    "No worlds yet. Make one in the game and it shows up here.",
                )))
                .children(saves.map(|saves| {
                    button("alt-worlds-folder", tr("Open folder")).on_click(move |_, _, cx| {
                        let _ = std::fs::create_dir_all(&saves);
                        crate::glass_toast::open(cx, &saves);
                    })
                }))
        } else {
            row
        };
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(heading(tr("Your worlds")))
            .child(body)
    }

    /// A world's picture: its own `icon.png`, drawn big and sharp; else a block texture of the
    /// player's own game files, darkened; else a plain tile with its initial.
    fn alt_world_art(&mut self, world: &World) -> Div {
        let scale = self.scale;
        let band = div()
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .w_full()
            .h(px(BAND))
            .overflow_hidden()
            .bg(rgb(BG));
        let stamp = world
            .last_played
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if let Some(icon) = world.icon.clone() {
            let key = format!("alt-world:{}:{stamp}", icon.display());
            let read = icon.clone();
            let picture = self.icon_image(key, BAND_ART, true, move || {
                std::fs::metadata(&read)
                    .ok()
                    .filter(|m| m.len() <= MAX_PICTURE)
                    .and_then(|_| std::fs::read(&read).ok())
            });
            if let Some(image) = picture {
                let dim = image.size(0);
                return band.child(
                    gpui::img(image)
                        .flex_none()
                        .w(px(dim.width.0 as f32 / scale))
                        .h(px(dim.height.0 as f32 / scale)),
                );
            }
        }
        let resources = self.game_resources();
        let block = block_for(&world.folder);
        let file = resources
            .join("assets/minecraft/textures/block")
            .join(format!("{block}.png"));
        let key = format!("alt-block:{}", file.display());
        let texture = self.icon_image(key, TILE, true, move || std::fs::read(file).ok());
        match texture {
            Some(image) => {
                let dim = image.size(0);
                let (w, h) = (dim.width.0 as f32 / scale, dim.height.0 as f32 / scale);
                let mut tiles = div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .flex()
                    .flex_wrap()
                    .w(px(w * 8.));
                for _ in 0..16 {
                    tiles = tiles.child(gpui::img(image.clone()).flex_none().w(px(w)).h(px(h)));
                }
                band.child(tiles)
                    .child(div().absolute().inset_0().bg(rgba(0x0000_0073)))
            }
            None => band
                .bg(rgb(CARD))
                .text_size(px(56.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(MUTED))
                .child(initial(&world.name)),
        }
    }

    fn alt_world_card(&mut self, world: &World, art: Div, cx: &mut Context<Self>) -> Div {
        let folder = world.folder.clone();
        let play = button(format!("alt-world-{}", world.folder), tr("Play")).on_click(cx.listener(
            move |this, _: &ClickEvent, _, cx| {
                this.alt_go("--world", folder.clone());
                cx.notify();
            },
        ));
        let facts = format!(
            "{} · {}",
            tr(world.mode),
            sentence(&worlds_page::ago(world.last_played))
        );
        card()
            .flex_1()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .child(art)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(14.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(TEXT))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(world.name.clone()),
                            )
                            .child(muted(facts)),
                    )
                    .child(play),
            )
    }

    fn alt_servers(&mut self, cx: &mut Context<Self>) -> Div {
        let typed = entry(self.friend_code.text());
        let focused = self.friend_focused;
        let field = div()
            .id("alt-address")
            .track_focus(&self.friend_focus)
            .flex_1()
            .min_w(px(0.))
            .h(px(38.))
            .px(px(12.))
            .flex()
            .items_center()
            .overflow_hidden()
            .rounded(px(8.))
            .bg(rgb(BG))
            .border_1()
            .border_color(rgb(if focused { GOLD } else { LINE }))
            .text_size(px(14.))
            .text_color(rgb(TEXT))
            .cursor_text()
            .child(line_edit::content(
                &self.friend_code,
                &self.theme,
                focused,
                false,
                false,
                "Server address or a friend's invite code",
            ))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                window.focus(&this.friend_focus, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                match line_edit::handle(&mut this.friend_code, ev, cx) {
                    line_edit::Key::Pass => return,
                    line_edit::Key::Enter => this.alt_join_typed(),
                    line_edit::Key::Escape => window.blur(cx),
                    _ => {}
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let field = line_edit::mouse(field, cx, |this| &mut this.friend_code);
        let address = matches!(typed, Entry::Address(_));
        let save = button("alt-server-save", tr("Save"));
        let save = if address {
            save.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.alt_save_typed();
                cx.notify();
            }))
        } else {
            save.opacity(0.4).cursor_default()
        };
        let joinable = matches!(typed, Entry::Address(_) | Entry::Invite);
        let join = button("alt-server-join", tr("Join"));
        let join = if joinable {
            join.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.alt_join_typed();
                cx.notify();
            }))
        } else {
            join.opacity(0.4).cursor_default()
        };
        let mut list = div().flex().flex_col();
        if let Some(e) = self.multiplayer.error.clone() {
            list = list.child(
                div()
                    .px(px(14.))
                    .py(px(8.))
                    .text_size(px(12.5))
                    .text_color(rgb(GOLD))
                    .child(e),
            );
        }
        if let Entry::Unusable = typed {
            list = list.child(
                div()
                    .px(px(14.))
                    .py(px(8.))
                    .text_size(px(12.5))
                    .text_color(rgb(MUTED))
                    .child(tr("That isn't an address or an invite code.")),
            );
        }
        let saved = self.multiplayer.list.clone();
        let mut shown = 0;
        for server in featured() {
            // Saved already: it shows once, as saved.
            if saved
                .iter()
                .any(|(s, _)| s.ip.eq_ignore_ascii_case(&server.address))
            {
                continue;
            }
            list = list.child(self.alt_featured_row(shown, server, cx));
            shown += 1;
        }
        for (i, (server, icon)) in saved.into_iter().enumerate() {
            list = list.child(self.alt_server_row(i, server, icon, cx));
            shown += 1;
        }
        if shown == 0 {
            list = list.child(
                div()
                    .px(px(14.))
                    .py(px(14.))
                    .child(muted(tr("No servers saved in this setup."))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(heading(tr("Servers")))
            .child(
                card()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .p(px(12.))
                            .child(field)
                            .child(save)
                            .child(join),
                    )
                    .child(list),
            )
    }

    fn alt_featured_row(&mut self, i: usize, server: Featured, cx: &mut Context<Self>) -> Div {
        let address = server.address.clone();
        let join = button(format!("alt-featured-{i}"), tr("Join")).on_click(cx.listener(
            move |this, _: &ClickEvent, _, cx| {
                this.alt_go("--server", address.clone());
                cx.notify();
            },
        ));
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(10.))
            .border_t_1()
            .border_color(rgb(LINE))
            .child(tile(&server.name, 36.))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .child(server.name)
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(rgb(MUTED))
                                    .child(tr("Featured")),
                            ),
                    )
                    .child(muted(server.note)),
            )
            .child(join)
    }

    fn alt_server_row(
        &mut self,
        i: usize,
        server: sunrise_launcher::server_list::Server,
        icon: Option<std::sync::Arc<gpui::Image>>,
        cx: &mut Context<Self>,
    ) -> Div {
        use multiplayer_page::Reach;
        let reach = self
            .multiplayer
            .reach
            .get(&server.ip)
            .copied()
            .unwrap_or(Reach::Asking);
        let line = match reach {
            Reach::Online { players, max } => trf(
                "{online} of {max}",
                &[("online", &players.to_string()), ("max", &max.to_string())],
            ),
            Reach::Offline => tr("No answer").to_string(),
            Reach::Asking => server.ip.clone(),
        };
        let offline = reach == Reach::Offline;
        let ip = server.ip.clone();
        let join = button(format!("alt-server-{i}-join"), tr("Join"));
        let join = if offline {
            join.opacity(0.4)
                .cursor_default()
                .tooltip(crate::glass_ui::tip(tr(
                    "This server doesn't answer right now",
                )))
        } else {
            join.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.alt_go("--server", ip.clone());
                cx.notify();
            }))
        };
        let open = self.alt.home.menu.filter(|(row, _)| *row == i);
        let more = match open {
            Some((_, asked)) => button(
                format!("alt-server-{i}-remove"),
                if asked {
                    tr("Click again to remove")
                } else {
                    tr("Remove")
                },
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                if asked {
                    this.alt.home.menu = None;
                    this.edit_servers(|list| list.remove(i).map(|_| ()));
                } else {
                    this.alt.home.menu = Some((i, true));
                }
                cx.notify();
            })),
            None => button(format!("alt-server-{i}-more"), "…").on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.alt.home.menu = Some((i, false));
                    cx.notify();
                },
            )),
        };
        let picture = match icon {
            Some(image) => gpui::img(image)
                .flex_none()
                .size(px(36.))
                .rounded(px(8.))
                .into_any_element(),
            None => tile(&server.name, 36.).into_any_element(),
        };
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(10.))
            .border_t_1()
            .border_color(rgb(LINE))
            .child(picture)
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
                            .child(server.name),
                    )
                    .child(muted(line)),
            )
            .child(join)
            .child(more)
    }

    /// Every destination goes through `alt_play`, which runs the sign-in, install, prepare,
    /// build and trust gates before the game starts.
    fn alt_go(&mut self, flag: &'static str, value: String) {
        self.quick_play = Some((flag, value));
        self.alt_play();
    }

    /// Join with what is typed: an invite code joins the friend, an address the server.
    fn alt_join_typed(&mut self) {
        match entry(self.friend_code.text()) {
            Entry::Invite => {
                self.joining = Some(self.friend_code.text().to_string());
                self.alt_play();
            }
            Entry::Address(address) => self.alt_go("--server", address),
            Entry::Empty | Entry::Unusable => {}
        }
    }

    /// Save with what is typed: the address goes into the game's own server list.
    fn alt_save_typed(&mut self) {
        let Entry::Address(address) = entry(self.friend_code.text()) else {
            return;
        };
        self.edit_servers(|list| list.add(&address, &address));
        if self.multiplayer.error.is_none() {
            self.friend_code.set(String::new());
        }
    }

    /// Asks the registry for the most downloaded mods once per game version.
    fn alt_popular_load(&mut self, cx: &mut Context<Self>) {
        let game = self.mods_game();
        match &self.alt.home.popular {
            Popular::Asking | Popular::Failed(_) => return,
            Popular::Ready(known, _) if *known == game => return,
            _ => {}
        }
        self.alt.home.popular = Popular::Asking;
        let query = Query {
            game: game.clone(),
            // A few more than listed: some may be installed already.
            limit: POPULAR * 4,
            ..Query::new("", Kind::Mod)
        };
        let task = cx.background_executor().spawn(async move {
            browser::source(SourceId::Sunrise)
                .ok_or_else(|| tr("The mod registry isn't set up.").to_string())
                .and_then(|source| source.search(&query))
        });
        cx.spawn(async move |this, cx| {
            let found = task.await;
            let _ = this.update(cx, |this, cx| {
                this.alt.home.popular = match found {
                    Ok(found) => Popular::Ready(game, found),
                    Err(e) => Popular::Failed(e),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn alt_popular(&mut self, cx: &mut Context<Self>) -> Div {
        let head = div()
            .flex()
            .items_center()
            .justify_between()
            .child(heading(tr("Popular mods")))
            .child(
                div()
                    .id("alt-browse-mods")
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(rgb(GOLD))
                    .child(tr("Browse all mods"))
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.alt.page = crate::alt::Page::Discover;
                        cx.notify();
                    })),
            );
        let game = self.mods_game();
        let body = match &self.alt.home.popular {
            Popular::Ready(_, found) => {
                let rows = popular_rows(found, &game, |p| self.mods.entry(p).is_some(), POPULAR);
                if rows.is_empty() {
                    card()
                        .p(px(14.))
                        .child(muted(tr("Nothing to suggest right now.")))
                } else {
                    let mut list = card();
                    for (i, project) in rows.into_iter().enumerate() {
                        list = list.child(self.alt_popular_row(i, project, cx));
                    }
                    list
                }
            }
            Popular::Failed(why) => card()
                .p(px(14.))
                .flex()
                .items_center()
                .gap(px(12.))
                .child(muted(trf(
                    "The mod registry didn't answer: {why}",
                    &[("why", why)],
                )))
                .child(
                    button("alt-popular-retry", tr("Try again")).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.alt.home.popular = Popular::Unasked;
                            cx.notify();
                        },
                    )),
                ),
            Popular::Unasked | Popular::Asking => card()
                .p(px(14.))
                .child(muted(tr("Asking the mod registry…"))),
        };
        let note = self.mods.message.clone().map(|m| match m {
            Ok(text) => muted(text),
            Err(text) => muted(text).text_color(rgb(GOLD)),
        });
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(head)
            .child(body)
            .children(note)
    }

    fn alt_popular_row(&mut self, i: usize, project: Project, cx: &mut Context<Self>) -> Div {
        let busy = self
            .mods
            .busy
            .as_ref()
            .map(|(id, line, _)| (id.clone(), line.clone()));
        let this_busy = busy.as_ref().is_some_and(|(id, _)| *id == project.id);
        let icon = self.mods_icon(&project, 36.);
        let add = button(
            format!("alt-popular-{i}-add"),
            tr(if this_busy { "Adding…" } else { "Add" }),
        );
        let add = if busy.is_none() {
            let p = project.clone();
            add.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.mods_install(p.source, p.id.clone(), Some(p.clone()));
                cx.notify();
            }))
        } else {
            add.opacity(0.4).cursor_default()
        };
        let open = project.clone();
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(10.))
            .when(i > 0, |d| d.border_t_1().border_color(rgb(LINE)))
            .child(icon)
            .child(
                div()
                    .id(SharedString::from(format!("alt-popular-{i}-open")))
                    .flex_1()
                    .min_w(px(0.))
                    .cursor_pointer()
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT))
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(project.title.clone()),
                    )
                    .child(muted(match busy.filter(|_| this_busy) {
                        Some((_, line)) => line,
                        None => project.summary.clone(),
                    }))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.mods_open(open.clone());
                        this.alt.page = crate::alt::Page::Discover;
                        cx.notify();
                    })),
            )
            .child(add)
    }
}

#[cfg(test)]
#[path = "alt_home/tests.rs"]
mod tests;
