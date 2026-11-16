//! Home's playing blocks: four textured blocks in the bottom-left corner (here) and four in the
//! bottom-right ([`crate::right_blocks`]), taking turns ([`run`]): a few short moves on one side, then
//! a few on the other, never both at once. Between moves nothing is drawn again.
//!
//! Bottom left: the corner block changes colour; the block on top sinks into it and later rises
//! out again; the block on its right turns into a diamond and back, or slides over the corner
//! block (which takes its colour) and later back out, turning into a diamond as it lands; while it
//! is inside, the small block can slide over to the corner block and back; the small block hops.
use crate::glass_ui::{color, pixel_fill, texel_noise, texel_shades, TEXEL, TEXTURE_BUTTON};
use gpui::{
    canvas, div, prelude::*, px, AnimationExt, App, AsyncApp, Canvas, Context, Div, ElementId,
    IntoElement, Render, WeakEntity, Window,
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
/// How long the blocks rest between moves, at least and at most, and before a move that must
/// follow the last at once (the turn as a block lands).
const REST_MIN: u64 = 600;
const REST_MAX: u64 = 1400;
pub(crate) const REST_CHAINED: u64 = 120;
/// How long the right block stands as a diamond before it turns back.
const REST_TURNED: u64 = 1500;
/// How long Ferris rests between his acts, at least and at most.
const FERRIS_REST_MIN: u64 = 1400;
const FERRIS_REST_MAX: u64 = 3600;
/// How many moves a side plays before the other side's turn, at least.
const TURN_MOVES: u64 = 2;
pub(crate) const MOVE: sunrise_theme::Motion = sunrise_theme::Motion {
    name: "corner-block",
    ms: 460,
    curve: [0.6, 0., 0.3, 1.],
};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Move {
    Tint,
    TopSink,
    TopRise,
    TopHop,
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
    /// When each block (corner, top, right, small) last moved, by move number.
    last_moved: [usize; 4],
    rng: Rng,
    /// Ferris, standing on the top block (or the corner block while it is down).
    ferris: crate::ferris::Ferris,
}

/// A fixed pseudo-random sequence (xorshift), seeded from the clock.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn seeded() -> Self {
        Self(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64)
                | 1,
        )
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    /// One of `from`, picked.
    pub(crate) fn pick<T: Copy>(&mut self, from: &[T]) -> T {
        from[(self.next() % from.len() as u64) as usize]
    }

    /// One of `from`, mostly one of those whose blocks have waited longest (`waited`), so every
    /// block gets its turn; now and then any of them.
    pub(crate) fn pick_waiting<T: Copy>(&mut self, from: &[T], waited: impl Fn(T) -> usize) -> T {
        if self.next() % 20 >= 17 {
            return self.pick(from);
        }
        let longest = from.iter().map(|&m| waited(m)).max().unwrap_or(0);
        let waiting: Vec<T> = from
            .iter()
            .copied()
            .filter(|&m| waited(m) == longest)
            .collect();
        self.pick(&waiting)
    }
}

/// A side's blocks, as [`run`] plays them.
pub(crate) trait Show: Render + Sized + 'static {
    /// The rest before a move that must follow the last one at once, if one must.
    fn chained(&self) -> Option<u64>;
    /// Picks a move the blocks can make from where they are, and plays it.
    fn play(&mut self, cx: &mut Context<Self>);
    /// Ends the move playing, leaving the blocks where it took them.
    fn finish(&mut self, cx: &mut Context<Self>);
    /// Whether the blocks could rest here while the other side plays: no air gap between them.
    fn settled(&self) -> bool {
        true
    }
}

/// Plays both sides' blocks for as long as they are there: a few moves on the left, then a few on
/// the right, one at a time, with short rests. A side hands over only once it has made any move
/// it must still make and its blocks are settled, never leaving a gap while the other side plays.
pub(crate) fn run(
    left: WeakEntity<CornerBlocks>,
    right: WeakEntity<crate::right_blocks::RightBlocks>,
    cx: &mut App,
) {
    // Ferris keeps his own time: a little act every few seconds, whichever side is playing.
    let crab = left.clone();
    cx.spawn(async move |cx| {
        let mut rng = Rng::seeded();
        loop {
            let rest = FERRIS_REST_MIN + rng.next() % (FERRIS_REST_MAX - FERRIS_REST_MIN);
            cx.background_executor()
                .timer(Duration::from_millis(rest))
                .await;
            let act = rng.pick(&crate::ferris::Act::ALL);
            let to = (rng.next() % 1000) as f32 / 1000.;
            let Ok(length) = crab.update(cx, |b, cx| b.ferris_act(act, to, cx)) else {
                return;
            };
            cx.background_executor().timer(length).await;
            if crab.update(cx, |b, cx| b.ferris_done(cx)).is_err() {
                return;
            }
        }
    })
    .detach();
    cx.spawn(async move |cx| {
        let mut rng = Rng::seeded();
        loop {
            for on_left in [true, false] {
                let count = TURN_MOVES + rng.next() % 2;
                let mut played = 0;
                loop {
                    let ready = if on_left {
                        step(&left, cx, &mut rng).await
                    } else {
                        step(&right, cx, &mut rng).await
                    };
                    let Some(ready) = ready else {
                        return;
                    };
                    played += 1;
                    if played >= count && ready {
                        break;
                    }
                }
            }
        }
    })
    .detach();
}

/// Plays one move of `blocks` after its rest. Whether the side could hand over now (no move must
/// follow and its blocks are settled); none once the blocks are gone.
async fn step<T: Show>(blocks: &WeakEntity<T>, cx: &mut AsyncApp, rng: &mut Rng) -> Option<bool> {
    let rest = blocks
        .update(cx, |b, _| b.chained())
        .ok()?
        .unwrap_or_else(|| REST_MIN + rng.next() % (REST_MAX - REST_MIN));
    cx.background_executor()
        .timer(Duration::from_millis(rest))
        .await;
    blocks.update(cx, |b, cx| b.play(cx)).ok()?;
    cx.background_executor().timer(MOVE.duration()).await;
    blocks.update(cx, |b, cx| b.finish(cx)).ok()?;
    blocks
        .update(cx, |b, _| b.chained().is_none() && b.settled())
        .ok()
}

/// How far a turn begun `at` has come, eased, asking for the next frame while it runs: a turn is
/// drawn texel by texel, so it is played frame by frame rather than by GPUI's animations.
pub(crate) fn turn_progress(at: Instant, window: &mut Window) -> f32 {
    let t = (at.elapsed().as_secs_f32() / MOVE.duration().as_secs_f32()).min(1.);
    if t < 1. {
        window.request_animation_frame();
    }
    MOVE.ease(t)
}

/// A block's side for this window: a share of its height, on whole texels so the texture stays
/// square.
pub(crate) fn block_side(window: &Window) -> f32 {
    let height = f32::from(window.viewport_size().height);
    ((height * SIDE_SHARE).clamp(SIDE_MIN, SIDE_MAX) / (2. * TEXEL)).round() * 2. * TEXEL
}

impl CornerBlocks {
    pub fn new() -> Self {
        Self {
            corner: 0,
            top_up: true,
            covered: false,
            turned: false,
            attached: false,
            playing: None,
            then: None,
            moves: 0,
            last_moved: [0; 4],
            rng: Rng::seeded(),
            ferris: crate::ferris::Ferris::new(),
        }
    }

    /// The blocks a move moves or changes: 0 the corner, 1 the top, 2 the right, 3 the small one.
    fn blocks_of(m: Move) -> &'static [usize] {
        match m {
            Move::Tint => &[0],
            Move::TopSink | Move::TopRise | Move::TopHop => &[1],
            // Covering it or sliding off it changes the corner block's colour too.
            Move::Cover | Move::Uncover => &[0, 2],
            Move::Turn | Move::Unturn => &[2],
            Move::Attach | Move::Detach | Move::Hop => &[3],
        }
    }

    /// Starts one of Ferris's acts (a scuttle goes `to` along his block); how long it lasts.
    /// Under reduced motion he keeps still.
    fn ferris_act(&mut self, act: crate::ferris::Act, to: f32, cx: &mut Context<Self>) -> Duration {
        if crate::motion::still() {
            return Duration::ZERO;
        }
        self.ferris.act = Some((act, Instant::now(), to));
        cx.notify();
        act.length()
    }

    fn ferris_done(&mut self, cx: &mut Context<Self>) {
        self.ferris.finish();
        cx.notify();
    }

    /// The moves the blocks can make from where they are.
    fn moves_from_here(&self) -> Vec<Move> {
        // The right block inside the corner leaves its cell empty: the next move fills it.
        if !self.settled() {
            return vec![Move::Attach, Move::Uncover];
        }
        let mut can = if self.top_up {
            vec![Move::TopSink, Move::TopHop]
        } else {
            vec![Move::TopRise]
        };
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

    fn apply(&mut self, done: Move) {
        match done {
            Move::Tint => self.corner = (self.corner + 1) % CORNER.len(),
            Move::TopSink => self.top_up = false,
            Move::TopRise => self.top_up = true,
            Move::TopHop => {}
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

    /// The next move: the one that must follow, or one picked from those the blocks can make.
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
}

impl Show for CornerBlocks {
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

    /// The right block inside the corner block leaves its cell empty until the small block
    /// moves up into it.
    fn settled(&self) -> bool {
        !self.covered || self.attached
    }
}

/// A textured block of `colour`, `side` px square, placed by its left and bottom edges.
pub(crate) fn block(colour: u32, side: f32, left: f32, bottom: f32) -> Div {
    div()
        .absolute()
        .left(px(left))
        .bottom(px(bottom))
        .size(px(side))
        .overflow_hidden()
        .child(pixel_fill(color(colour), TEXTURE_BUTTON))
}

/// A block `turn` of the way (0 to 1) from a square to a diamond: turned 45° and shrunk until its
/// corners touch its cell's sides. GPUI can't turn an element, so it is drawn here: one
/// straight-edged shape in the block's colour, with the texture's texels wherever they lie wholly
/// inside it. Place it in its cell, `side` px square.
pub(crate) fn turned_block(colour: u32, side: f32, turn: f32) -> Canvas<()> {
    let half = side / 2. * (1. - (1. - FRAC_1_SQRT_2) * turn);
    rotated_block(colour, side, turn * FRAC_PI_4, half)
}

/// A diamond spun `spin` of a quarter turn past its own place: the same diamond after a whole
/// quarter, so a spin begins and ends looking the same.
pub(crate) fn spun_block(colour: u32, side: f32, spin: f32) -> Canvas<()> {
    rotated_block(
        colour,
        side,
        FRAC_PI_4 + spin * 2. * FRAC_PI_4,
        side / 2. * FRAC_1_SQRT_2,
    )
}

/// A square `half` px from its centre to each side, turned `angle` radians, in the middle of its
/// cell, `side` px square.
fn rotated_block(colour: u32, side: f32, angle: f32, half: f32) -> Canvas<()> {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let base = color(colour);
            let shades = texel_shades(base, TEXTURE_BUTTON);
            let (sin, cos) = angle.sin_cos();
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
}

/// How high the top block is `t` of the way through its hop.
fn hop_top(side: f32, t: f32) -> f32 {
    side / 5. * 4. * t * (1. - t)
}

/// The right block turned `turn` of the way, in its cell.
fn right_turned(side: f32, turn: f32) -> Canvas<()> {
    turned_block(RIGHT, side, turn)
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
        let side = block_side(window);
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
            _ if self.top_up => match is(Move::TopHop) {
                Some(n) => d.child(top(side).with_animation(
                    key(n),
                    crate::motion::eased(MOVE),
                    move |d, t| d.bottom(px(side + hop_top(side, t))),
                )),
                None => d.child(top(side)),
            },
            _ => d,
        };

        // Ferris, on the top block, riding it down into the corner block and back up.
        let crab = {
            use crate::ferris::{Ferris, ROWS};
            let texel = Ferris::texel(side);
            // Room above him for a hop.
            let tall = (ROWS as f32 + 9.) * texel;
            let stand = div()
                .absolute()
                .left_0()
                .w(px(side))
                .h(px(tall))
                .child(self.ferris.draw(side, window).size_full());
            let ride = |n: usize| ElementId::named_usize("corner-ferris", n);
            match (is(Move::TopSink), is(Move::TopRise)) {
                (Some(n), _) => stand
                    .bottom(px(2. * side))
                    .with_animation(ride(n), crate::motion::eased(MOVE), move |d, t| {
                        d.bottom(px(side + side * (1. - t)))
                    })
                    .into_any_element(),
                (_, Some(n)) => stand
                    .bottom(px(side))
                    .with_animation(ride(n), crate::motion::eased(MOVE), move |d, t| {
                        d.bottom(px(side + side * t))
                    })
                    .into_any_element(),
                _ => match is(Move::TopHop) {
                    Some(n) => stand
                        .bottom(px(2. * side))
                        .with_animation(ride(n), crate::motion::eased(MOVE), move |d, t| {
                            d.bottom(px(2. * side + hop_top(side, t)))
                        })
                        .into_any_element(),
                    None => stand
                        .bottom(px(if self.top_up { 2. * side } else { side }))
                        .into_any_element(),
                },
            }
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
            let t = turn_progress(at, window);
            d.child(right_turned(side, if back { 1. - t } else { t }))
        } else if self.turned {
            d.child(right_turned(side, 1.))
        } else if !self.covered {
            d.child(block(RIGHT, side, side, 0.))
        } else {
            d
        };

        let d = d.child(crab);

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
            last_moved: [0; 4],
            rng: Rng(0x2545_f491_4f6c_dd1d),
            ferris: crate::ferris::Ferris::new(),
        }
    }

    #[test]
    fn every_move_is_one_the_blocks_can_make_and_sliding_out_lands_turning() {
        let mut b = blocks();
        let mut unsettled = 0;
        for _ in 0..5000 {
            let then = b.then;
            b.moves += 1;
            let pick = b.next_move();
            if let Some((next, _)) = then {
                assert_eq!(pick, next);
            }
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
                Move::TopHop => assert!(b.top_up),
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
            // Its turn can always end soon: never long with a gap the other side would watch.
            unsettled = if b.settled() { 0 } else { unsettled + 1 };
            assert!(unsettled <= 1, "left gapped for {unsettled} moves");
        }
    }
}
