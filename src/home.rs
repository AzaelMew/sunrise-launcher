//! Home: the selected profile, Play and its game build up front, and the profile's recent
//! worlds to continue.
//!
//! Everything shown is read from the player's real launcher data, and nothing is written until
//! the player acts. Play sets `quick_play` and calls `play_game`, which owns the sign-in,
//! install, prepare, build and trust gates:
//! - the header: the profile's mark and name, how long it was played and when last
//!   (`instances.json`), Play in every state `launch_state` knows with its one action beside
//!   it, and the build the profile plays, which opens the build chooser;
//! - worlds: `worlds::list` of the selected profile's `saves/` (`level.dat` and `icon.png`, read
//!   only), newest first; Play starts the game straight into one (`quick_play`), and the box
//!   opens its data packs;
//! - a world without `icon.png` is drawn from `dirt.png` of the player's own prepared game
//!   files (`backend::resources`, nothing is shipped), else a plain tile, no letter;
//! - signed out with no worlds, the panel is the welcome in place of worlds.
use super::*;
use crate::frame::button;
use crate::frame::Reach as _;
use crate::launch_state::Main;
use crate::paint::palette;
use crate::sheets::Sheet;
use crate::worlds_page;
use gpui::rgb;
use std::cell::Cell;
use std::rc::Rc;
use sunrise_launcher::worlds::World;

/// A world's picture in a row: its 64 pixels, one to one.
const WORLD_ICON: f32 = 64.;
/// And as a tile: its 64 pixels, doubled.
const WORLD_TILE: f32 = 128.;
/// How much of a tile's data packs button shows while nothing points at the tile.
const PACKS_AT_REST: f32 = 0.6;
/// The room between the header and the panel under it.
const GAP: f32 = 20.;
/// How far the header stands in from the panel's edge.
const INSET: f32 = 32.;

#[derive(Default)]
pub(crate) struct HomeState {
    /// Where the build picker was last drawn: the build chooser opens under it.
    picker: Rc<Cell<Option<Bounds<gpui::Pixels>>>>,
}

impl HomeState {
    /// Where the build chooser opens: the left edge and the top, in the window.
    pub(crate) fn chooser_at(&self) -> (f32, f32) {
        match self.picker.get() {
            Some(picker) => (f32::from(picker.left()), f32::from(picker.bottom()) + 6.),
            // Home was not drawn yet; it is by the next frame.
            None => (INSET, 96.),
        }
    }
}

pub(crate) fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// How long a profile was played and when last, as the line under its name. None until it
/// was played.
pub(crate) fn played_line(
    played: Duration,
    last: Option<std::time::SystemTime>,
) -> Option<(String, String)> {
    let last = last?;
    Some((
        trf("{time} played", &[("time", &instances::play_time(played))]),
        trf("Last played {when}", &[("when", &worlds_page::ago(last))]),
    ))
}

pub(crate) fn card() -> Div {
    div()
        .bg(palette().card())
        .border_1()
        .border_color(rgb(palette().divider))
        .rounded(px(tokens::R_CARD))
        .overflow_hidden()
}

pub(crate) fn heading(label: &str) -> Div {
    div()
        .text_size(px(tokens::HEADING))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(palette().text))
        .child(label.to_string())
}

pub(crate) fn muted(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(tokens::SMALL))
        .text_color(rgb(palette().muted))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text.into())
}

impl Launcher {
    pub(crate) fn home(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(instance) = self.selected_instance().cloned() else {
            return div()
                .p(px(32.))
                .text_color(rgb(palette().muted))
                .child(tr("Getting ready…"))
                .into_any_element();
        };
        self.load_worlds(instance.home.clone(), cx);
        let header = self.profile_header(&instance, window, cx);
        let first_start = self.accounts.selected().is_none() && self.worlds.list.is_empty();
        let body = if first_start {
            self.welcome(cx)
        } else {
            self.continue_playing(&instance, window, cx)
        };
        div()
            .id("alt-home")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&crate::frame::scrolls("home"))
            .flex()
            .flex_col()
            .gap(px(GAP))
            .pt(px(24.))
            // Home paints no page of its own: the header stands on the scenery, inset like the
            // text of the panel under it. A picture may be any colour, so over one the header
            // stands on a plate of its own, as wide as what it holds; the picture shows beside
            // it.
            .child(if self.theme.photo {
                div().flex_none().flex().child(
                    card()
                        .min_w_0()
                        .rounded(px(tokens::R_PANEL))
                        .px(px(INSET))
                        .py(px(20.))
                        .child(header),
                )
            } else {
                header.flex_none().px(px(INSET))
            })
            // The panel is the sidebar's material and fills Home's box to its left, right and
            // bottom edges, so the frame lines it up with the sidebar. It reaches the foot when
            // the worlds leave room and is never squeezed: a short window scrolls the page.
            .child(
                div()
                    .flex_grow(1.)
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .p(px(22.))
                    .rounded(px(tokens::R_PANEL))
                    .bg(palette().chrome_plate())
                    .border_1()
                    .border_color(rgb(palette().divider))
                    .child(body),
            )
            .into_any_element()
    }

    /// The selected profile: its mark and name, its play time, then Play, the build it plays
    /// and what Play has to say.
    fn profile_header(
        &mut self,
        instance: &instances::Instance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let played = match played_line(instance.played, instance.last_played) {
            Some((time, last)) => format!("{time}  ·  {last}"),
            None => tr("Not played yet").to_string(),
        };
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .text_size(px(tokens::SMALL))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette().text))
                    .child(tr("Selected profile")),
            )
            .child(
                div()
                    .mt(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .child(self.profile_mark(instance, 56.))
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(tokens::HERO))
                            .line_height(px(tokens::HERO * 1.15))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(palette().text))
                            .truncate()
                            .child(instance.name.clone()),
                    ),
            )
            .child(
                div()
                    .mt(px(10.))
                    .text_size(px(tokens::BODY))
                    .text_color(rgb(palette().text))
                    .truncate()
                    .child(played),
            )
            .child(self.play_row(instance, window, cx).mt(px(18.)))
    }

    /// Play, the build picker, Play's one action and its one sentence, then the cat's lane.
    fn play_row(
        &mut self,
        instance: &instances::Instance,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        self.mods_read_once();
        let view = self.launch_facts().view();
        let idle = view.main == Main::Play;
        let busy = self.launch_busy();
        let on = rgb(palette().accent_on);
        let play = div()
            .id("alt-play")
            .tab_index(0)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .rounded(px(tokens::R_CONTROL))
            .h(px(tokens::PLAY))
            .min_w(px(132.))
            .px(px(22.))
            .font_weight(FontWeight::BOLD)
            .text_size(px(tokens::LEAD))
            .text_color(on)
            .bg(rgb(palette().play_fill))
            .when(idle, |d| d.child(blocks::icon("play", 17., on.into())))
            .child(view.main.label())
            .focus_visible(|s| s.shadow(crate::frame::focus_ring(palette().focus)))
            .seen()
            // Work under way keeps Play's colours, so what it says reads; only a launcher that
            // can't play at all dims it.
            .when(!view.enabled && !busy, |d| d.opacity(0.6))
            .when(view.enabled, |d| {
                d.cursor_pointer()
                    .hover(|s| s.opacity(0.92))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cat_off_play();
                        this.press_play();
                        cx.notify();
                    }))
            });
        let act = view.act.clone().map(|act| {
            let id = match act {
                crate::launch_state::Act::StopBuild => "alt-build-stop",
                crate::launch_state::Act::Update => "alt-update",
                _ => "alt-fix",
            };
            button(id, act.label()).on_click(cx.listener(move |this, _, _, cx| {
                this.launch_act(&act);
                cx.notify();
            }))
        });
        let says = div()
            // As wide as what it says: the rest of the row is the cat's.
            .min_w_0()
            .text_size(px(tokens::LABEL))
            .text_color(rgb(if view.failed {
                palette().warning
            } else {
                palette().text
            }))
            // Two lines fit beside Play; a longer message is cut, never drawn outside the row.
            .line_clamp(2)
            .text_ellipsis()
            .children(self.build_line())
            .children(view.said.clone())
            // Nothing to act on here: the row under Mods says why and what to do.
            .children(view.mods_off.map(|count| {
                div()
                    .id("alt-not-on-build")
                    .reach()
                    .cursor_pointer()
                    .hover(|s| s.text_color(rgb(palette().accent_ink)))
                    .child(match count {
                        1 => tr("1 mod not on this build").to_string(),
                        n => trf(
                            "{count} mods not on this build",
                            &[("count", &n.to_string())],
                        ),
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_installed_mods();
                        cx.notify();
                    }))
            }));
        // The cat walks the empty stretch and stands on the panel's edge. It stays in while a
        // button stands in its way.
        let (lane, _) = self.cat(act.is_none(), false, window, cx);
        let tall = tokens::PLAY;
        div()
            .flex()
            .items_end()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_w_0()
                    .h(px(tall))
                    .child(play)
                    .child(self.build_picker(instance, cx))
                    .children(act)
                    .child(says),
            )
            .child(lane.ml(px(12.)).mb(px(-GAP)))
    }

    /// The build the profile plays and how it follows new ones: a press opens the chooser.
    fn build_picker(
        &mut self,
        instance: &instances::Instance,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let signed_in =
            self.accounts.selected().is_some() || sunrise_launcher::developer_offline::selected();
        let version = self.version_of(instance);
        // A game built with mods reads as the build it was made from, like any other.
        let made_from = version
            .filter(|t| native_mods::is_native(t))
            .and_then(|t| crate::glass_instance::native_base(t));
        let nightly = |t: &str| sunrise_launcher::nightly::date_of(t).is_some();
        let build = match version {
            Some(t) if nightly(made_from.as_deref().unwrap_or(t)) => trf(
                "Sunrise · {build}",
                &[("build", &crate::sheets::build_name(t))],
            ),
            Some(t) => self.version_label(t),
            None => tr("Sunrise").into(),
        };
        let follows = if version.is_none() && !signed_in {
            tr("Installs after you sign in")
        } else if version.is_none() {
            tr("Not installed yet")
        } else if instance.version.is_some() {
            tr("Stays on this build")
        } else if instance.channel == instances::Channel::Stable {
            tr("Stable")
        } else {
            tr("Nightly")
        };
        let at = self.ui.home.picker.clone();
        div()
            .id("alt-build")
            .flex_none()
            .rounded(px(tokens::R_CONTROL))
            .border_1()
            .border_color(rgb(palette().line))
            .bg(palette().card())
            .reach_edge()
            .cursor_pointer()
            .hover(|s| s.bg(rgb(palette().hover)))
            .flex()
            .items_center()
            .gap(px(14.))
            .h(px(tokens::PLAY))
            .pl(px(16.))
            .pr(px(14.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(tokens::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette().text))
                            .child(build),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::SMALL))
                            .text_color(rgb(palette().muted))
                            .child(follows),
                    ),
            )
            .child(blocks::icon(
                "chevron-down",
                13.,
                rgb(palette().muted).into(),
            ))
            .child(
                gpui::canvas(
                    move |bounds, window, cx| {
                        if at.replace(Some(bounds)) != Some(bounds) {
                            // An open chooser follows it, in the frame after this one.
                            let window = window.window_handle();
                            cx.defer(move |cx| {
                                window.update(cx, |_, window, _| window.refresh()).ok();
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if !this.launch_busy() && this.client.is_none() {
                    this.open_sheet(Sheet::Build);
                }
                cx.notify();
            }))
    }

    /// The selected profile's recent worlds, or what to do when it has none.
    fn continue_playing(
        &mut self,
        instance: &instances::Instance,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let list: Vec<World> = self.worlds.list.clone();
        let tiles = self.settings.world_layout == backend::WorldLayout::Tiles;
        let top = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                heading(tr("Continue playing"))
                    .text_size(px(tokens::TITLE - 4.))
                    .font_weight(FontWeight::BOLD),
            )
            .child(muted(trf(
                "Recent worlds in {profile}",
                &[("profile", &instance.name)],
            )));
        let body = if !list.is_empty() {
            if tiles {
                let mut grid = div().flex().flex_wrap().gap(px(16.));
                for world in list.iter() {
                    grid = grid.child(self.world_tile(world, window, cx));
                }
                grid
            } else {
                let mut rows = div().flex().flex_col().gap(px(8.));
                for world in list.iter() {
                    rows = rows.child(self.world_row(world, cx));
                }
                rows
            }
        } else if !self.worlds.fresh {
            // Still being read: no "none yet" until that is known.
            div()
        } else {
            let saves = instance.home.join("saves");
            div()
                .flex_grow(1.)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .py(px(28.))
                .child(blocks::icon("globe", 28., rgb(palette().muted).into()))
                .child(heading(tr("No worlds here yet")))
                .child(
                    div()
                        .max_w(px(560.))
                        .text_center()
                        .text_size(px(tokens::LABEL))
                        .text_color(rgb(palette().muted))
                        .child(tr(
                            "Press Play and make a world in the game. It shows up here, ready to continue.",
                        )),
                )
                .child(
                    button("alt-worlds-folder", tr("Open folder"))
                        .mt(px(8.))
                        .on_click(move |_, _, cx| {
                            let _ = std::fs::create_dir_all(&saves);
                            crate::glass_toast::open(cx, &saves);
                        }),
                )
        };
        div()
            .flex_grow(1.)
            .flex()
            .flex_col()
            .gap(px(16.))
            .child(top)
            .child(body)
    }

    /// A world's picture, `side` points square: its own `icon.png` (64 pixels square, as the
    /// game takes it) at a whole multiple of its pixels, so it stays sharp; else `dirt.png` of
    /// the player's own game files; else a plain tile with the map glyph.
    fn world_icon(&mut self, world: &World, side: f32) -> Div {
        let tile = div()
            .flex_none()
            .size(px(side))
            .rounded(px(tokens::R_CONTROL))
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(palette().selected));
        let stamp = world
            .last_played
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let own = world.icon.clone().and_then(|icon| {
            let key = format!("alt-world:{}:{stamp}", icon.display());
            self.icon_image(key, side, move || Some(icon))
        });
        // Always dirt, from the player's own prepared files; none shipped.
        let picture = own.or_else(|| {
            let file = self
                .game_resources()
                .join("assets/minecraft/textures/block/dirt.png");
            let key = format!("alt-dirt:{}", file.display());
            self.icon_image(key, side, move || Some(file))
        });
        match picture {
            Some(image) => tile.child(gpui::img(image).size(px(side))),
            None => tile.child(blocks::icon("map", side * 0.4, rgb(palette().muted).into())),
        }
    }

    /// "Survival · Today".
    fn world_facts(world: &World) -> String {
        format!(
            "{} · {}",
            tr(world.mode),
            sentence(&worlds_page::ago(world.last_played))
        )
    }

    /// One world as a row: its picture, what it is, and its two buttons at the end.
    fn world_row(&mut self, world: &World, cx: &mut Context<Self>) -> Div {
        let (folder, path) = (world.folder.clone(), world.path.clone());
        let packs = button(
            format!("alt-world-packs-{}", world.folder),
            tr("Data packs"),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.show_world_packs(path.clone());
            cx.notify();
        }));
        let play = button(format!("alt-world-{}", world.folder), "")
            .gap(px(6.))
            .child(blocks::icon("play", 15., rgb(palette().text).into()))
            .child(tr("Play"))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.play_to("--world", folder.clone());
                cx.notify();
            }));
        card()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(14.))
            .p(px(8.))
            .pr(px(14.))
            .child(self.world_icon(world, WORLD_ICON))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(tokens::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette().text))
                            .truncate()
                            .child(world.name.clone()),
                    )
                    .child(muted(Self::world_facts(world))),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .gap(px(8.))
                    .child(packs)
                    .child(play),
            )
    }

    /// One world as a tile: its picture, which plays it, with what it is under it. Its data
    /// packs button sits on the picture, faint until the pointer is over the tile or the
    /// keyboard is on the picture or on the button.
    fn world_tile(&mut self, world: &World, window: &Window, cx: &mut Context<Self>) -> Div {
        let (folder, path) = (world.folder.clone(), world.path.clone());
        let focus = self
            .worlds
            .tile_focus
            .entry(world.folder.clone())
            .or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        let group = SharedString::from(format!("alt-world-tile-{}", world.folder));
        let picture = div()
            .id(format!("alt-world-{}", world.folder))
            .track_focus(&focus)
            .relative()
            .cursor_pointer()
            .rounded(px(tokens::R_CONTROL))
            .focus_visible(|s| s.shadow(crate::frame::focus_ring(palette().focus)))
            .seen()
            .child(self.world_icon(world, WORLD_TILE))
            // What a press does, said on the picture itself.
            .child(
                div()
                    .absolute()
                    .right(px(8.))
                    .bottom(px(8.))
                    .size(px(32.))
                    .rounded(px(tokens::R_CONTROL))
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(rgb(palette().text))
                    .child(blocks::icon("play", 15., rgb(palette().background).into())),
            )
            .tooltip(crate::glass_ui::tip(trf(
                "Play {world}",
                &[("world", &world.name)],
            )))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.play_to("--world", folder.clone());
                cx.notify();
            }));
        let packs = crate::frame::button_base(format!("alt-world-packs-{}", world.folder), "")
            .absolute()
            .left(px(8.))
            .bottom(px(8.))
            .size(px(32.))
            .px_0()
            .bg(rgb(palette().raised))
            .occlude()
            .when(!focus.is_focused(window), |d| d.opacity(PACKS_AT_REST))
            .group_hover(group.clone(), |s| s.opacity(1.))
            .focus_visible(|s| s.opacity(1.).border_color(rgb(palette().focus)))
            .child(blocks::icon("archive", 17., rgb(palette().text).into()))
            .tooltip(crate::glass_ui::tip(tr("Data packs")))
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.show_world_packs(path.clone());
                cx.notify();
            }));
        div()
            .flex_none()
            .w(px(WORLD_TILE))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().group(group).relative().child(picture).child(packs))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        // A long name is cut at the picture's width: the tooltip has all of it.
                        div()
                            .id(format!("alt-world-name-{}", world.folder))
                            .text_size(px(tokens::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette().text))
                            .truncate()
                            .child(world.name.clone())
                            .tooltip(crate::glass_ui::tip(world.name.clone())),
                    )
                    .child(muted(Self::world_facts(world))),
            )
    }

    /// Every destination goes through `play_game`, which runs the sign-in, install, prepare,
    /// build and trust gates before the game starts.
    pub(crate) fn play_to(&mut self, flag: &'static str, value: String) {
        self.quick_play = Some((flag, value));
        self.play_game();
    }

    /// The first start: signed out, nothing to play yet. Nothing is read from outside the
    /// launcher's own data.
    fn welcome(&mut self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .px(px(12.))
            .py(px(18.))
            .child(
                div()
                    .text_size(px(tokens::DISPLAY))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(palette().text))
                    .child(tr("Welcome to Sunrise")),
            )
            .child(
                div()
                    .text_size(px(tokens::BODY))
                    .text_color(rgb(palette().text))
                    .child(tr(
                        "Sign in with the Microsoft account that owns Minecraft: Java Edition. Sunrise then installs itself and you can play.",
                    )),
            )
            .child(
                div()
                    .text_size(px(tokens::SMALL))
                    .text_color(rgb(palette().muted))
                    .child(tr(
                        "While you play, Sunrise counts you as playing once a minute. Nothing else is sent.",
                    )),
            )
            .child(
                div().flex().child(
                    div()
                        .id("alt-privacy")
                        .reach()
                        .cursor_pointer()
                        .text_size(px(tokens::LABEL))
                        .text_color(rgb(palette().accent_ink))
                        .child(tr("Privacy choices"))
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.settings_open(crate::settings::Section::Privacy);
                            this.navigate(crate::frame::Page::Settings);
                            cx.notify();
                        })),
                ),
            )
    }
}

#[cfg(test)]
#[path = "home/tests.rs"]
mod tests;
