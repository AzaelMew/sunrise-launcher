//! Installed's Add from GitHub sheet. Resolving only reads packages; the selected set goes to
//! the same install queue as Discover, with the profile captured when the sheet opened.
use super::*;
use crate::frame::{button, input, primary, Page, Reach as _};
use crate::mods_page::queue::Target;
use crate::paint::palette;
use crate::sheets::{self, Sheet};
use gpui::rgb;
use std::collections::BTreeSet;
use std::sync::mpsc::Receiver;
use sunrise_launcher::github_mods::{self, FetchError, Prepared, ResolveError};

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
            line: crate::mods::install_state::plain_failure(&detail),
            detail,
            retry: true,
        }
    }
}

struct Pick {
    prepared: Prepared,
    selected: BTreeSet<String>,
}

impl Pick {
    fn new(prepared: Prepared) -> Self {
        let selected = prepared.mods().iter().map(|m| m.id.clone()).collect();
        Self { prepared, selected }
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
    Resolving(Receiver<Result<Prepared, FormError>>),
    Pick(Pick),
    Failed(FormError, bool),
}

pub(super) struct Github {
    text: line_edit::LineEdit,
    stage: Stage,
    focus: Option<FocusHandle>,
    target: Option<Target>,
}

impl Github {
    pub(super) fn new() -> Self {
        Self {
            text: line_edit::LineEdit::default(),
            stage: Stage::Link,
            focus: None,
            target: None,
        }
    }

    pub(super) fn busy(&self) -> bool {
        matches!(self.stage, Stage::Resolving(_))
    }

    pub(super) fn close(&mut self) {
        // Dropping the receiver prevents a late answer from affecting a later sheet. Prepared
        // packages belong to the worker's answer and clean themselves up when it is dropped.
        self.stage = Stage::Link;
        self.target = None;
        self.text.set(String::new());
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

fn resolve_link(link: &github_mods::Link) -> Result<Prepared, FormError> {
    let found = github_mods::resolve(link, &github_mods::api).map_err(|error| match error {
        error @ ResolveError::NoRelease(_) => {
            FormError::new(tr("This release wasn’t found."), error.into())
        }
        ResolveError::Failed(why) => FormError::transport(why),
    })?;
    github_mods::fetch(found).map_err(|error| match error {
        FetchError::NoMods { release, detail } => FormError::new(
            if release {
                tr("No Sunrise mods were found in this release.")
            } else {
                tr("No Sunrise mods were found at this link.")
            },
            detail,
        ),
        FetchError::Failed(why) => FormError::transport(why),
    })
}

impl Launcher {
    pub(super) fn github_add_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        button("github-add", tr("Add from GitHub")).on_click(cx.listener(
            |this, _: &ClickEvent, window, cx| {
                this.github.target = Some(this.queue_target());
                this.open_sheet(Sheet::GithubAdd);
                let focus = this
                    .github
                    .focus
                    .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true));
                window.focus(focus, cx);
                cx.notify();
            },
        ))
    }

    pub(super) fn github_install_error(
        &mut self,
        link: String,
        target: Target,
        error: &sunrise_launcher::browser::installed::InstallError,
    ) {
        self.github_install_failure(
            link,
            target,
            crate::mods::install_state::failure_line(error),
            error.to_string(),
        );
    }

    pub(super) fn github_install_failure(
        &mut self,
        link: String,
        target: Target,
        line: String,
        detail: String,
    ) {
        // Don't steal the keyboard if the player has switched profiles, pages, or sheets.
        if self.ui.page != Page::Installed
            || self.ui.sheet.is_some()
            || self.blocking_dialog()
            || self.mods.world_ask.is_some()
            || !self.mods.removal.is_empty()
            || !self
                .selected_instance()
                .is_some_and(|i| i.id == target.instance)
        {
            return;
        }
        let error = FormError {
            line,
            detail,
            retry: true,
        };
        self.github.text.set(link);
        self.github.target = Some(target);
        self.github.stage = Stage::Failed(error, false);
        self.open_sheet(Sheet::GithubAdd);
        self.mods.message = None;
    }

    fn github_resolve(&mut self) {
        if !self.github.can_resolve() {
            return;
        }
        let link = match github_mods::parse(self.github.text.text()) {
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
            let _ = tx.send(resolve_link(&link));
        });
    }

    pub(super) fn github_tick(&mut self) -> bool {
        if self.ui.sheet != Some(Sheet::GithubAdd) {
            if self.github.busy() {
                self.github.close();
            }
            return false;
        }
        let Stage::Resolving(rx) = &self.github.stage else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(prepared)) if prepared.mods().len() == 1 => self.github_submit(prepared),
            Ok(Ok(prepared)) => self.github.stage = Stage::Pick(Pick::new(prepared)),
            Ok(Err(error)) => self.github.stage = Stage::Failed(error, false),
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.github.stage = Stage::Failed(
                    FormError::transport(tr("GitHub lookup stopped.").into()),
                    false,
                );
            }
        }
        true
    }

    fn github_pick(&mut self) {
        if !matches!(&self.github.stage, Stage::Pick(pick) if !pick.selected.is_empty()) {
            return;
        }
        if let Stage::Pick(mut pick) = std::mem::take(&mut self.github.stage) {
            pick.prepared.retain(&pick.selected);
            self.github_submit(pick.prepared);
        }
    }

    fn github_submit(&mut self, prepared: Prepared) {
        let Some(target) = self.github.target.take() else {
            return;
        };
        let link = self.github.text.text().to_owned();
        self.ui.sheet = None;
        self.github.close();
        self.mods_install_github(target, link, prepared);
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
                .children(pick.prepared.mods().iter().enumerate().map(|(i, m)| {
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
                    .child(muted(&pick.prepared.source().label()).truncate())
                    .child(rows),
                button,
            )
        } else {
            let resolving = self.github.busy();
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
                            | line_edit::Key::Cut(_) => this.github.stage = Stage::Link,
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
                        .child(field),
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
            let next = primary("github-add-continue", tr("Continue"));
            let next = if self.github.can_resolve() {
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
