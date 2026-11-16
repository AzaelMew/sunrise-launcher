//! Four textured blocks in Home's bottom-left corner, playing one short move every few seconds:
//! the corner block changes colour; the block on top sinks into it and later rises out again; the
//! block on its right turns into a diamond and back, or slides over the corner block (which takes
//! its colour) and later back out, turning into a diamond as it lands; while it is inside, the
//! small block can slide over to the corner block and back; the small block hops. Between moves
//! nothing is drawn again, so the corner costs nothing while it rests.
use crate::glass_ui::{color, pixel_fill, texel_noise, texel_shades, TEXEL, TEXTURE_BUTTON};
use gpui::{
    canvas, div, prelude::*, px, AnimationExt, Context, Div, ElementId, IntoElement, Render, Window,
};
use std::f32::consts::{FRAC_1_SQRT_2, FRAC_PI_4};
use std::time::{Duration, Instant};

/// A block's side as a share of the window's height, and at least and at most; the small one is
/// half as wide and hops a sixth of a block.
const SIDE_SHARE: f32 = 0.1;
const SIDE_MIN: f32 = 56.;
const SIDE_MAX: f32 = 128.;
/// The corner block's own colours, in the order it takes them, and the others': the launcher's
/// blues, from the deep indigo to the light accent text, and its slate.
const CORNER: [u32; 3] = [0x4f60e6, 0x3d4cc4, 0x6a7af0];
const TOP: u32 = 0x2c3a9e;
const RIGHT: u32 = 0x93b4ff;
const SMALL_COLOUR: u32 = 0x6b7399;
/// How long the corner rests between moves, at least and at most, and before the turn that
/// always follows sliding out.
const REST_MIN: u64 = 1800;
const REST_MAX: u64 = 4200;
const REST_CHAINED: u64 = 120;
/// How long the right block stands as a diamond before it turns back.
const REST_TURNED: u64 = 1500;
const MOVE: sunrise_theme::Motion = sunrise_theme::Motion {
    name: "corner-block",
    ms: 460,
    curve: [0.6, 0., 0.3, 1.],
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Move {
    Tint,
    TopSink,
    TopRise,
    Cover,
    Uncover,
    Turn,
    Unturn,
    Attach,
    Detach,
    Hop,
}

pub struct CornerBlocks {
    /// Which of [`CORNER`] the corner block wears under the right block.
    corner: usize,
    top_up: bool,
    /// The right block lies over the corner block.
    covered: bool,
    /// The right block stands turned, a diamond in its place.
    turned: bool,
    /// The small block sits against the corner block, in the right block's place.
    attached: bool,
    /// The move playing, its number (each one's animation is named by it), and when it began.
    playing: Option<(Move, usize, Instant)>,
    /// A move that must come next, and the rest before it.
    then: Option<(Move, u64)>,
    moves: usize,
    seed: u64,
}

impl CornerBlocks {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| loop {
            let rest = this.update(cx, |b, _| b.rest()).unwrap_or(REST_MAX);
            cx.background_executor()
                .timer(Duration::from_millis(rest))
                .await;
            if this.update(cx, |b, cx| b.start(cx)).is_err() {
                break;
            }
            cx.background_executor().timer(MOVE.duration()).await;
            if this.update(cx, |b, cx| b.finish(cx)).is_err() {
                break;
            }
        })
        .detach();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64)
            | 1;
        Self {
            corner: 0,
            top_up: true,
            covered: false,
            turned: false,
            attached: false,
            playing: None,
            then: None,
            moves: 0,
            seed,
        }
    }

    /// The next of a fixed pseudo-random sequence (xorshift).
    fn next(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    fn rest(&mut self) -> u64 {
        if let Some((_, rest)) = self.then {
            return rest;
        }
        REST_MIN + self.next() % (REST_MAX - REST_MIN)
    }

    /// The moves the blocks can make from where they are.
    fn moves_from_here(&self) -> Vec<Move> {
        let mut can = vec![if self.top_up {
            Move::TopSink
        } else {
            Move::TopRise
        }];
        if !self.attached {
            can.push(Move::Hop);
        }
        match (self.covered, self.turned, self.attached) {
            (true, _, true) => can.push(Move::Detach),
            (true, _, false) => can.extend([Move::Attach, Move::Uncover]),
            (false, true, _) => can.extend([Move::Unturn, Move::Tint]),
            (false, false, _) => can.extend([Move::Cover, Move::Turn, Move::Tint]),
        }
        can
    }

    /// Picks a move the blocks can make from where they are, and plays it.
    fn start(&mut self, cx: &mut Context<Self>) {
        let pick = match self.then.take() {
            Some((next, _)) => next,
            None => {
                let can = self.moves_from_here();
                can[(self.next() % can.len() as u64) as usize]
            }
        };
        self.moves += 1;
        if crate::motion::still() {
            self.apply(pick);
        } else {
            self.playing = Some((pick, self.moves, Instant::now()));
        }
        cx.notify();
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        if let Some((done, _, _)) = self.playing.take() {
            self.apply(done);
            cx.notify();
        }
    }

    fn apply(&mut self, done: Move) {
        match done {
            Move::Tint => self.corner = (self.corner + 1) % CORNER.len(),
            Move::TopSink => self.top_up = false,
            Move::TopRise => self.top_up = true,
            Move::Cover => self.covered = true,
            Move::Uncover => {
                self.covered = false;
                // Sliding out, it lands turning.
                self.then = Some((Move::Turn, REST_CHAINED));
            }
            Move::Turn => {
                self.turned = true;
                self.then = Some((Move::Unturn, REST_TURNED));
            }
            Move::Unturn => self.turned = false,
            Move::Attach => self.attached = true,
            Move::Detach => self.attached = false,
            Move::Hop => {}
        }
    }
}

/// A textured block of `colour`, `side` px square, placed by its left and bottom edges.
fn block(colour: u32, side: f32, left: f32, bottom: f32) -> Div {
    div()
        .absolute()
        .left(px(left))
        .bottom(px(bottom))
        .size(px(side))
        .overflow_hidden()
        .child(pixel_fill(color(colour), TEXTURE_BUTTON))
}

/// The right block `turn` of the way (0 to 1) from a square to a diamond: turned 45° and shrunk
/// until its corners touch its cell's sides. GPUI can't turn an element, so it is drawn here: one
/// straight-edged shape in the block's colour, with the texture's texels wherever they lie wholly
/// inside it.
fn turned_block(colour: u32, side: f32, turn: f32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let base = color(colour);
            let shades = texel_shades(base, TEXTURE_BUTTON);
            let (sin, cos) = (turn * FRAC_PI_4).sin_cos();
            let half = side / 2. * (1. - (1. - FRAC_1_SQRT_2) * turn);
            let centre = bounds.origin + gpui::point(px(side / 2.), px(side / 2.));
            // A point of the block's own frame (u along its side, v down it), on the page.
            let place =
                |u: f32, v: f32| centre + gpui::point(px(u * cos - v * sin), px(u * sin + v * cos));
            let mut shape = gpui::PathBuilder::fill();
            shape.move_to(place(-half, -half));
            shape.line_to(place(half, -half));
            shape.line_to(place(half, half));
            shape.line_to(place(-half, half));
            shape.close();
            if let Ok(shape) = shape.build() {
                window.paint_path(shape, base);
            }
            let inside = |x: f32, y: f32| {
                let (x, y) = (x - side / 2., y - side / 2.);
                let (u, v) = (x * cos + y * sin, y * cos - x * sin);
                u.abs() <= half && v.abs() <= half
            };
            let whole = |x: u32, y: u32| {
                let (x0, y0) = (x as f32 * TEXEL, y as f32 * TEXEL);
                [
                    (x0, y0),
                    (x0 + TEXEL, y0),
                    (x0, y0 + TEXEL),
                    (x0 + TEXEL, y0 + TEXEL),
                ]
                .into_iter()
                .all(|(x, y)| inside(x, y))
            };
            let shade = |x: u32, y: u32| shades.get((texel_noise(x, y) % 9) as usize).copied();
            let cells = (side / TEXEL) as u32;
            for y in 0..cells {
                let mut x = 0;
                while x < cells {
                    let Some(this) = shade(x, y).filter(|_| whole(x, y)) else {
                        x += 1;
                        continue;
                    };
                    // A run of one shade along the row is one quad.
                    let start = x;
                    while x < cells && whole(x, y) && shade(x, y) == Some(this) {
                        x += 1;
                    }
                    let at = gpui::point(
                        bounds.origin.x + px(start as f32 * TEXEL),
                        bounds.origin.y + px(y as f32 * TEXEL),
                    );
                    let size = gpui::size(px((x - start) as f32 * TEXEL), px(TEXEL));
                    window.paint_quad(gpui::fill(gpui::Bounds::new(at, size), this));
                }
            }
        },
    )
    .absolute()
    .left(px(side))
    .bottom_0()
    .size(px(side))
}

impl Render for CornerBlocks {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let playing = self.playing;
        let is = |m: Move| playing.filter(|(p, _, _)| *p == m).map(|(_, n, _)| n);
        let key = |n: usize| ElementId::named_usize("corner-block", n);
        let own = CORNER[self.corner];
        // Sized by the window, on whole texels so the texture stays square.
        let height = f32::from(window.viewport_size().height);
        let side =
            ((height * SIDE_SHARE).clamp(SIDE_MIN, SIDE_MAX) / (2. * TEXEL)).round() * 2. * TEXEL;
        let (small_side, hop) = (side / 2., side / 6.);
        let mut d = div()
            .relative()
            .w(px(2. * side + small_side))
            .h(px(2. * side));

        // The corner: the right block's colour while covered, else its own (already while the
        // right block slides back off it), fading to the next.
        let covered = self.covered && is(Move::Uncover).is_none();
        d = d.child(block(if covered { RIGHT } else { own }, side, 0., 0.));
        if let Some(n) = is(Move::Tint) {
            let next = CORNER[(self.corner + 1) % CORNER.len()];
            d = d.child(block(next, side, 0., 0.).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                move |d, t| d.opacity(t),
            ));
        }

        // The top block, sinking into the corner block or rising out of it, from its top edge.
        let top = |h: f32| block(TOP, side, 0., side).h(px(h));
        d = match (is(Move::TopSink), is(Move::TopRise)) {
            (Some(n), _) => d.child(top(side).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                move |d, t| d.h(px(side * (1. - t))),
            )),
            (_, Some(n)) => d.child(top(0.).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                move |d, t| d.h(px(side * t)),
            )),
            _ if self.top_up => d.child(top(side)),
            _ => d,
        };

        // The right block: sliding over the corner block or back out, turning, or at rest.
        let turning = playing.and_then(|(m, _, at)| match m {
            Move::Turn => Some((at, false)),
            Move::Unturn => Some((at, true)),
            _ => None,
        });
        d = if let Some(n) = is(Move::Cover) {
            d.child(block(RIGHT, side, side, 0.).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                move |d, t| d.left(px(side * (1. - t))),
            ))
        } else if let Some(n) = is(Move::Uncover) {
            d.child(block(RIGHT, side, 0., 0.).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                move |d, t| d.left(px(side * t)),
            ))
        } else if let Some((at, back)) = turning {
            // The turn is drawn texel by texel, so it is played here frame by frame.
            let t = (at.elapsed().as_secs_f32() / MOVE.duration().as_secs_f32()).min(1.);
            if t < 1. {
                window.request_animation_frame();
            }
            let t = MOVE.ease(t);
            d.child(turned_block(RIGHT, side, if back { 1. - t } else { t }))
        } else if self.turned {
            d.child(turned_block(RIGHT, side, 1.))
        } else if !self.covered {
            d.child(block(RIGHT, side, side, 0.))
        } else {
            d
        };

        // The small block: at home, or against the corner block, sliding between, or hopping.
        let (home, beside) = (2. * side, side);
        let small = |left: f32| block(SMALL_COLOUR, small_side, left, 0.);
        d.child(if let Some(n) = is(Move::Hop) {
            small(home)
                .with_animation(key(n), crate::motion::eased(MOVE), move |d, t| {
                    d.bottom(px(hop * 4. * t * (1. - t)))
                })
                .into_any_element()
        } else if let Some(n) = is(Move::Attach) {
            small(home)
                .with_animation(key(n), crate::motion::eased(MOVE), move |d, t| {
                    d.left(px(home + (beside - home) * t))
                })
                .into_any_element()
        } else if let Some(n) = is(Move::Detach) {
            small(beside)
                .with_animation(key(n), crate::motion::eased(MOVE), move |d, t| {
                    d.left(px(beside + (home - beside) * t))
                })
                .into_any_element()
        } else {
            small(if self.attached { beside } else { home }).into_any_element()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks() -> CornerBlocks {
        CornerBlocks {
            corner: 0,
            top_up: true,
            covered: false,
            turned: false,
            attached: false,
            playing: None,
            then: None,
            moves: 0,
            seed: 0x2545_f491_4f6c_dd1d,
        }
    }

    #[test]
    fn every_move_is_one_the_blocks_can_make_and_sliding_out_lands_turning() {
        let mut b = blocks();
        for _ in 0..5000 {
            let pick = match b.then.take() {
                Some((next, _)) => next,
                None => {
                    let can = b.moves_from_here();
                    can[(b.next() % can.len() as u64) as usize]
                }
            };
            match pick {
                Move::Uncover => assert!(b.covered && !b.attached),
                Move::Cover => assert!(!b.covered && !b.turned),
                Move::Turn => assert!(!b.covered && !b.turned),
                Move::Unturn => assert!(b.turned),
                Move::Attach => assert!(b.covered && !b.attached),
                Move::Detach => assert!(b.attached),
                Move::Tint => assert!(!b.covered),
                Move::Hop => assert!(!b.attached),
                Move::TopSink => assert!(b.top_up),
                Move::TopRise => assert!(!b.top_up),
            }
            b.apply(pick);
            if pick == Move::Uncover {
                assert_eq!(b.then.map(|(m, _)| m), Some(Move::Turn));
            }
            if pick == Move::Turn {
                assert_eq!(b.then.map(|(m, _)| m), Some(Move::Unturn));
            }
            assert!(
                !b.attached || b.covered,
                "the small block only sits in a free place"
            );
        }
    }
}
