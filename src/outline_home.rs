//! The Outline look's Home (`docs/design-system.md`, "Outline"; Figma: Explorations → Home): the
//! edition, the instance's name, the tagline, Play and the folder, the version and the invite code
//! centred on the window's first screen, over the theme's scenery. The instance's tabs stay
//! below, a scroll away.
use super::*;
use crate::glass_ui::{self as ui, mono, white};
use sunrise_launcher_theme::glass::{fill, ink};

/// The top bar above the first screen (before the page has been laid out once), and the room
/// under its floor.
const TOP: f32 = 70.;
const UNDER_FLOOR: f32 = 24.;
/// The first screen never squeezes shorter than this.
const FIRST_SCREEN_MIN: f32 = 560.;
/// Play and the folder side by side: the hero's widest row.
const HERO_WIDTH: f32 = 412.;

impl Launcher {
    /// The page's width between the sidebars.
    pub(super) fn outline_page_width(&self) -> f32 {
        self.window_width - 2. * self.outline_side_width()
    }

    /// How tall Home's first screen is: from the top bar down to the hero's floor.
    pub(super) fn first_screen(&self) -> f32 {
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
        let build = self.glass_build_notice(cx);
        let crash = self.crash.clone().map(|c| self.crash_notice(c, cx));
        let said: Vec<Div> = [self.prepare_error.clone(), self.client_error.clone()]
            .into_iter()
            .flatten()
            .map(|e| notice(e, &self.theme))
            .collect();
        let name = self
            .selected_instance()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let title = div()
            .pt(px(20.))
            .max_w_full()
            .truncate()
            .font_family(sunrise_launcher_theme::glass::DISPLAY_FONT)
            .font_weight(FontWeight::BOLD)
            .text_size(px(88.))
            .line_height(px(92.))
            .text_color(white(ink::FULL));
        let title = title.child(name);
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
            // The preparation's steps right under Play, where the player looks after pressing it,
            // and what Play could not do: a crash, a preparation that failed, why the game did not
            // start. Home fits the window, so anything after the first screen is never seen.
            .children(
                steps
                    .into_iter()
                    .chain(build)
                    .chain(crash)
                    .chain(said)
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
                .rounded(px(ui::rad(0.)))
                .font_family(sunrise_launcher_theme::glass::FONT)
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
            crate::press(
                mono(12., ink::BODY).id("home-version"),
                &"home-version".into(),
                |d| d.opacity(theme::PRESSED),
            )
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(30.))
            .px(px(10.))
            .cursor_pointer()
            .hover(|s| s.text_color(white(ink::FULL)))
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
