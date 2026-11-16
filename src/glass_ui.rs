//! The glass look's controls ("Glass" in `docs/design-system.md`): the parts of the mockup's
//! `components/launcher/launcher-ui.tsx` and its shell (`launcher-app.tsx`), at its sizes. White
//! text and fills at a few opacities over [`glass`](crate::glass) plates; Geist throughout.
use crate::{hover_fade, hover_is, keyboard, note_hover, theme::Theme, track_hover, Kind};
use gpui::{
    canvas, div, prelude::*, px, rgb, rgba, svg, AnimationExt, AnyElement, App, ClickEvent, Div,
    ElementId, FontWeight, Hsla, Pixels, SharedString, Stateful, Svg, TextRun, Window,
};
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use sunrise_theme::glass::{fill, ink, radius, Glass};

thread_local! {
    /// Whether this frame's pages draw the glass look (the dark appearance).
    static ON: Cell<bool> = const { Cell::new(false) };
    /// Its corners' radius ([`set_corners`]).
    static CORNERS: Cell<f32> = const { Cell::new(0.) };
    /// The look's colours for the player's accent.
    static LOOK: Cell<Glass> = Cell::new(Glass::default_look());
    /// Whether the rows being built go in a Settings card ([`flat_rows`]).
    static FLAT: Cell<bool> = const { Cell::new(false) };
    /// The light appearance: the glass look's white ink and fills turn to [`INK`].
    static LIGHT: Cell<bool> = const { Cell::new(false) };
    /// How many [`hover_back`] layers this frame has built so far: the next one's name.
    static BACKS: Cell<usize> = const { Cell::new(0) };
    /// Each [`hover_back`] layer's pointer: easing on or off, from where.
    static BACK_HOVER: RefCell<HashMap<SharedString, crate::motion::Fade>> =
        RefCell::new(HashMap::new());
}

/// The light appearance's ink: the mockup's text on white (`#100a1f`), at the opacities the dark
/// look gives white.
const INK: u32 = sunrise_theme::glass::ON_LIGHT;

/// Set as each frame's pages start being built: whether the appearance is light.
pub fn set_light(light: bool) {
    LIGHT.set(light);
}

/// Whether the light appearance is drawn.
pub fn light() -> bool {
    LIGHT.get()
}

/// A solid control's fill (`SOLID`'s white, the sliders' knob): white, or the ink in light.
pub fn paper() -> Hsla {
    if light() {
        color(INK)
    } else {
        gpui::white()
    }
}

/// Text on [`paper`].
pub fn on_paper() -> Hsla {
    if light() {
        gpui::white()
    } else {
        color(INK)
    }
}

/// The look's green (`--live`): darker in light, to read on white plates.
pub fn live() -> Hsla {
    color(if light() {
        0x15803d
    } else {
        sunrise_theme::glass::LIVE
    })
}

/// The look's amber (`--warn`).
pub fn warn() -> Hsla {
    color(if light() {
        0xb45309
    } else {
        sunrise_theme::glass::WARN
    })
}

/// The look's red (`--danger`).
pub fn danger() -> Hsla {
    color(if light() {
        0xdc2626
    } else {
        sunrise_theme::glass::DANGER
    })
}

/// Builds `f`'s rows as the mockup's `SettingRow`s, for a Settings card: `index_row` and
/// `section_title` ask [`flat`].
pub fn flat_rows<R>(f: impl FnOnce() -> R) -> R {
    let was = FLAT.replace(true);
    let built = f();
    FLAT.set(was);
    built
}

/// Whether rows are being built for a Settings card.
pub fn flat() -> bool {
    FLAT.get()
}

/// A Settings row (`SettingRow`): the name in 15 px semibold, its note in 13 px on 1.5 at 60 %,
/// the control on the right, 40 px away; 16 px around, no fill. On a window too narrow for both,
/// the control wraps under the name (12 px below it), which keeps at least 240 px.
pub fn setting_row(
    title: impl IntoElement,
    note: Option<gpui::AnyElement>,
    control: Option<gpui::AnyElement>,
) -> Div {
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x(px(40.))
        .gap_y(px(12.))
        .px(px(14.))
        .py(px(13.))
        .rounded(px(rad(12.)))
        .border_1()
        .border_color(gpui::transparent_black())
        .relative()
        .child(back_edge(12., 0.03, true))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(240.))
                .gap(px(4.))
                .child(
                    text(14.5, FontWeight::SEMIBOLD, ink::FULL)
                        .line_height(px(20.))
                        .child(title),
                )
                .children(note.map(|n| {
                    text(13., FontWeight::NORMAL, 0.6)
                        .line_height(px(19.5))
                        .max_w(px(560.))
                        .child(n)
                })),
        )
        .children(control.map(|c| div().flex().flex_none().items_center().gap(px(8.)).child(c)))
}

/// A stacked Settings row (`SettingRow stacked`): the name and note, the control under them at
/// full width.
pub fn setting_row_stacked(
    title: impl IntoElement,
    note: Option<gpui::AnyElement>,
    control: gpui::AnyElement,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(16.))
        .px(px(14.))
        .py(px(13.))
        .rounded(px(rad(12.)))
        .border_1()
        .border_color(gpui::transparent_black())
        .relative()
        .child(back_edge(12., 0.03, true))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    text(14.5, FontWeight::SEMIBOLD, ink::FULL)
                        .line_height(px(20.))
                        .child(title),
                )
                .children(note.map(|n| {
                    text(13., FontWeight::NORMAL, 0.6)
                        .line_height(px(19.5))
                        .max_w(px(580.))
                        .child(n)
                })),
        )
        .child(div().w_full().child(control))
}

/// The mockup's `WIDE_FIELD` look on a field built for the old one: 44 px, up to 560 px wide,
/// 12 px corners, white at 7 % (11 % while typed in), 14 px Geist.
pub fn wide_field(field: Stateful<Div>, focused: bool) -> Stateful<Div> {
    field
        .w_full()
        .max_w(px(560.))
        .h(px(44.))
        .px(px(16.))
        .border_0()
        .rounded(px(crate::glass_ui::rad(radius::ROW)))
        .bg(white(if focused { 0.11 } else { 0.07 }))
        .font_family(sunrise_theme::glass::FONT)
        .text_size(px(14.))
        .relative()
        .when(focused, |d| d.child(focus_ring(radius::ROW)))
}

/// The focus ring: one solid 1 px line of the accent over a focused field, inside its edge with
/// the field's corners (GPUI has no outline, and a field clips what lies outside it). Put it in a
/// `relative` field round at `radius`.
pub fn focus_ring(radius: f32) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .rounded(px(crate::glass_ui::rad(radius)))
        .border_1()
        .border_color(color(look().accent))
}

/// Whether the pages being built draw the glass look: helpers with no theme at hand ask.
pub fn on() -> bool {
    ON.get()
}

/// Set as each frame's pages start being built.
pub fn set_on(on: bool) {
    ON.set(on);
    BACKS.set(0);
    crate::glass::set_outline(on);
}

/// A corner of the glass parts: square, as the Outline look draws them (`docs/design-system.md`).
pub fn rad(r: f32) -> f32 {
    if on() {
        CORNERS.get()
    } else {
        r
    }
}

/// Set as each frame starts: the glass look's corner radius in px, the same for every corner
/// (Settings > Appearance > Corners; a small element rounds no further than half its side).
pub fn set_corners(radius: f32) {
    CORNERS.set(radius);
}

/// The Outline look's 1 px edge.
pub fn line() -> Hsla {
    white(0.12)
}

/// A label in the Outline look: Geist Mono, upper case, spaced out.
pub fn mono(size: f32, a: f32) -> Div {
    div()
        .font_family(crate::theme::MONO)
        .text_size(px(size))
        .line_height(px(size * 1.4))
        .text_color(white(a))
}

/// Set as each frame's pages start being built: the colours for the player's accent.
pub fn set_look(look: Glass) {
    LOOK.set(look);
}

/// The look's colours.
pub fn look() -> Glass {
    LOOK.get()
}

/// White at `a`: the dark look's ink and fills. In light, the ink at `a`, so every part keeps
/// its weight on the light plates.
pub fn white(a: f32) -> Hsla {
    if light() {
        color(INK).opacity(a)
    } else {
        gpui::white().opacity(a)
    }
}

/// A colour of the look (`0xrrggbb`).
pub fn color(c: u32) -> Hsla {
    rgb(c).into()
}

/// Text at `size` px in Geist, white at `a`, at `weight`.
pub fn text(size: f32, weight: FontWeight, a: f32) -> Div {
    div()
        .font_family(sunrise_theme::glass::FONT)
        .text_size(px(size))
        .line_height(px((size * 1.4).round()))
        .font_weight(weight)
        .text_color(white(a))
}

/// The widths text was laid out in, by key, as [`room`] recorded them on the last frame: what
/// [`cut`] compares a text's own width against.
pub type Rooms = Rc<RefCell<HashMap<SharedString, f32>>>;

/// Fills its (relative) parent and records the parent's laid-out width in `rooms` under `key`.
pub fn room(rooms: &Rooms, key: SharedString) -> impl IntoElement {
    let rooms = rooms.clone();
    gpui::canvas(
        move |bounds, _, _| {
            rooms.borrow_mut().insert(key, f32::from(bounds.size.width));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Whether `text` in `family` at `size` and `weight` is wider than the room recorded under `key`,
/// so a truncating parent cuts it: decides a full-text hover label. False until a room is known.
pub fn cut(
    cx: &App,
    rooms: &Rooms,
    key: &str,
    text: &str,
    family: &'static str,
    size: f32,
    weight: FontWeight,
) -> bool {
    let Some(room) = rooms.borrow().get(key).copied() else {
        return false;
    };
    let system = cx.text_system();
    let font = system.resolve_font(&gpui::Font {
        weight,
        ..gpui::font(family)
    });
    let width: f32 = text
        .chars()
        .filter_map(|c| system.advance(font, px(size), c).ok())
        .map(|a| f32::from(a.width))
        .sum();
    width > room + 0.5
}

/// One of the mockup's Hugeicons, stroked in `color`.
pub fn icon(name: &'static str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/glass/{name}.svg"))
        .flex_none()
        .size(px(size))
        .text_color(color)
}

/// A layer filling its parent, round at `r`, for hover fills.
fn layer(r: f32) -> impl Fn() -> Div {
    move || {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded(px(crate::glass_ui::rad(r)))
    }
}

/// `id`'s fill eased between `from` (white at that opacity) and `to` while hovered, as the
/// mockup's `transition-colors` does.
fn hover_fill(id: &SharedString, r: f32, from: f32, to: f32) -> gpui::AnyElement {
    hover_edge(id, r, to - from, false)
}

/// The side of one texel in a [`pixel_fill`].
pub(crate) const TEXEL: f32 = 4.;

/// How far a texel's lightness strays from its fill: on a coloured button, and on a sidebar,
/// where it stays well under the rows.
pub const TEXTURE_BUTTON: f32 = 0.02;
pub const TEXTURE_QUIET: f32 = 0.009;

/// A fill of `c` as a block texture: square texels `step` of lightness off it either way, the
/// same every frame, so it never shimmers. The steps are fixed, not a share of `c`'s lightness, so
/// a near-black sidebar shows them too. Put it first in a `relative` element, so its content is
/// drawn over it.
pub fn pixel_fill(c: Hsla, step: f32) -> AnyElement {
    let shades = texel_shades(c, step);
    // The look's corners, which a canvas can't clip to: the fill is drawn round, and texels
    // outside the round corners are left out.
    let r = rad(0.);
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let r = r.min(w / 2.).min(h / 2.);
            window.paint_quad(gpui::fill(bounds, c).corner_radii(px(r)));
            let (cols, rows) = ((w / TEXEL).ceil() as u32, (h / TEXEL).ceil() as u32);
            let inside = |x: u32, y: u32| {
                let (cx, cy) = ((x as f32 + 0.5) * TEXEL, (y as f32 + 0.5) * TEXEL);
                let (dx, dy) = (
                    (r - cx).max(cx - (w - r)).max(0.),
                    (r - cy).max(cy - (h - r)).max(0.),
                );
                dx * dx + dy * dy <= r * r
            };
            // Most texels keep the base colour; the rest take one of the shades.
            let tone = |x: u32, y: u32| {
                shades
                    .get((texel_noise(x, y) % 9) as usize)
                    .copied()
                    .filter(|_| inside(x, y))
            };
            for y in 0..rows {
                let top = y as f32 * TEXEL;
                let tall = TEXEL.min(h - top);
                let mut x = 0;
                while x < cols {
                    let Some(this) = tone(x, y) else {
                        x += 1;
                        continue;
                    };
                    // A run of one shade along the row is one quad, so a tall sidebar stays cheap.
                    let start = x;
                    while x < cols && tone(x, y) == Some(this) {
                        x += 1;
                    }
                    let left = start as f32 * TEXEL;
                    let wide = (x as f32 * TEXEL).min(w) - left;
                    let at = gpui::point(bounds.origin.x + px(left), bounds.origin.y + px(top));
                    let size = gpui::size(px(wide), px(tall));
                    window.paint_quad(gpui::fill(gpui::Bounds::new(at, size), this));
                }
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// How many texels a side of the window grain's tile has: it repeats every 256 px, which at
/// this faintness never shows.
const GRAIN_TILE: usize = 64;

thread_local! {
    /// The grain tile last baked, and what for (colour, step and scale, as bits).
    static GRAIN: RefCell<Option<([u32; 6], Arc<gpui::RenderImage>)>> = const { RefCell::new(None) };
}

/// A large area's grain (the sidebars, the top bar): `c` as the block texture of [`pixel_fill`],
/// `width` by `height`. One tile is baked once (at the window's `scale`, its texels on whole device
/// pixels) and repeated, so a full-height sidebar costs a few images rather than thousands of quads
/// each frame. Put it first in a `relative` element.
pub fn grain(c: Hsla, step: f32, width: f32, height: f32, scale: f32) -> Div {
    let tile = grain_tile(c, step, scale);
    let side = GRAIN_TILE as f32 * TEXEL;
    let (cols, rows) = (
        (width / side).ceil() as usize,
        (height / side).ceil() as usize,
    );
    let mut layer = div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .overflow_hidden()
        .bg(c);
    for y in 0..rows {
        for x in 0..cols {
            layer = layer.child(
                gpui::img(tile.clone())
                    .absolute()
                    .left(px(x as f32 * side))
                    .top(px(y as f32 * side))
                    .size(px(side)),
            );
        }
    }
    layer
}

fn grain_tile(c: Hsla, step: f32, scale: f32) -> Arc<gpui::RenderImage> {
    let key = [
        c.h.to_bits(),
        c.s.to_bits(),
        c.l.to_bits(),
        c.a.to_bits(),
        step.to_bits(),
        scale.to_bits(),
    ];
    if let Some(tile) = GRAIN.with_borrow(|g| {
        g.as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, t)| t.clone())
    }) {
        return tile;
    }
    let bytes = |h: Hsla| {
        let rgba = gpui::Rgba::from(h);
        [rgba.r, rgba.g, rgba.b, rgba.a].map(|v| (v.clamp(0., 1.) * 255.).round() as u8)
    };
    let shades = texel_shades(c, step).map(bytes);
    let base = bytes(c);
    let texels: Vec<[u8; 4]> = (0..GRAIN_TILE * GRAIN_TILE)
        .map(|i| {
            let (x, y) = ((i % GRAIN_TILE) as u32, (i / GRAIN_TILE) as u32);
            shades
                .get((texel_noise(x, y) % 9) as usize)
                .copied()
                .unwrap_or(base)
        })
        .collect();
    let tile = crate::art::bake(&texels, GRAIN_TILE, GRAIN_TILE, TEXEL, scale);
    GRAIN.set(Some((key, tile.clone())));
    tile
}

thread_local! {
    /// Each fold chevron's last state (open or not) and how often it has changed, so it turns
    /// only when its menu opens or closes, never as a page is first drawn.
    static FOLDS: RefCell<HashMap<&'static str, (bool, usize)>> = RefCell::new(HashMap::new());
}

/// A menu's chevron: pointing down while it is closed, turning up on the spring while it opens,
/// and back down as it closes. `id` names it among the others.
pub fn fold_chevron(id: &'static str, open: bool, size: f32, ink: Hsla) -> AnyElement {
    let turns = FOLDS.with_borrow_mut(|folds| {
        let (was, turns) = folds.entry(id).or_insert((open, 0));
        if *was != open {
            *was = open;
            *turns += 1;
        }
        *turns
    });
    let at = move |up: f32| gpui::Transformation::rotate(gpui::radians(std::f32::consts::PI * up));
    let chevron = icon("arrow-down-01", size, ink);
    if turns == 0 {
        return chevron.with_transformation(at(0.)).into_any_element();
    }
    chevron
        .with_animation(
            ElementId::named_usize(id, turns),
            crate::motion::eased(crate::motion::POP),
            move |d, t| d.with_transformation(at(if open { t } else { 1. - t })),
        )
        .into_any_element()
}

/// The texture's three shades of `c`, `step` of lightness off it.
pub(crate) fn texel_shades(c: Hsla, step: f32) -> [Hsla; 3] {
    let shade = move |d: f32| gpui::hsla(c.h, c.s, (c.l + d).clamp(0., 1.), c.a);
    [shade(step), shade(-step), shade(1.75 * step)]
}

/// A fixed scramble of a texel's place.
pub(crate) fn texel_noise(x: u32, y: u32) -> u32 {
    let mut n = x.wrapping_mul(0x9e37_79b1) ^ y.wrapping_mul(0x85eb_ca77);
    n ^= n >> 15;
    n = n.wrapping_mul(0x2c1b_3c6d);
    n ^ (n >> 12)
}

/// `accent` a little lighter, for a coloured button under the pointer.
pub fn lit_accent(accent: u32) -> Hsla {
    let c = color(accent);
    gpui::hsla(c.h, c.s, (c.l * 1.1).min(1.), 1.)
}

/// How opaque white outlines are: every control and inner card at rest, under the pointer, and
/// open or chosen (redesign pass 2: outlines everywhere, stronger as the pointer comes).
pub mod edge {
    pub const REST: f32 = 0.10;
    pub const HOVER: f32 = 0.24;
    pub const STRONG: f32 = 0.30;
}

/// `id`'s hover layer: a white fill at `fill`, eased in and out (no outline: a line flashing round
/// a row read as a glitch, not a hover). `bordered`: the element has its own 1 px outline, which
/// the layer reaches over.
pub(crate) fn hover_edge(id: &SharedString, r: f32, fill: f32, bordered: bool) -> gpui::AnyElement {
    let inset = if bordered { -1. } else { 0. };
    hover_fade(
        id,
        move || {
            div()
                .absolute()
                .top(px(inset))
                .left(px(inset))
                .right(px(inset))
                .bottom(px(inset))
                .rounded(px(rad(r)))
        },
        move |d, t| d.bg(white(fill * t)),
    )
}

/// How far a row's content slides along while the pointer is on it.
const NUDGE: f32 = 3.;

/// A row's content sliding [`NUDGE`] px along under the pointer, eased in and out: put it first
/// in a row whose children are `gap` px apart (it takes no gap of its own). Nothing at rest.
pub fn hover_nudge(id: &SharedString, gap: f32) -> AnyElement {
    crate::hover_ease(
        id,
        || div().flex_none().h(px(1.)),
        move |d, t| d.w(px(NUDGE * t)).mr(px(-gap)),
        sunrise_theme::QUICK,
    )
}

/// A hover layer that hears the pointer itself, for an element with no id, or with hoverable
/// parts in it ([`track_hover`] lights one element at a time, so a row would go dark under its own
/// button): `paint` draws it at 0 (rest) to 1 (hovered), eased in and out on QUICK, at once under
/// reduced motion. Put it first in a `relative` element, so the content is drawn over it. At rest
/// it draws nothing and plays nothing.
fn hover_back(inset: f32, r: f32, paint: impl Fn(Div, f32) -> Div + 'static) -> AnyElement {
    let n = BACKS.get();
    BACKS.set(n + 1);
    // Named by its place in the frame's build: the pages are built in the same order each frame.
    // shortcut: a row hovered as its page is swapped by the keyboard may stay lit until the
    // pointer moves over it; give the callers ids if that shows.
    let id = SharedString::from(format!("hover-back-{n}"));
    let now = std::time::Instant::now();
    let fade = BACK_HOVER.with_borrow(|all| all.get(&id).copied());
    let shown = fade.filter(|f| !f.rested(now)).map(|fade| {
        fade.play(
            &id,
            div()
                .absolute()
                .top(px(inset))
                .left(px(inset))
                .right(px(inset))
                .bottom(px(inset))
                .rounded(px(rad(r))),
            paint,
        )
    });
    let key = id.clone();
    let owner = crate::widgets::Owner::default();
    div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(owner.noting())
        .on_hover(move |hovered, window, cx| {
            BACK_HOVER.with_borrow_mut(|all| {
                let last = all.get(&key).copied();
                let fade = crate::motion::Fade::toward(last, *hovered, std::time::Instant::now());
                all.insert(key.clone(), fade);
            });
            owner.redraw(window, cx);
        })
        .children(shown)
        .into_any_element()
}

/// [`hover_back`] as [`hover_edge`]: white at `fill` and the hover outline. `bordered`: the
/// element has its own 1 px outline, which the layer's covers.
pub fn back_edge(r: f32, fill: f32, bordered: bool) -> AnyElement {
    hover_back(if bordered { -1. } else { 0. }, r, move |d, t| {
        d.bg(white(fill * t))
            .border_1()
            .border_color(white(edge::HOVER * t))
    })
}

/// [`hover_back`] in one colour (white at a few %, a dark tint, the danger red), no outline.
pub fn back_tint(r: f32, c: Hsla) -> AnyElement {
    hover_back(0., r, move |d, t| d.bg(c.opacity(c.a * t)))
}

/// `a` towards `b` by `t`, for hover-eased colours.
pub fn towards(a: Hsla, b: Hsla, t: f32) -> Hsla {
    Hsla {
        h: a.h + (b.h - a.h) * t,
        s: a.s + (b.s - a.s) * t,
        l: a.l + (b.l - a.l) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// `d`, a card built by [`card`] with its children in, wrapped to rise [`crate::motion::LIFT`] px
/// on the spring while the pointer is on it, its outline brightening. Pressing sinks it
/// [`crate::motion::PRESS`] px instead.
pub fn lift(id: &SharedString, d: Stateful<Div>) -> AnyElement {
    let rest = white(edge::REST + 0.03);
    let lit = white(edge::HOVER);
    crate::hover_move(
        id,
        d.border_1().active(|s| s.top(px(crate::motion::PRESS))),
        move |d, t| {
            d.top(px(-crate::motion::LIFT * t))
                .border_color(towards(rest, lit, t))
        },
    )
}

/// The site's card: white at 3.5 %, a quiet 1 px outline, `r` corners, its hover eased by [`lift`].
/// The look (outline, rise, glow) is [`lift`]'s; a card that should not lift takes this alone.
pub fn card(id: impl Into<SharedString>, r: f32, t: &Theme) -> Stateful<Div> {
    let id = id.into();
    keyboard(
        track_hover(
            div()
                .id(id.clone())
                .relative()
                .rounded(px(rad(r)))
                .bg(white(0.035))
                .border_1()
                .border_color(white(edge::REST + 0.03))
                .cursor_pointer(),
            id,
        ),
        t,
    )
}

/// The state dot before a row's second line (`Dot`): live green, amber for busy (the mockup
/// pulses it) and for a warning, the accent's soft shade (given) for an update, white at 35 % for
/// nothing yet.
#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Live,
    Busy,
    Warn,
    Accent(u32),
    Off,
}

pub fn dot(tone: Tone) -> Div {
    let c = match tone {
        Tone::Live => live(),
        Tone::Busy | Tone::Warn => warn(),
        Tone::Accent(soft) => color(soft),
        Tone::Off => white(0.35),
    };
    div().flex_none().size(px(7.)).rounded_full().bg(c)
}

/// The progress bar (`Bar`): 4 px, white at 12 %, filled in the accent's soft shade.
pub fn bar(value: f32, accent_soft: u32) -> Div {
    div()
        .h(px(4.))
        .w_full()
        .rounded(px(rad(2.)))
        .overflow_hidden()
        .bg(white(fill::ACTIVE))
        .child(
            div()
                .h_full()
                .rounded(px(rad(2.)))
                .w(gpui::relative(value.clamp(0., 1.)))
                .bg(color(accent_soft)),
        )
}

/// A sidebar row (an instance, a server): `rounded-[12px] p-2`, white at 10 % when it is the one
/// shown, 5 % under the pointer. Its icon and lines go in as children; `pr` leaves room for a
/// button laid over its right end.
pub fn row(id: impl Into<SharedString>, current: bool, pr: f32, t: &Theme) -> Stateful<Div> {
    let id = id.into();
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .items_center()
        .gap(px(12.))
        .w_full()
        .p(px(8.))
        .pr(px(pr))
        .rounded(px(rad(radius::ROW)))
        .cursor_pointer();
    // The one shown: white at 10 %, as the design's selected instance row.
    let d = if current {
        d.bg(white(fill::SELECTED))
    } else {
        d.child(hover_edge(&id, radius::ROW, fill::ROW_HOVER, true))
    };
    let d = d.child(hover_nudge(&id, 12.));
    keyboard(track_hover(d, id), t)
}

/// A row's two lines: the name (14 px semibold) and its state after a dot (12.5 px, white at
/// 65 %), both at the page's 1.5 line height.
pub fn row_text(name: impl Into<SharedString>, state: (Tone, String)) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(
            text(14., FontWeight::SEMIBOLD, ink::FULL)
                .line_height(px(21.))
                .truncate()
                .child(name.into()),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w_0()
                .child(dot(state.0))
                .child(
                    text(12.5, FontWeight::NORMAL, ink::MUTED)
                        .line_height(px(18.75))
                        .truncate()
                        .child(state.1),
                ),
        )
}

/// A square button with one icon (`ICON_BUTTON`): white at 65 %, white with a 8 % fill under
/// the pointer, white with 12 % while `active`.
pub fn icon_button(
    id: impl Into<SharedString>,
    name: &'static str,
    size: f32,
    icon_size: f32,
    r: f32,
    active: bool,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let group = SharedString::from(format!("{id}-group"));
    let d = div()
        .id(id.clone())
        .group(group.clone())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(crate::glass_ui::rad(r)))
        .cursor_pointer()
        .active(|s| s.top(px(1.)));
    let d = if active {
        d.bg(white(fill::ACTIVE))
            .border_1()
            .border_color(white(edge::STRONG))
    } else {
        d.child(hover_fill(&id, r, 0., fill::BUTTON))
    };
    let glyph = icon(
        name,
        icon_size,
        white(if active { ink::FULL } else { ink::MUTED }),
    )
    .relative()
    .group_hover(group, |s| s.text_color(white(ink::FULL)));
    keyboard(track_hover(d.child(glyph), id), t)
}

/// A top-bar page, as the site's nav: a naked tab — no plate, no pill — its label brightening and
/// a faint fill arriving under the pointer, with the accent bar growing under it: half out on
/// hover and all the way out on the page it names (`open`).
pub fn nav_tab(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    open: bool,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let look = look();
    let bar = div()
        .absolute()
        .bottom(px(1.))
        .h(px(2.))
        .rounded(px(rad(2.)))
        .bg(color(look.accent));
    let bar = if open {
        bar.left(px(12.)).right(px(12.)).into_any_element()
    } else {
        // Grows from the middle out to 80 % of the tab, at 70 % strength, on the spring.
        crate::hover_ease(
            &id,
            move || {
                div()
                    .absolute()
                    .bottom(px(1.))
                    .h(px(2.))
                    .rounded(px(rad(2.)))
                    .bg(color(look.accent))
            },
            move |d, t| {
                d.left(gpui::relative(0.5 - 0.4 * t))
                    .right(gpui::relative(0.5 - 0.4 * t))
                    .opacity(0.7 * t)
            },
            crate::motion::SPRING,
        )
    };
    let label: SharedString = label.into();
    keyboard(
        track_hover(
            div()
                .id(id.clone())
                .relative()
                .flex()
                .flex_none()
                .items_center()
                .h(px(36.))
                .px(px(13.))
                .rounded(px(rad(radius::CONTROL)))
                .cursor_pointer()
                .font_family(sunrise_theme::glass::FONT)
                .text_size(px(14.5))
                .line_height(px(20.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(white(if open { ink::FULL } else { ink::MUTED }))
                .when(!open, |d| {
                    d.hover(|s| s.text_color(white(ink::FULL)))
                        .child(back_tint(radius::CONTROL, white(0.04)))
                })
                .child(div().relative().child(label))
                .child(bar),
            id,
        ),
        t,
    )
}
const MENU_FONT_SIZE: f32 = 14.;
const MENU_ICON_SIZE: f32 = 17.;
const MENU_ROW_GAP: f32 = 12.;
const MENU_ROW_PADDING: f32 = 12.;
const POPOVER_PADDING: f32 = 6.;

/// Width needed to fit the longest translated row, including the icon and both paddings.
pub fn menu_content_width(window: &Window, labels: &[&str]) -> f32 {
    let font = gpui::font(sunrise_theme::glass::FONT);
    let widest = labels
        .iter()
        .map(|&label| {
            let run = TextRun {
                len: label.len(),
                font: font.clone(),
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window.text_system().shape_line(
                SharedString::from(label.to_owned()),
                px(MENU_FONT_SIZE),
                &[run],
                None,
            );
            f32::from(line.width)
        })
        .fold(0., f32::max);
    widest.ceil() + MENU_ICON_SIZE + MENU_ROW_GAP + 2. * (MENU_ROW_PADDING + POPOVER_PADDING)
}

/// A popover's row (`MENU_ROW`): 40 px, 14 px text after a 17 px icon at 60 %, white at 7 %
/// under the pointer. `danger` colours it as the mockup's Delete.
pub fn menu_row(
    id: impl Into<SharedString>,
    name: &'static str,
    label: impl Into<SharedString>,
    danger: bool,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let (fg, icon_color) = if danger {
        let c = self::danger();
        (c, c)
    } else {
        (white(ink::FULL), white(ink::NOTE))
    };
    let hover_color = if danger {
        rgba(sunrise_theme::glass::DANGER_HOVER << 8 | 0x1f).into()
    } else {
        white(0.07)
    };
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .items_center()
        .gap(px(MENU_ROW_GAP))
        .h(px(40.))
        .w_full()
        .px(px(MENU_ROW_PADDING))
        .rounded(px(crate::glass_ui::rad(radius::CONTROL)))
        .cursor_pointer()
        .group(id.clone())
        .child(hover_fade(&id, layer(radius::CONTROL), move |d, a| {
            d.bg(hover_color.opacity(hover_color.a * a))
        }))
        .child(hover_nudge(&id, MENU_ROW_GAP))
        .child(
            icon(name, MENU_ICON_SIZE, icon_color)
                .relative()
                .when(!danger, |d| {
                    d.group_hover(id.clone(), |s| s.text_color(white(ink::FULL)))
                }),
        )
        .child(
            text(MENU_FONT_SIZE, FontWeight::NORMAL, 1.)
                .relative()
                .min_w_0()
                .truncate()
                .text_color(fg)
                .child(label.into()),
        );
    keyboard(track_hover(d, id), t)
}

/// A popover (`POPOVER`): a menu's near-opaque plate (the page's plates let the scene through),
/// 6 px around its rows, a shadow setting it above the card under it.
pub fn popover(width: f32) -> Div {
    crate::glass::float(radius::POPOVER)
        .shadow_lg()
        .w(px(width))
        .p(px(POPOVER_PADDING))
        .flex()
        .flex_col()
}

/// What a dialog or the palette lays over the launcher (`bg-[#07050f]/50 backdrop-blur-xl`):
/// GPUI can't blur what it drew, so the scene's blurred copy (a menu's, `hzl-float`) covers the
/// page. The scrim goes over it.
pub fn scrim_backdrop() -> impl IntoElement {
    crate::glass::backdrop(crate::glass::Kind::Float, 0.)
}

/// The scrim over [`scrim_backdrop`]: 50 % in the dark look, 20 % in light, so the dimmed
/// launcher stays day-lit.
pub fn scrim() -> Hsla {
    color(sunrise_theme::glass::SCRIM).opacity(if light() { 0.2 } else { 0.5 })
}

/// A dialog's plate, round at `radius` (`hzl-glass` over the scrim): its piece of the scene
/// under the scrim's dimming, then the plates' white. The content goes after it.
pub fn dialog_plate(radius: f32) -> Div {
    let layer = |c: Hsla| {
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded(px(crate::glass_ui::rad(radius)))
            .bg(c)
    };
    crate::glass::plate(radius)
        // Light plates are already day-lit: no dimming under them.
        .child(layer(
            color(sunrise_theme::glass::SCRIM).opacity(if light() { 0. } else { 0.5 }),
        ))
        .child(layer(white(sunrise_theme::glass::TINT)))
}

/// Text on a white button (`SOLID`): `#100a1f`, medium.
pub fn solid(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    h: f32,
    size: f32,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    keyboard(
        div()
            .id(id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(8.))
            // `h` is the least height: a label that wraps in a narrow button (a long translation)
            // grows it instead of spilling out of it.
            .min_h(px(h))
            .py(px(4.))
            .px(px(16.))
            .rounded(px(crate::glass_ui::rad(radius::CONTROL)))
            .text_center()
            .bg(paper())
            .border_1()
            .border_color(white(edge::REST))
            .cursor_pointer()
            .hover(|s| {
                s.opacity(0.94).shadow(vec![gpui::BoxShadow {
                    color: gpui::black().opacity(0.25),
                    offset: gpui::point(px(0.), px(4.)),
                    blur_radius: px(14.),
                    spread_radius: px(0.),
                    inset: false,
                }])
            })
            .active(|s| s.opacity(0.85))
            .font_family(sunrise_theme::glass::FONT)
            .text_size(px(size))
            .font_weight(FontWeight::MEDIUM)
            .text_color(on_paper())
            .child(label.into()),
        t,
    )
}

/// A menu row with a note under its label (`MENU_ITEM`): 14 px medium over 12 px at 55 %, white
/// at 7 % under the pointer; at 40 % when `locked`. A mark (a tick) can follow on the right.
pub fn menu_item(
    id: impl Into<SharedString>,
    label: SharedString,
    note: String,
    locked: bool,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .w_full()
        .px(px(12.))
        .py(px(8.))
        .rounded(px(crate::glass_ui::rad(radius::CONTROL)))
        .when(!locked, |d| {
            d.cursor_pointer()
                .child(hover_fade(&id, layer(radius::CONTROL), |d, a| {
                    d.bg(white(0.07 * a))
                }))
        })
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    text(14., FontWeight::MEDIUM, ink::FULL)
                        .line_height(px(21.))
                        .child(label),
                )
                .child(
                    text(12., FontWeight::NORMAL, ink::QUIET)
                        .line_height(px(18.))
                        .truncate()
                        .child(note),
                ),
        );
    if locked {
        return d.opacity(0.4);
    }
    keyboard(track_hover(d, id), t)
}

/// A dropdown's choice: its name on one line, 36 tall, the accent's tick on the one `picked`.
pub fn pick_row(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    picked: bool,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .h(px(36.))
        .w_full()
        .px(px(12.))
        .rounded(px(crate::glass_ui::rad(radius::CONTROL)))
        .cursor_pointer()
        .child(hover_fade(&id, layer(radius::CONTROL), |d, a| {
            d.bg(white(0.07 * a))
        }))
        .child(
            text(14., FontWeight::NORMAL, ink::FULL)
                .relative()
                .truncate()
                .child(label.into()),
        )
        .children(picked.then(|| icon("tick-02", 17., color(look().accent_soft)).relative()));
    keyboard(track_hover(d, id), t)
}

/// A quiet button (`QUIET`): white at 8 %, 14 % under the pointer, 14 px medium, `h` tall.
pub fn quiet(
    id: impl Into<SharedString>,
    label: impl Into<SharedString>,
    h: f32,
    t: &Theme,
) -> Stateful<Div> {
    quiet_icon(id, None, label, h, t)
}

/// [`quiet`] with a 16 px icon before its label.
pub fn quiet_icon(
    id: impl Into<SharedString>,
    name: Option<&'static str>,
    label: impl Into<SharedString>,
    h: f32,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .h(px(h))
        .px(px(14.))
        .rounded(px(rad(radius::CONTROL)))
        .bg(white(fill::BUTTON))
        .border_1()
        .border_color(white(edge::REST))
        .cursor_pointer()
        .font_family(sunrise_theme::glass::FONT)
        .text_size(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(white(ink::FULL))
        .child(hover_edge(&id, radius::CONTROL, 0.06, true))
        .children(name.map(|n| icon(n, 16., white(ink::FULL)).relative()))
        .child(div().relative().child(label.into()));
    crate::press(keyboard(track_hover(d, id.clone()), t), &id, |d| {
        d.bg(white(0.18)).top(px(1.))
    })
}

/// A list row (`ROW`): `rounded-[12px] px-3 py-2.5`, white at 6 % under the pointer. `on_hover`
/// hears the pointer come and go too, for rows that show their actions then. `held`: the row
/// keeps its fill while the pointer is on one of its buttons or its menu is open, as CSS's
/// `:hover` stays on a parent.
pub fn list_row(
    id: impl Into<SharedString>,
    held: bool,
    on_hover: impl Fn(&bool, &mut Window, &mut App) + 'static,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let key = id.clone();
    let owner = crate::widgets::Owner::default();
    let fill = if held && !hover_is(&id) {
        layer(radius::ROW)()
            .bg(white(fill::ROW_HOVER))
            .border_1()
            .border_color(white(edge::HOVER))
            .into_any_element()
    } else {
        hover_fill(&id, radius::ROW, 0., fill::ROW_HOVER)
    };
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .items_center()
        .gap(px(12.))
        .w_full()
        .px(px(12.))
        .py(px(10.))
        .rounded(px(crate::glass_ui::rad(radius::ROW)))
        .child(fill)
        .child(owner.noting())
        .on_hover(move |hovered, window, cx| {
            note_hover(&key, *hovered, &owner, window, cx);
            on_hover(hovered, window, cx);
        });
    keyboard(d, t)
}

/// A list row's two lines: the name (14 px medium) and a note (13 px, white at 55 %).
pub fn list_text(name: impl Into<SharedString>, note: impl Into<SharedString>) -> Div {
    div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(
            text(14., FontWeight::MEDIUM, ink::FULL)
                .line_height(px(21.))
                .truncate()
                .child(name.into()),
        )
        .child(
            text(13., FontWeight::NORMAL, ink::QUIET)
                .line_height(px(19.5))
                .truncate()
                .child(note.into()),
        )
}

/// A tab's line while it has nothing to list (`Empty`): 14 px, white at 60 %, `px-3 py-10`.
pub fn empty() -> Div {
    text(14., FontWeight::NORMAL, ink::NOTE)
        .line_height(px(21.))
        .px(px(12.))
        .py(px(40.))
}

/// The line over a tab's list (`px-3 pb-2`): its note at 13 px, white at 60 %, then buttons.
pub fn list_head(note: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .px(px(12.))
        .pb(px(8.))
        .child(
            text(13., FontWeight::NORMAL, ink::NOTE)
                .line_height(px(19.5))
                .min_w_0()
                .truncate()
                .child(note.into()),
        )
}

/// A link in a line of text (`font-medium text-white underline-offset-4 hover:underline`).
pub fn link(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id.into())
        .font_weight(FontWeight::MEDIUM)
        .text_color(white(ink::FULL))
        .cursor_pointer()
        .hover(|s| s.underline())
        .child(label.into())
}

/// A field (`WIDE_FIELD`): 44 px, round at 12, white at 7 %, 11 % with the focus, 14 px. Red at
/// 12 % while what it holds is refused.
pub fn field(focused: bool, invalid: bool) -> Div {
    let bg: Hsla = if invalid {
        rgba(sunrise_theme::glass::DANGER_HOVER << 8 | 0x1f).into()
    } else {
        white(if focused {
            fill::FIELD_FOCUS
        } else {
            fill::FIELD
        })
    };
    div()
        .relative()
        .flex()
        .items_center()
        .h(px(44.))
        .px(px(16.))
        .overflow_hidden()
        .rounded(px(crate::glass_ui::rad(radius::ROW)))
        .bg(bg)
        .border_1()
        .border_color(if invalid {
            color(sunrise_theme::glass::DANGER_HOVER).opacity(0.6)
        } else if focused {
            color(look().accent)
        } else {
            white(edge::REST)
        })
        .when(!focused, |d| d.child(back_edge(radius::ROW, 0., true)))
        .font_family(sunrise_theme::glass::FONT)
        .text_size(px(14.))
}

thread_local! {
    /// The slider being dragged, if one is.
    static SLIDING: RefCell<Option<SharedString>> = const { RefCell::new(None) };
}

/// A slider from `min` to `max` in whole `step`s, standing at `value`, `width` px long: a 4 px
/// track filled in the accent up to a square knob. A press sets it where the pointer is, and it
/// follows a drag anywhere in the window until the button comes up; `on_change` hears each value.
pub fn slider(
    id: impl Into<SharedString>,
    value: f32,
    (min, max, step): (f32, f32, f32),
    width: f32,
    on_change: impl Fn(f32, &mut Window, &mut App) + 'static,
) -> Div {
    let id = id.into();
    let share = ((value - min) / (max - min)).clamp(0., 1.);
    let on_change = Rc::new(on_change);
    const KNOB: f32 = 14.;
    // The window's own mouse events, read where the slider was painted: a drag keeps going past
    // its ends, which an element's listeners (heard only under the pointer) would lose.
    let listen = canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let at = move |x: Pixels| {
                let share = ((x - bounds.origin.x) / bounds.size.width).clamp(0., 1.);
                ((min + share * (max - min)) / step).round() * step
            };
            let (down_id, change) = (id.clone(), on_change.clone());
            window.on_mouse_event(move |e: &gpui::MouseDownEvent, phase, window, cx| {
                if phase == gpui::DispatchPhase::Bubble
                    && e.button == gpui::MouseButton::Left
                    && bounds.contains(&e.position)
                {
                    SLIDING.set(Some(down_id.clone()));
                    change(at(e.position.x), window, cx);
                }
            });
            let (move_id, change) = (id.clone(), on_change.clone());
            window.on_mouse_event(move |e: &gpui::MouseMoveEvent, phase, window, cx| {
                let mine = SLIDING.with_borrow(|s| s.as_ref() == Some(&move_id));
                if phase == gpui::DispatchPhase::Bubble && mine && e.dragging() {
                    change(at(e.position.x), window, cx);
                }
            });
            window.on_mouse_event(|_: &gpui::MouseUpEvent, _, _, _| SLIDING.set(None));
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    div()
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .w(px(width))
        .h(px(24.))
        .cursor_pointer()
        .child(
            div()
                .relative()
                .w_full()
                .h(px(4.))
                .rounded(px(rad(0.).min(2.)))
                .overflow_hidden()
                .bg(white(fill::ACTIVE))
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(share))
                        .bg(color(look().accent)),
                ),
        )
        .child(
            div()
                .absolute()
                .left(gpui::relative(share))
                .ml(px(-KNOB / 2.))
                .size(px(KNOB))
                .rounded(px(rad(KNOB / 2.)))
                .bg(white(ink::FULL)),
        )
        .child(listen)
}

/// A switch (`Toggle`): 40×24, round; the accent with an `on_accent` knob on the right while on,
/// white at 14 % with a white knob at 70 % on the left while off.
pub fn toggle(
    id: impl Into<SharedString>,
    on: bool,
    accent: u32,
    on_accent: u32,
    t: &Theme,
) -> Stateful<Div> {
    let id = id.into();
    let knob = div()
        .absolute()
        .top(px(4.))
        .size(px(16.))
        .rounded_full()
        .bg(if on { color(on_accent) } else { white(0.7) });
    let knob = crate::motion::knob(&id, on, knob, 4., 16.);
    keyboard(
        div()
            .id(id)
            .relative()
            .flex_none()
            .w(px(40.))
            .h(px(24.))
            .rounded_full()
            .bg(if on {
                color(accent)
            } else {
                white(fill::BUTTON_HOVER)
            })
            .cursor_pointer()
            .child(knob),
        t,
    )
}

/// The generic [`button`](crate::button) in the glass look, 36 px: `Primary` is the mockup's
/// `PLAY` (the accent, semibold), `Secondary` its `QUIET` (white at 8 %, 14 % under the pointer),
/// `Ghost` its text button (white at 70 %, white on 6 % under the pointer).
pub fn button(id: SharedString, label: SharedString, kind: Kind, t: &Theme) -> Stateful<Div> {
    let look = look();
    let d = div()
        .id(id.clone())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .h(px(36.))
        .px(px(14.))
        .rounded(px(crate::glass_ui::rad(radius::CONTROL)))
        .cursor_pointer()
        .font_family(sunrise_theme::glass::FONT)
        .text_size(px(14.))
        .line_height(px(20.))
        .font_weight(FontWeight::MEDIUM);
    let d = match kind {
        Kind::Primary => {
            let c = if hover_is(&id) {
                lit_accent(look.accent)
            } else {
                color(look.accent)
            };
            d.text_color(color(look.on_accent))
                .font_weight(FontWeight::SEMIBOLD)
                .child(pixel_fill(c, TEXTURE_BUTTON))
        }
        Kind::Secondary => d
            .bg(white(fill::BUTTON))
            .border_1()
            .border_color(white(edge::REST))
            .text_color(white(ink::FULL))
            .child(hover_edge(&id, radius::CONTROL, 0.06, true)),
        Kind::Ghost => d
            .text_color(white(ink::BODY))
            .hover(|s| s.text_color(white(ink::FULL)))
            .child(hover_edge(&id, radius::CONTROL, 0.06, false)),
    };
    let el = keyboard(
        track_hover(d.child(div().relative().child(label)), id.clone()),
        t,
    );
    crate::press(el, &id, move |d| match kind {
        Kind::Primary => d.opacity(0.92).top(px(1.)),
        Kind::Secondary => d.bg(white(0.18)).top(px(1.)),
        Kind::Ghost => d.bg(white(0.1)).top(px(1.)),
    })
}

/// A tab in a row of tabs, `h` tall: the open one on a lit fill, the others with a faint fill
/// under the pointer. One weight and a 1 px edge on every tab (clear when not open), so
/// opening one moves nothing.
pub fn pill_tab(id: impl Into<SharedString>, on: bool, h: f32, t: &Theme) -> Stateful<Div> {
    let id = id.into();
    keyboard(
        track_hover(
            div()
                .id(id.clone())
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(7.))
                .h(px(h))
                .px(px(12.))
                .rounded(px(rad(radius::CONTROL)))
                .cursor_pointer()
                .font_family(sunrise_theme::glass::FONT)
                .font_weight(FontWeight::MEDIUM)
                .border_1()
                .text_color(white(if on { ink::FULL } else { ink::BODY }))
                .when(on, |d| d.bg(white(0.12)).border_color(white(edge::STRONG)))
                .when(!on, |d| {
                    d.border_color(gpui::transparent_black())
                        .child(hover_edge(&id, radius::CONTROL, 0.05, false))
                        .hover(|s| s.text_color(white(ink::FULL)))
                }),
            id,
        ),
        t,
    )
}

/// A segmented control in the glass look (the site's): white at 4 % round at 12, 3 px around
/// 32 px options round at 9; the chosen one in the accent's tint.
pub fn segments<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(T, &'static str)],
    current: T,
    t: &Theme,
    on_pick: impl Fn(T, &mut Window, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    let mut seg = div()
        .id(id)
        .relative()
        .flex()
        .flex_none()
        .gap(px(2.))
        .p(px(3.))
        .rounded(px(rad(radius::LARGE)))
        .bg(white(0.04))
        .border_1()
        .border_color(white(edge::REST));
    for &(value, label) in options {
        let active = current == value;
        let pick = on_pick.clone();
        let option = SharedString::from(format!("{id}-{label}"));
        let d = div()
            .id(option.clone())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(32.))
            .px(px(12.))
            .rounded(px(rad(radius::CONTROL - 1.)))
            .cursor_pointer()
            .font_family(sunrise_theme::glass::FONT)
            .text_size(px(13.5))
            .line_height(px(20.))
            .font_weight(FontWeight::MEDIUM)
            .border_1()
            .text_color(white(if active { ink::FULL } else { ink::MUTED }))
            .when(active, |d| {
                d.border_color(color(look().accent).opacity(0.6))
                    .bg(color(look().accent).opacity(0.2))
            })
            .when(!active, |d| {
                d.border_color(gpui::transparent_black())
                    .hover(|s| s.text_color(white(ink::FULL)))
                    .child(hover_edge(&option, radius::CONTROL - 1., 0.06, false))
            })
            .child(div().relative().child(crate::tr(label)))
            .on_click(move |_: &ClickEvent, window, cx| pick(value, window, cx));
        seg = seg.child(keyboard(track_hover(d, option.clone()), t));
    }
    seg
}

/// A 2 px line along a panel's top (absolute) with a short accent segment sweeping across it:
/// shown the moment a wait starts, so a click answers at once, and taking no room.
pub fn loading_line() -> impl IntoElement {
    div()
        .absolute()
        .top_0()
        .left(px(rad(radius::PANEL)))
        .right(px(rad(radius::PANEL)))
        .h(px(2.))
        .overflow_hidden()
        .child({
            let segment = div()
                .absolute()
                .top_0()
                .h_full()
                .w(gpui::relative(0.3))
                .bg(color(look().accent));
            // Reduced motion: the segment holds still in sight.
            if crate::motion::still() {
                segment.left(gpui::relative(0.35)).into_any_element()
            } else {
                segment
                    .with_animation(
                        "loading-line",
                        gpui::Animation::new(std::time::Duration::from_millis(1100))
                            .repeat()
                            .with_easing(gpui::ease_in_out),
                        |d, p| d.left(gpui::relative(-0.3 + 1.3 * p)),
                    )
                    .into_any_element()
            }
        })
}

/// `d` dimmed while a slow wait runs (search `n`): unchanged for its first 200 ms, then easing
/// to half, so a quick answer swaps what is shown with nothing in between.
pub fn waiting(d: Div, n: usize) -> AnyElement {
    d.with_animation(
        ElementId::named_usize("waiting", n),
        gpui::Animation::new(std::time::Duration::from_millis(400)),
        |d, p| d.opacity(1. - 0.5 * ((p - 0.5) * 2.).clamp(0., 1.)),
    )
    .into_any_element()
}

/// A page's head (`PageHead`): its title in Bricolage, 46 px at this width
/// (`clamp(2.25rem, 3.6vw, 3rem)`), its note under it at 15 px on 1.55, white at 70 %, at most
/// 62 characters wide; 40 px above, 28 below.
pub fn page_head(title: impl Into<SharedString>, note: impl Into<SharedString>) -> Div {
    let note: SharedString = note.into();
    div()
        .flex()
        .flex_col()
        .pt(px(28. - crate::glass::CONTENT_INSET))
        .mb(px(20.))
        .child(
            // A compact title: the page's content, not its name, gets the height.
            div()
                .font_family(sunrise_theme::glass::DISPLAY_FONT)
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(32.))
                .line_height(px(36.))
                .text_color(white(ink::FULL))
                .child(title.into()),
        )
        .when(!note.is_empty(), |d| {
            d.child(
                text(14., FontWeight::NORMAL, ink::BODY)
                    .mt(px(8.))
                    .max_w(px(615.))
                    .line_height(px(23.25))
                    .child(note),
            )
        })
}

/// A small heading in a card (`h3`): 13 px, medium, white at 70 %.
pub fn card_heading(label: impl Into<SharedString>) -> Div {
    text(13., FontWeight::MEDIUM, ink::BODY)
        .line_height(px(19.5))
        .child(label.into())
}

/// A card's note: 13 px on 1.5, white at 55 %.
pub fn card_note(label: impl Into<SharedString>) -> Div {
    text(13., FontWeight::NORMAL, ink::QUIET)
        .line_height(px(19.5))
        .child(label.into())
}

/// A tooltip's view: one line on a solid float, for controls shown by their icon alone.
pub struct Tip(SharedString);

impl Render for Tip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .max_w(px(360.))
            .px(px(10.))
            .py(px(6.))
            .rounded(px(rad(radius::CONTROL)))
            .bg(rgb(look().base))
            .border_1()
            .border_color(white(0.12))
            .font_family(sunrise_theme::glass::FONT)
            .text_size(px(13.))
            .text_color(white(ink::FULL))
            .child(self.0.clone())
    }
}

/// `.tooltip(ui::tip(label))`: the label of an icon-only control, on hover.
pub fn tip(label: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView {
    let label: SharedString = label.into();
    move |_, cx| cx.new(|_| Tip(label.clone())).into()
}
