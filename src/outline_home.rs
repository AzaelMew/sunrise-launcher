//! The Outline look's Home (`docs/design-system.md`, "Outline"): the edition, the instance's name,
//! its version and Play centred over the sun, the invite code under them, and "Jump back in" at the
//! foot of the first screen. The instance's tabs stay below, a scroll away.
use super::*;
use crate::glass_ui::{self as ui, line, mono, white};
use crate::outline_frame::hoverable;
use sunrise_theme::glass::{fill, ink};

/// The top bar and the page's padding above and below the first screen.
const FRAME: f32 = 70. + 48.;
/// The hero and Jump back in never squeeze closer than this.
const FIRST_SCREEN_MIN: f32 = 560.;
const CARD_HEIGHT: f32 = 54.;
/// About as wide as the hero's widest row: the version, Play and the folder.
const HERO_WIDTH: f32 = 620.;

impl Launcher {
    /// How far left a centred piece `width` wide moves so it centres on the window rather than on
    /// the page beside the sidebar: half the sidebar's width where the page has the room, less
    /// where it would reach the sidebar.
    pub(super) fn outline_centre_shift(&self, width: f32) -> f32 {
        let side = frame::SIDEBAR + frame::MARGIN;
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
        match (&self.client, self.client_since) {
            (Some(_), None) => self.client_since = Some(Instant::now()),
            (None, Some(_)) => self.client_since = None,
            _ => {}
        }
        let name = self
            .selected_instance()
            .map(|i| i.name.clone())
            .unwrap_or_default();
        let title = div()
            .mt(px(20.))
            .max_w_full()
            .truncate()
            .font_family(sunrise_theme::glass::DISPLAY_FONT)
            .font_weight(FontWeight::BOLD)
            .text_size(px(88.))
            .line_height(px(92.))
            .text_color(white(ink::FULL))
            .child(name);
        let actions = div()
            .mt(px(24.))
            .flex()
            .items_center()
            .flex_wrap()
            .justify_center()
            .gap(px(8.))
            .children(java.is_none().then(|| self.glass_picker(cx)))
            .child(self.play_group(tag, prepared || java.is_some(), cx))
            .children(self.folder_group(java.is_none(), cx));
        let hero = div()
            .relative()
            .left(px(-self.outline_centre_shift(HERO_WIDTH)))
            .flex()
            .flex_col()
            .items_center()
            .min_w_0()
            .pt(px(40.))
            .child(self.edition_tabs(java.is_some(), cx))
            .child(title)
            .child(actions)
            .child(self.outline_status().mt(px(16.)))
            // The preparation's steps right under Play, where the player looks after pressing it.
            .children(
                steps
                    .into_iter()
                    .chain(self.glass_build_notice(cx))
                    .map(|s| div().mt(px(20.)).w(px(HERO_WIDTH)).max_w_full().child(s)),
            )
            .child(
                div()
                    .mt(px(20.))
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

    /// The edition tabs (`Tabs`, size `sm`): the instance's own lit. An instance's edition is
    /// chosen when it is made, so the other tab goes to an instance of that edition
    /// ([`Self::switch_edition`]).
    fn edition_tabs(&mut self, java: bool, cx: &mut Context<Self>) -> Div {
        let grass = self.grass_mark(18.);
        let sunrise = self.sunrise_mark(18.);
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
                d.text_color(white(ink::OFF))
                    .cursor_pointer()
                    .hover(|s| s.bg(white(fill::ROW_HOVER)).text_color(white(ink::FULL)))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.switch_edition(label == "Java Edition", cx);
                        cx.notify();
                    }))
            }
        };
        let java_icon = match grass {
            Some(image) => gpui::img(image)
                .flex_none()
                .size(px(18.))
                .into_any_element(),
            None => div().into_any_element(),
        };
        div()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(tab("Rust Edition", sunrise, !java))
            .child(tab("Java Edition", java_icon, java))
    }

    /// "READY · LAST PLAYED 22 H AGO", or how long the game has run; then the players counts.
    fn outline_status(&self) -> Div {
        let mut parts: Vec<String> = Vec::new();
        match (self.client_since, self.selected_instance()) {
            (Some(since), _) => parts.push(trf(
                "Playing for {time} now. Have fun!",
                &[("time", &instances::play_time(since.elapsed()))],
            )),
            (None, Some(i)) => {
                // The sidebar row's own state: Ready, Needs preparing, Installing…
                parts.push(self.row_state(i).1);
                if let Some(when) = i.last_played {
                    parts.push(trf(
                        "Last played {when}",
                        &[("when", &glass_home::last_played(when))],
                    ));
                }
            }
            (None, None) => {}
        }
        mono(12., ink::QUIET)
            .flex()
            .gap(px(6.))
            .child(parts.join(" · ").to_uppercase())
    }

    /// The last played worlds, the first saved server and the newest notes, as one row of cards.
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
                    mono_line(facts),
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
        // Under 1280 px the four cards squeeze their names: the notes stay on Versions.
        if let Some(news) = (self.window_width >= 1280.)
            .then(|| self.whats_new())
            .flatten()
        {
            cards.push(
                news.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.go(Page::Versions);
                    cx.notify();
                }))
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
            .child(tr("All worlds"))
            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                // The tabs sit under the first screen: bring them up, then open Worlds a frame
                // later. The world list places its rows from where it was last drawn, so it is
                // first drawn where it will stay.
                let first = (this.window_height - FRAME).max(FIRST_SCREEN_MIN);
                this.content_scroll
                    .handle
                    .set_offset(gpui::point(px(0.), px(-first)));
                cx.on_next_frame(window, |this, window, cx| {
                    this.home_tab = home_tabs::HomeTab::Worlds;
                    // Its rows measured, the list draws the ones it guessed away once more.
                    cx.on_next_frame(window, |_, _, cx| cx.notify());
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

    /// The newest notes as a card: their date, "What's new" and the first change.
    fn whats_new(&self) -> Option<Stateful<Div>> {
        let sunrise = self.selected_instance().is_some_and(|i| i.java.is_none());
        let played = sunrise.then(|| self.selected().cloned()).flatten();
        let Load::Ready(list) = &self.releases else {
            return None;
        };
        let (_, sections) = patch_source(list, played.as_deref())?;
        let items: Vec<String> = sections.into_iter().flat_map(|(_, items)| items).collect();
        let summary = items.first()?.clone();
        let day = list
            .first()
            .map(|r| glass_versions::published(&r.date, "").to_uppercase())
            .unwrap_or_default();
        let date = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(36.))
            .px(px(8.))
            .border_1()
            .border_color(line())
            .child(
                mono(11., ink::BODY)
                    .text_color(ui::color(ui::look().accent_soft))
                    .child(day),
            );
        let text = div()
            .text_size(px(13.))
            .text_color(white(ink::QUIET))
            .truncate()
            .child(summary);
        Some(card(
            "jump-news".into(),
            date.into_any_element(),
            tr("What's new").to_string(),
            text,
        ))
    }
}

/// A Jump back in card: a picture, a name and one line under it, on the Outline look's plate.
fn card(id: String, picture: AnyElement, name: String, under: Div) -> Stateful<Div> {
    let id = SharedString::from(id);
    track_hover(div().id(id.clone()), id.clone())
        .relative()
        .active(|s| s.opacity(theme::PRESSED).top(px(1.)))
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
        .child(crate::outline_frame::edge(id))
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

fn mono_line(text: String) -> Div {
    mono(11., ink::QUIET).truncate().child(text)
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

impl Launcher {
    /// The version (`VersionPicker`): "Version" over the one played and a chevron; its versions
    /// live in instance settings, which a click opens on Installation.
    fn glass_picker(&self, cx: &mut Context<Self>) -> Div {
        let Some(instance) = self.selected_instance().cloned() else {
            return div();
        };
        let label = self.sheet_version(&instance);
        div().child(keyboard(
            hoverable("picker", fill::BUTTON)
                .bg(white(fill::BUTTON))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(24.))
                .h(px(56.))
                .flex_none()
                .min_w(px(if self.window_width >= glass_home::WIDE {
                    220.
                } else {
                    170.
                }))
                .px(px(16.))
                .cursor_pointer()
                .child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .child(
                            ui::text(12., FontWeight::NORMAL, ink::QUIET)
                                .line_height(px(18.))
                                .child(tr("Version")),
                        )
                        .child(
                            ui::text(15., FontWeight::MEDIUM, ink::FULL)
                                .line_height(px(22.))
                                .child(label),
                        ),
                )
                .child(ui::icon("arrow-down-01", 18., white(ink::NOTE)).relative())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.open_instance_sheet(instance.clone(), glass_sheet::SheetTab::Installation);
                    cx.notify();
                })),
            &self.theme,
        ))
    }
}

type Sections = Vec<(String, Vec<String>)>;

/// The notes to show: the played Nightly's (or the newest Nightly's) by section, else the newest
/// launcher release's, with the line under their title.
fn patch_source(releases: &[Release], played: Option<&str>) -> Option<(String, Sections)> {
    let day = |r: &Release| glass_versions::published(&r.date, "");
    if let Some(played) = played {
        let nightly = releases
            .iter()
            .filter(|r| r.tag == played)
            .chain(releases.iter().filter(|r| r.nightly.is_some()))
            .map(|r| (r, patch_sections(&r.body)))
            .find(|(_, sections)| !sections.is_empty());
        if let Some((r, sections)) = nightly {
            return Some((format!("{} · {}", r.name, day(r)), sections));
        }
    }
    let r = releases
        .iter()
        .find(|r| r.nightly.is_none() && !r.prerelease)?;
    let version = r
        .tag
        .rsplit('@')
        .next()
        .unwrap_or(&r.tag)
        .trim_start_matches('v');
    let note = format!(
        "{} · {}",
        trf("Launcher {version}", &[("version", version)]),
        day(r)
    );
    Some((note, patch_sections(&r.body)))
}

/// A release body's `### ` sections and their `- ` lines, Contributors left out.
fn patch_sections(body: &str) -> Sections {
    let mut out: Sections = Vec::new();
    for line in body.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("### ") {
            out.push((name.trim().to_string(), Vec::new()));
        } else if line == "---" || line.starts_with("Your launcher updates itself") {
            break;
        } else if let (Some(item), Some((section, items))) =
            (line.strip_prefix("- "), out.last_mut())
        {
            if section.as_str() != "Contributors" {
                items.push(strip_refs(item));
            }
        }
    }
    out.retain(|(name, items)| name != "Contributors" && !items.is_empty());
    out
}

/// "Fix the picker (#6389, fixes #6342)" → "Fix the picker".
fn strip_refs(item: &str) -> String {
    let mut s = item.trim();
    while let Some(open) = s.rfind(" (") {
        let tail = &s[open..];
        if tail.ends_with(')') && tail.contains('#') {
            s = s[..open].trim_end();
        } else {
            break;
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_body_becomes_its_sections_without_refs_or_thanks() {
        let body = "### New\n- Native mods: export a headless engine (#6462)\n\n### Fixed\n- Play with friends: any port (#6363, fixes #6342)\n- Keep (this) aside\n\n### Contributors\nThanks to @a.\n\nYour launcher updates itself. New here?\n\n---\nNot an official Minecraft product.";
        assert_eq!(
            patch_sections(body),
            vec![
                (
                    "New".to_string(),
                    vec!["Native mods: export a headless engine".to_string()]
                ),
                (
                    "Fixed".to_string(),
                    vec![
                        "Play with friends: any port".to_string(),
                        "Keep (this) aside".to_string()
                    ]
                ),
            ]
        );
    }
}
