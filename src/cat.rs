//! The website's cat, hiding behind Play: now and then it peeks out, or walks out into the empty
//! part of the play bar and stays a while (click it to pet it), or climbs up and sleeps on Play.
//!
//! Its look and its ways are the Sunrise site's, ported from sunrise-org/sunny
//! (`frontend/site/art/`: `cat.ts` for the poses and coats, `cat-actions.ts` for what it does and
//! how it takes being petted, `core.ts` for the pixel helpers). The site's cat breathes all the
//! time; this one holds still between actions, so a resting cat costs no frames.
use super::*;
use gpui::RenderImage;
use std::sync::Arc;
use std::time::{Duration, Instant};

type Rgb = [f32; 3];

const fn hex(v: u32) -> Rgb {
    [
        ((v >> 16) & 255) as f32,
        ((v >> 8) & 255) as f32,
        (v & 255) as f32,
    ]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn unit(v: f32) -> f32 {
    v.clamp(0., 1.)
}

const BAYER4: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

// A pose is these numbers; a key of an action sets some of them.
const BREATH: usize = 0;
const LEAN: usize = 1;
const CROUCH: usize = 2;
const HEAD_X: usize = 3;
const HEAD_Y: usize = 4;
const TILT: usize = 5;
const TURN: usize = 6;
const EAR_NEAR: usize = 7;
const EAR_FAR: usize = 8;
const EYES: usize = 9;
const MOUTH: usize = 10;
const TONGUE: usize = 11;
const PAW: usize = 12;
const TAIL_CURL: usize = 13;
const TAIL_SWAY: usize = 14;
const TAIL_WRAP: usize = 15;
const ANGER: usize = 16;
const WALK: usize = 17;

type Pose = [f32; 18];

const SIT: Pose = {
    let mut p = [0.; 18];
    p[EYES] = 1.;
    p[TAIL_CURL] = 0.3;
    p[WALK] = -1.;
    p
};

fn with(mut base: Pose, set: &[(usize, f32)]) -> Pose {
    for &(field, value) in set {
        base[field] = value;
    }
    base
}

struct Coat {
    base: Rgb,
    shade: Rgb,
    light: Rgb,
    mark: Rgb,
    mark_shade: Rgb,
    cream: Rgb,
    eye: Rgb,
    nose: Rgb,
    inner_ear: Rgb,
    outline: Rgb,
    /// Dark points (the Siamese), else stripes.
    points: bool,
    tail_width: f32,
}

/// Seal-point Siamese: cream body, dark points, blue eyes, a slim tail.
const SIAMESE: Coat = Coat {
    base: hex(0xecdfc8),
    shade: hex(0xcdb89c),
    light: hex(0xfff6e4),
    mark: hex(0x4e3527),
    mark_shade: hex(0x36231a),
    cream: hex(0xf7efe0),
    eye: hex(0x5cb2ff),
    nose: hex(0x8e5850),
    inner_ear: hex(0x7a4c44),
    outline: hex(0x2a1c1e),
    points: true,
    tail_width: 1.,
};

/// Orange tabby: warm fur, darker stripes and rings, a cream chest, a thicker tail.
const TABBY: Coat = Coat {
    base: hex(0xe88f3e),
    shade: hex(0xbf6a2c),
    light: hex(0xffd7a0),
    mark: hex(0xa9541d),
    mark_shade: hex(0x874114),
    cream: hex(0xf8dcae),
    eye: hex(0xc4d64e),
    nose: hex(0xe2928c),
    inner_ear: hex(0xefa59a),
    outline: hex(0x3a2218),
    points: false,
    tail_width: 1.35,
};

const CAT_W: usize = 48;
const CAT_H: usize = 36;
/// The picture: the cat on the bottom edge, where its shadow lies.
const W: usize = CAT_W;
const H: usize = CAT_H - 1;
const GY: f32 = 33.;

#[derive(Clone, Copy, PartialEq)]
enum Part {
    None,
    Tail,
    FarLeg,
    Haunch,
    Foot,
    Chest,
    NearLeg,
    Head,
    Ear,
    Muzzle,
    Paw,
}

impl Part {
    /// Which parts lie in front of which: a part behind another gets a dark edge where they meet.
    fn layer(self) -> u8 {
        match self {
            Part::None => 0,
            Part::Tail => 1,
            Part::FarLeg => 2,
            Part::Haunch | Part::Foot | Part::Chest => 3,
            Part::NearLeg => 4,
            Part::Head | Part::Ear | Part::Muzzle => 5,
            Part::Paw => 6,
        }
    }
}

#[derive(Clone, Copy)]
struct Ellipse {
    x: f32,
    y: f32,
    rx: f32,
    ry: f32,
}

impl Ellipse {
    fn holds(&self, x: f32, y: f32) -> bool {
        ((x - self.x) / self.rx).powi(2) + ((y - self.y) / self.ry).powi(2) <= 1.
    }
}

#[derive(Clone, Copy)]
struct Seg {
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    r: f32,
}

impl Seg {
    fn distance(&self, x: f32, y: f32) -> f32 {
        let (dx, dy) = (self.bx - self.ax, self.by - self.ay);
        let len = dx * dx + dy * dy;
        let t = unit(((x - self.ax) * dx + (y - self.ay) * dy) / if len == 0. { 1. } else { len });
        (x - (self.ax + dx * t)).hypot(y - (self.ay + dy * t))
    }

    fn holds(&self, x: f32, y: f32) -> bool {
        self.distance(x, y) <= self.r
    }
}

fn in_triangle(px: f32, py: f32, a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let s = |p: [f32; 2], q: [f32; 2]| (px - q[0]) * (p[1] - q[1]) - (p[0] - q[0]) * (py - q[1]);
    let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
    !((d1 < 0. || d2 < 0. || d3 < 0.) && (d1 > 0. || d2 > 0. || d3 > 0.))
}

fn bezier(p: &[[f32; 2]; 4], t: f32) -> [f32; 2] {
    let u = 1. - t;
    [0, 1].map(|i| {
        u * u * u * p[0][i]
            + 3. * u * u * t * p[1][i]
            + 3. * u * t * t * p[2][i]
            + t * t * t * p[3][i]
    })
}

struct Layout {
    haunch: Ellipse,
    chest: Ellipse,
    head: Ellipse,
    neck: Seg,
    far_leg: Seg,
    near_leg: Seg,
    hind_far: Option<Seg>,
    hind_near: Option<Seg>,
    foot: Ellipse,
    tail: [[f32; 2]; 4],
    tail_r: [f32; 2],
}

/// Where every part sits for a pose: sitting at crouch 0, a loaf at crouch 1.
fn layout(p: &Pose, coat: &Coat) -> Layout {
    let (c, b) = (p[CROUCH], p[BREATH] * 0.45);
    let mut haunch = Ellipse {
        x: 18.,
        y: lerp(GY - 7., GY - 5., c),
        rx: lerp(8., 9.5, c),
        ry: lerp(7., 5.2, c),
    };
    let mut chest = Ellipse {
        x: lerp(25., 26.5, c) + p[LEAN],
        y: lerp(GY - 12.5, GY - 5.2, c),
        rx: lerp(4.6, 6., c) + b,
        ry: lerp(7.2, 5., c) + b,
    };
    let mut head = Ellipse {
        x: lerp(28.5, 32., c) + p[HEAD_X] + p[LEAN] * 0.6,
        y: lerp(GY - 22., GY - 10.5, c) + p[HEAD_Y],
        rx: 5.9,
        ry: 5.2,
    };
    let mut neck = Seg {
        ax: chest.x,
        ay: chest.y - chest.ry * 0.5,
        bx: head.x - 1.,
        by: head.y + 1.5,
        r: lerp(3.6, 3.2, c),
    };
    let leg_len = lerp(1., 0.15, c);
    let mut far_leg = Seg {
        ax: chest.x - 1.5,
        ay: chest.y + 2.,
        bx: chest.x - 1.,
        by: GY - 1. - (1. - leg_len) * 2.,
        r: 1.45,
    };
    let paw_at = [
        lerp(chest.x + 1.8, head.x + 3.2, p[PAW]),
        lerp(GY - 1., head.y + 3.2, p[PAW]),
    ];
    let mut near_leg = Seg {
        ax: chest.x + 0.6,
        ay: chest.y + 1.,
        bx: lerp(paw_at[0], chest.x + 3., c * (1. - p[PAW])),
        by: lerp(paw_at[1], GY - 1., c * (1. - p[PAW])),
        r: 1.6,
    };
    let foot = Ellipse {
        x: lerp(22., 21., c),
        y: GY - 1.4,
        rx: lerp(3., 2.4, c),
        ry: 1.5,
    };
    let (mut hind_far, mut hind_near) = (None, None);
    let walking = p[WALK] >= 0.;
    if walking {
        let bob = (p[WALK] * 2.).cos() * 0.4;
        // A relaxed walk: a level back, the head carried just above the shoulders.
        haunch = Ellipse {
            x: 18.,
            y: GY - 13. + bob,
            rx: 7.5,
            ry: 4.6,
        };
        chest = Ellipse {
            x: 27.,
            y: GY - 13. + bob,
            rx: 6.,
            ry: 4.8,
        };
        head.x = 33.;
        head.y = GY - 18.5 + bob;
        neck = Seg {
            ax: 29.,
            ay: GY - 14.5 + bob,
            bx: 32.,
            by: GY - 17. + bob,
            r: 3.3,
        };
        let leg = |x: f32, phase: f32, r: f32| Seg {
            ax: x,
            ay: GY - 11.5 + bob,
            bx: x - phase.cos() * 3.6,
            by: GY - 1. - phase.sin().max(0.) * 2.2,
            r,
        };
        let pi = std::f32::consts::PI;
        far_leg = leg(26.5, p[WALK] + pi, 1.45);
        near_leg = leg(28.5, p[WALK], 1.65);
        hind_far = Some(leg(18.5, p[WALK], 1.45));
        hind_near = Some(leg(16., p[WALK] + pi, 1.65));
    }
    let (sway, curl, wrap) = (p[TAIL_SWAY], p[TAIL_CURL], p[TAIL_WRAP]);
    // Walking, the tail is carried up in a loose S; sitting, it lies along the ground and curls.
    let tail = if walking {
        [
            [haunch.x - haunch.rx * 0.8, haunch.y - 1.],
            [haunch.x - haunch.rx - 4., haunch.y - 3.],
            [haunch.x - haunch.rx - 4. + sway * 2., haunch.y - 10.],
            [haunch.x - haunch.rx - 1. + sway * 3., haunch.y - 13.],
        ]
    } else {
        [
            [haunch.x - haunch.rx * 0.75, GY - 2.5],
            [
                lerp(haunch.x - haunch.rx - 4., haunch.x + 2., wrap),
                lerp(GY - 0.5, GY + 0.6, wrap),
            ],
            [
                lerp(3. + sway * 2., chest.x + 2., wrap),
                lerp(GY - 5. - curl * 7., GY + 0.4, wrap),
            ],
            [
                lerp(4.5 + sway * 4.5, chest.x + 7., wrap),
                lerp(GY - 9. - curl * 6., GY - 1.2, wrap),
            ],
        ]
    };
    Layout {
        haunch,
        chest,
        head,
        neck,
        far_leg,
        near_leg,
        hind_far,
        hind_near,
        foot,
        tail,
        tail_r: [1.75 * coat.tail_width, 1.05 * coat.tail_width],
    }
}

/// A picture of `W` by `H` pixels, four bytes each, in the order the window draws (BGRA).
struct Pixels(Vec<u8>);

impl Pixels {
    fn new() -> Self {
        Self(vec![0; W * H * 4])
    }

    fn set(&mut self, x: i32, y: i32, c: Rgb, alpha: f32) {
        if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
            return;
        }
        let o = (y as usize * W + x as usize) * 4;
        let px = &mut self.0[o..o + 4];
        if alpha >= 1. {
            px.copy_from_slice(&[
                c[2].round() as u8,
                c[1].round() as u8,
                c[0].round() as u8,
                255,
            ]);
            return;
        }
        let base = px[3] as f32 / 255.;
        for (i, channel) in [c[2], c[1], c[0]].into_iter().enumerate() {
            px[i] = (px[i] as f32 * (1. - alpha) + channel * alpha) as u8;
        }
        px[3] = ((base + (1. - base) * alpha) * 255.).round() as u8;
    }
}

/// One pose of a cat, drawn from body parts with its coat's markings, `oy` rows down.
fn draw_cat(out: &mut Pixels, coat: &Coat, p: &Pose, oy: i32) {
    let l = layout(p, coat);
    let t = p[TURN];
    let muzzle = Ellipse {
        x: lerp(3.7, 0., t),
        y: lerp(1.5, 1.9, t),
        rx: lerp(2.5, 2.7, t),
        ry: lerp(1.9, 1.6, t),
    };
    let eyes: &[[f32; 2]] = if t < 0.45 {
        &[[lerp(1.9, 1.4, t), -0.7]]
    } else {
        &[[-2.1, -0.6], [2.1, -0.6]]
    };
    let nose = [lerp(5.6, 0., t), lerp(0.8, 1.1, t)];
    let ears = [
        (
            lerp(-0.42, -0.72, t) - p[EAR_NEAR] * 0.55 - p[ANGER] * 0.35,
            4.4 - p[EAR_NEAR] * 1.1 - p[ANGER],
        ),
        (
            lerp(0.5, 0.72, t) + p[EAR_FAR] * 0.45 + p[ANGER] * 0.35,
            4.2 - p[EAR_FAR] - p[ANGER],
        ),
    ];
    // A point in the head's own frame, where the face is drawn upright.
    let to_head = |x: f32, y: f32| {
        let (dx, dy) = (x - l.head.x, y - l.head.y);
        let (c, s) = ((-p[TILT]).cos(), (-p[TILT]).sin());
        [dx * c - dy * s, dx * s + dy * c]
    };
    let walking = p[WALK] >= 0.;

    let mut label = [Part::None; CAT_W * CAT_H];
    let mut tail_t = [0f32; CAT_W * CAT_H];
    let tail_pts: Vec<[f32; 2]> = (0..24).map(|i| bezier(&l.tail, i as f32 / 23.)).collect();
    for y in 0..CAT_H {
        for x in 0..CAT_W {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let (mut r, mut tt) = (Part::None, 0.);
            for i in 0..tail_pts.len() - 1 {
                let k = (i as f32 + 0.5) / 23.;
                let s = Seg {
                    ax: tail_pts[i][0],
                    ay: tail_pts[i][1],
                    bx: tail_pts[i + 1][0],
                    by: tail_pts[i + 1][1],
                    r: lerp(l.tail_r[0], l.tail_r[1], k),
                };
                if s.holds(px, py) {
                    r = Part::Tail;
                    tt = k;
                    break;
                }
            }
            if l.far_leg.holds(px, py) || l.hind_far.is_some_and(|s| s.holds(px, py)) {
                r = Part::FarLeg;
            }
            if l.haunch.holds(px, py) {
                r = Part::Haunch;
            }
            if !walking && p[CROUCH] < 0.7 && l.foot.holds(px, py) {
                r = Part::Foot;
            }
            if l.chest.holds(px, py) || l.neck.holds(px, py) {
                r = Part::Chest;
            }
            if p[PAW] < 0.05 && l.near_leg.holds(px, py) {
                r = Part::NearLeg;
            }
            if l.hind_near.is_some_and(|s| s.holds(px, py)) {
                r = Part::NearLeg;
            }
            let [hx, hy] = to_head(px, py);
            if (hx / l.head.rx).powi(2) + (hy / l.head.ry).powi(2) <= 1. {
                r = Part::Head;
            }
            for (a, len) in ears {
                let (bx, by) = (a.sin() * 4.6, -a.cos() * 4.1);
                let tip = [bx + a.sin() * len, by - a.cos() * len];
                let w = 2.1;
                let left = [bx - a.cos() * w, by - a.sin() * w];
                let right = [bx + a.cos() * w, by + a.sin() * w];
                if in_triangle(hx, hy, left, right, tip) {
                    r = Part::Ear;
                }
            }
            if muzzle.holds(hx, hy) {
                r = Part::Muzzle;
            }
            if p[PAW] >= 0.05 && l.near_leg.holds(px, py) {
                r = Part::Paw;
            }
            label[y * CAT_W + x] = r;
            tail_t[y * CAT_W + x] = tt;
        }
    }
    let at = |x: i32, y: i32| {
        if x < 0 || y < 0 || x >= CAT_W as i32 || y >= CAT_H as i32 {
            Part::None
        } else {
            label[y as usize * CAT_W + x as usize]
        }
    };
    let mut put = |x: i32, y: i32, c: Rgb| out.set(x, oy + y, c, 1.);

    for y in 0..CAT_H as i32 {
        for x in 0..CAT_W as i32 {
            let r = at(x, y);
            if r == Part::None {
                continue;
            }
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let [hx, hy] = to_head(px, py);
            let tt = tail_t[y as usize * CAT_W + x as usize];
            let leg = matches!(r, Part::FarLeg | Part::NearLeg);
            let mut c = coat.base;
            if coat.points {
                let mark = match r {
                    Part::Ear => 1.,
                    Part::Tail => unit(0.15 + tt * 1.3),
                    Part::FarLeg | Part::NearLeg | Part::Paw => unit((py - (GY - 9.)) / 6.),
                    Part::Muzzle => 0.9,
                    Part::Head => unit(1.3 - ((hx - muzzle.x * 0.75) * 0.9).hypot(hy - 0.9) / 3.4),
                    Part::Foot => 0.6,
                    _ => 0.,
                };
                if mark > 0.62 {
                    c = coat.mark;
                } else if mark > 0.36 {
                    c = mix(c, coat.mark, 0.5);
                }
            } else {
                let mark = match r {
                    Part::Tail if (tt * std::f32::consts::PI * 9.).sin() > 0.35 => 1.,
                    Part::Haunch | Part::Chest
                        if (px * 1.05 + py * 0.32).sin() > 0.62 && py < GY - 3. =>
                    {
                        1.
                    }
                    Part::Head if hy < -1.6 && (hx * 1.6).sin().abs() > 0.75 => 1.,
                    _ if leg && !walking && (py * 1.7).sin() > 0.7 => 0.8,
                    _ => 0.,
                };
                if r == Part::Muzzle
                    || (r == Part::Chest && px > l.chest.x + 1.2 && py > l.chest.y - 3.)
                {
                    c = coat.cream;
                }
                let threshold =
                    (BAYER4[(y as usize & 3) * 4 + (x as usize & 3)] as f32 + 0.5) / 16.;
                if mark > 0. && mark > threshold {
                    c = coat.mark;
                }
            }
            // Underside and far leg in shade; the top edge catches the light.
            let below = at(x, y + 2) == Part::None || at(x, y + 1) == Part::None;
            if r == Part::FarLeg {
                c = mix(c, coat.mark_shade, 0.35);
            } else if below && r != Part::Ear {
                c = mix(c, coat.mark_shade, 0.22);
            }
            if at(x, y - 1) == Part::None && r != Part::Muzzle {
                c = mix(c, coat.light, 0.55);
            }
            if r == Part::Ear
                && hx.abs() < 3.6
                && hy < -4.9
                && hy > -7.2
                && at(x, y - 1) != Part::None
            {
                c = if coat.points {
                    coat.inner_ear
                } else {
                    mix(coat.inner_ear, c, 0.3)
                };
            }
            put(x, y, c);
        }
    }

    for y in 0..CAT_H as i32 {
        for x in 0..CAT_W as i32 {
            let r = at(x, y);
            if r == Part::None {
                continue;
            }
            // While walking, legs grow out of the body without a seam at the hip or shoulder.
            let seamless = walking && matches!(r, Part::Haunch | Part::Chest);
            let behind = [at(x - 1, y), at(x + 1, y), at(x, y - 1), at(x, y + 1)]
                .into_iter()
                .any(|n| n.layer() > r.layer());
            if !seamless && behind {
                put(x, y, mix(coat.outline, coat.shade, 0.35));
            }
        }
    }

    // The outline, outside the silhouette only.
    for y in 0..CAT_H as i32 {
        for x in 0..CAT_W as i32 {
            if at(x, y) == Part::None
                && [at(x - 1, y), at(x + 1, y), at(x, y - 1), at(x, y + 1)]
                    .into_iter()
                    .any(|n| n != Part::None)
            {
                put(x, y, coat.outline);
            }
        }
    }

    // Eyes, nose and mouth, in the head's frame so they follow its tilt.
    let from_head = |fx: f32, fy: f32| {
        let (c, s) = (p[TILT].cos(), p[TILT].sin());
        [
            (l.head.x + fx * c - fy * s).floor() as i32,
            (l.head.y + fx * s + fy * c).floor() as i32,
        ]
    };
    let side_on = t < 0.45;
    for &[ex, ey] in eyes {
        let [x, y] = from_head(ex, ey);
        if p[EYES] > 0.5 {
            put(x, y, coat.eye);
            put(x, y + 1, coat.eye);
            put(x + side_on as i32, y, coat.outline);
            if !side_on {
                put(x, y + 1, mix(coat.eye, coat.outline, 0.5));
            }
        } else {
            put(x, y + 1, coat.outline);
            put(x + 1, y + 1, coat.outline);
        }
        if p[ANGER] > 0.4 {
            put(x, y - 2, coat.outline);
            put(x + if ex < 0. { 1 } else { -1 }, y - 1, coat.outline);
        }
    }
    let [nx, ny] = from_head(nose[0], nose[1]);
    put(nx, ny, coat.nose);
    if p[ANGER] > 0.5 && p[MOUTH] > 0.3 {
        for y in 1..=3 {
            for x in -2..=2 {
                put(nx + x, ny + y, hex(0x34191c));
            }
        }
        put(nx - 1, ny + 1, coat.cream);
        put(nx + 1, ny + 1, coat.cream);
        for x in -1..=1 {
            put(nx + x, ny + 3, coat.nose);
        }
    } else if p[MOUTH] > 0.15 {
        let open = (p[MOUTH] * 3.2).round() as i32;
        let side = -(side_on as i32);
        for i in 1..=open {
            put(nx + side, ny + i, hex(0x3a1418));
            put(
                nx + side - 1,
                ny + i,
                if i == open {
                    hex(0xe46a7a)
                } else {
                    hex(0x5a1e26)
                },
            );
            if !side_on {
                put(nx + 1, ny + i, hex(0x3a1418));
            }
        }
    } else if p[TONGUE] > 0.5 {
        put(nx, ny + 2, hex(0xe46a7a));
    }
}

/// The cat on its shadow, as the site's `critter` draws it.
fn picture(coat: &Coat, p: &Pose) -> Pixels {
    let mut px = Pixels::new();
    let to = if p[CROUCH] > 0.5 { 38 } else { 30 };
    for x in 6..to {
        let edge = (x - 6).min(to - 1 - x) < 2;
        px.set(
            x,
            H as i32 - 1,
            [30., 20., 18.],
            if edge { 0.2 } else { 0.4 },
        );
    }
    draw_cat(&mut px, coat, p, -1);
    px
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    Sit,
    Loaf,
    Sleep,
}

fn rest(state: State) -> Pose {
    match state {
        State::Sit => SIT,
        State::Loaf => with(SIT, &[(CROUCH, 1.), (TAIL_WRAP, 1.), (TAIL_CURL, 0.)]),
        State::Sleep => with(
            SIT,
            &[
                (CROUCH, 1.),
                (TAIL_WRAP, 1.),
                (TAIL_CURL, 0.),
                (EYES, 0.),
                (HEAD_Y, 3.5),
                (HEAD_X, -2.),
                (TILT, 0.45),
                (TURN, 0.35),
                (EAR_NEAR, 0.4),
            ],
        ),
    }
}

/// A key pose: some numbers set over the state's rest pose, or a whole pose.
enum Key {
    Over(f32, Vec<(usize, f32)>),
    Whole(f32, Pose),
}

impl Key {
    fn at(&self) -> f32 {
        match self {
            Key::Over(at, _) | Key::Whole(at, _) => *at,
        }
    }

    fn pose(&self, base: &Pose) -> Pose {
        match self {
            Key::Over(_, set) => with(*base, set),
            Key::Whole(_, pose) => *pose,
        }
    }
}

struct Action {
    name: &'static str,
    from: State,
    to: Option<State>,
    keys: Vec<Key>,
}

fn over(at: f32, set: &[(usize, f32)]) -> Key {
    Key::Over(at, set.to_vec())
}

fn whole(at: f32, state: State, set: &[(usize, f32)]) -> Key {
    Key::Whole(at, with(rest(state), set))
}

/// A beat repeated `times`: `a` at `start`, `b` at `start + half`, every `every` ms.
fn beats(
    times: usize,
    start: f32,
    half: f32,
    every: f32,
    a: &[(usize, f32)],
    b: &[(usize, f32)],
) -> Vec<Key> {
    (0..times)
        .flat_map(|i| {
            let at = start + i as f32 * every;
            [over(at, a), over(at + half, b)]
        })
        .collect()
}

/// Each action starts and ends on its state's rest pose; the keys between are the performance.
fn actions() -> Vec<Action> {
    use State::*;
    let act = |name, from, to, keys| Action {
        name,
        from,
        to,
        keys,
    };
    let mut lick = vec![
        over(0., &[]),
        over(260., &[(LEAN, -0.6), (HEAD_Y, 0.5)]),
        over(700., &[(PAW, 1.), (HEAD_Y, 1.5), (TILT, 0.3)]),
    ];
    lick.extend(beats(
        4,
        900.,
        210.,
        420.,
        &[
            (PAW, 1.),
            (HEAD_Y, 1.5),
            (TILT, 0.3),
            (TONGUE, 1.),
            (EYES, 0.),
        ],
        &[
            (PAW, 1.),
            (HEAD_Y, 2.2),
            (TILT, 0.42),
            (TONGUE, 0.),
            (EYES, 0.),
        ],
    ));
    lick.extend([
        over(2900., &[(PAW, 1.), (HEAD_Y, 1.2), (TILT, 0.55), (EYES, 0.)]),
        over(
            3400.,
            &[(PAW, 0.9), (HEAD_Y, -0.5), (TILT, -0.2), (EYES, 0.)],
        ),
        over(3800., &[(PAW, 0.), (HEAD_Y, 0.4)]),
        over(4300., &[]),
    ]);
    let watching = [
        (HEAD_Y, -1.6),
        (HEAD_X, 0.6),
        (TILT, -0.32),
        (TAIL_CURL, 0.6),
    ];
    let mut bird = vec![
        over(0., &[]),
        over(260., &[(EAR_NEAR, 0.), (EAR_FAR, 0.), (HEAD_Y, -0.4)]),
        over(700., &watching),
    ];
    bird.extend(beats(
        3,
        1300.,
        130.,
        260.,
        &[watching.as_slice(), &[(MOUTH, 0.3), (TAIL_SWAY, 0.5)]].concat(),
        &[watching.as_slice(), &[(MOUTH, 0.), (TAIL_SWAY, -0.3)]].concat(),
    ));
    let turned = [
        (HEAD_Y, -1.2),
        (HEAD_X, 1.2),
        (TILT, -0.1),
        (TAIL_CURL, 0.5),
    ];
    bird.extend([over(2400., &turned), over(3300., &turned), over(3900., &[])]);
    let grooming = [(HEAD_Y, 2.6), (HEAD_X, -1.), (TILT, 0.65), (LEAN, 0.3)];
    let mut groom = vec![over(0., &[]), over(400., &grooming)];
    groom.extend(beats(
        5,
        600.,
        170.,
        340.,
        &[grooming.as_slice(), &[(TONGUE, 1.), (EYES, 0.)]].concat(),
        &[
            (HEAD_Y, 3.1),
            (HEAD_X, -1.2),
            (TILT, 0.72),
            (LEAN, 0.3),
            (TONGUE, 0.),
            (EYES, 0.),
        ],
    ));
    groom.extend([
        over(2500., &[(HEAD_Y, 1.), (TILT, 0.2), (EYES, 0.6)]),
        over(2900., &[]),
    ]);
    let stretched = [
        (LEAN, 2.4),
        (CROUCH, 0.6),
        (HEAD_Y, 2.2),
        (HEAD_X, 1.5),
        (TILT, -0.2),
        (EYES, 0.),
    ];
    vec![
        act(
            "blink",
            Sit,
            None,
            vec![
                over(0., &[]),
                over(70., &[(EYES, 0.)]),
                over(170., &[(EYES, 0.)]),
                over(240., &[]),
            ],
        ),
        act(
            "slow blink",
            Loaf,
            None,
            vec![
                over(0., &[]),
                over(300., &[(EYES, 0.)]),
                over(900., &[(EYES, 0.)]),
                over(1200., &[]),
            ],
        ),
        act(
            "ear flick",
            Sit,
            None,
            vec![
                over(0., &[]),
                over(60., &[(EAR_NEAR, 1.)]),
                over(160., &[(EAR_NEAR, 0.2)]),
                over(230., &[(EAR_NEAR, 0.9)]),
                over(380., &[]),
            ],
        ),
        act(
            "ear flick",
            Loaf,
            None,
            vec![over(0., &[]), over(70., &[(EAR_FAR, 1.)]), over(260., &[])],
        ),
        act(
            "ear flick",
            Sleep,
            None,
            vec![over(0., &[]), over(80., &[(EAR_NEAR, 1.)]), over(320., &[])],
        ),
        act(
            "tail flick",
            Sit,
            None,
            vec![
                over(0., &[]),
                over(180., &[(TAIL_CURL, 0.9), (TAIL_SWAY, 0.8)]),
                over(420., &[(TAIL_CURL, 0.6), (TAIL_SWAY, -0.4)]),
                over(800., &[]),
            ],
        ),
        act(
            "look at you",
            Sit,
            None,
            vec![
                over(0., &[]),
                over(120., &[(EAR_NEAR, 0.4), (HEAD_Y, -0.5)]),
                over(380., &[(TURN, 1.), (HEAD_Y, -0.5)]),
                over(2200., &[(TURN, 1.)]),
                over(2400., &[(TURN, 1.), (EYES, 0.)]),
                over(2550., &[(TURN, 1.)]),
                over(3000., &[]),
            ],
        ),
        act(
            "head tilt",
            Sit,
            None,
            vec![
                over(0., &[]),
                over(300., &[(TURN, 1.), (EAR_FAR, 0.3)]),
                over(700., &[(TURN, 1.), (TILT, -0.34), (HEAD_X, -0.5)]),
                over(1900., &[(TURN, 1.), (TILT, -0.3)]),
                over(2300., &[(TURN, 1.), (TILT, 0.06)]),
                over(2800., &[]),
            ],
        ),
        act("lick paw", Sit, None, lick),
        act(
            "yawn",
            Sit,
            None,
            vec![
                over(0., &[]),
                over(
                    350.,
                    &[(HEAD_Y, 0.6), (EYES, 0.3), (EAR_NEAR, 0.4), (EAR_FAR, 0.4)],
                ),
                over(
                    900.,
                    &[
                        (HEAD_Y, -1.),
                        (TILT, -0.35),
                        (MOUTH, 0.6),
                        (EYES, 0.),
                        (EAR_NEAR, 0.8),
                        (EAR_FAR, 0.7),
                    ],
                ),
                over(
                    1500.,
                    &[
                        (HEAD_Y, -1.2),
                        (TILT, -0.4),
                        (MOUTH, 1.),
                        (EYES, 0.),
                        (EAR_NEAR, 0.9),
                        (EAR_FAR, 0.8),
                    ],
                ),
                over(
                    1900.,
                    &[(HEAD_Y, -0.6), (TILT, -0.2), (MOUTH, 0.2), (EYES, 0.)],
                ),
                over(2100., &[(TILT, 0.12), (EYES, 0.)]),
                over(2250., &[(TILT, -0.12), (EYES, 0.)]),
                over(2700., &[]),
            ],
        ),
        act("watch a bird", Sit, None, bird),
        act("groom chest", Sit, None, groom),
        act(
            "big stretch",
            Sit,
            None,
            vec![
                over(0., &[]),
                over(500., &[(LEAN, 1.4), (CROUCH, 0.3), (HEAD_Y, 1.)]),
                over(
                    1200.,
                    &[
                        stretched.as_slice(),
                        &[
                            (MOUTH, 0.5),
                            (EAR_NEAR, 0.6),
                            (EAR_FAR, 0.6),
                            (TAIL_CURL, 0.9),
                        ],
                    ]
                    .concat(),
                ),
                over(
                    1900.,
                    &[
                        stretched.as_slice(),
                        &[
                            (MOUTH, 0.8),
                            (EAR_NEAR, 0.7),
                            (EAR_FAR, 0.7),
                            (TAIL_CURL, 1.),
                        ],
                    ]
                    .concat(),
                ),
                over(
                    2500.,
                    &[(LEAN, 0.8), (CROUCH, 0.2), (HEAD_Y, 0.4), (EYES, 0.4)],
                ),
                over(3000., &[]),
            ],
        ),
        act(
            "dream",
            Sleep,
            None,
            vec![
                over(0., &[]),
                over(120., &[(EAR_FAR, 0.8)]),
                over(260., &[(EAR_FAR, 0.1), (MOUTH, 0.15)]),
                over(420., &[(EAR_FAR, 0.7), (MOUTH, 0.)]),
                over(900., &[]),
            ],
        ),
        act(
            "lie down",
            Sit,
            Some(Loaf),
            vec![
                over(0., &[]),
                over(400., &[(LEAN, 1.), (HEAD_Y, 0.6), (TAIL_CURL, 0.)]),
                over(
                    1200.,
                    &[
                        (CROUCH, 0.6),
                        (LEAN, 1.2),
                        (TAIL_WRAP, 0.5),
                        (TAIL_CURL, 0.),
                    ],
                ),
                over(1700., &[(CROUCH, 1.08), (TAIL_WRAP, 0.9), (TAIL_CURL, 0.)]),
                whole(2100., Loaf, &[]),
            ],
        ),
        act(
            "doze off",
            Loaf,
            Some(Sleep),
            vec![
                over(0., &[]),
                over(900., &[(EYES, 0.)]),
                over(1400., &[(EYES, 0.6)]),
                over(2300., &[(EYES, 0.), (HEAD_Y, 1.5), (TILT, 0.2)]),
                whole(3600., Sleep, &[]),
            ],
        ),
        act(
            "wake and sit up",
            Sleep,
            Some(Sit),
            vec![
                over(0., &[]),
                whole(500., Sleep, &[(EAR_NEAR, 0.), (EAR_FAR, 0.5)]),
                whole(1100., Loaf, &[(HEAD_Y, -0.5)]),
                whole(
                    1700.,
                    Loaf,
                    &[
                        (HEAD_Y, -1.),
                        (TILT, -0.35),
                        (MOUTH, 1.),
                        (EYES, 0.),
                        (EAR_NEAR, 0.8),
                    ],
                ),
                whole(2200., Loaf, &[(EYES, 0.5)]),
                whole(
                    2700.,
                    Sit,
                    &[(CROUCH, 0.4), (LEAN, 1.), (TAIL_WRAP, 0.3), (TAIL_CURL, 0.)],
                ),
                whole(
                    3300.,
                    Sit,
                    &[(CROUCH, -0.06), (LEAN, -0.4), (TAIL_CURL, 0.7)],
                ),
                whole(3800., Sit, &[]),
            ],
        ),
    ]
}

/// How a cat spends its time: what it likes doing, how long it rests, how fast it moves.
struct Ways {
    coat: &'static Coat,
    weights: &'static [(&'static str, f32)],
    rest: [f32; 2],
    pace: f32,
    blink_every: [f32; 2],
}

/// The Siamese is curious and alert.
const SIAMESE_WAYS: Ways = Ways {
    coat: &SIAMESE,
    weights: &[
        ("look at you", 3.),
        ("head tilt", 3.),
        ("ear flick", 4.),
        ("tail flick", 3.),
        ("lick paw", 1.),
        ("yawn", 0.6),
        ("lie down", 0.35),
        ("doze off", 0.4),
        ("slow blink", 1.),
        ("wake and sit up", 1.2),
        ("watch a bird", 3.),
        ("groom chest", 1.),
        ("big stretch", 0.8),
        ("dream", 1.),
    ],
    rest: [2500., 6500.],
    pace: 1.,
    blink_every: [2500., 5500.],
};

/// The tabby is sleepy and slow.
const TABBY_WAYS: Ways = Ways {
    coat: &TABBY,
    weights: &[
        ("yawn", 2.5),
        ("lick paw", 2.),
        ("lie down", 1.8),
        ("doze off", 2.5),
        ("look at you", 1.),
        ("ear flick", 2.),
        ("tail flick", 1.),
        ("head tilt", 0.4),
        ("slow blink", 2.),
        ("wake and sit up", 0.5),
        ("watch a bird", 0.6),
        ("groom chest", 2.),
        ("big stretch", 1.6),
        ("dream", 2.5),
    ],
    rest: [4500., 11000.],
    pace: 1.3,
    blink_every: [4000., 8000.],
};

impl Ways {
    fn weight(&self, name: &str) -> f32 {
        self.weights
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(0., |(_, w)| *w)
    }
}

/// The site's seeded random source.
struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        Self((seed % 2147483647).max(1))
    }

    fn next(&mut self) -> f32 {
        self.0 = self.0 * 16807 % 2147483647;
        self.0 as f32 / 2147483647.
    }

    fn span(&mut self, [lo, hi]: [f32; 2]) -> f32 {
        lo + self.next() * (hi - lo)
    }
}

/// What is playing: one of the cat's own actions, or its answer to being petted.
enum Playing {
    Own(usize),
    Petted(Action),
}

struct Cat {
    ways: &'static Ways,
    actions: Vec<Action>,
    state: State,
    random: Random,
    playing: Option<(Playing, f32)>,
    next_at: f32,
    next_blink: f32,
    last: &'static str,
    last_at: Vec<f32>,
}

impl Cat {
    fn new(ways: &'static Ways, seed: u64) -> Self {
        let mut random = Random::new(seed);
        let actions = actions();
        Self {
            ways,
            state: State::Sit,
            next_at: random.span(ways.rest),
            next_blink: random.span(ways.blink_every),
            random,
            playing: None,
            last: "",
            last_at: vec![f32::MIN; actions.len()],
            actions,
        }
    }

    fn action(&self) -> Option<(&Action, f32)> {
        self.playing.as_ref().map(|(playing, started)| {
            (
                match playing {
                    Playing::Own(i) => &self.actions[*i],
                    Playing::Petted(action) => action,
                },
                *started,
            )
        })
    }

    fn choose(&mut self, now: f32) -> Option<usize> {
        let options: Vec<(usize, f32)> = self
            .actions
            .iter()
            .enumerate()
            .filter(|(i, a)| {
                a.from == self.state
                    && a.name != "blink"
                    && a.name != self.last
                    && now - self.last_at[*i] > 12000.
            })
            .map(|(i, a)| (i, self.ways.weight(a.name)))
            .collect();
        let total: f32 = options.iter().map(|(_, w)| w).sum();
        let mut pick = self.random.next() * total;
        options.into_iter().find_map(|(i, w)| {
            pick -= w;
            (pick <= 0.).then_some(i)
        })
    }

    /// Moves the cat's day on to `now` (ms): ends an action that is over, starts the next one.
    fn advance(&mut self, now: f32) {
        let playing = self.action().map(|(action, started)| {
            let end = action.keys.last().map_or(0., Key::at) * self.ways.pace;
            (now - started >= end, action.to, action.name)
        });
        if let Some((over, to, name)) = playing {
            if !over {
                return;
            }
            if let Some(to) = to {
                self.state = to;
            }
            if name != "blink" {
                self.last = name;
            }
            if let Some((Playing::Own(i), _)) = self.playing {
                self.last_at[i] = now;
            }
            self.playing = None;
            self.next_at = now + self.random.span(self.ways.rest);
        } else if now >= self.next_at {
            match self.choose(now) {
                Some(i) => self.playing = Some((Playing::Own(i), now)),
                None => self.next_at = now + 1500.,
            }
        } else if now >= self.next_blink && self.state == State::Sit {
            self.playing = Some((Playing::Own(0), now));
            self.next_blink = now + self.random.span(self.ways.blink_every);
        }
    }

    /// When the cat next needs a look: now while it moves, else when it next does something.
    fn next_change(&self) -> Option<f32> {
        match self.playing {
            Some(_) => None,
            None if self.state == State::Sit => Some(self.next_at.min(self.next_blink)),
            None => Some(self.next_at),
        }
    }

    fn pose(&self, now: f32) -> Pose {
        let base = rest(self.state);
        let Some((action, started)) = self.action() else {
            return base;
        };
        let t = (now - started) / self.ways.pace;
        let keys = &action.keys;
        let mut i = 0;
        while i + 2 < keys.len() && t > keys[i + 1].at() {
            i += 1;
        }
        let (a, b) = (&keys[i], &keys[i + 1]);
        let span = b.at() - a.at();
        let k = unit((t - a.at()) / if span == 0. { 1. } else { span });
        let k = k * k * (3. - 2. * k);
        let (pa, pb) = (a.pose(&base), b.pose(&base));
        std::array::from_fn(|f| lerp(pa[f], pb[f], k))
    }

    /// A pet interrupts whatever the cat was doing. Awake, it yells at you, head up and mouth
    /// wide, twice; asleep, it wakes with a hiss and settles again.
    fn pet(&mut self, now: f32) {
        let current = self.pose(now);
        let back = rest(self.state);
        let key = Key::Whole;
        let keys = if self.state != State::Sleep {
            let meow = with(
                SIT,
                &[
                    (TURN, 1.),
                    (HEAD_Y, -1.2),
                    (TILT, -0.18),
                    (EAR_NEAR, 0.),
                    (EAR_FAR, 0.),
                    (EYES, 0.7),
                    (MOUTH, 1.),
                    (TAIL_CURL, 0.9),
                ],
            );
            vec![
                key(0., current),
                key(
                    220.,
                    with(SIT, &[(TURN, 1.), (HEAD_Y, -0.6), (TAIL_CURL, 0.7)]),
                ),
                key(480., with(meow, &[(TAIL_SWAY, 0.6)])),
                key(900., with(meow, &[(MOUTH, 0.85), (TAIL_SWAY, -0.4)])),
                key(
                    1100.,
                    with(meow, &[(MOUTH, 0.1), (HEAD_Y, -0.8), (EYES, 1.)]),
                ),
                key(1350., with(meow, &[(TAIL_SWAY, 0.5)])),
                key(1800., with(meow, &[(MOUTH, 0.9)])),
                key(2150., with(SIT, &[(TURN, 1.), (TAIL_CURL, 0.6)])),
                key(2700., back),
            ]
        } else {
            let hiss = with(
                SIT,
                &[
                    (CROUCH, 0.1),
                    (TURN, 1.),
                    (HEAD_X, -1.),
                    (EAR_NEAR, 1.),
                    (EAR_FAR, 1.),
                    (ANGER, 1.),
                    (MOUTH, 0.9),
                    (TAIL_CURL, 0.8),
                ],
            );
            vec![
                key(0., current),
                key(180., with(current, &[(EYES, 0.5), (EAR_NEAR, 0.8)])),
                key(
                    450.,
                    with(
                        SIT,
                        &[(CROUCH, 0.3), (TURN, 1.), (EAR_NEAR, 0.6), (EAR_FAR, 0.6)],
                    ),
                ),
                key(700., with(hiss, &[(TAIL_SWAY, 0.9)])),
                key(1100., with(hiss, &[(TAIL_SWAY, -0.8), (HEAD_Y, -0.5)])),
                key(1500., with(hiss, &[(TAIL_SWAY, 0.6)])),
                key(1800., with(hiss, &[(MOUTH, 0.), (ANGER, 0.7), (EYES, 0.5)])),
                key(2200., with(rest(State::Loaf), &[(TURN, 1.), (EYES, 0.5)])),
                key(2800., back),
            ]
        };
        let name = if self.state == State::Sleep {
            "grumpy"
        } else {
            "meow"
        };
        self.playing = Some((
            Playing::Petted(Action {
                name,
                from: self.state,
                to: None,
                keys,
            }),
            now,
        ));
    }
}

/// The bar is this wide before the cat has room beside Play, and the window this tall.
const ROOM: (f32, f32) = (1000., 560.);
/// Screen pixels per pixel of the cat, at a scale factor of 1.
const SIZE: f32 = 2.;
const WIDTH: f32 = W as f32 * SIZE;
/// A cat acting in place is drawn 12 times a second, a cat on the move 25 times.
const FRAME: Duration = Duration::from_millis(83);
const STEP_FRAME: Duration = Duration::from_millis(40);
/// The cat's lane runs this far under Play's edge, so the cat comes out from behind the button.
const UNDER: f32 = 8.;
/// A peek shows the head and chest, and lasts this long.
const PEEK_OUT: f32 = 44.;
const PEEK: f32 = 4200.;
/// Walking: pixels a millisecond, and the leg swing that keeps the paws from sliding at that pace.
const SPEED: f32 = 0.055;
const STRIDE: f32 = 0.012;
/// Sleeping on Play: the climb up from behind the button, curling up, and uncurling again, in
/// ms. A cat that has to leave at once drops back behind the button in `DROP`.
const CLIMB: f32 = 800.;
const CURL: f32 = 1300.;
const DROP: f32 = 260.;

/// Where the cat is. Mostly it is away, behind Play; now and then it peeks out, or walks out
/// into the empty part of the bar, stays a while and walks back.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Scene {
    Away {
        until: f32,
    },
    Peek {
        since: f32,
    },
    /// `from` and `to` are how far out of hiding the cat is, in pixels.
    Walk {
        from: f32,
        to: f32,
        since: f32,
    },
    Visit {
        at: f32,
        until: f32,
    },
    /// Asleep on top of Play until `until`, then it uncurls and climbs back down.
    Perch {
        since: f32,
        until: f32,
    },
    /// Off Play at once, from the `pose` and height (`up`) it had.
    Drop {
        since: f32,
        pose: Pose,
        up: f32,
    },
}

/// What to draw of the cat: how far out it is, its pose, and whether it faces left.
struct View {
    out: f32,
    pose: Pose,
    left: bool,
    pettable: bool,
    /// On Play instead of in the bar: how far it has climbed over the button's top edge, 0 to 1.
    perch: Option<f32>,
}

struct Outing {
    cat: Cat,
    scene: Scene,
    /// The first time out is a visit, so the cat introduces itself properly.
    been_out: bool,
    /// Its next time out is a sleep on Play, when Play is free.
    perch_next: bool,
}

fn ease(k: f32) -> f32 {
    let k = unit(k);
    k * k * (3. - 2. * k)
}

impl Outing {
    fn new(ways: &'static Ways, seed: u64, first: f32) -> Self {
        Self {
            cat: Cat::new(ways, seed),
            scene: Scene::Away { until: first },
            been_out: false,
            perch_next: false,
        }
    }

    /// The cat on Play at `now`: its pose and how far up it is.
    fn perched(since: f32, until: f32, now: f32) -> (Pose, f32) {
        let (t, left) = (now - since, now - until);
        // Curled up once it is up, and uncurled again before it climbs down.
        let curl = ease((t - CLIMB) / CURL) * ease(-left / CURL);
        let (sit, sleep) = (with(SIT, &[(TURN, 0.35)]), rest(State::Sleep));
        let mut pose = sit;
        for (value, asleep) in pose.iter_mut().zip(sleep) {
            *value = lerp(*value, asleep, curl);
        }
        // Eyes are open or shut, never half.
        pose[EYES] = if curl > 0.6 { 0. } else { 1. };
        (pose, ease(t / CLIMB) * ease((CURL + CLIMB - left) / CLIMB))
    }

    /// Whatever it was doing on Play, it gets off now.
    fn leave_play(&mut self, now: f32) {
        if let Scene::Perch { since, until } = self.scene {
            let (pose, up) = Self::perched(since, until, now);
            self.scene = Scene::Drop {
                since: now,
                pose: with(pose, &[(EYES, 1.), (EAR_NEAR, 0.), (EAR_FAR, 0.)]),
                up,
            };
        }
    }

    fn hide(&mut self, now: f32) {
        self.scene = Scene::Away {
            until: now + self.cat.random.span([60000., 200000.]),
        };
    }

    /// Moves the scene on to `now` (ms). `room` is the empty width beside Play, in pixels: the
    /// cat never goes further out than that, and stays hidden when there is none. `free` says
    /// Play is idle and the bar has nothing to say, so the cat may sleep on the button.
    fn step(&mut self, now: f32, room: f32, free: bool) {
        match self.scene {
            Scene::Perch { .. } if !free => self.leave_play(now),
            Scene::Perch { until, .. } if now >= until + CURL + CLIMB => self.hide(now),
            Scene::Drop { since, .. } if now - since >= DROP => self.hide(now),
            Scene::Away { until } if now >= until => {
                if free && (self.perch_next || self.been_out && self.cat.random.next() < 0.25) {
                    self.perch_next = false;
                    self.scene = Scene::Perch {
                        since: now,
                        until: now + self.cat.random.span([45000., 150000.]),
                    };
                } else if room < PEEK_OUT {
                    self.scene = Scene::Away { until: now + 5000. };
                } else if room >= WIDTH * 1.5 && (!self.been_out || self.cat.random.next() < 0.7) {
                    let far = room.min(WIDTH + 16. + 520.);
                    let to = self.cat.random.span([WIDTH + 16., far]);
                    self.scene = Scene::Walk {
                        from: 0.,
                        to: (to / SIZE).round() * SIZE,
                        since: now,
                    };
                    self.been_out = true;
                } else {
                    self.scene = Scene::Peek { since: now };
                }
            }
            Scene::Peek { since } if now - since >= PEEK => self.hide(now),
            Scene::Walk { from, to, since } => {
                let gone = (now - since) * SPEED;
                if gone >= (to - from).abs() {
                    if to <= 0. {
                        self.hide(now);
                    } else {
                        // It sits down fresh, and looks around before doing anything else.
                        self.cat.state = State::Sit;
                        self.cat.playing = None;
                        self.cat.next_at = now + self.cat.random.span([900., 2200.]);
                        self.scene = Scene::Visit {
                            at: to,
                            until: now + self.cat.random.span([20000., 60000.]),
                        };
                    }
                } else if to > from && from + gone > room {
                    // The room shrank ahead of it: it turns round.
                    self.scene = Scene::Walk {
                        from: from + gone,
                        to: 0.,
                        since: now,
                    };
                }
            }
            Scene::Visit { at, until } => {
                self.cat.advance(now);
                // It goes home when its time is up, but never mid-action and never asleep.
                if (now >= until || at > room)
                    && self.cat.playing.is_none()
                    && self.cat.state == State::Sit
                {
                    self.scene = Scene::Walk {
                        from: at,
                        to: 0.,
                        since: now,
                    };
                }
            }
            _ => {}
        }
    }

    fn view(&self, now: f32) -> Option<View> {
        match self.scene {
            Scene::Away { .. } => None,
            Scene::Perch { since, until } => {
                let (pose, up) = Self::perched(since, until, now);
                Some(View {
                    out: 0.,
                    pose,
                    left: true,
                    pettable: false,
                    perch: Some(up),
                })
            }
            Scene::Drop { since, pose, up } => Some(View {
                out: 0.,
                pose,
                left: true,
                pettable: false,
                perch: Some(up * (1. - ease((now - since) / DROP))),
            }),
            Scene::Peek { since } => {
                let t = now - since;
                // Out, a look at you with a blink and a tilt of the head, and back in.
                let look = ease((t - 900.) / 300.) * ease((PEEK - 900. - t) / 300.);
                let tilt = ease((t - 2500.) / 250.) * ease((3400. - t) / 250.);
                let blink = (1900. ..2050.).contains(&t);
                Some(View {
                    out: PEEK_OUT * ease(t / 700.) * ease((PEEK - t) / 600.),
                    pose: with(
                        SIT,
                        &[
                            (TURN, look),
                            (TILT, -0.3 * tilt),
                            (EYES, if blink { 0. } else { 1. }),
                        ],
                    ),
                    left: true,
                    pettable: false,
                    perch: None,
                })
            }
            Scene::Walk { from, to, since } => {
                let t = now - since;
                let gone = (t * SPEED).min((to - from).abs());
                Some(View {
                    out: from + gone * (to - from).signum(),
                    pose: with(
                        SIT,
                        &[(WALK, t * STRIDE), (TAIL_SWAY, (t * STRIDE * 0.5).sin())],
                    ),
                    left: to > from,
                    pettable: false,
                    perch: None,
                })
            }
            Scene::Visit { at, .. } => Some(View {
                out: at,
                pose: self.cat.pose(now),
                left: true,
                pettable: true,
                perch: None,
            }),
        }
    }

    /// How long until the cat next needs a look.
    fn next_change(&self, now: f32) -> Duration {
        let until = |at: f32| Duration::from_secs_f32((at - now).max(0.) / 1000.);
        match self.scene {
            Scene::Away { until: at } => until(at),
            Scene::Peek { .. } | Scene::Walk { .. } | Scene::Drop { .. } => STEP_FRAME,
            // Asleep it is one still picture: the next look is when it wakes.
            Scene::Perch { since, until } => {
                if now - since < CLIMB + CURL || now >= until - CURL {
                    STEP_FRAME
                } else {
                    self::until_ms(until - CURL - now)
                }
            }
            Scene::Visit { until: at, .. } => self
                .cat
                .next_change()
                .map_or(FRAME, |change| until(change.min(at.max(now + 500.)))),
        }
    }
}

fn until_ms(ms: f32) -> Duration {
    Duration::from_secs_f32(ms.max(0.) / 1000.)
}

#[derive(Default)]
pub(super) struct CatState {
    cat: Option<Outing>,
    /// The cat's own time, in ms: it stands still while the window is in the background.
    clock: f32,
    seen: Option<Instant>,
    /// The width of the cat's lane as last laid out.
    lane: std::rc::Rc<std::cell::Cell<f32>>,
    /// The picture on screen, and the pose, scale and facing it was drawn for.
    shown: Option<(Pose, usize, bool, Arc<RenderImage>)>,
    /// When the pending wake fires: one timer at a time.
    wake: Option<Instant>,
}

impl Launcher {
    /// The empty stretch of the play bar between its text and Play, and the cat when it is out.
    /// The cat is drawn inside this stretch and cut at its edges, under Play, so it comes out
    /// from behind the button and can never lie over text or a control. `beside` says Play is
    /// what follows; with other buttons in between the cat stays in.
    ///
    /// The second element is the cat asleep on Play, for Play's own box: it is drawn only above
    /// the button's top edge and takes no clicks. `free` says Play is idle and the bar has
    /// nothing to say; the cat climbs up only then, and drops off the moment that ends.
    pub(super) fn cat(
        &mut self,
        beside: bool,
        free: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Div, Option<Div>) {
        let measured = self.ui.cat.lane.clone();
        let lane = div()
            .flex_1()
            .min_w_0()
            .h(px(H as f32 * SIZE))
            .relative()
            .overflow_hidden()
            // Over the bar's gap and a little under Play.
            .when(beside, |d| d.mr(px(-(14. + UNDER))))
            .child(
                gpui::canvas(
                    move |bounds, _, _| measured.set(f32::from(bounds.size.width)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        let roomy = beside && self.window_width >= ROOM.0 && self.window_height >= ROOM.1;
        let room = if roomy {
            self.ui.cat.lane.get() - UNDER - 6.
        } else {
            0.
        };
        let free = free && roomy;
        // A window in the background keeps its cat still: nothing is scheduled until it is back.
        let awake = window.is_window_active();
        if awake {
            if let Some(seen) = self.ui.cat.seen {
                self.ui.cat.clock += seen.elapsed().as_secs_f32() * 1000.;
            }
        }
        self.ui.cat.seen = awake.then(Instant::now);
        let now = self.ui.cat.clock;
        let outing = self.ui.cat.cat.get_or_insert_with(|| {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(4, |d| d.as_millis() as u64);
            // One of the site's two cats, a different one from launch to launch.
            let ways = if seed.is_multiple_of(2) {
                &SIAMESE_WAYS
            } else {
                &TABBY_WAYS
            };
            let mut outing = Outing::new(ways, seed, 0.);
            outing.scene = Scene::Away {
                until: outing.cat.random.span([15000., 40000.]),
            };
            outing
        });
        if awake {
            if room <= 0.
                && matches!(
                    outing.scene,
                    Scene::Peek { .. } | Scene::Walk { .. } | Scene::Visit { .. }
                )
            {
                outing.hide(now);
            }
            outing.step(now, room, free);
        }
        let after = outing.next_change(now);
        let coat = outing.cat.ways.coat;
        let view = outing
            .view(now)
            .filter(|view| room > 0. || view.perch.is_some());
        if awake {
            self.cat_wake(after, cx);
        }
        let Some(view) = view else {
            return (lane, None);
        };
        let scale = (SIZE * self.scale).round().max(1.) as usize;
        let image = match &self.ui.cat.shown {
            Some((drawn, at, left, image))
                if *drawn == view.pose && *at == scale && *left == view.left =>
            {
                image.clone()
            }
            _ => {
                let small = picture(coat, &view.pose);
                let mut big = vec![0u8; W * scale * H * scale * 4];
                for (y, row) in big.chunks_exact_mut(W * scale * 4).enumerate() {
                    for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        // The cat is drawn facing right; facing left is its mirror image.
                        let x = if view.left {
                            W - 1 - x / scale
                        } else {
                            x / scale
                        };
                        let from = (y / scale * W + x) * 4;
                        px.copy_from_slice(&small.0[from..from + 4]);
                    }
                }
                let frame = image::RgbaImage::from_raw((W * scale) as u32, (H * scale) as u32, big)
                    .expect("the buffer is the picture's size");
                let image = Arc::new(RenderImage::new([image::Frame::new(frame)]));
                if let Some((_, _, _, old)) =
                    self.ui
                        .cat
                        .shown
                        .replace((view.pose, scale, view.left, image.clone()))
                {
                    let _ = window.drop_image(old);
                }
                image
            }
        };
        let tall = H as f32 * SIZE;
        if let Some(up) = view.perch {
            // A window above Play's top edge, as tall as the cat: it climbs into it from below,
            // its feet on the edge, and nothing of it is ever drawn over the button.
            let feet = (H as f32 - GY - 1.) * SIZE;
            let down = ((1. - up) * (tall - feet) / SIZE).round() * SIZE;
            let perch = div()
                .absolute()
                .top(px(feet - tall))
                .right(px(18.))
                .w(px(WIDTH))
                .h(px(tall - feet))
                .overflow_hidden()
                .child(
                    gpui::img(image)
                        .absolute()
                        .top(px(down))
                        .left(px(0.))
                        .w(px(WIDTH))
                        .h(px(tall)),
                );
            return (lane, Some(perch));
        }
        // Whole cat pixels, so the picture stays crisp as it moves.
        let out = (view.out / SIZE).round() * SIZE;
        let cat = div()
            .id("alt-cat")
            .absolute()
            .top(px(0.))
            .right(px(out + UNDER - WIDTH))
            .child(gpui::img(image).w(px(WIDTH)).h(px(tall)));
        let lane = lane.child(if view.pettable {
            cat.cursor_pointer()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    let now = this.ui.cat.clock;
                    if let Some(outing) = &mut this.ui.cat.cat {
                        outing.cat.pet(now);
                    }
                    cx.notify();
                }))
        } else {
            cat
        });
        (lane, None)
    }

    /// Play was pressed: a cat asleep on it hops off.
    pub(super) fn cat_off_play(&mut self) {
        let now = self.ui.cat.clock;
        if let Some(outing) = &mut self.ui.cat.cat {
            outing.leave_play(now);
        }
    }

    /// Draws the launcher again `after` this, unless a wake is already due by then.
    fn cat_wake(&mut self, after: Duration, cx: &mut Context<Self>) {
        let at = Instant::now() + after;
        if self
            .ui
            .cat
            .wake
            .is_some_and(|due| due <= at + Duration::from_millis(5))
        {
            return;
        }
        self.ui.cat.wake = Some(at);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            let _ = this.update(cx, |this, cx| {
                if this.ui.cat.wake == Some(at) {
                    this.ui.cat.wake = None;
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque(px: &Pixels) -> usize {
        px.0.as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255)
            .count()
    }

    #[test]
    fn the_cat_hides_comes_out_into_the_room_it_has_and_goes_back() {
        let mut outing = Outing::new(&TABBY_WAYS, 7, 1000.);
        outing.step(0., 400., false);
        assert!(outing.view(0.).is_none(), "it starts out of sight");
        let (mut now, mut furthest, mut visited) = (0., 0f32, false);
        // Until it has come out and is away again.
        while !(visited && matches!(outing.scene, Scene::Away { .. })) {
            now += 40.;
            assert!(now < 600_000., "it went home within ten minutes");
            outing.step(now, 400., false);
            if let Some(view) = outing.view(now) {
                furthest = furthest.max(view.out);
                visited |= view.pettable;
            }
        }
        assert!(
            furthest > WIDTH && furthest <= 400.,
            "wholly out, and never past the room: {furthest}"
        );
        assert!(outing.view(now).is_none());
    }

    #[test]
    fn with_too_little_room_the_cat_only_peeks_and_with_none_it_stays_in() {
        let mut outing = Outing::new(&SIAMESE_WAYS, 3, 0.);
        for step in 0..200 {
            outing.step(step as f32 * 40., 0., false);
            assert!(outing.view(step as f32 * 40.).is_none());
        }
        let mut furthest = 0f32;
        for step in 200..2000 {
            let now = step as f32 * 40.;
            outing.step(now, 60., false);
            if let Some(view) = outing.view(now) {
                assert!(!view.pettable);
                furthest = furthest.max(view.out);
            }
        }
        assert!(
            furthest > PEEK_OUT - 1. && furthest <= PEEK_OUT,
            "head and chest, no further: {furthest}"
        );
    }

    #[test]
    fn on_play_the_cat_sleeps_without_frames_and_drops_off_when_play_is_needed() {
        let mut outing = Outing::new(&TABBY_WAYS, 7, 0.);
        outing.perch_next = true;
        // Play is busy: it does not climb up.
        outing.step(0., 400., false);
        assert!(!matches!(outing.scene, Scene::Perch { .. }));
        let mut outing = Outing::new(&TABBY_WAYS, 7, 0.);
        outing.perch_next = true;
        outing.step(0., 0., true);
        let mut now = 0.;
        while now < CLIMB + CURL + 40. {
            now += 40.;
            outing.step(now, 0., true);
        }
        let view = outing.view(now).expect("it is on Play");
        assert_eq!(view.perch, Some(1.), "wholly up");
        assert_eq!(view.pose, rest(State::Sleep), "and curled up asleep");
        let wait = outing.next_change(now);
        assert!(wait >= Duration::from_secs(40), "no frames while it sleeps");
        assert_eq!(outing.view(now + 30_000.).map(|v| v.pose), Some(view.pose));
        // Play is pressed, or turns busy: off it goes, and it is gone within the drop.
        outing.step(now + 30_000., 0., false);
        assert!(matches!(outing.scene, Scene::Drop { .. }));
        outing.step(now + 30_000. + DROP, 0., false);
        assert!(outing.view(now + 30_000. + DROP).is_none());
    }

    #[test]
    fn a_resting_cat_asks_for_no_frames_until_it_next_does_something() {
        let mut cat = Cat::new(&TABBY_WAYS, 7);
        cat.advance(0.);
        let next = cat.next_change().expect("a resting cat waits");
        assert!(
            next >= 4000.,
            "the first rest or blink is seconds away: {next}"
        );
        assert_eq!(
            cat.pose(0.),
            cat.pose(next - 1.),
            "and it holds still until then"
        );
        cat.advance(next);
        assert!(cat.next_change().is_none(), "then it moves, frame by frame");
    }

    #[test]
    fn petting_wakes_a_meow_and_the_cat_settles_back_where_it_was() {
        let mut cat = Cat::new(&SIAMESE_WAYS, 3);
        cat.pet(100.);
        assert_eq!(cat.action().map(|(a, _)| a.name), Some("meow"));
        let loud = cat.pose(100. + 480.);
        assert!(
            loud[MOUTH] > 0.9 && loud[TURN] > 0.9,
            "mouth wide, facing you"
        );
        cat.advance(100. + 2701.);
        assert!(cat.action().is_none());
        assert_eq!(cat.pose(3000.), rest(State::Sit));
    }

    #[test]
    fn a_sleeping_cat_hisses_when_petted_and_stays_asleep_after() {
        let mut cat = Cat::new(&TABBY_WAYS, 5);
        cat.state = State::Sleep;
        cat.pet(0.);
        assert_eq!(cat.action().map(|(a, _)| a.name), Some("grumpy"));
        assert!(cat.pose(700. * TABBY_WAYS.pace)[ANGER] > 0.9);
        cat.advance(2800. * TABBY_WAYS.pace + 1.);
        assert_eq!(cat.state, State::Sleep);
    }

    #[test]
    fn each_coat_draws_a_whole_cat_with_its_own_eyes() {
        for (coat, eye) in [(&SIAMESE, [255, 178, 92]), (&TABBY, [78, 214, 196])] {
            let px = picture(coat, &SIT);
            assert!(opaque(&px) > 500, "a body, not a few pixels");
            assert!(
                px.0.as_chunks::<4>().0.iter().any(|p| p[..3] == eye),
                "its eye colour, as BGR"
            );
        }
    }

    #[test]
    fn every_action_ends_where_its_state_rests() {
        for (i, action) in actions().iter().enumerate() {
            let mut cat = Cat::new(&SIAMESE_WAYS, 9);
            cat.state = action.from;
            cat.playing = Some((Playing::Own(i), 0.));
            let end = action.keys.last().map_or(0., Key::at);
            let to = action.to.unwrap_or(action.from);
            let (ends, rests) = (cat.pose(end), rest(to));
            assert!(
                ends.iter().zip(rests).all(|(a, b)| (a - b).abs() < 1e-4),
                "{} ends at rest",
                action.name
            );
        }
    }
}
