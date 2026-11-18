//! The launcher's frame in the night look (docs/design-system.md, "Night"): the sidebar's plate
//! and rows, and the top bar's tabs, icon buttons and account chip. The sky shows between them;
//! only the sidebar and the selected or hovered parts are plates.
use crate::theme::{self, night, Step};
use gpui::{div, prelude::*, px, rgb, Div, FontWeight, Hsla};

/// A night colour as GPUI's.
#[cfg_attr(not(windows), allow(dead_code))] // the Windows setup window
pub fn c(color: u32) -> Hsla {
    rgb(color).into()
}

/// Text in one of the night's type steps (`night::TYPE`), at its own weight.
pub fn text(step: Step, color: Hsla) -> Div {
    div()
        .font_family(if step.px >= night::DISPLAY.px {
            theme::DISPLAY_FONT
        } else {
            theme::FONT
        })
        .font_weight(FontWeight(step.weight as f32))
        .text_size(px(step.px))
        .line_height(px(step.line_px()))
        .text_color(color)
}
