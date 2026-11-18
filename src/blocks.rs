//! The blocky look's parts (`docs/design-system.md`, "Blocks"): pixel type, pixel icons, panels
//! with a dark outline, a light bevel on top and a darker lip below, sunk wells, badges, the
//! inventory tabs, and the slots that hold icons downloaded at run time.
use crate::theme::{self, Theme};
use gpui::{
    div, img, prelude::*, px, svg, AnyElement, Div, FontWeight, Hsla, ImageSource, ObjectFit,
    SharedString, Svg,
};
use std::borrow::Cow;

/// The icons, served to `svg()` as `icons/<name>.svg`.
pub struct Assets;

impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        if path == "dawn.webp" {
            return Ok(Some(Cow::Borrowed(include_bytes!(
                "../assets/media/dawn.webp"
            ))));
        }
        if let Some(name) = path
            .strip_prefix("icons/glass/")
            .and_then(|p| p.strip_suffix(".svg"))
        {
            return Ok(sunrise_launcher_theme::glass_icons::get(name).map(Cow::Borrowed));
        }
        let name = path
            .strip_prefix("icons/")
            .and_then(|p| p.strip_suffix(".svg"));
        Ok(name
            .and_then(sunrise_launcher_theme::icons::get)
            .map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> gpui::Result<Vec<SharedString>> {
        Ok(sunrise_launcher_theme::icons::ALL
            .iter()
            .map(|(name, _)| format!("icons/{name}.svg").into())
            .chain(
                sunrise_launcher_theme::glass_icons::ALL
                    .iter()
                    .map(|(name, _)| format!("icons/glass/{name}.svg").into()),
            )
            .collect())
    }
}

/// A pixel icon (`sunrise_launcher_theme::icons`) in `color`.
pub fn icon(name: &'static str, size: f32, color: Hsla) -> Svg {
    let glass = match name {
        "home" => "dashboard-square-01",
        "package" | "box" | "archive" => "package-01",
        "server" => "computer",
        "sliders" => "sliders-horizontal",
        "play" => "play",
        "chevron-down" => "arrow-down-01",
        "chevron-right" => "arrow-right-01",
        "search" => "search-01",
        "check" => "tick-02",
        "user" => "user",
        "map" => "map",
        "plus" => "plus-sign",
        "globe" => "earth",
        "message" => "chat",
        "folder" => "folder-01",
        "close" => "cancel-01",
        _ => {
            return svg()
                .path(format!("icons/{name}.svg"))
                .flex_none()
                .size(px(size))
                .text_color(color)
        }
    };
    svg()
        .path(format!("icons/glass/{glass}.svg"))
        .flex_none()
        .size(px(size))
        .text_color(color)
}

/// Text in the pixel face: headings, navigation, buttons, badges.
pub fn pixel(size: f32, weight: FontWeight) -> Div {
    div()
        .font_family(theme::PIXEL)
        .font_weight(weight)
        .text_size(px(size))
        .line_height(px((size * 1.2).round()))
}

/// The light edge along the top of a raised block and the darker lip along its bottom; the
/// block must be `relative`.
pub fn edges(top: Hsla, bottom: Hsla, lip: f32) -> [AnyElement; 2] {
    [
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(theme::BEVEL))
            .bg(top)
            .into_any_element(),
        div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(lip))
            .bg(bottom)
            .into_any_element(),
    ]
}

/// A square slot for an icon: the picture at `source` when there is one, else the first letter of
/// `name` on a raised block.
pub fn slot(source: Option<ImageSource>, name: &str, size: f32, t: &Theme) -> Div {
    let frame = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .overflow_hidden()
        .border_2()
        .border_color(t.outline)
        .bg(t.well);
    match source {
        Some(source) => frame.child(img(source).size(px(size - 4.)).object_fit(ObjectFit::Cover)),
        None => frame.bg(t.raised).child(
            pixel((size * 0.42).round(), FontWeight::BOLD)
                .text_color(t.muted)
                .child(
                    name.chars()
                        .find(|c| c.is_alphanumeric())
                        .unwrap_or('?')
                        .to_uppercase()
                        .to_string(),
                ),
        ),
    }
}
