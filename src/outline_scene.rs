//! Home's hero scene (Figma: Components → Hero scene), alive the way mistral.ai's hero is: equal
//! framed columns on both sides with hairlines, a floor line and a faint arc; Dusk blocks rise out
//! of the floor into mirrored towers, Sunny pops onto the left tower, the claw and Ferris (the Rust
//! crab, public domain) appear. Then the scene keeps rearranging itself in short moves with rests
//! between them: a block slides out and the one above drops, the diamond tumbles over into a
//! square, the right tower lifts on a block rising out of the floor, the claw lowers and lifts,
//! Ferris walks a stretch, a block wobbles. Every move is undone later in the cycle, so it loops
//! without a seam.
//!
//! It is a view of its own behind the launcher ([`crate::window_root::Backdrop`]), placed over
//! Home's first screen: its frames draw the scene alone, never the launcher, and only while
//! something moves. Under reduced motion, or while the window is inactive, it rests as built.
use crate::glass_ui::white;
use gpui::{
    canvas, div, point, prelude::*, px, rgb, Bounds, Context, Div, Hsla, IntoElement, PathBuilder,
    Pixels, Render, Window,
};
use std::time::{Duration, Instant};

/// A side column's width.
pub const COLUMN: f32 = 168.;
const BLOCK: f32 = 72.;
const PIXEL: f32 = 4.;

const SUN: u32 = 0xf2b862;
const AMBER: u32 = 0xe0873a;
const VIOLET: u32 = 0x6e64c8;
const PLUM: u32 = 0xb0477d;
const DEEP: u32 = 0x4a3f86;

/// The build: when each row of blocks starts rising and how long it takes; the sprites follow.
const ROW_MS: f32 = 350.;
const RISE_MS: f32 = 450.;
const BUILT_MS: f32 = 3. * ROW_MS + RISE_MS + 150.;
const APPEAR_MS: f32 = 300.;
/// Below this many ms of rest the view keeps drawing; above it, it sleeps until the next move.
const FRAME_MS: u64 = 16;

/// How far the claw's cable reaches up and down.
const CABLE_UP: f32 = 102.;
const CABLE_DOWN: f32 = 212.;
/// Where Ferris stands, from the page's left, and how far it walks.
const FERRIS_HOME: f32 = 280.;
const FERRIS_WALK: f32 = 136.;

const SUNNY: [&str; 12] = [
    "...b....b....b..",
    "....b...b...b...",
    "................",
    ".....cccccc.....",
    "....cbbbbbbc....",
    "...cbbddddbbc...",
    "..cbbbddddbbbc..",
    "..cbbbbbbbbbbc..",
    "..eeeeeeeeeeee..",
    "................",
    ".ffffffffffffff.",
    ".gggggggggggggg.",
];
const CLAW: [&str; 8] = [
    "....bb....",
    "....cc....",
    "..bccccb..",
    ".cbbbbbc..",
    ".c......c.",
    "c........c",
    "c........c",
    ".b......b.",
];
const FERRIS: [&str; 12] = [
    "..bbb..........bbb..",
    ".bccdb........bddeb.",
    ".bcddb........bddeb.",
    "..bdb..........bdb..",
    "...bb.bbbbbbbb.bb...",
    "....bccddddddddeb...",
    "...bcdfgddddfgddeb..",
    "...bcdffddddffddeb..",
    "...bdddddbbdddddeb..",
    "....beddddddddeeb...",
    "...b.bbbbbbbbbb.b...",
    "..b..b..b..b..b..b..",
];

/// The things that move: the blocks by index into [`BLOCKS`], then the sprites.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    Block(usize),
    Claw,
    Ferris,
}

/// A block at rest: its side, how far in from that side's edge and up from the floor, its
/// size, colour and turn (45° is a diamond), and the build row it rises with (none: it starts
/// hidden, below the floor or past the edge).
struct Block {
    right: bool,
    side: f32,
    bottom: f32,
    size: f32,
    colour: u32,
    turn: f32,
    row: Option<f32>,
}

const fn block(right: bool, side: f32, bottom: f32, colour: u32, row: f32) -> Block {
    Block {
        right,
        side,
        bottom,
        size: BLOCK,
        colour,
        turn: 0.,
        row: Some(row),
    }
}

// The left tower is blocks 0 (A1, the base) to 2; B1 the diamond beside it.
const A2: usize = 1;
const A3: usize = 2;
const B1: usize = 3;
const D1: usize = 4;
const D2: usize = 5;
const D3: usize = 6;
const D4: usize = 7;
const C1: usize = 8;
const C2: usize = 9;
/// Waits under the right tower, below the floor, until it lifts the tower.
const LIFT: usize = 10;

const BLOCKS: [Block; 11] = [
    block(false, 0., 0., SUN, 0.),
    block(false, 0., BLOCK, AMBER, 1.),
    block(false, 0., 2. * BLOCK, VIOLET, 2.),
    Block {
        turn: 45.,
        ..block(false, BLOCK, 0., PLUM, 0.)
    },
    block(true, 0., 0., DEEP, 0.),
    block(true, 0., BLOCK, VIOLET, 1.),
    block(true, 0., 2. * BLOCK, PLUM, 2.),
    Block {
        turn: 45.,
        ..block(true, 0., 3. * BLOCK, SUN, 2.)
    },
    block(true, BLOCK, 0., SUN, 0.),
    Block {
        size: 36.,
        ..block(true, 90., BLOCK, AMBER, 1.)
    },
    Block {
        row: None,
        ..block(true, 0., -BLOCK, AMBER, 0.)
    },
];

/// One move of the cycle: which parts shift by how much (inwards from their side, up, and in
/// degrees), how long it takes, and the rest after it.
struct Move {
    parts: &'static [Part],
    by: (f32, f32, f32),
    ms: f32,
    rest: f32,
    /// A wobble: out and back within the move, ending where it began.
    there_and_back: bool,
}

const fn shift(parts: &'static [Part], by: (f32, f32, f32), ms: f32, rest: f32) -> Move {
    Move {
        parts,
        by,
        ms,
        rest,
        there_and_back: false,
    }
}

const TOWER: &[Part] = &[
    Part::Block(LIFT),
    Part::Block(D1),
    Part::Block(D2),
    Part::Block(D3),
    Part::Block(D4),
];
const SUNNY_BLOCK: &[Part] = &[Part::Block(A3)];
const STACK: &[Part] = &[Part::Block(C1), Part::Block(C2)];

/// The cycle, after the build. Each move is undone later, so the scene ends where it began.
const MOVES: [Move; 15] = [
    shift(TOWER, (0., BLOCK, 0.), 520., 1600.),
    shift(&[Part::Block(A2)], (-BLOCK, 0., 0.), 480., 700.),
    shift(SUNNY_BLOCK, (0., -BLOCK, 0.), 420., 1800.),
    shift(&[Part::Block(B1)], (BLOCK, 0., 45.), 620., 1400.),
    shift(&[Part::Claw], (0., CABLE_DOWN - CABLE_UP, 0.), 900., 1500.),
    shift(STACK, (BLOCK, 0., 0.), 500., 1200.),
    shift(&[Part::Ferris], (FERRIS_WALK, 0., 0.), 2000., 1300.),
    Move {
        there_and_back: true,
        ..shift(&[Part::Block(C2)], (0., 0., 22.), 560., 1500.)
    },
    shift(TOWER, (0., -BLOCK, 0.), 520., 1200.),
    shift(&[Part::Claw], (0., CABLE_UP - CABLE_DOWN, 0.), 900., 900.),
    shift(SUNNY_BLOCK, (0., BLOCK, 0.), 420., 300.),
    shift(&[Part::Block(A2)], (BLOCK, 0., 0.), 480., 1500.),
    shift(&[Part::Block(B1)], (-BLOCK, 0., -45.), 620., 1000.),
    shift(STACK, (-BLOCK, 0., 0.), 500., 900.),
    shift(&[Part::Ferris], (-FERRIS_WALK, 0., 0.), 2000., 1600.),
];

/// Where Home's first screen is in the window, as the launcher's last frame laid it out.
#[derive(Clone, Copy, PartialEq)]
pub struct Place {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    /// There is room for the columns beside the hero; narrower, only the floor is drawn.
    pub wide: bool,
}

/// The scene's view: where to draw it, when its build started, and whether its next frame is
/// already asked for.
pub struct Hero {
    place: Option<Place>,
    since: Instant,
    waiting: bool,
}

impl Hero {
    pub fn new() -> Self {
        Hero {
            place: None,
            since: Instant::now(),
            waiting: false,
        }
    }

    /// Called from the launcher's frame: `None` away from Home.
    pub fn set(&mut self, place: Option<Place>, cx: &mut Context<Self>) {
        if self.place != place {
            self.place = place;
            cx.notify();
        }
    }

    /// Builds the scene again from its first block, as Home is opened.
    pub fn replay(&mut self, cx: &mut Context<Self>) {
        self.since = Instant::now();
        cx.notify();
    }
}

impl Render for Hero {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(place) = self.place else {
            return div();
        };
        let live = !crate::motion::still() && window.is_window_active();
        let ms = if live {
            self.since.elapsed().as_secs_f32() * 1000.
        } else {
            BUILT_MS + APPEAR_MS
        };
        let now = state(ms);
        // Smooth frames while something moves; between moves, sleep until the next one.
        if live && !self.waiting {
            self.waiting = true;
            let wait = now.idle_ms.max(FRAME_MS as f32) as u64;
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(wait))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.waiting = false;
                    cx.notify();
                });
            })
            .detach();
        }
        div()
            .absolute()
            .left(px(place.left))
            .top(px(place.top))
            .w(px(place.width))
            .h(px(place.height))
            .child(scene(&now, place))
    }
}

/// Every part's shift at one moment, how far the build has come, and how long until something
/// next moves (0 while something does).
struct State {
    shift: [(f32, f32, f32); BLOCKS.len() + 2],
    built: f32,
    shown: f32,
    walking: Option<bool>,
    idle_ms: f32,
}

fn slot(part: Part) -> usize {
    match part {
        Part::Block(i) => i,
        Part::Claw => BLOCKS.len(),
        Part::Ferris => BLOCKS.len() + 1,
    }
}

/// The moves' sum at `ms` after the build began: those done in full, the current one eased.
fn state(ms: f32) -> State {
    let mut s = State {
        shift: [(0., 0., 0.); BLOCKS.len() + 2],
        built: ms,
        shown: ((ms - BUILT_MS) / APPEAR_MS).clamp(0., 1.),
        walking: None,
        idle_ms: 0.,
    };
    let start = BUILT_MS + APPEAR_MS;
    if ms < start {
        return s;
    }
    let cycle: f32 = MOVES.iter().map(|m| m.ms + m.rest).sum();
    let mut t = (ms - start) % cycle;
    for m in &MOVES {
        let p = if t >= m.ms {
            1.
        } else {
            let p = t / m.ms;
            if m.parts == [Part::Ferris] {
                s.walking = Some(m.by.0 < 0.);
                // Whole steps: Ferris moves on its pixels, not in a glide.
                (p * m.ms / 125.).floor() * 125. / m.ms
            } else {
                in_out(p)
            }
        };
        let p = if m.there_and_back {
            (p * std::f32::consts::PI).sin() * if t >= m.ms { 0. } else { 1. }
        } else {
            p
        };
        for &part in m.parts {
            let d = &mut s.shift[slot(part)];
            d.0 += m.by.0 * p;
            d.1 += m.by.1 * p;
            d.2 += m.by.2 * p;
        }
        if t < m.ms {
            return s;
        }
        t -= m.ms;
        if t < m.rest {
            s.idle_ms = m.rest - t;
            return s;
        }
        t -= m.rest;
    }
    s
}

/// Cubic ease in and out, as the moves start and settle.
fn in_out(t: f32) -> f32 {
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

/// The scene as `now` has it.
fn scene(now: &State, place: Place) -> Div {
    let floor = div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(1.))
        .bg(white(0.10));
    let scene = div().absolute().size_full().overflow_hidden().child(floor);
    if !place.wide {
        return scene;
    }
    let column = |right: bool| {
        let d = div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(1.))
            .bg(white(0.07));
        if right {
            d.right(px(COLUMN))
        } else {
            d.left(px(COLUMN))
        }
    };
    // The arc: a faint circle around the left column's top corner.
    let arc = div()
        .absolute()
        .left(px(COLUMN - 310.))
        .top(px(-310.))
        .size(px(620.))
        .rounded_full()
        .border_1()
        .border_color(white(0.06));
    let blocks = blocks(now);
    let sunny_lift = now.shift[A3].1;
    let sunny = sprite(&SUNNY, false, |c| match c {
        'b' => Some(0xffd978),
        'c' => Some(SUN),
        'd' => Some(0xfff3c4),
        'e' => Some(AMBER),
        'f' => Some(VIOLET),
        'g' => Some(DEEP),
        _ => None,
    })
    .absolute()
    .left(px(4.))
    .bottom(px(3. * BLOCK + sunny_lift))
    .opacity(now.shown);
    scene
        .child(arc)
        .child(column(false))
        .child(column(true))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| paint_blocks(bounds, &blocks, window),
            )
            .absolute()
            .size_full(),
        )
        .child(sunny)
        .child(crane(now))
        .child(ferris(now))
}

/// Each block's colour, centre (from the scene's bottom left) and turn, lower rows last so a
/// block rising through them stays behind.
fn blocks(now: &State) -> Vec<(u32, f32, f32, f32, f32, bool)> {
    let mut out: Vec<_> = BLOCKS
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let (dx, dy, turn) = now.shift[i];
            // The build: each row rises out of the floor in turn.
            let rise = match b.row {
                Some(row) => {
                    -(b.bottom + b.size) * (1. - ease((now.built - row * ROW_MS) / RISE_MS))
                }
                None => 0.,
            };
            let inward = b.side + dx + b.size / 2.;
            let up = b.bottom + dy + rise + b.size / 2.;
            (b.colour, inward, up, b.size, b.turn + turn, b.right)
        })
        .collect();
    out.sort_by(|a, b| b.2.total_cmp(&a.2));
    out
}

fn paint_blocks(
    bounds: Bounds<Pixels>,
    blocks: &[(u32, f32, f32, f32, f32, bool)],
    window: &mut Window,
) {
    let (o, w, h) = (bounds.origin, bounds.size.width, bounds.size.height);
    for &(colour, inward, up, size, turn, right) in blocks {
        let cx = if right { w - px(inward) } else { px(inward) };
        let cy = h - px(up);
        let (sin, cos) = turn.to_radians().sin_cos();
        let r = size / 2.;
        let mut path = PathBuilder::fill();
        for (k, (x, y)) in [(-r, -r), (r, -r), (r, r), (-r, r)].into_iter().enumerate() {
            let p = point(
                o.x + cx + px(x * cos - y * sin),
                o.y + cy + px(x * sin + y * cos),
            );
            if k == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        path.close();
        if let Ok(path) = path.build() {
            window.paint_path(path, rgb(colour));
        }
    }
}

/// The design's settling curve on `t`, clamped to 0..1.
fn ease(t: f32) -> f32 {
    crate::motion::PAGE.ease(t.clamp(0., 1.))
}

/// The claw in the left column on its cable from the top line, holding a block.
fn crane(now: &State) -> Div {
    let cable = CABLE_UP + now.shift[slot(Part::Claw)].1;
    div()
        .absolute()
        .top_0()
        .left(px(64.))
        .w(px(40.))
        .h(px(cable + 58.))
        .opacity(now.shown)
        .child(
            div()
                .absolute()
                .top_0()
                .left(px(19.))
                .w(px(2.))
                .h(px(cable))
                .bg(white(0.25)),
        )
        .child(
            div()
                .absolute()
                .top(px(cable + 20.))
                .left(px(2.))
                .size(px(36.))
                .bg(rgb(SUN)),
        )
        .child(
            sprite(&CLAW, false, |c| match c {
                'b' => Some(0x5d6b7a),
                'c' => Some(0xa9b6c6),
                _ => None,
            })
            .absolute()
            .top(px(cable - 2.)),
        )
}

/// Ferris on the floor, facing the way it walks, a pixel up on every other step.
fn ferris(now: &State) -> Div {
    let x = FERRIS_HOME + now.shift[slot(Part::Ferris)].0;
    let left = now.walking.unwrap_or(false);
    let lift = match now.walking {
        Some(_) if ((x / PIXEL) as usize / 2) % 2 == 1 => PIXEL,
        _ => 0.,
    };
    sprite(&FERRIS, left, |c| match c {
        'b' => Some(0x4a1500),
        'c' => Some(0xff9a5a),
        'd' => Some(0xf25a12),
        'e' => Some(0xb83a00),
        'f' => Some(0x0c0b11),
        'g' => Some(0xffffff),
        _ => None,
    })
    .absolute()
    .left(px((x / PIXEL).round() * PIXEL))
    .bottom(px(lift))
    .opacity(now.shown)
}

/// Pixel art from rows of letters at 4 px a pixel (mirrored when `flip`), each run of one colour
/// a single box.
fn sprite(rows: &[&str], flip: bool, colour: impl Fn(char) -> Option<u32>) -> Div {
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut d = div()
        .flex_none()
        .w(px(width as f32 * PIXEL))
        .h(px(rows.len() as f32 * PIXEL));
    for (y, row) in rows.iter().enumerate() {
        let mut chars: Vec<char> = row.chars().collect();
        if flip {
            chars.reverse();
        }
        let mut x = 0;
        while x < chars.len() {
            let c = chars[x];
            let run = chars[x..].iter().take_while(|&&k| k == c).count();
            if let Some(fill) = colour(c) {
                d = d.child(
                    div()
                        .absolute()
                        .left(px(x as f32 * PIXEL))
                        .top(px(y as f32 * PIXEL))
                        .w(px(run as f32 * PIXEL))
                        .h(px(PIXEL))
                        .bg(Hsla::from(rgb(fill))),
                );
            }
            x += run;
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cycle undoes every move it makes, so the scene loops without a jump.
    #[test]
    fn a_cycle_ends_where_it_began() {
        let cycle: f32 = MOVES.iter().map(|m| m.ms + m.rest).sum();
        let start = BUILT_MS + APPEAR_MS;
        let end = state(start + cycle - 1.);
        for (i, d) in end.shift.iter().enumerate() {
            assert!(
                d.0.abs() < 0.01 && d.1.abs() < 0.01 && d.2.abs() < 0.01,
                "part {i} ends at {d:?}"
            );
        }
    }
}
