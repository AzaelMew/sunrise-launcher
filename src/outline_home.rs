//! The Outline look's Home (`docs/design-system.md`, "Outline"; Figma: Explorations → Home): the
//! edition, the instance's name, the tagline, Play and the folder, the version and the invite code
//! centred on the window. The name stands in a framed card of the Sunny panel's sunrise: the night
//! sky over it, the hard bands and the pixel sun under it. Play and the rest sit on the plain page
//! below the card.
use super::*;
use crate::glass_ui::{self as ui, mono, white};
use sunrise_theme::glass::{fill, ink};

/// The top bar above the first screen (before the page has been laid out once), and the page's
/// own padding under it, which the sky reaches over to the window's foot.
const TOP: f32 = 70.;
const PAGE_FOOT: f32 = 48.;
/// The first screen never squeezes shorter than this.
const FIRST_SCREEN_MIN: f32 = 560.;
/// Play and the folder side by side: the hero's widest row.
const HERO_WIDTH: f32 = 412.;
/// The hero card at its widest, and the height of its banded sky.
const CARD_MAX: f32 = 880.;
const CARD_SKY: f32 = 200.;
/// The night above the bands, where the card's name stands.
const SKY_NIGHT: u32 = 0x0c1030;
/// The card's 1 px edge (the panel's stroke).
const CARD_EDGE: u32 = 0x2c3250;
/// The sky's bands as the Sunny panel draws them, top down: each colour and where it ends, as a
/// share of the sky's height. Hard edges, no blending.
const SKY: [(u32, f32); 5] = [
    (0x161a4a, 0.29),
    (0x2c2168, 0.51),
    (0x5a2d7c, 0.71),
    (0xb34a78, 0.87),
    (0xf39a5a, 1.),
];
/// Px to a unit of the sun's 16-unit grid.
const SUN_UNIT: f32 = 8.;

impl Launcher {
    /// How far left of the page's centre the hero and the top bar's pages sit: half the sidebar,
    /// which centres them on the window, but never so far that the hero's widest row would come
    /// closer than the gutter to the sidebar.
    pub(super) fn outline_centre_shift(&self) -> f32 {
        if self.outline_right_side() {
            // Between two sidebars as wide as each other, the page's centre is the window's.
            return 0.;
        }
        let page = self.outline_page_width();
        let room = ((page - HERO_WIDTH) / 2. - theme::PAGE_GUTTER).max(0.);
        (frame::SIDEBAR / 2.).min(room)
    }

    /// The page's width between the sidebars.
    pub(super) fn outline_page_width(&self) -> f32 {
        self.window_width - frame::SIDEBAR - self.outline_right_width()
    }

    /// How tall Home's first screen is: from the top bar down to the page's foot padding.
    fn first_screen(&self) -> f32 {
        // The page's own height, so a drawn title bar is left out too.
        let page = f32::from(self.content_scroll.handle.bounds().size.height);
        let page = if page > 0. {
            page
        } else {
            self.window_height - TOP
        };
        (page - PAGE_FOOT).max(FIRST_SCREEN_MIN)
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
            .pt(px(16.))
            .max_w_full()
            .truncate()
            .font_family(sunrise_theme::glass::DISPLAY_FONT)
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(64.))
            .line_height(px(68.))
            .text_color(white(ink::FULL))
            .child(name);
        let tagline = java.is_none().then(|| {
            ui::text(16., FontWeight::NORMAL, ink::NOTE)
                .pt(px(6.))
                .child(tr("Minecraft. Rewritten in Rust."))
        });
        let actions = div()
            .pt(px(28.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(self.play_group(tag, prepared || java.is_some(), cx))
            .children(self.folder_group(java.is_none(), cx));
        let night = div()
            .flex()
            .flex_col()
            .items_center()
            .min_w_0()
            .pt(px(36.))
            .pb(px(32.))
            .px(px(24.))
            .bg(gpui::rgb(SKY_NIGHT))
            .child(self.edition_tabs(java.is_some(), cx))
            .child(title)
            .children(tagline);
        let card = div()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(CARD_MAX))
            .border_1()
            .border_color(gpui::rgb(CARD_EDGE))
            .overflow_hidden()
            .child(night)
            .child(home_sky());
        let hero = div()
            .relative()
            .right(px(self.outline_centre_shift()))
            .flex()
            .flex_col()
            .items_center()
            .min_w_0()
            .pt(px(32.))
            .child(card)
            .child(actions)
            .children(
                self.version_chip(cx)
                    .map(|chip| div().pt(px(12.)).child(chip)),
            )
            // The preparation's steps right under Play, where the player looks after pressing it.
            .children(
                steps
                    .into_iter()
                    .chain(self.glass_build_notice(cx))
                    .map(|s| div().pt(px(20.)).w(px(HERO_WIDTH)).max_w_full().child(s)),
            )
            .child(
                div()
                    .pt(px(28.))
                    .w(px(360.))
                    .max_w_full()
                    .child(self.glass_join_friend(tag, false, cx)),
            );
        div()
            .relative()
            .flex()
            .flex_col()
            .min_h(px(self.first_screen()))
            .child(hero)
            .children(self.home_section_panel(cx))
    }

    /// The edition tabs: the instance's own lit. An instance's edition is chosen when it is made,
    /// so the other tab goes to an instance of that edition ([`Self::switch_edition`]).
    fn edition_tabs(&mut self, java: bool, cx: &mut Context<Self>) -> Div {
        let grass = match self.grass_mark(16.) {
            Some(image) => gpui::img(image)
                .flex_none()
                .size(px(16.))
                .into_any_element(),
            None => div().into_any_element(),
        };
        let tab = |label: &'static str, icon: AnyElement, open: bool| {
            let d = div()
                .id(SharedString::from(format!("edition-{label}")))
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .h(px(32.))
                .px(px(12.))
                .font_family(sunrise_theme::glass::FONT)
                .text_size(px(13.))
                .line_height(px(18.))
                .child(icon)
                .child(tr(label));
            if open {
                d.bg(white(fill::ACTIVE))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(white(ink::FULL))
            } else {
                d.text_color(white(ink::BODY))
                    .cursor_pointer()
                    .hover(|s| s.bg(white(fill::ROW_HOVER)).text_color(white(ink::FULL)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.switch_edition(label == "Java Edition", cx);
                        cx.notify();
                    }))
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(tab("Sunrise", crate::mark(16.).into_any_element(), !java))
            .child(tab("Java Edition", grass, java))
    }

    /// The version, quietly under Play: what the instance plays and a chevron. Its versions are
    /// in instance settings, which a click opens on Installation.
    fn version_chip(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let instance = self.selected_instance()?.clone();
        let label = self.sheet_version(&instance);
        Some(
            mono(12., ink::BODY)
                .id("home-version")
                .flex()
                .items_center()
                .gap(px(6.))
                .h(px(30.))
                .px(px(10.))
                .cursor_pointer()
                .hover(|s| s.text_color(white(ink::FULL)))
                .active(|s| s.opacity(theme::PRESSED))
                .child(label.to_uppercase())
                .child(ui::icon("arrow-down-01", 14., white(ink::BODY)))
                .tooltip(ui::tip(tr("Change the version in instance settings")))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.open_instance_sheet(instance.clone(), glass_sheet::SheetTab::Installation);
                    cx.notify();
                })),
        )
    }
}

/// The hero card's sky, as on the Sunny panel: hard bands from night blue down to orange, and the
/// pixel sun coming up onto the horizon at its foot.
fn home_sky() -> Div {
    let mut sky = div().relative().flex().flex_col().h(px(CARD_SKY));
    let mut from = 0.;
    for (colour, to) in SKY {
        sky = sky.child(
            div()
                .flex_basis(px(0.))
                .flex_grow(to - from)
                .bg(gpui::rgb(colour)),
        );
        from = to;
    }
    let sun = crate::widgets::sunrise_glyph(SUN_UNIT).with_animation(
        "home-sun-rise",
        crate::motion::eased(crate::motion::RISE),
        |d, t| d.top(px(crate::motion::SUN_RISE * (1. - t))),
    );
    sky.child(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .flex()
            .justify_center()
            .overflow_hidden()
            .child(sun),
    )
}
