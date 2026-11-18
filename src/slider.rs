//! A slider: pressing on its track sets it there and dragging moves it, past the track's ends
//! too; the arrow keys step it while it has the keyboard.
//!
//! A slider keeps its round rail and thumb whatever the player's corner rounding.
//!
//! The track is found by a scroll handle that scrolls nothing: GPUI Fast moves a scrolled page's
//! layer without drawing it again, and only a handle's bounds follow that move.
use crate::paint::palette;
use crate::Launcher;
use gpui::{
    canvas, div, px, relative, rgb, App, Context, DispatchPhase, Div, ElementId,
    InteractiveElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, Pixels, Point, ScrollHandle, Stateful, StatefulInteractiveElement, Styled,
    WeakEntity, Window,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// What a slider does with a new value (0 to 1); `done` once the player lets go.
pub(crate) type Change = Rc<dyn Fn(&mut Launcher, f32, bool, &mut Context<Launcher>)>;

const THUMB: f32 = 16.;

thread_local! {
    static TRACKS: RefCell<HashMap<&'static str, ScrollHandle>> = RefCell::default();
    /// The slider the pointer holds, if one.
    static HELD: RefCell<Option<&'static str>> = const { RefCell::new(None) };
}

fn track(id: &'static str) -> ScrollHandle {
    TRACKS.with_borrow_mut(|t| t.entry(id).or_default().clone())
}

/// Where along the track `at` is, 0 to 1.
fn along(id: &'static str, at: Point<Pixels>) -> f32 {
    let b = track(id).bounds();
    let travel = f32::from(b.size.width) - THUMB;
    if travel <= 0. {
        return 0.;
    }
    ((f32::from(at.x - b.left()) - THUMB / 2.) / travel).clamp(0., 1.)
}

/// A slider at `value` (0 to 1), stepping by `step` from the keyboard.
pub(crate) fn slider(
    id: &'static str,
    value: f32,
    step: f32,
    change: Change,
    cx: &mut Context<Launcher>,
) -> Stateful<Div> {
    let value = value.clamp(0., 1.);
    let me: WeakEntity<Launcher> = cx.entity().downgrade();
    let (press, keys, drag) = (change.clone(), change.clone(), change);
    let follow = canvas(
        |_, _, _| {},
        move |_, _, window: &mut Window, _: &mut App| {
            let me_move = me.clone();
            let drag_move = drag.clone();
            window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture || HELD.with_borrow(|h| *h != Some(id)) {
                    return;
                }
                if ev.pressed_button != Some(MouseButton::Left) {
                    HELD.set(None);
                    return;
                }
                let v = along(id, ev.position);
                let _ = me_move.update(cx, |this, cx| drag_move(this, v, false, cx));
            });
            let me_up = me.clone();
            let drag_up = drag.clone();
            window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture || HELD.with_borrow(|h| *h != Some(id)) {
                    return;
                }
                HELD.set(None);
                let v = along(id, ev.position);
                let _ = me_up.update(cx, |this, cx| drag_up(this, v, true, cx));
            });
        },
    )
    .absolute()
    .size_full();
    div()
        .id(ElementId::Name(id.into()))
        .tab_index(0)
        .relative()
        .flex_none()
        .w(px(200.))
        .h(px(22.))
        .cursor_pointer()
        .track_scroll(&track(id))
        .focus_visible(|s| s.border_color(rgb(palette().focus)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                HELD.set(Some(id));
                press(this, along(id, ev.position), false, cx);
                cx.notify();
            }),
        )
        .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, _, cx| {
            let by = match ev.keystroke.key.as_str() {
                "left" | "down" => -step,
                "right" | "up" => step,
                _ => return,
            };
            keys(this, (value + by).clamp(0., 1.), true, cx);
            cx.stop_propagation();
            cx.notify();
        }))
        // The rail, filled up to the thumb.
        .child(
            div()
                .absolute()
                .left(px(THUMB / 2.))
                .right(px(THUMB / 2.))
                .top(px(9.))
                .h(px(4.))
                .rounded_full()
                .bg(rgb(palette().line))
                .child(
                    div()
                        .h_full()
                        .w(relative(value))
                        .rounded_full()
                        .bg(rgb(palette().text)),
                ),
        )
        // The thumb's travel: the track less half a thumb at each end.
        .child(
            div()
                .absolute()
                .left(px(THUMB / 2.))
                .right(px(THUMB / 2.))
                .top(px(3.))
                .h(px(THUMB))
                .child(
                    div()
                        .absolute()
                        .left(relative(value))
                        .ml(px(-THUMB / 2.))
                        .size(px(THUMB))
                        .rounded_full()
                        .bg(rgb(palette().text)),
                ),
        )
        .child(follow)
}
