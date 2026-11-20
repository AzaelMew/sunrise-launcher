//! A box of text to read and take away: a JSON preview, a code block, a rule to paste
//! elsewhere. Its text can be selected like any page text ([`crate::selectable`]), and a copy
//! button sits in the box's top-right corner, on the box itself rather than on the text, so it
//! stays put while the text scrolls under it. A copy turns it into a tick that pops in, then it
//! eases back to the copy glyph.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, rgb, Animation, AnimationExt, ClickEvent, Div, ElementId, Hsla, InteractiveElement,
    IntoElement, ParentElement, SharedString, StatefulInteractiveElement, Styled, StyledText,
    Window,
};

use crate::paint::palette;
use crate::selectable::Selectable;

/// How long the tick shows after a copy.
const SHOWN: Duration = Duration::from_millis(1400);

thread_local! {
    /// Each box's last copy: when, and how many so far (a new copy restarts the pop).
    static COPIED: RefCell<HashMap<SharedString, (Instant, usize)>> = RefCell::new(HashMap::new());
}

fn copied(id: &SharedString) -> Option<usize> {
    COPIED.with_borrow(|m| {
        m.get(id)
            .filter(|(at, _)| at.elapsed() < SHOWN)
            .map(|&(_, n)| n)
    })
}

/// A box of `blocks` (paragraphs, shown with a little space between; one block keeps its own
/// line breaks). Copy takes them all, a blank line between blocks. `max_h` makes the text
/// scroll inside the box past that height. The box comes in the field look; style the
/// returned element (fill, corners, type) to suit where it sits.
pub fn code_box(id: impl Into<SharedString>, blocks: Vec<SharedString>, max_h: Option<f32>) -> Div {
    let id: SharedString = id.into();
    let whole: String = blocks
        .iter()
        .map(|b| b.trim_end())
        .collect::<Vec<_>>()
        .join("\n\n");
    let text = div()
        .id(SharedString::from(format!("{id}-text")))
        .flex()
        .flex_col()
        .gap(px(6.))
        .p(px(12.))
        // Room for the button, so it never sits over the end of the first line.
        .pr(px(44.))
        .when_some(max_h, |d, h| {
            d.max_h(px(h))
                .overflow_y_scroll()
                .on_scroll(|_, _, cx| cx.stop_propagation())
        })
        .children(blocks.into_iter().enumerate().map(|(n, b)| {
            let b: SharedString = b.trim_end().to_string().into();
            Selectable::styled(("code-box-block", n), StyledText::new(b))
        }));
    div()
        .relative()
        .min_w_0()
        .rounded(crate::tokens::r(crate::tokens::R_CONTROL))
        .bg(rgb(palette().field))
        .border_1()
        .border_color(rgb(palette().divider))
        .font_family(crate::theme::MONO)
        .text_size(px(crate::tokens::SMALL))
        .text_color(rgb(palette().muted))
        .child(text)
        .child(copy_button(id, whole))
}

/// Linear over `length`, or at its end at once under reduced motion.
fn played(length: Duration) -> Animation {
    let a = Animation::new(length);
    match crate::motion::still() {
        true => a.with_easing(|_| 1.),
        false => a,
    }
}

/// The copy button for a box's corner: put it in a `relative` box, after the text.
pub(crate) fn copy_button(id: SharedString, whole: String) -> impl IntoElement {
    let shown = copied(&id);
    let muted: Hsla = rgb(palette().muted).into();
    let accent: Hsla = rgb(palette().accent).into();
    let glyph = match shown {
        // The tick pops in: from small, past its size, back to it.
        Some(n) => crate::glass_ui::icon("tick-02", 14., accent)
            .with_animation(
                ElementId::named_usize(SharedString::from(format!("{id}-tick")), n),
                played(Duration::from_millis(280)),
                |g, t| {
                    // An ease-out-back: overshoots by about 15 % at its height.
                    let s = 1. + 2.7 * (t - 1.).powi(3) + 1.7 * (t - 1.).powi(2);
                    g.size(px(14. * (0.4 + 0.6 * s))).opacity(t.min(1.))
                },
            )
            .into_any_element(),
        None => crate::glass_ui::icon("copy-01", 14., muted)
            .with_animation(
                ElementId::Name(format!("{id}-copy").into()),
                played(Duration::from_millis(200)),
                |g, t| g.opacity(t),
            )
            .into_any_element(),
    };
    let tint = if shown.is_some() {
        accent.opacity(0.14)
    } else {
        Hsla::from(rgb(palette().divider)).opacity(0.0)
    };
    div()
        .id(SharedString::from(format!("{id}-copy-button")))
        .occlude()
        .absolute()
        .top(px(6.))
        .right(px(6.))
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(crate::tokens::r(crate::tokens::R_SMALL))
        .bg(tint)
        .when(shown.is_some(), |d| {
            d.border_1().border_color(accent.opacity(0.4))
        })
        .cursor_pointer()
        .hover(|d| d.bg(Hsla::from(rgb(palette().divider)).opacity(0.6)))
        .tooltip(crate::glass_ui::tip(crate::i18n::tr(if shown.is_some() { "Copied" } else { "Copy" })))
        .child(glyph)
        .on_click(move |_: &ClickEvent, window: &mut Window, cx| {
            let n = COPIED.with_borrow_mut(|m| {
                let e = m.entry(id.clone()).or_insert((Instant::now(), 0));
                *e = (Instant::now(), e.1 + 1);
                e.1
            });
            crate::widgets::copy_to_clipboard(cx, gpui::ClipboardItem::new_string(whole.clone()));
            // The view isn't known outside a draw, so redraw the window.
            window.refresh();
            // Back to the copy glyph once the tick has shown, unless another copy came since.
            let id = id.clone();
            window
                .spawn(cx, async move |cx| {
                    cx.background_executor().timer(SHOWN).await;
                    let last = COPIED.with_borrow(|m| m.get(&id).map(|&(_, n)| n));
                    if last == Some(n) {
                        let _ = cx.update(|window, _| window.refresh());
                    }
                })
                .detach();
        })
}
