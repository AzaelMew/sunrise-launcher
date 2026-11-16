//! The Outline look's Home (`docs/design-system.md`, "Outline"): the edition, the instance's name
//! and Play centred on the window, the version under Play and the invite code under them, then
//! "Jump back in" at the foot of the first screen. The instance's tabs stay below, a scroll away.
use super::*;
use crate::glass_ui::{self as ui, mono, white};
use crate::outline_frame::{edge, hoverable};
use sunrise_theme::glass::{fill, ink};

/// The top bar and the page's padding above and below the first screen.
const FRAME: f32 = 64. + 48.;
/// The hero and Jump back in never squeeze closer than this.
const FIRST_SCREEN_MIN: f32 = 560.;
const CARD_HEIGHT: f32 = 54.;
/// About as wide as the hero's widest row: Play and the folder.
const HERO_WIDTH: f32 = 560.;

impl Launcher {
    /// How far left a centred piece `width` wide moves so it centres on the window rather than on
    /// the page beside the rail: half the rail's width where the page has the room, less where it
    /// would reach the rail.
    fn outline_centre_shift(&self, width: f32) -> f32 {
        let side = frame::RAIL + frame::MARGIN;
        let page = self.window_width - side - 2. * theme::PAGE_GUTTER;
        ((page - width) / 2.).clamp(0., side / 2.)
    }

    pub(super) fn outline_home_head(
        &mut self,
        tag: &str,
        prepared: bool,
        steps: Option<Div>,
        cx: &mut Context<Self>,
    ) -> Div {
        let java = self.selected_instance().and_then(|i| i.java.clone());
        let name = self
            .selected_instance()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let title = div()
            .mt(px(24.))
            .max_w_full()
            .truncate()
            .font_family(sunrise_theme::glass::DISPLAY_FONT)
            .font_weight(FontWeight::BOLD)
            .text_size(px(80.))
            .line_height(px(88.))
            .text_color(white(ink::FULL))
            .child(name);
        let tagline = java.is_none().then(|| {
            ui::text(17., FontWeight::NORMAL, ink::BODY)
                .mt(px(4.))
                .child(tr("Minecraft. Rewritten in Rust."))
        });
        let actions = div()
            .mt(px(28.))
            .flex()
            .items_center()
            .flex_wrap()
            .justify_center()
            .gap(px(8.))
            .child(self.play_group(tag, prepared || java.is_some(), cx))
            .children(self.folder_group(java.is_none(), cx));
        let notice = self.glass_build_notice(cx);
        let hero = div()
            .relative()
            .left(px(-self.outline_centre_shift(HERO_WIDTH)))
            .flex()
            .flex_col()
            .items_center()
            .min_w_0()
            .pt(px(40.))
            .child(motion::enter(
                ElementId::named_usize("home-editions", self.switches),
                0,
                self.edition_tabs(java.is_some(), cx),
            ))
            .child(motion::enter(
                ElementId::named_usize("home-title", self.switches),
                1,
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .max_w_full()
                    .child(title)
                    .children(tagline),
            ))
            .child(motion::enter(
                ElementId::named_usize("home-actions", self.switches),
                2,
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(actions)
                    .children(
                        self.version_chip(cx)
                            .map(|chip| div().mt(px(12.)).child(chip)),
                    ),
            ))
            // The preparation's steps right under Play, where the player looks after pressing it.
            .children(
                steps
                    .into_iter()
                    .chain(notice)
                    .map(|s| div().mt(px(20.)).w(px(HERO_WIDTH)).max_w_full().child(s)),
            )
            .child(
                div()
                    .mt(px(24.))
                    .w(px(360.))
                    .max_w_full()
                    .child(self.glass_join_friend(tag, false, cx)),
            );
        let first = (self.window_height - FRAME).max(FIRST_SCREEN_MIN);
        div()
            .flex()
            .flex_col()
            .justify_between()
            .min_h(px(first))
            .child(hero)
            .child(self.jump_back_in(cx))
    }

    /// The edition tabs: Rust Edition and Java Edition, the selected instance's lit. An
    /// instance's edition is chosen when it is made, so the other tab goes to an instance of
    /// that edition ([`Self::switch_edition`]).
    fn edition_tabs(&mut self, java: bool, cx: &mut Context<Self>) -> Div {
        let sunrise = self.sunrise_mark(16.);
        let grass = match self.grass_mark(16.) {
            Some(image) => gpui::img(image)
                .flex_none()
                .size(px(16.))
                .into_any_element(),
            None => div().into_any_element(),
        };
        let t = self.theme;
        let tab = |id: &'static str, label: &'static str, icon: AnyElement, on: bool| {
            keyboard(
                hoverable(id, fill::HOVER)
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(32.))
                    .px(px(12.))
                    .cursor_pointer()
                    .when(on, |d| d.bg(white(fill::ACTIVE)).child(edge(id)))
                    .child(
                        div()
                            .relative()
                            .opacity(if on { 1. } else { 0.6 })
                            .child(icon),
                    )
                    .child(
                        mono(12., if on { ink::FULL } else { ink::NOTE })
                            .relative()
                            .child(tr(label).to_uppercase()),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if on {
                            return;
                        }
                        this.switch_edition(id == "edition-java", cx);
                        cx.notify();
                    })),
                &t,
            )
        };
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(tab("edition-rust", "Rust Edition", sunrise, !java))
            .child(tab("edition-java", "Java Edition", grass, java))
    }

    /// The last played worlds and the first saved server, as one row of cards.
    fn jump_back_in(&mut self, cx: &mut Context<Self>) -> Div {
        let Some(instance) = self.selected_instance().cloned() else {
            return div();
        };
        self.load_worlds(instance.home.clone(), cx);
        self.load_servers(instance.home.clone());
        self.ping_servers(cx);
        let can_play = self.can_start_selected();
        let mut cards: Vec<AnyElement> = Vec::new();
        for w in self.worlds.list.iter().take(2).cloned() {
            let facts = format!(
                "{} · {}",
                tr(w.mode),
                glass_home::last_played(w.last_played)
            )
            .to_uppercase();
            let picture = match &w.icon {
                Some(path) => div()
                    .flex_none()
                    .w(px(56.))
                    .h(px(36.))
                    .overflow_hidden()
                    .child(
                        gpui::img(gpui::ImageSource::from(path.clone()))
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover),
                    )
                    .into_any_element(),
                None => tile("earth"),
            };
            let folder = w.folder.clone();
            cards.push(
                card(
                    format!("jump-world-{}", w.folder),
                    picture,
                    w.name.clone(),
                    mono(11., ink::QUIET).truncate().child(facts),
                )
                .when(!can_play, |d| d.cursor_default())
                .when(can_play, |d| {
                    d.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.quick_play("--world", folder.clone());
                        cx.notify();
                    }))
                })
                .into_any_element(),
            );
        }
        if let Some((server, icon)) = self.multiplayer.list.first().cloned() {
            let (live, state) = match self.multiplayer.reach.get(&server.ip) {
                Some(multiplayer_page::Reach::Online { players, max }) => (
                    true,
                    trf(
                        "{online} of {max}",
                        &[("online", &players.to_string()), ("max", &max.to_string())],
                    ),
                ),
                Some(multiplayer_page::Reach::Offline) => (false, tr("Offline").to_string()),
                _ => (false, server.ip.clone()),
            };
            let picture = match icon {
                Some(image) => gpui::img(gpui::ImageSource::from(image))
                    .flex_none()
                    .size(px(36.))
                    .into_any_element(),
                None => tile("computer"),
            };
            let state = mono(11., ink::QUIET)
                .flex()
                .items_center()
                .gap(px(6.))
                .children(live.then(|| div().flex_none().size(px(6.)).bg(ui::live())))
                .child(state.to_uppercase());
            let ip = server.ip.clone();
            cards.push(
                card(
                    format!("jump-server-{}", server.ip),
                    picture,
                    server.name.clone(),
                    state,
                )
                .when(!can_play, |d| d.cursor_default())
                .when(can_play, |d| {
                    d.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.quick_play("--server", ip.clone());
                        cx.notify();
                    }))
                })
                .into_any_element(),
            );
        }
        if cards.is_empty() {
            return div();
        }
        let all = mono(12., ink::NOTE)
            .id("jump-all-worlds")
            .cursor_pointer()
            .hover(|s| s.text_color(white(ink::FULL)))
            .child(tr("All worlds").to_uppercase())
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                // The tabs sit under the first screen: bring them up, then open Worlds a frame
                // later, so the world list is first drawn where it will stay.
                let first = (this.window_height - FRAME).max(FIRST_SCREEN_MIN);
                this.content_scroll
                    .handle
                    .set_offset(gpui::point(px(0.), px(-first)));
                cx.on_next_frame(window, |this, _, cx| {
                    this.home_tab = home_tabs::HomeTab::Worlds;
                    cx.notify();
                });
                cx.notify();
            }));
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .pt(px(32.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(mono(12., ink::NOTE).child(tr("Jump back in").to_uppercase()))
                    .child(all),
            )
            .child(div().flex().gap(px(10.)).children(cards))
    }
}

/// A Jump back in card: a picture, a name and one line under it, on the Outline look's plate.
fn card(id: String, picture: AnyElement, name: String, under: Div) -> Stateful<Div> {
    let id = SharedString::from(id);
    track_hover(div().id(id.clone()), id.clone())
        .relative()
        .active(|s| s.opacity(theme::PRESSED))
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(px(12.))
        .h(px(CARD_HEIGHT))
        .px(px(8.))
        .cursor_pointer()
        .child(crate::glass::backdrop(crate::glass::Kind::Glass, 0.))
        .child(hover_layer(&id, white(0.07), 0., 0.))
        .child(edge(id))
        .child(div().relative().flex_none().child(picture))
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    ui::text(15., FontWeight::SEMIBOLD, ink::FULL)
                        .truncate()
                        .child(name),
                )
                .child(under.min_w_0()),
        )
}

fn tile(icon: &'static str) -> AnyElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(56.))
        .h(px(36.))
        .bg(white(fill::BUTTON))
        .child(ui::icon(icon, 17., white(ink::QUIET)))
        .into_any_element()
}
