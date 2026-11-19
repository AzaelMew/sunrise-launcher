//! Settings › Appearance: the theme (the built-in ones, then the player's own files), the
//! accent under it, what is behind Home, and the themes folder.
//!
//! Every choice here is one of `sunrise_launcher::appearance`'s operations, the same the
//! command line calls. A choice is painted only once it is saved: one that can't be saved is
//! taken back and said, and what a choice replaced (an older kept picture) goes only after
//! the save. The themes folder is read when the launcher starts and on Reload, never while a
//! frame is drawn; a theme's picture is decoded off the window's thread.
use super::*;
use crate::paint::Palette;
use gpui::RenderImage;
use std::path::PathBuf;
use std::sync::Arc;
use sunrise_launcher::appearance::{self, Behind};
use sunrise_launcher::theme_files::{self, Entry};
use sunrise_launcher::themes::{self, Accent};

/// A theme as Appearance lists it.
struct Choice {
    /// What `select_theme` takes.
    id: String,
    name: SharedString,
    about: SharedString,
    /// Its colours with the accent it would start with, for the preview.
    palette: Palette,
}

/// What stands behind Home, as the frame's backdrop draws it: the player's own picture or
/// their theme's, decoded to one still picture off the window's thread. The window is never
/// handed a file to decode itself.
pub(crate) type HomePhoto = Arc<RenderImage>;

/// The settings what is behind Home depends on: the theme, the player's picture, scenery.
type BehindKey = (Option<String>, Option<String>, bool);

pub(crate) struct State {
    /// The themes to choose from, made again when the folder is read or a choice changes.
    choices: Option<Vec<Choice>>,
    accent: line_edit::LineEdit,
    accent_focus: Option<FocusHandle>,
    /// The keyboard's place on "Worlds on Home", and the search's request to be taken to it:
    /// whether that was done yet.
    worlds_focus: Option<FocusHandle>,
    worlds_asked: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    /// What the last action could not do.
    error: Option<String>,
    /// What Reload found.
    note: Option<String>,
    /// What is behind Home for the settings it was worked out from (theme, picture, scenery).
    behind: Option<(BehindKey, Behind)>,
    /// The picture behind Home: its file, and the picture once it is decoded (None: it could
    /// not be).
    photo: Option<(PathBuf, Option<Arc<RenderImage>>)>,
    decoding: bool,
    /// The kept copy of the player's own picture that could not be decoded: Home shows what
    /// comes after it, and Appearance says so.
    unshown: Option<PathBuf>,
    /// Pictures no longer shown, for the window to let go of.
    retired: Vec<Arc<RenderImage>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            choices: None,
            accent: line_edit::LineEdit::new(String::new(), 32, 18),
            accent_focus: None,
            worlds_focus: None,
            worlds_asked: None,
            error: None,
            note: None,
            behind: None,
            photo: None,
            decoding: false,
            unshown: None,
            retired: Vec::new(),
        }
    }
}

impl State {
    /// The search opened "Worlds on Home": go to its control when the section is next drawn.
    pub(crate) fn ask_for_worlds(&mut self) {
        self.worlds_asked = Some(Default::default());
    }
}

/// The look's part of the settings, to put back when a change can't be saved.
struct Look {
    theme: Option<String>,
    accent: Accent,
    accents: std::collections::BTreeMap<String, Accent>,
    background: Option<String>,
    scenery: Option<bool>,
}

/// A theme's preview: its page, a panel on it with both inks, and Play in its own fill.
/// The least width of a theme's card, and the room between cards.
const CARD_MIN: f32 = 168.;
const GAP: f32 = 12.;

thread_local! {
    /// Where the grid of themes was last drawn, for its number of columns.
    static THEME_GRID: gpui::ScrollHandle = gpui::ScrollHandle::new();
}

fn preview(p: &Palette) -> Div {
    let bar = |w: f32, color: u32| {
        div()
            .h(px(4.))
            .w(px(w))
            .rounded(px(tokens::R_BAR))
            .bg(rgb(color))
    };
    div()
        .h(px(92.))
        .p(px(10.))
        .bg(rgb(p.background))
        .flex()
        .gap(px(6.))
        .child(
            div()
                .w(px(14.))
                .h_full()
                .rounded(px(tokens::R_SMALL))
                .bg(rgb(p.chrome))
                .border_1()
                .border_color(rgb(p.chrome_line))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(4.))
                .pt(px(6.))
                .child(bar(6., p.chrome_text))
                .child(bar(6., p.chrome_muted)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h_full()
                .rounded(px(tokens::R_SMALL))
                .bg(rgb(p.card_color))
                .border_1()
                .border_color(rgb(p.divider))
                .p(px(8.))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_size(px(tokens::LABEL))
                                .font_weight(FontWeight::BOLD)
                                .text_color(rgb(p.text))
                                .truncate()
                                .child(tr("Selected profile")),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::CAPTION))
                                .text_color(rgb(p.muted))
                                .truncate()
                                .child(tr("Not played yet")),
                        ),
                )
                .child(
                    div().flex().child(
                        div()
                            .px(px(10.))
                            .h(px(20.))
                            .flex()
                            .items_center()
                            .rounded(px(tokens::R_SMALL))
                            .border_1()
                            .border_color(p.input_edge())
                            .bg(p.play_gradient())
                            .text_size(px(tokens::CAPTION))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(p.accent_on))
                            .child(tr("Play")),
                    ),
                ),
        )
}

impl Launcher {
    fn look(&self) -> Look {
        Look {
            theme: self.settings.theme.clone(),
            accent: self.settings.accent,
            accents: self.settings.theme_accents.clone(),
            background: self.settings.background.clone(),
            scenery: self.settings.scenery_on,
        }
    }

    /// Makes one change to the look and saves it. A change that is refused or can't be saved
    /// leaves the settings as they were and is said under the row.
    fn appearance_change(
        &mut self,
        change: impl FnOnce(&mut backend::Settings) -> Result<(), String>,
        window: &Window,
    ) {
        let before = self.look();
        let done = change(&mut self.settings).and_then(|()| self.settings.save());
        let state = &mut self.ui.settings.appearance;
        state.note = None;
        state.error = done.err();
        if state.error.is_some() {
            let Look {
                theme,
                accent,
                accents,
                background,
                scenery,
            } = before;
            self.settings.theme = theme;
            self.settings.accent = accent;
            self.settings.theme_accents = accents;
            self.settings.background = background;
            self.settings.scenery_on = scenery;
            return;
        }
        self.appearance_changed(window);
    }

    /// The look changed, by a choice or by Reload: what was worked out from the old one is
    /// worked out again.
    fn appearance_changed(&mut self, window: &Window) {
        self.theme = self.settings_theme(window);
        let state = &mut self.ui.settings.appearance;
        state.choices = None;
        state.behind = None;
        state.accent.set(appearance::accent(&self.settings).name());
    }

    /// Reads the themes folder again: a repaired theme is back, an edited one is as edited.
    fn appearance_reload(&mut self, window: &Window) {
        let entries = theme_files::reload();
        let state = &mut self.ui.settings.appearance;
        // The same file may hold another picture now.
        if let Some((_, Some(old))) = state.photo.take() {
            state.retired.push(old);
        }
        let bad = entries
            .iter()
            .filter(|e| matches!(e, Entry::Bad { .. }))
            .count();
        state.error = None;
        state.note = Some(match (entries.len() - bad, bad) {
            (0, 0) => tr("No themes in the folder yet.").to_string(),
            (good, 0) => trf(
                "Read {count} of your themes.",
                &[("count", &good.to_string())],
            ),
            (good, bad) => trf(
                "Read {count} of your themes; {bad} can't be used.",
                &[("count", &good.to_string()), ("bad", &bad.to_string())],
            ),
        });
        self.appearance_changed(window);
    }

    /// What stands behind Home, for the frame's backdrop: the player's picture, else their
    /// theme's once it is decoded. None is the theme's scene (or nothing, with scenery off).
    /// Worked out when the settings it depends on change, not every frame.
    pub(crate) fn home_photo(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<HomePhoto> {
        let key = (
            self.settings.theme.clone(),
            self.settings.background.clone(),
            self.settings.scenery(),
        );
        let state = &mut self.ui.settings.appearance;
        for old in state.retired.drain(..) {
            let _ = window.drop_image(old);
        }
        if state.behind.as_ref().is_none_or(|(old, _)| *old != key) {
            let behind = match appearance::behind_home(&self.settings, &backend::data_dir()) {
                Behind::Personal(path) if state.unshown.as_ref() == Some(&path) => {
                    appearance::behind_without_own(&self.settings)
                }
                behind => behind,
            };
            state.behind = Some((key, behind));
        }
        let (path, own) = match state.behind.as_ref().map(|(_, behind)| behind.clone())? {
            Behind::Plain | Behind::Scene => return None,
            Behind::Personal(path) => (path, true),
            Behind::Theme(path) => (path, false),
        };
        if let Some((_, picture)) = state.photo.as_ref().filter(|(of, _)| *of == path) {
            return picture.clone();
        }
        if state.decoding {
            return None;
        }
        state.decoding = true;
        let read = path.clone();
        let task = cx.background_executor().spawn(async move {
            if own {
                let name = read
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                themes::background_picture(&backend::data_dir(), name)
            } else {
                theme_files::picture(&read).map_err(|reason| reason.text())
            }
            .map(|still| theme_files::fitted(still, sunrise_launcher::pictures::gpu_limit()))
        });
        cx.spawn(async move |this, cx| {
            let decoded = task.await;
            let _ = this.update(cx, |this, cx| {
                let state = &mut this.ui.settings.appearance;
                state.decoding = false;
                let picture = match decoded {
                    Ok(mut rgba) => {
                        // The window draws BGRA, its alpha premultiplied.
                        for px in rgba.pixels_mut() {
                            let a = u16::from(px.0[3]);
                            let [r, g, b] = [0, 1, 2].map(|i| (u16::from(px.0[i]) * a / 255) as u8);
                            px.0 = [b, g, r, a as u8];
                        }
                        Some(Arc::new(RenderImage::new([image::Frame::new(rgba)])))
                    }
                    Err(said) => {
                        state.error = Some(said);
                        if own {
                            // A kept copy that can't be decoded is never claimed as shown:
                            // Home goes on to the theme's picture, or the scene.
                            state.unshown = Some(path.clone());
                            if let Some((_, behind)) = &mut state.behind {
                                *behind = appearance::behind_without_own(&this.settings);
                            }
                        }
                        None
                    }
                };
                if let Some((_, Some(old))) = state.photo.replace((path, picture)) {
                    state.retired.push(old);
                }
                cx.notify();
            });
        })
        .detach();
        None
    }

    /// The themes to choose from: Dawn first, Sunrise's own violet, the other built-in ones,
    /// then the player's.
    fn appearance_choices(&mut self) {
        if self.ui.settings.appearance.choices.is_some() {
            return;
        }
        let tint = self.settings.glass_tint();
        let in_use = appearance::effective_id(&self.settings);
        let choice = |id: String, name: SharedString, about: SharedString, night| {
            // Each preview wears its theme's own accent; the one in use, the accent in use.
            let accent = if id == in_use {
                appearance::accent(&self.settings).rgb()
            } else {
                themes::own_accent(Some(&id)).rgb()
            };
            Choice {
                palette: Palette::plain(night, accent, tint),
                id,
                name,
                about,
            }
        };
        let builtin = |t: &themes::Theme| {
            choice(
                t.id.into(),
                tr(t.name).into(),
                tr(t.description).into(),
                *t.palette.unwrap_or(&theme::night::NIGHT),
            )
        };
        let mut all = vec![
            builtin(&themes::DAWN_THEME),
            choice(
                themes::TWILIGHT.into(),
                tr(themes::TWILIGHT_NAME).into(),
                tr(themes::TWILIGHT_DESCRIPTION).into(),
                theme::night::NIGHT,
            ),
        ];
        all.extend(
            themes::THEMES
                .iter()
                .filter(|t| t.id != themes::DAWN_THEME.id)
                .map(builtin),
        );
        for entry in theme_files::loaded().iter() {
            if let Entry::Good(local) = entry {
                all.push(choice(
                    local.id(),
                    local.name.clone().into(),
                    trf("Your theme · {folder}", &[("folder", &local.slug)]).into(),
                    local.roles.night(),
                ));
            }
        }
        self.ui.settings.appearance.choices = Some(all);
    }

    pub(super) fn settings_appearance(&mut self, window: &Window, cx: &mut Context<Self>) -> Div {
        self.appearance_choices();
        let missing = appearance::missing(&self.settings);
        let selected = match &self.settings.theme {
            // A missing theme is still the choice: nothing else is ticked in its place.
            Some(id) if missing => id.clone(),
            _ => appearance::effective_id(&self.settings),
        };
        // As many columns as fit at a card's least width, and the cards share the row between
        // them: no strip of empty space on the right at any width. The row's width is the one it
        // was last drawn at.
        let grid = THEME_GRID.with(|g| g.clone());
        let width = f32::from(grid.bounds().size.width);
        let columns = (((width + GAP) / (CARD_MIN + GAP)).floor() as u16).max(1);
        let mut cards = div()
            .id("alt-theme-grid")
            .track_scroll(&grid)
            .grid()
            .grid_cols(columns)
            .gap(px(GAP));
        let choices = self
            .ui
            .settings
            .appearance
            .choices
            .as_deref()
            .unwrap_or_default();
        for (n, choice) in choices.iter().enumerate() {
            let on = choice.id == selected;
            let id = choice.id.clone();
            cards = cards.child(
                div()
                    .id(ElementId::named_usize("alt-theme", n))
                    .tab_index(0)
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .rounded(px(tokens::R_CARD))
                    .overflow_hidden()
                    .border_1()
                    .border_color(rgb(if on {
                        palette().text
                    } else {
                        palette().divider
                    }))
                    .bg(rgb(palette().card_color))
                    .cursor_pointer()
                    .focus_visible(|s| s.shadow(crate::frame::focus_ring(palette().focus)))
                    .seen()
                    .child(preview(&choice.palette))
                    .child(
                        div()
                            .px(px(10.))
                            .py(px(8.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(tokens::LABEL))
                                            .truncate()
                                            .child(choice.name.clone()),
                                    )
                                    // Chosen is said in words and by the tick, not by colour
                                    // alone.
                                    .child(if on {
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(4.))
                                            .text_size(px(tokens::SMALL))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(blocks::icon(
                                                "check",
                                                13.,
                                                rgb(palette().text).into(),
                                            ))
                                            .child(tr("Selected"))
                                    } else {
                                        hint(choice.about.clone()).truncate()
                                    }),
                            ),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                        this.appearance_change(|s| appearance::select_theme(s, &id), window);
                        cx.notify();
                    })),
            );
        }
        // A folder that isn't a theme is listed with why, and can't be chosen.
        let bad: Vec<Div> = theme_files::loaded()
            .iter()
            .filter_map(|entry| match entry {
                Entry::Bad { folder, reason } => Some(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(10.))
                        .opacity(0.9)
                        .child(
                            blocks::icon("info-box", 15., rgb(palette().warning).into()).mt(px(2.)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_size(px(tokens::LABEL))
                                        .child(trf(
                                            "{folder} can't be used",
                                            &[("folder", folder)],
                                        )),
                                )
                                .child(hint(reason.text())),
                        ),
                ),
                Entry::Good(_) => None,
            })
            .collect();
        let state = &self.ui.settings.appearance;
        let said = state
            .error
            .clone()
            .map(warning)
            .or_else(|| state.note.clone().map(hint));
        let gone = missing.then(|| {
            warning(trf(
                "The chosen theme, {id}, can't be read. Dawn is shown until it is repaired: fix its file, then press Reload.",
                &[("id", self.settings.theme.as_deref().unwrap_or_default())],
            ))
        });
        let theme = stacked(
            tr("Theme"),
            tr("Colours and scenery for the whole launcher. Play uses white text in every theme."),
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .children(gone)
                .child(cards),
        );
        let mut rows = vec![theme];
        if !bad.is_empty() {
            rows.push(div().flex().flex_col().gap(px(10.)).children(bad));
        }
        rows.push(self.accent_row(missing, window, cx));
        rows.push(self.background_row(cx));
        let layout = self.settings.world_layout;
        let layouts = [
            (backend::WorldLayout::Rows, "Rows"),
            (backend::WorldLayout::Tiles, "Tiles"),
        ]
        .into_iter()
        .map(|(way, label)| {
            let pick: crate::mods::Choose = std::rc::Rc::new(move |this, _| {
                this.settings.world_layout = way;
                this.save_settings();
            });
            (tr(label).into(), way == layout, pick)
        })
        .collect();
        rows.push(row(
            tr("Worlds on Home"),
            tr(match layout {
                backend::WorldLayout::Rows => "Continue playing lists one world a row.",
                backend::WorldLayout::Tiles => {
                    "Continue playing shows the worlds' pictures side by side, as many as fit across; the rest wrap under them."
                }
            }),
            {
                let state = &mut self.ui.settings.appearance;
                let focus = state
                    .worlds_focus
                    .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
                    .clone();
                // Opened from the search: the page scrolls here and the keyboard is on it.
                let asked = state.worlds_asked.clone().map(|done| {
                    crate::mods::reveal(
                        crate::frame::scrolls("settings"),
                        done,
                        Some(focus.clone()),
                    )
                });
                div().child(
                    crate::mods::choices("alt-world-layout", layouts, cx)
                        .track_focus(&focus)
                        .gap(px(6.))
                        .children(asked),
                )
            },
        ));
        rows.push(
            self.wide_row(
                tr("Your themes"),
                tr("A theme is a folder with a theme.toml: up to six colours and one picture. Reload reads the folder again."),
                div()
                    .child(
                        button("alt-themes-folder", tr("Open themes folder")).on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                let dir = theme_files::dir();
                                // A link there is not the launcher's folder: never opened
                                // or made through.
                                if theme_files::root_linked(&dir) {
                                    this.ui.settings.appearance.error = Some(format!(
                                        "{}: {}",
                                        dir.display(),
                                        theme_files::Reason::Linked("folder").text()
                                    ));
                                    cx.notify();
                                    return;
                                }
                                match std::fs::create_dir_all(&dir) {
                                    Ok(()) => crate::glass_toast::open(cx, &dir),
                                    Err(e) => {
                                        this.ui.settings.appearance.error =
                                            Some(format!("{}: {e}", dir.display()))
                                    }
                                }
                                cx.notify();
                            }),
                        ),
                    )
                    .child(button("alt-themes-reload", tr("Reload")).on_click(cx.listener(
                        |this, _: &ClickEvent, window, cx| {
                            this.appearance_reload(window);
                            cx.notify();
                        },
                    ))),
            ),
        );
        let cat = self.settings.show_cat();
        rows.push(row(
            tr("Show the cat"),
            tr("It visits Home now and then. It stays away while motion is reduced."),
            div().child(switch("alt-cat-toggle", cat).seen().on_click(cx.listener(
                move |this, _: &ClickEvent, _, cx| {
                    this.settings.cat = Some(!cat);
                    this.save_settings();
                    cx.notify();
                },
            ))),
        ));
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(group(rows))
            .children(said)
    }

    /// The accent: its colour, one field that takes a name, a hue or `#rrggbb`, and the way
    /// back to the theme's own.
    fn accent_row(&mut self, missing: bool, window: &Window, cx: &mut Context<Self>) -> Div {
        let state = &mut self.ui.settings.appearance;
        let focus = state
            .accent_focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        let focused = focus.is_focused(window);
        let shown = appearance::accent(&self.settings);
        if !focused {
            state.accent.set(shown.name());
        }
        let own = themes::own_accent(self.settings.theme.as_deref()) == shown;
        let edit: fn(&mut Self) -> &mut line_edit::LineEdit =
            |this| &mut this.ui.settings.appearance.accent;
        let field_theme = crate::mods::field_theme(self.theme);
        let to_focus = focus.clone();
        let field = crate::frame::input(focused)
            .id("alt-accent")
            .track_focus(&focus)
            .seen()
            .w(px(150.))
            .h(px(tokens::FIELD))
            .px(px(12.))
            .font_family(theme::MONO)
            .text_size(px(tokens::LABEL))
            .text_color(rgb(palette().text))
            .cursor_text()
            .child(line_edit::content(
                &self.ui.settings.appearance.accent,
                &field_theme,
                focused,
                false,
                false,
                "teal, 176 or #2563eb",
            ))
            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                window.focus(&to_focus, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, window, cx| {
                match line_edit::handle(edit(this), ev, cx) {
                    line_edit::Key::Pass => return,
                    // Enter takes what is typed; Escape leaves the accent as it was.
                    line_edit::Key::Enter => {
                        let typed = edit(this).text().to_string();
                        this.appearance_change(|s| appearance::type_accent(s, &typed), window);
                    }
                    line_edit::Key::Escape => {
                        let shown = appearance::accent(&this.settings).name();
                        edit(this).set(shown);
                        window.blur(cx);
                    }
                    _ => {}
                }
                cx.stop_propagation();
                cx.notify();
            }));
        let reset = button("alt-accent-reset", tr("Use theme accent"));
        let reset = if own || missing {
            off(reset)
        } else {
            reset.on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.appearance_change(appearance::reset_accent, window);
                cx.notify();
            }))
        };
        let field = line_edit::mouse(field, cx, edit);
        self.wide_row(
            tr("Accent"),
            tr("A colour name, a hue in degrees or #rrggbb, then Enter. Play darkens a pale accent just enough for its white label."),
            div()
                .child(
                    div()
                        .flex_none()
                        .size(px(tokens::FIELD))
                        .rounded(px(tokens::R_CONTROL))
                        .border_1()
                        .border_color(rgb(palette().line))
                        .bg(rgb(palette().accent)),
                )
                .child(if missing {
                    div().opacity(0.4).child(field)
                } else {
                    div().child(field)
                })
                .child(reset),
        )
    }

    /// What is behind Home: where it comes from, a picture of the player's own, the way back
    /// to the theme's, and the switch for all of it.
    fn background_row(&mut self, cx: &mut Context<Self>) -> Div {
        let data = backend::data_dir();
        // A kept copy Home could not decode is not what is behind Home.
        let unshown = self
            .ui
            .settings
            .appearance
            .unshown
            .as_ref()
            .is_some_and(|path| {
                appearance::behind_home(&self.settings, &data) == Behind::Personal(path.clone())
            });
        let behind = if unshown {
            appearance::behind_without_own(&self.settings)
        } else {
            appearance::behind_home(&self.settings, &data)
        };
        let lost = appearance::background_lost(&self.settings, &data);
        let source = match &behind {
            Behind::Plain => tr("Off: the theme's plain background."),
            Behind::Personal(_) => tr("Your picture, on Home."),
            Behind::Theme(_) => tr("The theme's picture, on Home."),
            Behind::Scene => tr("The theme's scene."),
        };
        let about = if unshown {
            format!(
                "{} {}",
                source,
                tr("The picture you chose can't be shown; choose it again or use the theme's.")
            )
        } else if lost {
            format!(
                "{} {}",
                source,
                tr("The picture you chose is no longer there; choose it again or use the theme's.")
            )
        } else {
            source.to_string()
        };
        let choose = button("alt-background-choose", tr("Choose picture…")).on_click(cx.listener(
            |_, _: &ClickEvent, window, cx| {
                let paths = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some(tr("Choose").into()),
                });
                let window = window.window_handle();
                cx.spawn(async move |this, cx| {
                    // Closing the dialog keeps the picture there was.
                    let Ok(Ok(Some(mut paths))) = paths.await else {
                        return;
                    };
                    let Some(file) = paths.pop() else {
                        return;
                    };
                    // Read, checked and decoded off the window's thread, then kept.
                    let kept = cx
                        .background_executor()
                        .spawn(async move { themes::keep_background(&backend::data_dir(), &file) })
                        .await;
                    let _ = window.update(cx, |_, window, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.appearance_change(
                                |s| {
                                    s.background = Some(kept?);
                                    Ok(())
                                },
                                window,
                            );
                            cx.notify();
                        });
                    });
                })
                .detach();
            },
        ));
        let theirs = button("alt-background-theme", tr("Use theme background"));
        let theirs = if self.settings.background.is_some() {
            theirs.on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                this.appearance_change(
                    |s| {
                        s.background = None;
                        Ok(())
                    },
                    window,
                );
                cx.notify();
            }))
        } else {
            off(theirs)
        };
        let on = self.settings.scenery();
        let show = switch("alt-scenery", on)
            .seen()
            .tooltip(crate::glass_ui::tip(tr("Show background")))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.appearance_change(
                    |s| {
                        s.scenery_on = Some(!on);
                        Ok(())
                    },
                    window,
                );
                cx.notify();
            }));
        self.wide_row(
            tr("Home background"),
            about,
            div()
                .child(choose)
                .child(theirs)
                .child(hint(tr("Show background")))
                .child(show),
        )
    }
}
