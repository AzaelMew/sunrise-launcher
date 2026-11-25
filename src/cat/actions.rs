//! What the cat does: each action as poses over time.
use super::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum State {
    Sit,
    Loaf,
    Sleep,
}

pub(super) fn rest(state: State) -> Pose {
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
pub(super) enum Key {
    Over(f32, Vec<(usize, f32)>),
    Whole(f32, Pose),
}

impl Key {
    pub(super) fn at(&self) -> f32 {
        match self {
            Key::Over(at, _) | Key::Whole(at, _) => *at,
        }
    }

    pub(super) fn pose(&self, base: &Pose) -> Pose {
        match self {
            Key::Over(_, set) => with(*base, set),
            Key::Whole(_, pose) => *pose,
        }
    }
}

pub(super) struct Action {
    pub(super) name: &'static str,
    pub(super) from: State,
    pub(super) to: Option<State>,
    pub(super) keys: Vec<Key>,
}

pub(super) fn over(at: f32, set: &[(usize, f32)]) -> Key {
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
pub(super) fn actions() -> Vec<Action> {
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
