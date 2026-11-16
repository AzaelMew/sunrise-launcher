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
use sunrise_theme::night::Night;

/// Logical pixels per texel of a plate's piece of backdrop.
const PIECE: f32 = 4.;
/// The Outline look's flat plate, under its 1 px line.
pub const PANEL: u32 = 0x171b2e;

thread_local! {
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

/// A corner: square in the Outline look.
fn rad(r: f32) -> f32 {
    if outline() {
        0.
    } else {
        r
    }
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

/// What a painted sky's blurred copy was made for: the night, the window's size and the blur.
type SkyKey = (Night, (i32, i32), f32);

struct Pieces {
    backdrop: Option<Backdrop>,
    /// A painted theme's own sky, blurred ([`frost_sky`]): its plates read this, not the scene.
    sky: Option<(SkyKey, Backdrop)>,
    /// A blur of it being made off the main thread ([`frost_sky_later`]).
    pending: Option<SkyKey>,
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
    /// Solid plates' 1 px edge (an OLED theme's), opaque.
    edge: Option<u32>,
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
        sky: None,
        pending: None,
        look: Glass::default_look(),
        generation: 0,
        build: 0,
        pieces: HashMap::new(),
        retired: Vec::new(),
        light: false,
        plain: false,
        edge: None,
        full_glass: None,
        full_float: None,
        full_size: (0., 0.),
        full_generation: 0,
    });
}

/// Performance mode's plates, and an OLED theme's: solid, with no glass behind them, and the
/// theme's edge around them if it has one.
pub fn set_plain(plain: bool, edge: Option<u32>) {
    PIECES.with_borrow_mut(|p| {
        if p.plain != plain || p.edge != edge {
            p.plain = plain;
            p.edge = edge;
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
    /// Plates drawn as a plain fill: the light look's, and performance mode's or an OLED theme's
    /// (`plain`), unless the look frosts its own painted sky (Taiga's light one too).
    fn solid(&self) -> bool {
        (self.light || self.plain) && !self.look.own_sky
    }

    /// Plates drawn as the light look's flat white pieces.
    fn flat_light(&self) -> bool {
        self.light && !self.look.own_sky
    }

    /// The blurred picture plates sample: the painted sky's for a look that frosts it, else the
    /// scene's.
    fn source(&self) -> Option<&Backdrop> {
        if self.look.own_sky {
            self.sky.as_ref().map(|(_, b)| b)
        } else {
            self.backdrop.as_ref()
        }
    }
}

/// A painted theme's sky, blurred for its plates, as the scene's is: made again only when the
/// night, the window's size (`w` × `h` logical pixels) or the blur changes, never per frame.
/// `texels` gives the sky's still parts: colours, columns, rows and logical pixels per texel.
pub fn frost_sky(
    night: &Night,
    (w, h): (f32, f32),
    texels: impl FnOnce() -> (Vec<[f64; 3]>, usize, usize, f32),
) {
    PIECES.with_borrow_mut(|p| {
        if !p.look.own_sky {
            return;
        }
        let key = (*night, (w.round() as i32, h.round() as i32), p.look.blur);
        if p.sky.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        p.sky = Some((key, blur_sky(key, texels)));
        p.generation += 1;
    });
}

fn blur_sky(key: SkyKey, texels: impl FnOnce() -> (Vec<[f64; 3]>, usize, usize, f32)) -> Backdrop {
    let (base, cols, rows, unit) = texels();
    Backdrop::from_texels(&base, cols, rows, f64::from(unit), f64::from(key.2), false)
}

/// A painted sky's blur made off the main thread, to hand to [`frost_done`].
pub struct Frosted(SkyKey, Backdrop);

/// [`frost_sky`] for a window that keeps drawing meanwhile: the first blur is made here (plates
/// have nothing to frost before it), a later one (a new blur, size or theme) is returned to run
/// on a worker while the plates keep the last one. One at a time: asked again while one runs,
/// it returns nothing, and the next frame after [`frost_done`] asks for the latest.
pub fn frost_sky_later(
    night: &Night,
    (w, h): (f32, f32),
    texels: impl FnOnce() -> (Vec<[f64; 3]>, usize, usize, f32) + Send + 'static,
) -> Option<impl FnOnce() -> Frosted + Send + 'static> {
    PIECES.with_borrow_mut(|p| {
        if !p.look.own_sky {
            return None;
        }
        let key = (*night, (w.round() as i32, h.round() as i32), p.look.blur);
        if p.sky.as_ref().is_some_and(|(k, _)| *k == key) || p.pending.is_some() {
            return None;
        }
        if p.sky.is_none() {
            p.sky = Some((key, blur_sky(key, texels)));
            p.generation += 1;
            return None;
        }
        p.pending = Some(key);
        Some(move || Frosted(key, blur_sky(key, texels)))
    })
}

/// A blur from [`frost_sky_later`] is ready: the plates take it (unless the look stopped
/// frosting its own sky meanwhile).
pub fn frost_done(Frosted(key, backdrop): Frosted) {
    PIECES.with_borrow_mut(|p| {
        p.pending = None;
        if p.look.own_sky {
            p.sky = Some((key, backdrop));
            p.generation += 1;
        }
    });
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
            if !look.own_sky {
                p.sky = None;
            }
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
            .bg(rgb(panel()).opacity(if kind == Kind::Float { 0.96 } else { 0.78 }))
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

/// A plate's 1 px edge: a theme's own (Hypr Neon's neon), else a faint light line, so a plate
/// reads as a surface rather than as fog over the scene.
fn edge() -> gpui::Hsla {
    if outline() {
        return gpui::white().opacity(0.12);
    }
    PIECES.with_borrow(|p| match p.edge {
        Some(edge) if !p.light => rgb(edge).into(),
        _ if p.light => gpui::Hsla::from(rgb(sunrise_theme::glass::ON_LIGHT)).opacity(0.10),
        _ => gpui::white().opacity(0.09),
    })
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
    // Solid plates need no backdrop (an OLED theme draws none): a plain quad.
    let plain = PIECES.with_borrow(|p| p.solid().then(|| (solid_fill(p, kind), p.edge, p.light)));
    if let Some((colour, edge, light)) = plain {
        let mut quad = gpui::fill(bounds, colour).corner_radii(corners);
        if let Some(edge) = edge.filter(|_| !light) {
            quad = quad.border_widths(px(1.)).border_color(rgb(edge));
        }
        window.paint_quad(quad);
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
        let light = p.flat_light();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn generation() -> u32 {
        PIECES.with_borrow(|p| p.generation)
    }

    /// A painted sky is blurred once per night, window size and blur, never per frame; a look
    /// that does not frost its own sky makes none and keeps none.
    #[test]
    fn a_painted_sky_is_blurred_only_when_it_changes() {
        let mut look = Glass::default_look();
        look.own_sky = true;
        set_light(false);
        set_plain(true, None);
        set_look(look);
        let night = sunrise_theme::night::NIGHT;
        let made = Cell::new(0);
        let texels = || {
            made.set(made.get() + 1);
            (vec![[0.5; 3]; 4 * 3], 4, 3, 4.)
        };
        frost_sky(&night, (16., 12.), texels);
        let g = generation();
        frost_sky(&night, (16., 12.), texels);
        assert_eq!((made.get(), generation()), (1, g), "same frame again");
        // Plates frost it although performance-mode's `plain` is set: the look decides.
        assert!(PIECES.with_borrow(|p| !p.solid() && p.source().is_some()));
        frost_sky(&night, (20., 12.), texels);
        assert_eq!(made.get(), 2, "new size");
        look.blur = 40.;
        set_look(look);
        frost_sky(&night, (20., 12.), texels);
        assert_eq!(made.get(), 3, "new blur");
        // Later: a new blur is made apart and taken when done; one at a time.
        look.blur = 10.;
        set_look(look);
        let job = frost_sky_later(&night, (20., 12.), || (vec![[0.5; 3]; 4 * 3], 4, 3, 4.))
            .expect("a new blur to make");
        assert!(frost_sky_later(&night, (20., 12.), || unreachable!()).is_none());
        let g = generation();
        frost_done(job());
        assert!(generation() > g);
        assert!(frost_sky_later(&night, (20., 12.), || unreachable!()).is_none());
        look.own_sky = false;
        set_look(look);
        frost_sky(&night, (20., 12.), texels);
        assert_eq!(made.get(), 3, "no painted sky");
        assert!(PIECES.with_borrow(|p| p.solid() && p.sky.is_none()));
    }
}
