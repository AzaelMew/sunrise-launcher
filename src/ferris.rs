//! Ferris, the Rust crab, in pixels, standing on Home's bottom-left blocks. He is never quite still:
//! between his little acts (a blink, a snap of his pincers, a scuttle sideways, a hop, a wave) he
//! breathes and flexes his pincers, a pixel at a time on a slow tick, as pixel art moves. An act is
//! played frame by frame while it lasts.
use gpui::{canvas, px, Canvas, Hsla, Window};
use std::time::{Duration, Instant};

/// Ferris on a 28 by 17 grid, drawn after the Rust book's Ferris: `B` his body, `D` its shade
/// and his smile, `K` his eyes, `W` their glint.
const SPRITE: [&str; 17] = [
    "..........B..B..B...........",
    ".......B.BB.BB.BB..B........",
    ".......BB.BBBBBBBBB.........",
    ".....B.BBBBBBBBBBBBBBB......",
    "...B..BBBBBBBBBBBBBBB..B....",
    "...B.BBBBBBBBBBBBBBBBBBB....",
    "....BBBBBKWBBBBBKWBBBBB.....",
    ".BBBBBBBBWKKBBBBWKKBBBBBBB..",
    "..BBBBBBBKKKBBBBKKKBBBBB.B..",
    "BBBBBBBBBKKKBBBBKKKBBBBBBBB.",
    "BDBBBBBDBBKBBBBBBKBBDBBBB.DB",
    "BDBBBBBBDDDDDDDDDDDDBBBBB.DB",
    "BD...BBBBBBBBBBBBBBBBBB...DB",
    "BD......B.B......B.B......DB",
    ".B.....BB.BB....BB.BB.....B.",
    ".......BBBBB....BBBBB.......",
    "........BBB......BBB........",
];
pub(crate) const COLS: usize = 28;
pub(crate) const ROWS: usize = 17;
const BODY: u32 = 0xe33b26;
const SHADE: u32 = 0x8f1f1d;
const EYE: u32 = 0x000000;
const GLINT: u32 = 0xffffff;
/// The row his closed eyes become a line on, and where his pincers are (rows from, columns of
/// each).
const BLINK_ROW: usize = 8;
const PINCER_ROWS: usize = 13;
const PINCERS: [(usize, usize); 2] = [(7, 11), (16, 20)];
/// His arms: the two columns at each side, from his shoulder to his foot.
const ARM_COLS: usize = 2;
/// The rows above his pincers rise as he breathes; this is the first that doesn't.
const CHEST: usize = 12;
/// How often his idle loop steps.
pub(crate) const TICK: Duration = Duration::from_millis(260);
/// How high he hops, in his own pixels, and how many steps a scuttle takes.
const HOP: f32 = 7.;
const STEPS: f32 = 4.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Act {
    Blink,
    Snap,
    Scuttle,
    Hop,
    Wave,
}

impl Act {
    /// His acts as often as he does them: a wave about a third of the time.
    pub(crate) const ALL: [Act; 8] = [
        Act::Wave,
        Act::Wave,
        Act::Wave,
        Act::Blink,
        Act::Snap,
        Act::Scuttle,
        Act::Hop,
        Act::Blink,
    ];

    pub(crate) fn length(self) -> Duration {
        Duration::from_millis(match self {
            Act::Blink => 160,
            Act::Snap => 640,
            Act::Scuttle => 720,
            Act::Hop => 520,
            Act::Wave => 1100,
        })
    }
}

/// Where Ferris stands along his block (0 its left end, 1 its right) and what he is doing.
#[derive(Clone, Copy)]
pub(crate) struct Ferris {
    pub(crate) at: f32,
    /// The act playing, when it began, and where a scuttle goes (for a wave, under a half his
    /// left arm, else his right).
    pub(crate) act: Option<(Act, Instant, f32)>,
    /// His idle loop's step.
    pub(crate) tick: u32,
}

impl Ferris {
    pub(crate) fn new() -> Self {
        Self {
            at: 0.5,
            act: None,
            tick: 0,
        }
    }

    /// Steps his idle loop.
    pub(crate) fn idle(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    /// Ends the act playing, leaving him where it took him.
    pub(crate) fn finish(&mut self) {
        if let Some((Act::Scuttle, _, to)) = self.act.take() {
            self.at = to;
        }
    }

    /// How big one of his pixels is on a block `side` px wide: whole pixels, so he stays sharp.
    pub(crate) fn texel(side: f32) -> f32 {
        (side * 0.62 / COLS as f32).round().max(2.)
    }

    /// Ferris drawn into a box `side` px wide standing on its foot, asking for frames while an
    /// act plays. The box must leave room above him for a hop.
    pub(crate) fn draw(self, side: f32, window: &mut Window) -> Canvas<()> {
        let texel = Self::texel(side);
        let room = side - COLS as f32 * texel;
        let (mut x, mut lift, mut blink) = (self.at * room, 0., false);
        // His idle loop: a breath in and out over four steps, his pincers flexing every third.
        let breath = self.tick % 4 >= 2;
        let mut snap = self.tick.is_multiple_of(3);
        let mut wave = None;
        if let Some((act, began, to)) = self.act {
            let t = (began.elapsed().as_secs_f32() / act.length().as_secs_f32()).min(1.);
            if t < 1. {
                window.request_animation_frame();
            }
            match act {
                Act::Blink => blink = t < 1.,
                // Two snaps: up, down, up, down.
                Act::Snap => snap = t < 1. && ((t * 4.) as u32).is_multiple_of(2),
                Act::Scuttle => {
                    let eased = sunrise_theme::ease_out_cubic(t);
                    x = (self.at + (to - self.at) * eased) * room;
                    // A little bob on every step.
                    lift = if t < 1. && ((t * STEPS * 2.) as u32).is_multiple_of(2) {
                        1.
                    } else {
                        0.
                    };
                }
                Act::Hop => lift = HOP * 4. * t * (1. - t),
                // His arm up beside him, waving up and down three times.
                Act::Wave if t < 1. => {
                    wave = Some(if ((t * 6.) as u32).is_multiple_of(2) {
                        4.
                    } else {
                        3.
                    })
                }
                Act::Wave => {}
            }
        }
        let (x, lift) = (x.round(), (lift * texel).round());
        canvas(
            |_, _, _| {},
            move |bounds, (), window, _| {
                let foot = bounds.origin.y + bounds.size.height;
                let at = |col: usize, row: usize, up: f32| {
                    gpui::point(
                        bounds.origin.x + px(x + col as f32 * texel),
                        foot - px((ROWS - row) as f32 * texel + lift + up),
                    )
                };
                let colour = |ch: char, row: usize| -> Option<Hsla> {
                    let eye = matches!(ch, 'K' | 'W');
                    let c = match ch {
                        'B' => BODY,
                        'D' => SHADE,
                        _ if eye && blink => {
                            if row == BLINK_ROW {
                                EYE
                            } else {
                                BODY
                            }
                        }
                        'K' => EYE,
                        'W' => GLINT,
                        _ => return None,
                    };
                    Some(gpui::rgb(c).into())
                };
                for (row, line) in SPRITE.iter().enumerate() {
                    for (col, ch) in line.chars().enumerate() {
                        let Some(c) = colour(ch, row) else {
                            continue;
                        };
                        let pincer = row >= PINCER_ROWS
                            && PINCERS.iter().any(|&(a, b)| (a..=b).contains(&col));
                        let arm = match self.act.map(|(_, _, to)| to < 0.5) {
                            Some(true) => col < ARM_COLS,
                            _ => col >= COLS - ARM_COLS,
                        };
                        let chest = row < CHEST;
                        let up = match wave {
                            Some(raise) if arm => raise * texel,
                            _ if pincer && snap => texel,
                            _ if chest && breath => texel,
                            _ => 0.,
                        };
                        let size = gpui::size(px(texel), px(texel));
                        window.paint_quad(gpui::fill(gpui::Bounds::new(at(col, row, up), size), c));
                        // Breathing in, his chest's lowest row stays put too, so he stretches
                        // rather than parting.
                        if breath && row + 1 == CHEST && up > 0. {
                            window.paint_quad(gpui::fill(
                                gpui::Bounds::new(at(col, row, 0.), size),
                                c,
                            ));
                        }
                    }
                }
            },
        )
    }
}
