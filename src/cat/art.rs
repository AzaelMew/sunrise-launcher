//! The cat's look: the pose a frame is drawn from, and the two coats.
use super::*;

pub(super) const BAYER4: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

// A pose is these numbers; a key of an action sets some of them.
pub(super) const BREATH: usize = 0;
pub(super) const LEAN: usize = 1;
pub(super) const CROUCH: usize = 2;
pub(super) const HEAD_X: usize = 3;
pub(super) const HEAD_Y: usize = 4;
pub(super) const TILT: usize = 5;
pub(super) const TURN: usize = 6;
pub(super) const EAR_NEAR: usize = 7;
pub(super) const EAR_FAR: usize = 8;
pub(super) const EYES: usize = 9;
pub(super) const MOUTH: usize = 10;
pub(super) const TONGUE: usize = 11;
pub(super) const PAW: usize = 12;
pub(super) const TAIL_CURL: usize = 13;
pub(super) const TAIL_SWAY: usize = 14;
pub(super) const TAIL_WRAP: usize = 15;
pub(super) const ANGER: usize = 16;
pub(super) const WALK: usize = 17;

pub(super) type Pose = [f32; 18];

pub(super) const SIT: Pose = {
    let mut p = [0.; 18];
    p[EYES] = 1.;
    p[TAIL_CURL] = 0.3;
    p[WALK] = -1.;
    p
};

pub(super) fn with(mut base: Pose, set: &[(usize, f32)]) -> Pose {
    for &(field, value) in set {
        base[field] = value;
    }
    base
}

pub(super) struct Coat {
    pub(super) base: Rgb,
    pub(super) shade: Rgb,
    pub(super) light: Rgb,
    pub(super) mark: Rgb,
    pub(super) mark_shade: Rgb,
    pub(super) cream: Rgb,
    pub(super) eye: Rgb,
    pub(super) nose: Rgb,
    pub(super) inner_ear: Rgb,
    pub(super) outline: Rgb,
    /// Dark points (the Siamese), else stripes.
    pub(super) points: bool,
    pub(super) tail_width: f32,
}

/// Seal-point Siamese: cream body, dark points, blue eyes, a slim tail.
pub(super) const SIAMESE: Coat = Coat {
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
pub(super) const TABBY: Coat = Coat {
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
