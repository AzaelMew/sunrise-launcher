//! Sunrise's own scene behind Home: first light over a still sea. The sky falls in dithered
//! pixel bands from night to amber, the sun stands half out of the water in stepped rings of
//! glow, and a blocky island in the foreground, rim-lit on its sun side, is mirrored in the sea.
//! What moves does so a pixel at a time on a slow beat: the sun's column of glints on the water,
//! ripples across it, streaks of cloud by the horizon, a few birds flapping low over the sea, and
//! stars twinkling above.
//!
//! The still picture is baked once per size into an image; a frame paints that and a few hundred
//! quads, a few times a second, and none while reduced motion holds the scene or the window has
//! been behind others for a while.
use crate::art::bake;
use crate::clock::{motion, Clock, Motion};
use gpui::{canvas, div, img, prelude::*, px, Bounds, Hsla, Pixels, RenderImage};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Logical pixels per texel.
const TEXEL: f32 = 4.;
/// How often what moves steps.
const BEAT: Duration = Duration::from_millis(180);
/// How long the scene keeps moving after the window loses focus.
const UNFOCUSED: Duration = Duration::from_secs(10);
/// The horizon, as a share of the height; the sun's radius as one too.
const SUNRISE: f32 = 0.82;
const SUN_RADIUS: f32 = 0.10;
const SUN_X: f32 = 0.74;
/// The island's blocks, in texels, and each column's height in blocks from the left edge.
const BLOCK: usize = 4;
const ISLAND: [usize; 11] = [6, 6, 5, 5, 5, 4, 4, 3, 3, 2, 1];

/// The colours of a scene.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Palette {
    /// The sky from the top to the horizon, and the sea from the horizon down.
    pub sky: [u32; 9],
    pub sea: [u32; 5],
    /// The sun's middle and rim, and the glow its rings mix into the sky.
    pub sun: u32,
    pub rim: u32,
    pub glow: u32,
    /// Far islands on the horizon, in the haze; the island in front, its block seams, and the
    /// light on its sun side.
    pub haze: u32,
    pub land: u32,
    pub seam: u32,
    pub lit: u32,
    /// Glints near the sun and far from it, ripples, clouds (their lit underside and their
    /// top), birds, and stars.
    pub glint: u32,
    pub glint_far: u32,
    pub ripple: u32,
    pub cloud_lit: u32,
    pub cloud: u32,
    pub bird: u32,
    pub star: u32,
}

/// Sunrise's own: night indigo through the violet to amber.
pub const SUNRISE: Palette = Palette {
    sky: [
        0x0b0a20, 0x13113a, 0x211752, 0x371c66, 0x582271, 0x842d77, 0xb54374, 0xe26c60, 0xf6a05c,
    ],
    sea: [0x4a2366, 0x361a54, 0x271342, 0x1b0e33, 0x120a25],
    sun: 0xfff0c2,
    rim: 0xffc070,
    glow: 0xf6a05c,
    haze: 0x5c2c6c,
    land: 0x140b24,
    seam: 0x0d0719,
    lit: 0x9a3f6e,
    glint: 0xffd88e,
    glint_far: 0xd2607a,
    ripple: 0x5a2f7c,
    cloud_lit: 0xf3956c,
    cloud: 0x6b3680,
    bird: 0x1c1030,
    star: 0xcfc8f0,
};

/// Dawn's: plum through rose to peach.
pub const DAWN: Palette = Palette {
    sky: [
        0x170a1b, 0x24102c, 0x3a163c, 0x561d48, 0x7a2850, 0xa23a57, 0xc9535c, 0xe8785f, 0xf8ac74,
    ],
    sea: [0x5e2645, 0x461c39, 0x32142c, 0x230e21, 0x170a17],
    sun: 0xfff3d6,
    rim: 0xffc792,
    glow: 0xf8ac74,
    haze: 0x6e2c4c,
    land: 0x1b0a17,
    seam: 0x12060f,
    lit: 0xb24c63,
    glint: 0xffe0a6,
    glint_far: 0xe0717a,
    ripple: 0x6c2c58,
    cloud_lit: 0xf8a17a,
    cloud: 0x7a3460,
    bird: 0x220c1c,
    star: 0xf0d8e0,
};

/// The still picture: baked for a palette at a size (columns, rows) and scale.
struct Baked {
    key: (Palette, usize, usize, u32),
    image: Arc<RenderImage>,
}

pub struct Coast {
    palette: Palette,
    size: (f32, f32),
    scale: f32,
    baked: Option<Baked>,
    replaced: Vec<Arc<RenderImage>>,
    clock: Clock,
    unfocused: Option<Instant>,
    wake: Option<(Instant, gpui::Task<()>)>,
    _activation: gpui::Subscription,
}

impl Coast {
    pub fn new(window: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> Self {
        Self {
            palette: SUNRISE,
            size: (0., 0.),
            scale: 1.,
            baked: None,
            replaced: Vec::new(),
            clock: Clock::default(),
            unfocused: None,
            wake: None,
            // Back in front, a scene held while the window was behind others moves again.
            _activation: cx.observe_window_activation(window, |_, _, cx| cx.notify()),
        }
    }

    /// The scene to draw: `palette`, `size` logical pixels big at `scale`.
    pub fn set(
        &mut self,
        palette: Palette,
        size: (f32, f32),
        scale: f32,
        cx: &mut gpui::Context<Self>,
    ) {
        if (self.palette, self.size, self.scale) != (palette, size, scale) {
            (self.palette, self.size, self.scale) = (palette, size, scale);
            cx.notify();
        }
    }

    fn wake(&mut self, after: Duration, cx: &mut gpui::Context<Self>) {
        let at = Instant::now() + after;
        if self.wake.as_ref().is_some_and(|(when, _)| *when <= at) {
            return;
        }
        let timer = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            let _ = this.update(cx, |this, cx| {
                this.wake = None;
                cx.notify();
            });
        });
        self.wake = Some((at, timer));
    }
}

impl gpui::Render for Coast {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        for image in self.replaced.drain(..) {
            let _ = window.drop_image(image);
        }
        let (cols, rows) = (
            (self.size.0 / TEXEL).ceil() as usize,
            (self.size.1 / TEXEL).ceil() as usize,
        );
        if cols < 8 || rows < 8 {
            return div();
        }
        let key = (self.palette, cols, rows, (self.scale * 100.).round() as u32);
        if self.baked.as_ref().is_none_or(|b| b.key != key) {
            let texels = still(&self.palette, cols, rows);
            let image = bake(&texels, cols, rows, TEXEL, self.scale);
            if let Some(old) = self.baked.replace(Baked { key, image }) {
                self.replaced.push(old.image);
            }
        }
        let now = Instant::now();
        if window.is_window_active() {
            self.unfocused = None;
        } else {
            self.unfocused.get_or_insert(now);
        }
        let run = match motion() {
            Motion::Still => false,
            Motion::InFront => self.unfocused.is_none_or(|since| now - since < UNFOCUSED),
            Motion::Always => true,
        };
        self.clock.set(run, now);
        let beat = (self.clock.at(now).as_millis() / BEAT.as_millis()) as u32;
        if run {
            self.wake(BEAT, cx);
        }
        let quads = moving(&self.palette, cols, rows, beat);
        let image = self.baked.as_ref().expect("baked above").image.clone();
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .overflow_hidden()
            .child(
                img(image)
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(cols as f32 * TEXEL))
                    .h(px(rows as f32 * TEXEL)),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds: Bounds<Pixels>, (), window, _| {
                        for (x, y, w, c) in &quads {
                            let at = bounds.origin
                                + gpui::point(px(*x as f32 * TEXEL), px(*y as f32 * TEXEL));
                            let size = gpui::size(px(*w as f32 * TEXEL), px(TEXEL));
                            window.paint_quad(gpui::fill(Bounds::new(at, size), *c));
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
    }
}

/// A 4×4 ordered dither's threshold at a texel, 0 to 1.
fn bayer(x: usize, y: usize) -> f32 {
    const M: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    (f32::from(M[y % 4][x % 4]) + 0.5) / 16.
}

/// A stable pseudo-random 0..1 for `i`.
fn hash(i: u32) -> f32 {
    let mut h = i.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b;
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    (h & 0xffff) as f32 / 65535.
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (a, b) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        ((a + (b - a) * t).round().clamp(0., 255.) as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// `stops` across 0..1 at `f`: solid bands, dithered only in a short seam where one meets the
/// next, as pixel art shades a gradient.
fn dithered(stops: &[u32], f: f32, x: usize, y: usize) -> u32 {
    let at = f.clamp(0., 1.) * (stops.len() - 1) as f32;
    let i = (at.floor() as usize).min(stops.len() - 2);
    let seam = ((at - i as f32 - 0.7) / 0.3).clamp(0., 1.);
    if seam > bayer(x, y) {
        stops[i + 1]
    } else {
        stops[i]
    }
}

/// Where the scene's parts are, in texels.
struct Layout {
    sunrise: usize,
    sun: (f32, f32, f32),
    /// The island's foot (its water line) and its right end.
    shore: usize,
    island_end: usize,
}

fn layout(cols: usize, rows: usize) -> Layout {
    let sunrise = (rows as f32 * SUNRISE).round() as usize;
    let r = (rows as f32 * SUN_RADIUS).max(6.);
    Layout {
        sunrise,
        sun: (cols as f32 * SUN_X, sunrise as f32 + r * 0.2, r),
        shore: sunrise + ((rows - sunrise) as f32 * 0.5) as usize,
        island_end: ISLAND.len() * BLOCK,
    }
}

/// The tree's crown, top row first: each row's width, centred on the trunk.
const CROWN: [usize; 7] = [5, 9, 11, 11, 11, 9, 5];
/// The trunk's height in texels, and its column: on the third block.
const TRUNK: usize = 6;
const TRUNK_AT: usize = 2 * BLOCK + 2;

/// Whether texel (`x`, `y`) is the island: its blocks, or the tree on the third one.
fn land(l: &Layout, x: usize, y: usize) -> bool {
    let Some(&blocks) = ISLAND.get(x / BLOCK) else {
        return false;
    };
    if y < l.shore && y + blocks * BLOCK >= l.shore {
        return true;
    }
    let ground = l.shore - ISLAND[TRUNK_AT / BLOCK] * BLOCK;
    let dx = x.abs_diff(TRUNK_AT);
    if dx == 0 && y < ground && y + TRUNK >= ground {
        return true;
    }
    let crown_top = ground - TRUNK - CROWN.len();
    (crown_top..ground - TRUNK).contains(&y) && dx <= (CROWN[y - crown_top] - 1) / 2
}

/// Whether the island's block at (`x`, `y`) shows a seam: the grid between its blocks.
fn seam(l: &Layout, x: usize, y: usize) -> bool {
    x < l.island_end
        && y < l.shore
        && (x.is_multiple_of(BLOCK) || (l.shore - y).is_multiple_of(BLOCK))
}

/// The still picture, `cols` by `rows` texels.
fn still(p: &Palette, cols: usize, rows: usize) -> Vec<[u8; 4]> {
    let l = layout(cols, rows);
    let (sx, sy, r) = l.sun;
    let mut out = vec![[0u8; 4]; cols * rows];
    for y in 0..rows {
        for x in 0..cols {
            let mut c = if y < l.sunrise {
                let mut c = dithered(&p.sky, y as f32 / l.sunrise as f32, x, y);
                // A few dim stars in the night above.
                let f = y as f32 / l.sunrise as f32;
                let n = hash((y * 7919 + x) as u32);
                if f < 0.45 && n > 0.996 - f * 0.004 {
                    c = mix(c, p.star, 0.5);
                }
                c
            } else {
                dithered(
                    &p.sea,
                    (y - l.sunrise) as f32 / (rows - l.sunrise) as f32,
                    x,
                    y,
                )
            };
            let (dx, dy) = (x as f32 + 0.5 - sx, y as f32 + 0.5 - sy);
            let d = (dx * dx + dy * dy).sqrt();
            if y < l.sunrise {
                // The glow: three stepped rings, then the sun, its rim dithered into its middle.
                for (ring, strength) in [(1.8, 0.12), (1.45, 0.22), (1.2, 0.34)] {
                    if d < r * ring {
                        c = mix(c, p.glow, strength);
                    }
                }
                if d < r {
                    c = if d / r > 0.78 + 0.12 * bayer(x, y) {
                        p.rim
                    } else {
                        p.sun
                    };
                }
                // Far islands in the haze, low on the horizon.
                let far = |from: f32, to: f32, high: f32| {
                    let (a, b) = (from * cols as f32, to * cols as f32);
                    let xf = x as f32;
                    if xf < a || xf > b {
                        return false;
                    }
                    let s = ((xf - a) / (b - a) * std::f32::consts::PI).sin();
                    let h = (s * high).round() as usize;
                    y + h >= l.sunrise
                };
                if far(0.24, 0.36, 3.) || far(0.84, 0.97, 4.) {
                    c = p.haze;
                }
            } else {
                // The sun's light lies on the water below it, a column that widens towards
                // the viewer, on every other row.
                let depth = (y - l.sunrise) as f32;
                if dx.abs() < r * 0.55 + depth * 0.22 && (y - l.sunrise).is_multiple_of(2) {
                    c = mix(c, p.glint_far, 0.22);
                }
            }
            // The island: blocks with seams, lit on the sun side; mirrored, dimmer, below.
            if land(&l, x, y) {
                let trunk = x == TRUNK_AT && y + ISLAND[x / BLOCK] * BLOCK < l.shore;
                c = if !land(&l, x + 1, y) && !trunk {
                    p.lit
                } else if seam(&l, x, y) && land(&l, x, y.saturating_sub(1)) {
                    p.seam
                } else {
                    p.land
                };
            } else if y >= l.shore && y.is_multiple_of(2) {
                let mirrored = (l.shore - 1).checked_sub((y - l.shore) * 2);
                if mirrored.is_some_and(|m| land(&l, x, m)) {
                    c = mix(c, p.land, 0.55);
                }
            }
            let [r8, g8, b8] = [(c >> 16) as u8, (c >> 8) as u8, c as u8];
            out[y * cols + x] = [r8, g8, b8, 255];
        }
    }
    out
}

/// What moves at `beat`: runs of texels as (column, row, length, colour).
fn moving(p: &Palette, cols: usize, rows: usize, beat: u32) -> Vec<(usize, usize, usize, Hsla)> {
    let l = layout(cols, rows);
    let (sx, _, r) = l.sun;
    let colour = |c: u32| -> Hsla { gpui::rgb(c).into() };
    let mut out = Vec::new();
    // The sun's glints on the water: short runs in its column, new ones every beat.
    for y in (l.sunrise + 1..rows).step_by(2) {
        let depth = (y - l.sunrise) as f32;
        let half = r * 0.55 + depth * 0.22;
        let near = 1. - depth / (rows - l.sunrise) as f32;
        for j in 0..3u32 {
            let n = hash(y as u32 * 131 + j * 17 + beat * 7);
            if n < 0.35 {
                continue;
            }
            let x = sx + (hash(y as u32 * 53 + j * 29 + beat * 13) - 0.5) * 2. * half;
            let len = 2 + (n * 4.) as usize;
            let c = if hash(y as u32 + j + beat) < near * 0.8 {
                p.glint
            } else {
                p.glint_far
            };
            if x >= 0. && (x as usize) < cols && !over_island(&l, x as usize, y) {
                out.push((x as usize, y, len, colour(c)));
            }
        }
    }
    // Ripples across the rest of the sea, a few at a time, each lasting four beats.
    for i in 0..28u32 {
        let slot = beat / 4 + i * 97;
        let x = (hash(slot * 3) * cols as f32) as usize;
        let y = l.sunrise + 2 + (hash(slot * 5) * (rows - l.sunrise - 2) as f32) as usize;
        if !over_island(&l, x, y) {
            out.push((x, y, 3 + (hash(slot * 11) * 6.) as usize, colour(p.ripple)));
        }
    }
    // Streaks of cloud by the horizon, drifting right: a lit underside under a dimmer top.
    let t = beat as f32 * BEAT.as_secs_f32();
    for (i, (at, len, speed)) in [(0.88f32, 46usize, 1.2f32), (0.94, 30, 0.8), (0.80, 22, 1.6)]
        .into_iter()
        .enumerate()
    {
        let span = (cols + len) as f32;
        let x = ((hash(i as u32 * 41) * span + t * speed) % span) as i32 - len as i32;
        let y = (l.sunrise as f32 * at) as usize;
        for (row, inset, c) in [
            (0usize, 6usize, p.cloud),
            (1, 0, p.cloud_lit),
            (2, 10, p.cloud),
        ] {
            // Far off, the clouds pass behind the island.
            let from = x + inset as i32;
            let w = len.saturating_sub(inset * 2);
            let (from, w) = clip(from, w, cols);
            let end = from + w;
            let from = from.max(l.island_end + 1);
            let w = end.saturating_sub(from);
            if w > 0 {
                out.push((from, y + row, w, colour(c)));
            }
        }
    }
    // Birds flapping low over the water, right to left, wings up and down by turns.
    const UP: [(i32, i32); 7] = [(0, 0), (1, 0), (2, 1), (3, 2), (4, 1), (5, 0), (6, 0)];
    const DOWN: [(i32, i32); 7] = [(0, 2), (1, 1), (2, 1), (3, 2), (4, 1), (5, 1), (6, 2)];
    for i in 0..3u32 {
        let span = cols as f32 + 40.;
        let x = span - ((hash(i * 23) * span + t * (5. + i as f32)) % span) - 20.;
        let y = l.sunrise as f32 * (0.84 + 0.045 * i as f32) + (t * 0.8 + i as f32).sin() * 1.5;
        let wings = if (beat + i * 2) % 4 < 2 { &UP } else { &DOWN };
        for &(dx, dy) in wings {
            let (bx, by) = (x as i32 + dx, y as i32 + dy);
            if bx >= 0 && (bx as usize) < cols && by >= 0 {
                out.push((bx as usize, by as usize, 1, colour(p.bird)));
            }
        }
    }
    // Stars twinkling in the night above, a few lit at a time.
    for i in 0..24u32 {
        if hash(i * 7 + beat / 3) < 0.6 {
            continue;
        }
        let x = (hash(i * 13) * cols as f32) as usize;
        let y = (hash(i * 19) * l.sunrise as f32 * 0.4) as usize;
        out.push((x, y, 1, colour(p.star)));
    }
    out
}

/// Whether texel (`x`, `y`) lies on the island or its reflection, which nothing moving covers.
fn over_island(l: &Layout, x: usize, y: usize) -> bool {
    x <= l.island_end && y <= l.shore + ISLAND[0] * BLOCK / 2
}

/// A run from `from` (maybe off the left edge) `w` long, cut to `cols`.
fn clip(from: i32, w: usize, cols: usize) -> (usize, usize) {
    let start = from.max(0) as usize;
    let end = ((from + w as i32).max(0) as usize).min(cols);
    (start, end.saturating_sub(start))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_still_picture_fills_its_texels() {
        let (cols, rows) = (180, 120);
        let texels = still(&SUNRISE, cols, rows);
        assert_eq!(texels.len(), cols * rows);
        assert!(texels.iter().all(|t| t[3] == 255));
    }

    #[test]
    fn what_moves_stays_on_the_picture() {
        let (cols, rows) = (180, 120);
        for beat in 0..200 {
            for (x, y, w, _) in moving(&DAWN, cols, rows, beat) {
                assert!(x < cols && y < rows + 4 && w > 0, "{x},{y},{w}");
            }
        }
    }
}
