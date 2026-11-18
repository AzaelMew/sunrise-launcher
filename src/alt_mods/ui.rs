//! Small pieces the three Mods screens share. Colours are the alternative look's (`crate::alt`).
use super::*;
use crate::alt::{BG, CARD, GOLD, LINE, MUTED, SOFT, TEXT};
use gpui::{rgb, Hsla};
use std::path::PathBuf;

pub(super) fn c(x: u32) -> Hsla {
    rgb(x).into()
}

pub(super) fn tag(label: &'static str) -> Div {
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(5.))
        .border_1()
        .border_color(c(LINE))
        .text_size(px(11.))
        .text_color(c(MUTED))
        .child(label)
}

pub(super) fn muted(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .text_color(c(MUTED))
        .child(text.into())
}

/// The main action on a screen, in cream: gold is Play's alone.
pub(super) fn primary(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    crate::alt::button(id, label)
        .bg(c(TEXT))
        .border_color(c(TEXT))
        .text_color(c(BG))
        .hover(|s| s.bg(c(0xffffff)))
}

/// A control that can't be used now: drawn dim, with no handler.
pub(super) fn dim(b: Stateful<Div>) -> Stateful<Div> {
    b.opacity(0.45).cursor_default()
}

pub(super) fn chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .px(px(14.))
        .h(px(32.))
        .flex()
        .items_center()
        .rounded(px(16.))
        .border_1()
        .border_color(if on { c(TEXT) } else { c(LINE) })
        .bg(if on { c(TEXT) } else { c(BG) })
        .text_color(if on { c(BG) } else { c(TEXT) })
        .text_size(px(13.))
        .cursor_pointer()
        .child(label.into())
}

pub(crate) fn switch(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .w(px(40.))
        .h(px(22.))
        .rounded(px(11.))
        .p(px(2.))
        .flex()
        .bg(if on { c(TEXT) } else { c(LINE) })
        .when(on, |d| d.justify_end())
        .cursor_pointer()
        .child(
            div()
                .size(px(18.))
                .rounded(px(9.))
                .bg(if on { c(BG) } else { c(MUTED) }),
        )
}

pub(super) fn checkbox(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(18.))
        .rounded(px(4.))
        .border_1()
        .border_color(if on { c(TEXT) } else { c(MUTED) })
        .bg(if on { c(TEXT) } else { c(BG) })
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .children(on.then(|| blocks::icon("check", 12., c(BG))))
}

/// A mod's picture, or its first letter until it arrives.
pub(super) fn tile(path: Option<PathBuf>, name: &str, side: f32) -> Div {
    let t = div()
        .flex_none()
        .size(px(side))
        .rounded(px(8.))
        .overflow_hidden()
        .bg(c(CARD))
        .border_1()
        .border_color(c(LINE))
        .flex()
        .items_center()
        .justify_center();
    match path {
        Some(p) => t.child(gpui::img(p).size(px(side))),
        None => t
            .text_size(px(side * 0.45))
            .text_color(c(MUTED))
            .child(name.chars().next().map(String::from).unwrap_or_default()),
    }
}

pub(super) fn card() -> Div {
    div()
        .rounded(px(12.))
        .border_1()
        .border_color(c(SOFT))
        .bg(c(CARD))
}

/// The theme the shared text field draws with, in this look's colours.
pub(super) fn field_theme(mut t: crate::theme::Theme) -> crate::theme::Theme {
    t.ink = c(TEXT);
    t.muted = c(MUTED);
    t.glass = false;
    t
}

/// "Example data" strip drawn over every screen that shows the opt-in example lists.
pub(super) fn example_banner() -> Div {
    div()
        .px(px(12.))
        .py(px(8.))
        .rounded(px(8.))
        .border_1()
        .border_color(c(GOLD))
        .text_size(px(12.))
        .text_color(c(GOLD))
        .child(tr(
            "Example data for design review. Nothing here is read from the registry or your files, and nothing can be installed, changed or removed.",
        ))
}
