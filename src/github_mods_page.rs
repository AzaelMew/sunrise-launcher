//! Installed's link form. Fetching only reads packages; the normal install worker owns mutation.
use super::*;
use crate::frame::{button, input, primary, Page, Reach as _};
use crate::mods_page::install::GithubTarget;
use crate::paint::palette;
use crate::sheets::{self, Sheet};
use gpui::rgb;
use std::collections::BTreeSet;
use sunrise_launcher::github_mods::{self, FetchError, Origin, Packed, ResolveError, Resolved};

pub(crate) struct GithubInput {
    pub(crate) link: String,
    pub(crate) source: Resolved,
    pub(crate) selection: GithubSelection,
}

pub(crate) enum GithubSelection {
    Add(Vec<Packed>),
    Update(Packed),
}

pub(crate) struct Feedback {
    pub(crate) profile: instances::Instance,
    pub(crate) link: String,
    pub(crate) result: Result<String, (String, String)>,
}

struct FormError {
    line: String,
    detail: String,
    retry: bool,
}

impl FormError {
    fn new(line: &str, detail: String) -> Self {
        Self {
            line: line.into(),
            detail,
            retry: false,
        }
    }

    fn transport(detail: String) -> Self {
        Self {
            line: tr("Could not reach GitHub. Try again.").into(),
            detail,
            retry: true,
        }
    }
}

struct Pick {
    source: Resolved,
    mods: Vec<Packed>,
    selected: BTreeSet<String>,
}

impl Pick {
    fn new(source: Resolved, mods: Vec<Packed>) -> Self {
        let selected = mods.iter().map(|m| m.id.clone()).collect();
        Self {
            source,
            mods,
            selected,
        }
    }

    fn toggle(&mut self, id: &str) {
        if !self.selected.remove(id) {
            self.selected.insert(id.to_owned());
        }
    }
}

#[derive(Default)]
enum Stage {
    #[default]
    Link,
    Resolving(Receiver<Result<Resolution, FormError>>),
    Pick(Pick),
    Failed(FormError, bool),
}

enum Resolution {
    Ready(Resolved, Vec<Packed>),
    Current,
}

#[derive(Default)]
pub(super) struct Github {
    text: line_edit::LineEdit,
    stage: Stage,
    focus: Option<FocusHandle>,
    target: Option<GithubTarget>,
    update: Option<Origin>,
}

impl Github {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn close(&mut self) {
        // Dropping a pending lookup's receiver releases its packages even if it finishes later.
        self.stage = Stage::Link;
        self.target = None;
        self.update = None;
        self.text.set(String::new());
    }

    fn resolving(&self) -> bool {
        matches!(self.stage, Stage::Resolving(_))
    }

    fn can_resolve(&self) -> bool {
        !self.text.text().trim().is_empty()
            && match &self.stage {
                Stage::Link => true,
                Stage::Failed(error, _) => error.retry,
                _ => false,
            }
    }
}

fn resolve_link(
    link: &github_mods::Link,
    update: Option<&Origin>,
) -> Result<Resolution, FormError> {
    let found = github_mods::resolve(link, &github_mods::api).map_err(|error| match error {
        error @ ResolveError::NoRelease(_) => {
            FormError::new(tr("This release wasn’t found."), error.into())
        }
        ResolveError::Lookup(why) => {
            FormError::new(tr("This GitHub link could not be found."), why)
        }
        ResolveError::Transfer(why) => FormError::transport(why),
    })?;
    if update.is_some_and(|old| !github_mods::is_update(old, &found)) {
        return Ok(Resolution::Current);
    }
    let mut mods = github_mods::fetch_checked(&found).map_err(|error| match error {
        FetchError::NoMods { release, detail } => FormError::new(
            if release {
                tr("No Sunrise mods were found in this release.")
            } else {
                tr("No Sunrise mods were found at this link.")
            },
            detail,
        ),
        FetchError::Package(why) => {
            FormError::new(tr("The GitHub mod package was not valid."), why)
        }
        FetchError::Transfer(why) => FormError::transport(why),
    })?;
    if let Some(old) = update {
        mods.retain(|package| package.id == old.id);
        if mods.is_empty() || found.assets.is_empty() {
            return Err(FormError::new(
                tr("The new release does not contain this mod."),
                trf(
                    "No release package was found for {mod}. The installed mod is unchanged.",
                    &[("mod", &old.id)],
                ),
            ));
        }
    }
    Ok(Resolution::Ready(found, mods))
}

impl Launcher {
    pub(super) fn github_add_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        button("github-add", tr("Add from GitHub")).on_click(cx.listener(
            |this, _: &ClickEvent, window, cx| {
                this.open_github_add(None);
                let focus = this
                    .github
                    .focus
                    .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true));
                window.focus(focus, cx);
                cx.notify();
            },
        ))
    }

    pub(super) fn open_github_add(&mut self, update: Option<Origin>) {
        self.github.close();
        self.open_sheet(Sheet::GithubAdd);
        match self.github_target() {
            Ok(target) => self.github.target = Some(target),
            Err(error) => {
                self.github.stage =
                    Stage::Failed(FormError::new(tr("Can't start the install"), error), false);
                return;
            }
        }
        if let Some(update) = update {
            self.github.text.set(update.source_url());
            self.github.update = Some(update);
            self.github_resolve();
        }
    }

    pub(crate) fn show_github_result(&mut self, feedback: Feedback) {
        let Feedback {
            profile,
            link,
            result,
        } = feedback;
        if !self.selected_instance().is_some_and(|current| {
            current.home == profile.home
                && current.id == profile.id
                && current.incarnation == profile.incarnation
        }) {
            return;
        }
        match result {
            Ok(message) => self.mods.message = Some(Ok(message)),
            Err((line, detail)) => {
                if self.ui.page == Page::Installed
                    && self.ui.sheet.is_none()
                    && !self.blocking_dialog()
                {
                    self.open_github_add(None);
                    self.github.text.set(link);
                    self.github.stage = Stage::Failed(FormError::new(&line, detail), false);
                    self.mods.message = None;
                } else {
                    self.mods.message = Some(Err(format!("{line}\n{detail}")));
                }
            }
        }
    }

    fn github_resolve(&mut self) {
        if !self.github.can_resolve() || self.github.target.is_none() {
            return;
        }
        let update = self.github.update.clone();
        let link = update
            .as_ref()
            .map(|old| Ok(old.follows()))
            .unwrap_or_else(|| github_mods::parse(self.github.text.text()));
        let link = match link {
            Ok(link) => link,
            Err(why) => {
                self.github.stage =
                    Stage::Failed(FormError::new(tr("Enter a GitHub link."), why), false);
                return;
            }
        };
        let (tx, rx) = crate::wake::channel();
        self.github.stage = Stage::Resolving(rx);
        std::thread::spawn(move || {
            let _ = tx.send(resolve_link(&link, update.as_ref()));
        });
    }

    pub(super) fn github_tick(&mut self) -> bool {
        if self.ui.sheet != Some(Sheet::GithubAdd) {
            if self.github.target.is_some() {
                self.github.close();
            }
            return false;
        }
        let Stage::Resolving(rx) = &self.github.stage else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(Resolution::Ready(source, mods))) if mods.len() == 1 => {
                self.github_submit(source, mods)
            }
            Ok(Ok(Resolution::Ready(source, mods))) => {
                self.github.stage = Stage::Pick(Pick::new(source, mods))
            }
            Ok(Ok(Resolution::Current)) => {
                self.close_sheet();
                self.mods.message = Some(Ok(tr("This GitHub mod is up to date.").into()));
            }
            Ok(Err(error)) => self.github.stage = Stage::Failed(error, false),
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.github.stage = Stage::Failed(
                    FormError::transport(tr("GitHub lookup stopped.").into()),
                    false,
                )
            }
        }
        true
    }

    fn github_pick(&mut self) {
        if !matches!(&self.github.stage, Stage::Pick(pick) if !pick.selected.is_empty()) {
            return;
        }
        if let Stage::Pick(mut pick) = std::mem::take(&mut self.github.stage) {
            pick.mods.retain(|m| pick.selected.contains(&m.id));
            self.github_submit(pick.source, pick.mods);
        }
    }

    fn github_submit(&mut self, source: Resolved, mut mods: Vec<Packed>) {
        let Some(target) = self.github.target.take() else {
            return;
        };
        let selection = if self.github.update.is_some() {
            GithubSelection::Update(mods.pop().expect("the update names one package"))
        } else {
            GithubSelection::Add(mods)
        };
        let input = GithubInput {
            link: self.github.text.text().to_owned(),
            source,
            selection,
        };
        self.close_sheet();
        self.queue_github(target, input);
    }

    pub(crate) fn github_add_sheet(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let focus = self
            .github
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        let muted = |text: &str| {
            div()
                .text_size(px(tokens::LABEL))
                .text_color(rgb(palette().muted))
                .child(text.to_owned())
        };
        let body = div().flex().flex_col();
        let (body, action) = if let Stage::Pick(pick) = &self.github.stage {
            let rows = div()
                .id("github-add-list")
                .max_h(px(184.))
                .overflow_y_scroll()
                .rounded(crate::tokens::r(tokens::R_CARD))
                .border_1()
                .border_color(rgb(palette().divider))
                .children(pick.mods.iter().enumerate().map(|(i, m)| {
                    let id = m.id.clone();
                    div()
                        .id(("github-add-choice", i))
                        .reach()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .px(px(12.))
                        .h(px(48.))
                        .cursor_pointer()
                        .when(i > 0, |r| {
                            r.border_t_1().border_color(rgb(palette().divider))
                        })
                        .hover(|s| s.bg(rgb(palette().hover)))
                        .child(
                            crate::mods::checkbox(
                                ("github-add-check", i),
                                pick.selected.contains(&m.id),
                            )
                            .tab_stop(false),
                        )
                        .child(crate::mod_icons::tile(
                            None::<gpui::ImageSource>,
                            &m.id,
                            &m.name,
                            26.,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(m.name.clone()),
                        )
                        .child(muted(&m.version).flex_none())
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            if let Stage::Pick(pick) = &mut this.github.stage {
                                pick.toggle(&id);
                            }
                            cx.notify();
                        }))
                }));
            let button = primary("github-add-install", tr("Install"));
            let button = if pick.selected.is_empty() {
                widgets::disabled(button)
            } else {
                button.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.github_pick();
                    cx.notify();
                }))
            };
            (
                body.child(sheets::title(tr("Choose mods to install").into()))
                    .child(muted(&pick.source.label()).truncate())
                    .child(muted(tr(github_mods::WARNING)))
                    .child(rows),
                button,
            )
        } else {
            let resolving = self.github.resolving();
            let focused = !resolving && focus.is_focused(window);
            let mut field_theme = crate::mods::field_theme(self.theme);
            field_theme.glass = true;
            let field = input(focused)
                .id("github-add-field")
                .h(px(tokens::ACTION))
                .px(px(12.))
                .text_size(px(tokens::BODY))
                .child(line_edit::content_with(
                    &self.github.text,
                    &field_theme,
                    focused,
                    false,
                    false,
                    "https://github.com/owner/repo".into(),
                ));
            let field = if resolving {
                field.opacity(0.55)
            } else {
                let field = field
                    .track_focus(&focus)
                    .seen()
                    .cursor_text()
                    .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                        window.focus(&focus, cx);
                        cx.notify();
                    }))
                    .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                        match line_edit::handle(&mut this.github.text, e, cx) {
                            line_edit::Key::Pass | line_edit::Key::Escape => return,
                            line_edit::Key::Enter => this.github_resolve(),
                            line_edit::Key::Edited
                            | line_edit::Key::Paste
                            | line_edit::Key::Cut(_) => {
                                this.github.stage = Stage::Link;
                                this.github.update = None;
                            }
                            _ => {}
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }));
                line_edit::mouse(field, cx, |this| &mut this.github.text)
            };
            let mut body = body
                .child(sheets::title(tr("Add from GitHub").into()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(muted(tr(
                            "Paste a link to a repository, release, tag, branch or commit.",
                        )))
                        .child(field)
                        .child(muted(tr(github_mods::WARNING))),
                );
            if resolving {
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(muted(tr("Checking GitHub…")))
                        .child(widgets::progress_bar(None, &self.theme).flex_none()),
                );
            }
            if let Stage::Failed(error, expanded) = &self.github.stage {
                body =
                    body.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(tokens::LABEL))
                                    .text_color(rgb(palette().danger))
                                    .child(error.line.clone()),
                            )
                            .child(button("github-add-details", tr("Details")).on_click(
                                cx.listener(|this, _: &ClickEvent, _, cx| {
                                    if let Stage::Failed(_, expanded) = &mut this.github.stage {
                                        *expanded ^= true;
                                    }
                                    cx.notify();
                                }),
                            )),
                    );
                if *expanded {
                    body = body.child(
                        div()
                            .id("github-add-error-detail")
                            .max_h(px(120.))
                            .overflow_y_scroll()
                            .p(px(10.))
                            .rounded(crate::tokens::r(tokens::R_SMALL))
                            .bg(rgb(palette().background))
                            .text_size(px(tokens::LABEL))
                            .text_color(rgb(palette().muted))
                            .child(crate::selectable::selectable(error.detail.clone())),
                    );
                }
            }
            let next = primary(
                "github-add-continue",
                if matches!(&self.github.stage, Stage::Failed(error, _) if error.retry) {
                    tr("Try again")
                } else {
                    tr("Continue")
                },
            );
            let next = if self.github.target.is_some() && self.github.can_resolve() {
                next.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.github_resolve();
                    cx.notify();
                }))
            } else {
                widgets::disabled(next)
            };
            (body, next)
        };
        let body = body.child(
            div()
                .flex()
                .justify_end()
                .gap(px(8.))
                .child(
                    button("github-add-cancel", tr("Cancel")).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.close_sheet();
                            cx.notify();
                        },
                    )),
                )
                .child(action),
        );
        sheets::frame("github-add-sheet", 500., body, cx)
    }
}
