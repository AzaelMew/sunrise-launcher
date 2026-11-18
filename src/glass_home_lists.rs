//! Home's Worlds and Multiplayer tabs in the glass look (the mockup's `Worlds` and
//! `Multiplayer`, `launcher-views.tsx`): one `ROW` per world or server. A world's row plays it,
//! its size turning into "Play →" under the pointer; its ⋯ menu holds what the launcher does
//! besides (back up, copy, data packs, folder, delete). A server's row has Join, and Remove
//! (clicked twice) under the pointer. Dark appearance only.
use super::*;
use crate::glass_ui::{self as ui, white};
use gpui::{deferred};


use sunrise_launcher_theme::glass::{fill, ink, radius};


/// A listener of the launcher's, boxed so a helper can return it.
pub(super) type Listener<E> = Box<dyn Fn(&E, &mut Window, &mut App)>;

/// The 36 px picture at a row's start (`PixelIcon` at 36), round at `round(36 · 0.26)`.
const PICTURE: f32 = 36.;
const PICTURE_RADIUS: f32 = 9.;


/// A row's 36 px picture, or a plain tile with `fallback`'s icon.
pub(super) fn picture(image: Option<gpui::ImageSource>, fallback: &'static str) -> AnyElement {
    match image {
        Some(image) => gpui::img(image)
            .relative()
            .flex_none()
            .size(px(PICTURE))
            .rounded(px(crate::glass_ui::rad(PICTURE_RADIUS)))
            .into_any_element(),
        None => div()
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(PICTURE))
            .rounded(px(crate::glass_ui::rad(PICTURE_RADIUS)))
            .bg(white(fill::BUTTON))
            .child(ui::icon(fallback, 17., white(ink::QUIET)))
            .into_any_element(),
    }
}

