//! Drawing a pose: the pixel helpers, the layout of its parts, and the picture.
use super::*;

pub(super) type Rgb = [f32; 3];

pub(super) const fn hex(v: u32) -> Rgb {
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

pub(super) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(super) fn unit(v: f32) -> f32 {
    v.clamp(0., 1.)
}

const CAT_W: usize = 48;
const CAT_H: usize = 36;
/// The picture: the cat on the bottom edge, where its shadow lies.
pub(super) const W: usize = CAT_W;
pub(super) const H: usize = CAT_H - 1;
pub(super) const GY: f32 = 33.;

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
pub(super) struct Pixels(pub(super) Vec<u8>);

impl Pixels {
    pub(super) fn new() -> Self {
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
pub(super) fn picture(coat: &Coat, p: &Pose) -> Pixels {
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
