//! The Outline look's Home (`docs/design-system.md`, "Outline"; Figma: Explorations → Home): the
//! edition, the instance's name, the tagline, Play and the folder, the version and the invite code
//! centred on the window between the first screen's separator lines, whose floor ends it. The
//! instance's tabs stay below, a scroll away.
use super::*;
use crate::glass_ui::{self as ui, mono, white};
use sunrise_theme::glass::{fill, ink};

/// The top bar above the first screen (before the page has been laid out once), and the room
/// under its floor.
const TOP: f32 = 70.;
const UNDER_FLOOR: f32 = 24.;
/// The first screen never squeezes shorter than this.
const FIRST_SCREEN_MIN: f32 = 560.;
/// Play and the folder side by side: the hero's widest row.
const HERO_WIDTH: f32 = 412.;
/// How far in from each edge the first screen's column lines run.
const COLUMN: f32 = 168.;
/// The faint arc around the left column line's top.
const ARC: f32 = 620.;

impl Launcher {
    /// How far left of the page's centre the hero and the top bar's pages sit: half the sidebar,
    /// which centres them on the window, but never so far that the hero's widest row would come
    /// closer than the gutter to the sidebar.
    pub(super) fn outline_centre_shift(&self) -> f32 {
        let page = self.window_width - frame::SIDEBAR;
        let room = ((page - HERO_WIDTH) / 2. - theme::PAGE_GUTTER).max(0.);
        (frame::SIDEBAR / 2.).min(room)
    }

    /// How tall Home's first screen is: from the top bar down to the hero's floor.
    fn first_screen(&self) -> f32 {
        // The page's own height, so a drawn title bar is left out too.
        let page = f32::from(self.content_scroll.handle.bounds().size.height);
        let page = if page > 0. {
            page
        } else {
            self.window_height - TOP
        };
        (page - UNDER_FLOOR).max(FIRST_SCREEN_MIN)
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
            .pt(px(20.))
            .max_w_full()
            .truncate()
            .font_family(sunrise_theme::glass::DISPLAY_FONT)
            .font_weight(FontWeight::BOLD)
            .text_size(px(88.))
            .line_height(px(92.))
            .text_color(white(ink::FULL))
            .child(name);
        let tagline = java.is_none().then(|| {
            ui::text(18., FontWeight::NORMAL, ink::NOTE)
                .pt(px(8.))
                .child(tr("Minecraft. Rewritten in Rust."))
        });
        let actions = div()
            .pt(px(28.))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(self.play_group(tag, prepared || java.is_some(), cx))
            .children(self.folder_group(java.is_none(), cx));
        let hero = div()
            .relative()
            .right(px(self.outline_centre_shift()))
            .flex()
            .flex_col()
            .items_center()
            .min_w_0()
            .pt(px(56.))
            .child(self.edition_tabs(java.is_some(), cx))
            .child(title)
            .children(tagline)
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
            .h(px(self.first_screen()))
            .child(self.home_lines())
            .child(hero)
    }

    /// The first screen's separator lines, part of the page so they scroll with it: the floor,
    /// and where the window has room beside the hero, a column line near each edge and a faint
    /// arc around the left one's top.
    fn home_lines(&self) -> Div {
        let page = self.window_width - frame::SIDEBAR - 2. * theme::PAGE_GUTTER;
        let wide = page >= HERO_WIDTH + 2. * COLUMN + 64.;
        let column = |right: bool| {
            let d = div()
                .absolute()
                .top_0()
                .bottom_0()
                .w(px(1.))
                .bg(white(0.07));
            if right {
                d.right(px(COLUMN))
            } else {
                d.left(px(COLUMN))
            }
        };
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(px(-theme::PAGE_GUTTER))
            .right(px(-theme::PAGE_GUTTER))
            .overflow_hidden()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(1.))
                    .bg(white(0.10)),
            )
            .when(wide, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(COLUMN - ARC / 2.))
                        .top(px(-ARC / 2.))
                        .size(px(ARC))
                        .rounded_full()
                        .border_1()
                        .border_color(white(0.06)),
                )
                .child(column(false))
                .child(column(true))
            })
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
