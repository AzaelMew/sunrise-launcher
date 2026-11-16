//! Four textured blocks in Home's bottom-left corner, playing one short move every few seconds:
//! the corner block changes colour; the block on top sinks into it and later rises out again; the
//! block on its right slides over it (the corner takes its colour) and later slides back out
//! (the corner gets its own colour back); the small block hops. Between moves nothing is drawn
//! again, so the corner costs nothing while it rests.
use crate::glass_ui::{color, pixel_fill, TEXTURE_BUTTON};
use gpui::{
    div, prelude::*, px, AnimationExt, Context, Div, ElementId, IntoElement, Render, Window,
};
use std::time::Duration;

/// A block's side, and the small one's.
const SIDE: f32 = 48.;
const SMALL: f32 = 24.;
/// How high the small block hops.
const HOP: f32 = 10.;
/// The corner block's own colours, in the order it takes them.
const CORNER: [u32; 3] = [0x5cc4e8, 0x4cd08a, 0x93b4ff];
const TOP: u32 = 0xb79bff;
const RIGHT: u32 = 0xff8fb4;
const SMALL_COLOUR: u32 = 0x4f60e6;
/// How long the corner rests between moves, at least and at most.
const REST_MIN: u64 = 1800;
const REST_MAX: u64 = 4200;
const MOVE: sunrise_theme::Motion = sunrise_theme::Motion {
    name: "corner-block",
    ms: 460,
    curve: [0.6, 0., 0.3, 1.],
};

#[derive(Clone, Copy, PartialEq)]
enum Move {
    Tint,
    TopSink,
    TopRise,
    Cover,
    Uncover,
    Hop,
}

pub struct CornerBlocks {
    /// Which of [`CORNER`] the corner block wears under the right block.
    corner: usize,
    top_up: bool,
    /// The right block lies over the corner block.
    covered: bool,
    /// The move playing and its number (each one's animation is named by it).
    playing: Option<(Move, usize)>,
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
            playing: None,
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
        REST_MIN + self.next() % (REST_MAX - REST_MIN)
    }

    /// Picks a move the blocks can make from where they are, and plays it.
    fn start(&mut self, cx: &mut Context<Self>) {
        let mut can = vec![Move::Hop];
        can.push(if self.top_up {
            Move::TopSink
        } else {
            Move::TopRise
        });
        if self.covered {
            can.push(Move::Uncover);
        } else {
            can.extend([Move::Cover, Move::Tint]);
        }
        let pick = can[(self.next() % can.len() as u64) as usize];
        self.moves += 1;
        if crate::motion::still() {
            self.apply(pick);
        } else {
            self.playing = Some((pick, self.moves));
        }
        cx.notify();
    }

    fn finish(&mut self, cx: &mut Context<Self>) {
        if let Some((done, _)) = self.playing.take() {
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
            Move::Uncover => self.covered = false,
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

impl Render for CornerBlocks {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let playing = self.playing;
        let is = |m: Move| playing.filter(|(p, _)| *p == m).map(|(_, n)| n);
        let key = |n: usize| ElementId::named_usize("corner-block", n);
        let own = CORNER[self.corner];
        let mut d = div().relative().w(px(2. * SIDE + SMALL)).h(px(2. * SIDE));

        // The corner: the right block's colour while covered, else its own (already while the
        // right block slides back off it), fading to the next.
        let covered = self.covered && is(Move::Uncover).is_none();
        d = d.child(block(if covered { RIGHT } else { own }, SIDE, 0., 0.));
        if let Some(n) = is(Move::Tint) {
            let next = CORNER[(self.corner + 1) % CORNER.len()];
            d = d.child(block(next, SIDE, 0., 0.).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                |d, t| d.opacity(t),
            ));
        }

        // The top block, sinking into the corner block or rising out of it, from its top edge.
        let top = |h: f32| block(TOP, SIDE, 0., SIDE).h(px(h));
        d = match (is(Move::TopSink), is(Move::TopRise)) {
            (Some(n), _) => d.child(top(SIDE).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                |d, t| d.h(px(SIDE * (1. - t))),
            )),
            (_, Some(n)) => d.child(top(0.).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                |d, t| d.h(px(SIDE * t)),
            )),
            _ if self.top_up => d.child(top(SIDE)),
            _ => d,
        };

        // The right block, sliding over the corner block or back out to its place.
        d = match (is(Move::Cover), is(Move::Uncover)) {
            (Some(n), _) => d.child(block(RIGHT, SIDE, SIDE, 0.).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                |d, t| d.left(px(SIDE * (1. - t))),
            )),
            (_, Some(n)) => d.child(block(RIGHT, SIDE, 0., 0.).with_animation(
                key(n),
                crate::motion::eased(MOVE),
                |d, t| d.left(px(SIDE * t)),
            )),
            _ if !self.covered => d.child(block(RIGHT, SIDE, SIDE, 0.)),
            _ => d,
        };

        // The small block, hopping once in place.
        let small = block(SMALL_COLOUR, SMALL, 2. * SIDE, 0.);
        match is(Move::Hop) {
            Some(n) => d.child(
                small.with_animation(key(n), crate::motion::eased(MOVE), |d, t| {
                    d.bottom(px(HOP * 4. * t * (1. - t)))
                }),
            ),
            None => d.child(small),
        }
    }
}
