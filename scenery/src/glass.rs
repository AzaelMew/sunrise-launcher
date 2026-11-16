//! The glass look's plates (`.hzl-glass` and `.hzl-float` in the mockup): the scene behind,
//! blurred and saturated, under a little white and the accent's dark violet, with round corners.
//!
//! GPUI has no backdrop filter, but nothing sits under a plate except the scene (`scene.rs`), so
//! each plate paints the matching piece of the scene's blurred copy ([`scene::Backdrop`]) with its
//! own corners, its tint mixed in. The pieces are small images (a texel per 4 logical pixels: a
//! 24 px blur has nothing finer), kept while the pages that drew them may still be on screen.
use crate::scene::{Backdrop, Model};
use gpui::{
    canvas, div, prelude::*, px, rgb, AnyElement, Bounds, Corners, Div, Pixels, RenderImage, Window,
};
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use sunrise_theme::glass::Glass;

/// Logical pixels per texel of a plate's piece of backdrop.
const PIECE: f32 = 4.;
/// The Outline look's flat plate, under its 1 px line.
pub const PANEL: u32 = 0x171b2e;

thread_local! {
    /// The Outline look's corner radius ([`set_corners`]).
    static CORNERS: std::cell::Cell<f32> = const { std::cell::Cell::new(0.) };
    /// The flat plate's colour for the theme and appearance shown ([`set_panel`]).
    static OUTLINE_PANEL: Cell<u32> = const { Cell::new(PANEL) };
    /// Whether the pages being built draw the Outline look ([`set_outline`]).
    static OUTLINE: Cell<bool> = const { Cell::new(false) };
}

/// Set with the launcher's look as each frame's pages start being built.
pub fn set_outline(on: bool) {
    OUTLINE.set(on);
}

fn outline() -> bool {
    OUTLINE.get()
}

/// A corner: in the Outline look, the radius the player picked, the same for every corner
/// ([`set_corners`]).
fn rad(r: f32) -> f32 {
    if outline() {
        CORNERS.get()
    } else {
        r
    }
}

/// Set as each frame starts: the Outline look's corner radius in px.
pub fn set_corners(radius: f32) {
    CORNERS.set(radius);
}

/// Set as each frame starts: the theme's card colour, so plates match its sky.
pub fn set_panel(c: u32) {
    OUTLINE_PANEL.set(c);
}

/// The Outline look's plate colour now.
pub fn panel() -> u32 {
    OUTLINE_PANEL.get()
}

/// A plate's kind: `.hzl-glass` (panels, the sidebar, dialogs) or `.hzl-float` (menus and
/// popovers: twice the blur, darker).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Glass,
    Float,
}

/// A piece's place and size, its kind, the backdrop's generation, and its shape: 0 for a plate,
/// else the device scale (×100) of a fillet's piece.
type Key = (i32, i32, i32, i32, Kind, u32, u32);

struct Pieces {
    backdrop: Option<Backdrop>,
    look: Glass,
    /// Bumped when the backdrop or the look changes: older pieces are thrown away.
    generation: u32,
    /// Bumped each time the launcher's pages are built: a piece last painted two builds ago is
    /// on no frame any more (GPUI Fast copies a view's last frame, never an older one).
    build: u64,
    pieces: HashMap<Key, (Arc<RenderImage>, u64)>,
    retired: Vec<Arc<RenderImage>>,
    /// The light appearance: plates are white at [`LIGHT_GLASS`] or [`LIGHT_FLOAT`] over the dawn
    /// sky, which has no blurred copy.
    light: bool,
    /// Performance mode: plates are a solid colour instead of glass over the scene, so nothing
    /// is blurred, tinted or sampled.
    plain: bool,
    /// Full-window backdrop images baked once per window size/look/generation.
    full_glass: Option<Arc<RenderImage>>,
    full_float: Option<Arc<RenderImage>>,
    full_size: (f32, f32),
    full_generation: u32,
}

/// How much white a light plate takes, and a light menu's.
const LIGHT_GLASS: f32 = 0.84;
const LIGHT_FLOAT: f32 = 0.94;

thread_local! {
    static PIECES: RefCell<Pieces> = RefCell::new(Pieces {
        backdrop: None,
        look: Glass::default_look(),
        generation: 0,
        build: 0,
        pieces: HashMap::new(),
        retired: Vec::new(),
        light: false,
        plain: false,
        full_glass: None,
        full_float: None,
        full_size: (0., 0.),
        full_generation: 0,
    });
}

/// Performance mode's plates: solid, with no glass behind them.
pub fn set_plain(plain: bool) {
    PIECES.with_borrow_mut(|p| {
        if p.plain != plain {
            p.plain = plain;
            p.generation += 1;
        }
    });
}

/// A solid plate's fill: the look's plate in the dark, white at a light plate's opacity in the
/// light (the dark plate under the light look's dark text did not read).
fn solid_fill(p: &Pieces, kind: Kind) -> gpui::Hsla {
    if p.light {
        let a = if kind == Kind::Float {
            LIGHT_FLOAT
        } else {
            LIGHT_GLASS
        };
        gpui::white().opacity(a)
    } else {
        rgb(p.look.plate).into()
    }
}

impl Pieces {
    /// Plates drawn as a plain fill: the light look's, and performance mode's (`plain`).
    fn solid(&self) -> bool {
        self.light || self.plain
    }

    /// The blurred picture plates sample: the scene's.
    fn source(&self) -> Option<&Backdrop> {
        self.backdrop.as_ref()
    }
}

/// The scene was built again (a new window size or tint), or the blur changed: plates take
/// their pieces from this backdrop.
/// How far below the top bar the pages' scroll area starts, so what scrolls up stops short of the
/// bar instead of running into it. The pages' heads hold that much less above their titles, so
/// nothing moves at rest.
pub const CONTENT_INSET: f32 = 14.;

pub fn set_backdrop(model: &Model, tint: Option<u32>, blur: f32, fast: bool) {
    let backdrop = Backdrop::new(model, blur as f64, fast);
    put_backdrop(backdrop, tint);
}

/// The plates' backdrop blurred from `model`, made apart (on a worker) for [`put_backdrop`].
pub fn blur_backdrop(model: &Model, blur: f32) -> Backdrop {
    Backdrop::new(model, blur as f64, false)
}

/// Plates take `backdrop` from now on, in the accent's `tint`.
pub fn put_backdrop(mut backdrop: Backdrop, tint: Option<u32>) {
    backdrop.set_tint(tint);
    PIECES.with_borrow_mut(|p| {
        p.backdrop = Some(backdrop);
        p.generation += 1;
    });
}

/// The accent's tint changed (the window and the blur did not): the backdrop is tinted as its
/// pieces are read, so they are painted again, and nothing is blurred.
pub fn set_backdrop_tint(tint: Option<u32>) {
    PIECES.with_borrow_mut(|p| {
        if let Some(backdrop) = p.backdrop.as_mut() {
            if backdrop.set_tint(tint) {
                p.generation += 1;
            }
        }
    });
}

/// The player's accent (and so the plates' tint) changed: pieces are painted again.
pub fn set_look(look: Glass) {
    PIECES.with_borrow_mut(|p| {
        if p.look != look {
            p.look = look;
            p.generation += 1;
        }
    });
}

/// The appearance changed between light and dark: pieces are painted again.
pub fn set_light(light: bool) {
    PIECES.with_borrow_mut(|p| {
        if p.light != light {
            p.light = light;
            p.generation += 1;
        }
    });
}

/// A light piece's texel: white at the plate's opacity, `shape` of it covered.
fn light_texel(kind: Kind, shape: f32) -> image::Rgba<u8> {
    let a = if kind == Kind::Float {
        LIGHT_FLOAT
    } else {
        LIGHT_GLASS
    };
    image::Rgba([255, 255, 255, (a * shape * 255.).round() as u8])
}

/// The launcher's pages are being built: pieces nothing drew in the last build are freed (call
/// at the start of the launcher's render).
pub fn begin_build(window: &mut Window) {
    let retired = PIECES.with_borrow_mut(|p| {
        p.build += 1;
        let (build, generation) = (p.build, p.generation);
        let stale: Vec<Key> = p
            .pieces
            .iter()
            .filter(|(k, (_, used))| k.5 != generation || *used + 1 < build)
            .map(|(k, _)| *k)
            .collect();
        for k in stale {
            if let Some((image, _)) = p.pieces.remove(&k) {
                p.retired.push(image);
            }
        }
        std::mem::take(&mut p.retired)
    });
    for image in retired {
        let _ = window.drop_image(image);
    }
}

/// A plate's backdrop with its corners: put it first inside a `relative` element, the content
/// after it. In performance mode it is a solid colour: no backdrop, no glass.
pub fn backdrop(kind: Kind, radius: f32) -> AnyElement {
    if outline() {
        return div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            // Solid: the flat look has no frost, and a see-through panel lets what is under a
            // dialog read through it.
            .bg(rgb(panel()))
            .rounded(px(rad(radius)))
            .into_any_element();
    }
    // The light look's plates are flat white with no blur: a plain fill draws them exactly (its
    // image pieces came out missing, the sky showing through the panels).
    let plain = PIECES.with_borrow(|p| p.solid().then(|| solid_fill(p, kind)));
    if let Some(colour) = plain {
        return div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .rounded(px(radius))
            .bg(colour)
            .into_any_element();
    }
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| paint(bounds, kind, Corners::all(px(radius)), window),
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// A plate's 1 px edge: a faint line, so a plate reads as a surface rather than as fog over the
/// scene.
fn edge() -> gpui::Hsla {
    if outline() {
        gpui::white().opacity(0.12)
    } else if PIECES.with_borrow(|p| p.light) {
        gpui::Hsla::from(rgb(sunrise_theme::glass::ON_LIGHT)).opacity(0.10)
    } else {
        gpui::white().opacity(0.09)
    }
}

/// A `relative` div on a glass plate with `radius` corners.
pub fn plate(radius: f32) -> Div {
    div()
        .relative()
        .rounded(px(rad(radius)))
        .border_1()
        .border_color(edge())
        .child(backdrop(Kind::Glass, radius))
}

/// The same for a menu or popover.
pub fn float(radius: f32) -> Div {
    div()
        .relative()
        .rounded(px(rad(radius)))
        .border_1()
        .border_color(edge())
        .child(backdrop(Kind::Float, radius))
}

fn build_dark_full_image(
    backdrop: &Backdrop,
    look: &Glass,
    kind: Kind,
    vw: f32,
    vh: f32,
) -> Arc<RenderImage> {
    let (cols, rows) = texels(vw, vh);
    let tint = [16, 8, 0].map(|s| ((look.plate >> s) & 255) as f32 / 255.);
    let a = look.plate_alpha;
    let float = kind == Kind::Float;

    let mut buf = image::RgbaImage::new(cols + 2, rows + 2);
    for (row, line) in buf.rows_mut().enumerate() {
        let y = (row as f32 - 0.5) * vh / rows as f32;
        for (col, p) in line.enumerate() {
            let x = (col as f32 - 0.5) * vw / cols as f32;
            let c = backdrop.at(float, x as f64, y as f64);
            let mixed = [0, 1, 2].map(|i| ((tint[i] * a + c[i] * (1. - a)) * 255.).round() as u8);
            // BGRA, opaque.
            *p = image::Rgba([mixed[2], mixed[1], mixed[0], 255]);
        }
    }

    Arc::new(RenderImage::new([image::Frame::new(buf)]))
}

fn paint(bounds: Bounds<Pixels>, kind: Kind, corners: Corners<Pixels>, window: &mut Window) {
    // Solid plates need no backdrop: a plain quad.
    if let Some(colour) = PIECES.with_borrow(|p| p.solid().then(|| solid_fill(p, kind))) {
        window.paint_quad(gpui::fill(bounds, colour).corner_radii(corners));
        return;
    }
    let viewport = window.viewport_size();
    let (vw, vh) = (f32::from(viewport.width), f32::from(viewport.height));
    if vw <= 0. || vh <= 0. {
        return;
    }

    let (cols, rows) = texels(vw, vh);
    let (tx, ty) = (vw / cols as f32, vh / rows as f32);
    let outer = Bounds::new(
        gpui::point(-px(tx), -px(ty)),
        viewport + gpui::size(px(tx * 2.), px(ty * 2.)),
    );

    let (image, image_bounds) = PIECES.with_borrow_mut(|p| {
        let light = p.light;
        if !light && p.source().is_none() {
            return (None, bounds);
        }

        if p.full_generation != p.generation || p.full_size != (vw, vh) {
            if let Some(old) = p.full_glass.take() {
                p.retired.push(old);
            }
            if let Some(old) = p.full_float.take() {
                p.retired.push(old);
            }
            p.full_generation = p.generation;
            p.full_size = (vw, vh);
        }

        let missing = match kind {
            Kind::Glass => p.full_glass.is_none(),
            Kind::Float => p.full_float.is_none(),
        };
        if missing {
            let made = if light {
                let buf = image::RgbaImage::from_pixel(2, 2, light_texel(kind, 1.));
                Arc::new(RenderImage::new([image::Frame::new(buf)]))
            } else if let Some(backdrop) = p.source() {
                build_dark_full_image(backdrop, &p.look, kind, vw, vh)
            } else {
                return (None, bounds);
            };
            match kind {
                Kind::Glass => p.full_glass = Some(made),
                Kind::Float => p.full_float = Some(made),
            }
        }
        let image = match kind {
            Kind::Glass => p.full_glass.clone(),
            Kind::Float => p.full_float.clone(),
        };

        let img_bounds = if light { bounds } else { outer };
        (image, img_bounds)
    });

    if let Some(image) = image {
        let _ = window.paint_image(bounds, image_bounds, corners, image, 0, false);
    }
}

/// How many texels a piece `w` × `h` logical pixels has across and down.
fn texels(w: f32, h: f32) -> (u32, u32) {
    (
        (w / PIECE).ceil().max(1.) as u32,
        (h / PIECE).ceil().max(1.) as u32,
    )
}
