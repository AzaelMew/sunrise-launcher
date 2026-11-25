//! How each coat acts, and the cat that picks its next action.
use super::*;

/// How a cat spends its time: what it likes doing, how long it rests, how fast it moves.
pub(super) struct Ways {
    pub(super) coat: &'static Coat,
    weights: &'static [(&'static str, f32)],
    rest: [f32; 2],
    pub(super) pace: f32,
    blink_every: [f32; 2],
}

/// The Siamese is curious and alert.
pub(super) const SIAMESE_WAYS: Ways = Ways {
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
pub(super) const TABBY_WAYS: Ways = Ways {
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
pub(super) struct Random(u64);

impl Random {
    pub(super) fn new(seed: u64) -> Self {
        Self((seed % 2147483647).max(1))
    }

    pub(super) fn next(&mut self) -> f32 {
        self.0 = self.0 * 16807 % 2147483647;
        self.0 as f32 / 2147483647.
    }

    pub(super) fn span(&mut self, [lo, hi]: [f32; 2]) -> f32 {
        lo + self.next() * (hi - lo)
    }
}

/// What is playing: one of the cat's own actions, or its answer to being petted.
pub(super) enum Playing {
    Own(usize),
    Petted(Action),
}

pub(super) struct Cat {
    pub(super) ways: &'static Ways,
    actions: Vec<Action>,
    pub(super) state: State,
    pub(super) random: Random,
    pub(super) playing: Option<(Playing, f32)>,
    pub(super) next_at: f32,
    next_blink: f32,
    last: &'static str,
    last_at: Vec<f32>,
}

impl Cat {
    pub(super) fn new(ways: &'static Ways, seed: u64) -> Self {
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

    pub(super) fn action(&self) -> Option<(&Action, f32)> {
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
    pub(super) fn advance(&mut self, now: f32) {
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
    pub(super) fn next_change(&self) -> Option<f32> {
        match self.playing {
            Some(_) => None,
            None if self.state == State::Sit => Some(self.next_at.min(self.next_blink)),
            None => Some(self.next_at),
        }
    }

    pub(super) fn pose(&self, now: f32) -> Pose {
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
    pub(super) fn pet(&mut self, now: f32) {
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
