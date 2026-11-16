//! Four textured blocks standing in Home's bottom-right corner, played by
//! [`crate::corner_blocks::run`] in turn with the bottom-left ones. A column on the page's right
//! edge (4, a diamond, on top; 3; then 2 on the floor) with 1 left of 2. 1 turns between a diamond
//! and a block; as a block it slides right into 2's place, pushing 2 off the page; then 3 can drop
//! onto it, carrying 4 down with it, and later rise again; then 1 slides back, 2 slides back in,
//! and 1 turns into a diamond again. Between those, 4 spins a quarter turn, and 3 bounces with 4
//! riding on it.
use crate::corner_blocks::{
    block, block_side, spun_block, turn_progress, turned_block, Rng, Show, MOVE, REST_CHAINED,
};
use gpui::{
    div, prelude::*, px, AnimationExt, Canvas, Context, ElementId, IntoElement, Render, Window,
};
use std::time::Instant;

/// Each block's colour, by its number: the launcher's blues.
const ONE: u32 = 0x6a7af0;
const TWO: u32 = 0x2c3a9e;
const THREE: u32 = 0x4f60e6;
const FOUR: u32 = 0x93b4ff;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Move {
    /// 1 turns from a diamond into a block, or back.
    Unturn,
    Turn,
    /// 1 slides into 2's place, pushing 2 off the page, or back, 2 sliding in again.
    Push,
    Unpush,
    /// 3 drops onto 1 in the corner, 4 with it, or rises again.
    Drop,
    Lift,
    /// 4 spins a quarter turn on top of 3.
    Spin,
    /// 3 bounces, 4 riding on it.
    Bounce,
}

pub struct RightBlocks {
    /// 1 stands as a diamond.
    turned: bool,
    /// 1 is in 2's place, 2 off the page.
    pushed: bool,
    /// 3 lies on 1, 4 on 3.
    dropped: bool,
    playing: Option<(Move, usize, Instant)>,
    then: Option<(Move, u64)>,
    moves: usize,
    /// When each block (1 to 4, from 0) last moved, by move number.
    last_moved: [usize; 4],
    rng: Rng,
}

impl RightBlocks {
    pub fn new() -> Self {
        Self {
            turned: true,
            pushed: false,
            dropped: false,
            playing: None,
            then: None,
            moves: 0,
            last_moved: [0; 4],
            rng: Rng::seeded(),
        }
    }

    /// The blocks a move moves, numbered from 0 (1) to 3 (4).
    fn blocks_of(m: Move) -> &'static [usize] {
        match m {
            Move::Unturn | Move::Turn => &[0],
            Move::Push | Move::Unpush => &[0, 1],
            Move::Drop | Move::Lift | Move::Bounce => &[2, 3],
            Move::Spin => &[3],
        }
    }

    fn moves_from_here(&self) -> Vec<Move> {
        let mut can = match (self.pushed, self.dropped, self.turned) {
            (true, true, _) => vec![Move::Lift],
            (true, false, _) => vec![Move::Drop, Move::Unpush],
            (false, _, true) => vec![Move::Unturn],
            (false, _, false) => vec![Move::Push, Move::Turn],
        };
        can.push(Move::Spin);
        if !self.dropped {
            can.push(Move::Bounce);
        }
        can
    }

    fn next_move(&mut self) -> Move {
        let pick = match self.then.take() {
            Some((next, _)) => next,
            None => {
                let can = self.moves_from_here();
                let (moves, last) = (self.moves, self.last_moved);
                self.rng.pick_waiting(&can, |m| {
                    Self::blocks_of(m)
                        .iter()
                        .map(|&b| moves - last[b])
                        .max()
                        .unwrap_or(0)
                })
            }
        };
        for &b in Self::blocks_of(pick) {
            self.last_moved[b] = self.moves;
        }
        pick
    }

    fn apply(&mut self, done: Move) {
        match done {
            Move::Unturn => self.turned = false,
            Move::Turn => self.turned = true,
            Move::Push => self.pushed = true,
            Move::Unpush => {
                self.pushed = false;
                // Back in its place, it turns.
                self.then = Some((Move::Turn, REST_CHAINED));
            }
            Move::Drop => self.dropped = true,
            Move::Lift => self.dropped = false,
            Move::Spin | Move::Bounce => {}
        }
    }
}

impl Show for RightBlocks {
    fn chained(&self) -> Option<u64> {
        self.then.map(|(_, rest)| rest)
    }

    fn play(&mut self, cx: &mut Context<Self>) {
        let pick = self.next_move();
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
}

impl Render for RightBlocks {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let playing = self.playing;
        let is = |m: Move| playing.filter(|(p, _, _)| *p == m).map(|(_, n, _)| n);
        let key = |n: usize| ElementId::named_usize("right-block", n);
        let s = block_side(window);
        let eased = || crate::motion::eased(MOVE);
        // Two columns, three rows, standing on the floor; 2 leaves through the right edge.
        let mut d = div()
            .relative()
            .w(px(2. * s))
            .h(px(3. * s))
            .overflow_hidden();

        // 2, in the corner unless pushed off the page, or sliding out or back in.
        d = if let Some(n) = is(Move::Push) {
            d.child(
                block(TWO, s, s, 0.)
                    .with_animation(key(n), eased(), move |d, t| d.left(px(s + s * t))),
            )
        } else if let Some(n) = is(Move::Unpush) {
            d.child(
                block(TWO, s, 2. * s, 0.)
                    .with_animation(key(n), eased(), move |d, t| d.left(px(2. * s - s * t))),
            )
        } else if !self.pushed {
            d.child(block(TWO, s, s, 0.))
        } else {
            d
        };

        // 1: sliding between its place and the corner, turning, or at rest.
        let one_turned = |turn: f32| -> Canvas<()> {
            turned_block(ONE, s, turn)
                .absolute()
                .left_0()
                .bottom_0()
                .size(px(s))
        };
        let turning = playing.and_then(|(m, _, at)| match m {
            Move::Turn => Some((at, false)),
            Move::Unturn => Some((at, true)),
            _ => None,
        });
        d = if let Some(n) = is(Move::Push) {
            d.child(
                block(ONE, s, 0., 0.)
                    .with_animation(key(n), eased(), move |d, t| d.left(px(s * t))),
            )
        } else if let Some(n) = is(Move::Unpush) {
            d.child(
                block(ONE, s, s, 0.)
                    .with_animation(key(n), eased(), move |d, t| d.left(px(s - s * t))),
            )
        } else if let Some((at, back)) = turning {
            let t = turn_progress(at, window);
            d.child(one_turned(if back { 1. - t } else { t }))
        } else if self.pushed {
            d.child(block(ONE, s, s, 0.))
        } else if self.turned {
            d.child(one_turned(1.))
        } else {
            d.child(block(ONE, s, 0., 0.))
        };

        // 3 and 4 move together: 3 on its row or down on the corner, 4 always on top of it; 3
        // bounces with 4 riding, and 4 spins in its place.
        let (up, down) = (s, 0.);
        let spin = playing
            .filter(|(m, _, _)| *m == Move::Spin)
            .map(|(_, _, at)| turn_progress(at, window));
        let four = |bottom: f32| -> Canvas<()> {
            match spin {
                Some(t) => spun_block(FOUR, s, t),
                None => turned_block(FOUR, s, 1.),
            }
            .absolute()
            .left(px(s))
            .bottom(px(bottom))
            .size(px(s))
        };
        let falling = is(Move::Drop).map(|n| (n, up, down));
        let rising = is(Move::Lift).map(|n| (n, down, up));
        let at = if self.dropped { down } else { up };
        if let Some((n, from, to)) = falling.or(rising) {
            d.child(
                block(THREE, s, s, from).with_animation(key(n), eased(), move |d, t| {
                    d.bottom(px(from + (to - from) * t))
                }),
            )
            .child(four(from + s).with_animation(
                ElementId::named_usize("right-block-four", n),
                eased(),
                move |d, t| d.bottom(px(from + s + (to - from) * t)),
            ))
        } else if let Some(n) = is(Move::Bounce) {
            let bounce = move |t: f32| s / 5. * 4. * t * (1. - t);
            d.child(
                block(THREE, s, s, at)
                    .with_animation(key(n), eased(), move |d, t| d.bottom(px(at + bounce(t)))),
            )
            .child(four(at + s).with_animation(
                ElementId::named_usize("right-block-four", n),
                eased(),
                move |d, t| d.bottom(px(at + s + bounce(t))),
            ))
        } else {
            d.child(block(THREE, s, s, at)).child(four(at + s))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_only_drops_onto_one_in_the_corner_and_one_turns_back_in_its_place() {
        let mut b = RightBlocks {
            turned: true,
            pushed: false,
            dropped: false,
            playing: None,
            then: None,
            moves: 0,
            last_moved: [0; 4],
            rng: Rng::seeded(),
        };
        for _ in 0..5000 {
            b.moves += 1;
            let pick = b.next_move();
            match pick {
                Move::Unturn => assert!(b.turned && !b.pushed),
                Move::Turn => assert!(!b.turned && !b.pushed),
                Move::Push => assert!(!b.turned && !b.pushed),
                Move::Unpush => assert!(b.pushed && !b.dropped),
                Move::Drop => assert!(b.pushed && !b.dropped),
                Move::Lift => assert!(b.dropped),
                Move::Bounce => assert!(!b.dropped),
                Move::Spin => {}
            }
            b.apply(pick);
            // No block is left out for long.
            for (block, &last) in b.last_moved.iter().enumerate() {
                assert!(
                    b.moves - last <= 15,
                    "block {block} waited {}",
                    b.moves - last
                );
            }
            if pick == Move::Unpush {
                assert_eq!(b.then.map(|(m, _)| m), Some(Move::Turn));
            }
            assert!(!b.dropped || b.pushed, "3 only lies on 1 in the corner");
        }
    }
}
