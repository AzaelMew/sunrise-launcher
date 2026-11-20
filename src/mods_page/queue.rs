//! What waits behind the install on screen: installs asked for while another runs, in order,
//! and the one build that takes up what they put in. Each is bound to the profile and game it
//! was asked for, whatever is selected by the time its turn comes.
use std::collections::VecDeque;
use std::path::PathBuf;
use sunrise_launcher::browser::{Project, SourceId};

/// The profile an install or build was asked for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Target {
    pub(crate) instance: String,
    pub(super) home: PathBuf,
    /// The Minecraft version that profile plays.
    pub(super) game: String,
}

/// An install that was asked for and waits its turn.
pub(crate) enum Request {
    One {
        target: Target,
        source: SourceId,
        id: String,
        known: Option<Box<Project>>,
        /// Installed registry mods this install removes with it (the ones it can't live with).
        replacing: Vec<sunrise_launcher::browser::installed::Replace>,
    },
    /// A collection's mods, and the collection's name.
    Many(Target, Vec<String>, String),
}

/// What runs next.
pub(super) enum Step {
    Install(Request),
    /// The native mods the build takes up, on the profile selected when they went in.
    Build(Vec<String>),
}

#[derive(Default)]
pub(super) struct Queue {
    waiting: VecDeque<Request>,
    /// An install put native code in: one build takes it up. Each mod with the profile it is in.
    build: Vec<(String, String)>,
}

impl Queue {
    /// A request just asked for: it starts now (returned) unless an install or a build runs or
    /// others wait, and then it goes behind them. This is the only way in: what the queue hands
    /// out through [`Self::next`] starts without passing here again.
    pub(super) fn submit(&mut self, request: Request, busy: bool) -> Option<Request> {
        if busy || !self.waiting.is_empty() {
            self.waiting.push_back(request);
            None
        } else {
            Some(request)
        }
    }

    /// Whether anything waits for its turn: a new request goes behind it.
    #[cfg(test)]
    pub(super) fn is_waiting(&self) -> bool {
        !self.waiting.is_empty()
    }

    /// Whether mod `id` waits for its turn to install.
    pub(super) fn waits(&self, id: &str) -> bool {
        self.waiting
            .iter()
            .any(|r| matches!(r, Request::One { id: waiting, .. } if waiting == id))
    }

    /// Whether the follow-up build is to take up mod `id`.
    pub(super) fn will_build(&self, id: &str) -> bool {
        self.build.iter().any(|(_, b)| b == id)
    }

    /// An install put native code in, in profile `instance`. Installs that follow each other
    /// share the one build.
    pub(super) fn build_after_install(&mut self, instance: &str, ids: Vec<String>) {
        for id in ids {
            if !self.will_build(&id) {
                self.build.push((instance.to_owned(), id));
            }
        }
    }

    /// What starts now. Nothing starts while an install or a build runs: a running build can't
    /// take new work in, so what was asked for meanwhile waits for it. `locked`: the game is
    /// running or Sunrise is getting ready to play, so neither an install nor a build starts.
    /// The waiting installs go first, in order, then the one build for the mods that went into
    /// the `selected` profile, which is the one a build is made for.
    pub(super) fn next(
        &mut self,
        selected: &str,
        installing: bool,
        building: bool,
        locked: bool,
    ) -> Option<Step> {
        if installing || building || locked {
            return None;
        }
        if let Some(request) = self.waiting.pop_front() {
            return Some(Step::Install(request));
        }
        let (ids, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.build)
            .into_iter()
            .partition(|(instance, _)| instance == selected);
        self.build = rest;
        (!ids.is_empty()).then(|| Step::Build(ids.into_iter().map(|(_, id)| id).collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Target {
        Target {
            instance: "main".into(),
            home: "/home/main".into(),
            game: "26.3".into(),
        }
    }

    fn one(id: &str) -> Request {
        Request::One {
            target: target(),
            source: SourceId::Sunrise,
            id: id.into(),
            known: None,
            replacing: Vec::new(),
        }
    }

    fn id_of(step: Option<Step>) -> String {
        match step {
            Some(Step::Install(Request::One { id, .. })) => id,
            Some(Step::Install(Request::Many(..))) => "many".into(),
            Some(Step::Build(ids)) => format!("build {}", ids.join(",")),
            None => "nothing".into(),
        }
    }

    #[test]
    fn installs_that_followed_each_other_start_one_build() {
        let mut queue = Queue::default();
        assert_eq!(id_of(queue.next("main", false, false, false)), "nothing");
        queue.build_after_install("main", vec!["m".into()]);
        queue.build_after_install("main", vec!["m".into()]);
        assert_eq!(id_of(queue.next("main", false, false, false)), "build m");
        assert_eq!(id_of(queue.next("main", false, false, false)), "nothing");
    }

    #[test]
    fn a_running_build_is_never_joined_the_follow_up_waits_for_it() {
        let mut queue = Queue::default();
        queue.build_after_install("main", vec!["m".into()]);
        assert_eq!(
            id_of(queue.next("main", false, true, false)),
            "nothing",
            "a build runs"
        );
        assert_eq!(
            id_of(queue.next("main", true, false, false)),
            "nothing",
            "an install runs"
        );
        queue.build_after_install("main", vec!["m".into()]);
        assert_eq!(id_of(queue.next("main", false, true, false)), "nothing");
        assert_eq!(id_of(queue.next("main", false, false, false)), "build m");
        assert_eq!(id_of(queue.next("main", false, false, false)), "nothing");
    }

    #[test]
    fn a_second_install_waits_and_runs_in_order_before_the_one_build() {
        let mut queue = Queue::default();
        assert!(queue.submit(one("a"), true).is_none());
        assert!(queue
            .submit(
                Request::Many(target(), vec!["x".into()], "Pack".into()),
                true
            )
            .is_none());
        assert!(queue.submit(one("b"), true).is_none());
        assert!(queue.waits("a") && queue.waits("b") && !queue.waits("c"));
        queue.build_after_install("main", vec!["m".into()]);
        // Nothing starts while one runs.
        assert_eq!(id_of(queue.next("main", true, false, false)), "nothing");
        assert_eq!(
            id_of(queue.next("main", false, true, false)),
            "nothing",
            "behind a build"
        );
        assert_eq!(id_of(queue.next("main", false, false, false)), "a");
        assert!(!queue.waits("a") && queue.is_waiting());
        assert_eq!(id_of(queue.next("main", false, false, false)), "many");
        assert_eq!(id_of(queue.next("main", false, false, false)), "b");
        assert!(!queue.is_waiting());
        assert_eq!(id_of(queue.next("main", false, false, false)), "build m");
    }

    #[test]
    fn a_waiting_install_stays_queued_while_the_game_runs() {
        let mut queue = Queue::default();
        assert!(queue.submit(one("a"), true).is_none());
        assert_eq!(id_of(queue.next("main", false, false, true)), "nothing");
        assert!(queue.waits("a"));
        assert_eq!(id_of(queue.next("main", false, false, false)), "a");
    }

    #[test]
    fn the_build_is_told_which_mods_it_was_started_for_each_once() {
        let mut queue = Queue::default();
        queue.build_after_install("main", vec!["a".into(), "b".into()]);
        queue.build_after_install("main", vec!["b".into(), "c".into()]);
        assert_eq!(
            id_of(queue.next("main", false, false, false)),
            "build a,b,c"
        );
    }

    #[test]
    fn three_overlapping_installs_run_one_after_the_other_and_none_is_asked_again() {
        let mut queue = Queue::default();
        // A runs; B and C are asked for meanwhile.
        assert!(queue.submit(one("b"), true).is_none());
        assert!(queue.submit(one("c"), true).is_none());
        // A ends: B comes out of the queue to start, and starting it asks nothing of the
        // queue. C stays where it is, in its place.
        assert_eq!(id_of(queue.next("main", false, false, false)), "b");
        assert!(queue.waits("c") && !queue.waits("b"));
        assert_eq!(id_of(queue.next("main", true, false, false)), "nothing");
        assert_eq!(id_of(queue.next("main", false, false, false)), "c");
        assert!(!queue.is_waiting());
    }

    #[test]
    fn a_new_request_with_nothing_running_and_nothing_waiting_starts_at_once() {
        let mut queue = Queue::default();
        assert!(queue.submit(one("a"), false).is_some());
        assert!(!queue.is_waiting());
        // Behind one that waits, even when nothing runs at this moment.
        assert!(queue.submit(one("b"), true).is_none());
        assert!(queue.submit(one("c"), false).is_none());
        assert_eq!(id_of(queue.next("main", false, false, false)), "b");
    }

    #[test]
    fn a_follow_up_build_waits_for_the_game_and_for_its_own_profile() {
        let mut queue = Queue::default();
        queue.build_after_install("main", vec!["m".into()]);
        queue.build_after_install("other", vec!["o".into()]);
        assert_eq!(
            id_of(queue.next("main", false, false, true)),
            "nothing",
            "the game runs or Play prepares"
        );
        assert!(queue.will_build("m"));
        assert_eq!(id_of(queue.next("main", false, false, false)), "build m");
        assert!(queue.will_build("o") && !queue.will_build("m"));
        assert_eq!(
            id_of(queue.next("main", false, false, false)),
            "nothing",
            "the other profile's build is for that profile"
        );
        assert_eq!(id_of(queue.next("other", false, false, false)), "build o");
    }

    #[test]
    fn a_request_held_for_the_native_question_starts_again_whole_and_the_queue_waits() {
        let mut queue = Queue::default();
        let held = Request::One {
            target: Target {
                instance: "other".into(),
                home: "/home/other".into(),
                game: "26.2".into(),
            },
            source: SourceId::Sunrise,
            id: "lumen".into(),
            known: None,
            replacing: vec![sunrise_launcher::browser::installed::Replace {
                id: "lamps".into(),
                checksum: "SHA-256:1".into(),
                worlds_ok: false,
            }],
        };
        // What came behind it waits while the question is open (the caller holds the queue)...
        assert!(queue.submit(one("later"), false).is_some());
        let mut queue = Queue::default();
        queue.submit(one("behind"), true);
        assert_eq!(id_of(queue.next("main", false, false, true)), "nothing");
        // ...and the answer submits the held request again: it goes behind what waits, with
        // its own profile, game and approvals, whatever is selected by then.
        assert!(queue.submit(held, false).is_none());
        assert_eq!(id_of(queue.next("main", false, false, false)), "behind");
        match queue.next("main", false, false, false) {
            Some(Step::Install(Request::One {
                target, replacing, ..
            })) => {
                assert_eq!(
                    (target.instance.as_str(), target.game.as_str()),
                    ("other", "26.2")
                );
                assert_eq!(replacing[0].id, "lamps");
            }
            _ => panic!("the held request starts"),
        }
    }
}
