//! What waits behind the install on screen: installs asked for while another runs, in order,
//! and the one build that takes up what they put in.
use std::collections::VecDeque;
use sunrise_launcher::browser::{Project, SourceId};

/// An install that was asked for and waits its turn.
pub(super) enum Request {
    One {
        source: SourceId,
        id: String,
        known: Option<Box<Project>>,
        shown: Option<String>,
    },
    /// A collection's mods, and the collection's name.
    Many(Vec<String>, String),
}

/// What runs next.
pub(super) enum Step {
    Install(Request),
    /// The native mods the build takes up.
    Build(Vec<String>),
}

#[derive(Default)]
pub(super) struct Queue {
    waiting: VecDeque<Request>,
    /// An install put native code in: one build takes it up.
    build: Vec<String>,
}

impl Queue {
    pub(super) fn push(&mut self, request: Request) {
        self.waiting.push_back(request);
    }

    /// Whether anything waits for its turn: a new request goes behind it.
    pub(super) fn is_waiting(&self) -> bool {
        !self.waiting.is_empty()
    }

    /// Whether mod `id` waits for its turn to install.
    pub(super) fn waits(&self, id: &str) -> bool {
        self.waiting
            .iter()
            .any(|r| matches!(r, Request::One { id: waiting, .. } if waiting == id))
    }

    /// An install put native code in. Installs that follow each other share the one build.
    pub(super) fn build_after_install(&mut self, ids: Vec<String>) {
        for id in ids {
            if !self.build.contains(&id) {
                self.build.push(id);
            }
        }
    }

    /// What starts now. Nothing starts while an install or a build runs: a running build can't
    /// take new work in, so what was asked for meanwhile waits for it. `locked`: the game is
    /// running or Sunrise is getting ready to play, so no install starts. The waiting installs
    /// go first, in order, then the one build for all of them.
    pub(super) fn next(&mut self, installing: bool, building: bool, locked: bool) -> Option<Step> {
        if installing || building {
            return None;
        }
        if !self.waiting.is_empty() {
            return if locked {
                None
            } else {
                self.waiting.pop_front().map(Step::Install)
            };
        }
        (!self.build.is_empty()).then(|| Step::Build(std::mem::take(&mut self.build)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(id: &str) -> Request {
        Request::One {
            source: SourceId::Sunrise,
            id: id.into(),
            known: None,
            shown: None,
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
        assert_eq!(id_of(queue.next(false, false, false)), "nothing");
        queue.build_after_install(vec!["m".into()]);
        queue.build_after_install(vec!["m".into()]);
        assert_eq!(id_of(queue.next(false, false, false)), "build m");
        assert_eq!(id_of(queue.next(false, false, false)), "nothing");
    }

    #[test]
    fn a_running_build_is_never_joined_the_follow_up_waits_for_it() {
        let mut queue = Queue::default();
        queue.build_after_install(vec!["m".into()]);
        assert_eq!(
            id_of(queue.next(false, true, false)),
            "nothing",
            "a build runs"
        );
        assert_eq!(
            id_of(queue.next(true, false, false)),
            "nothing",
            "an install runs"
        );
        queue.build_after_install(vec!["m".into()]);
        assert_eq!(id_of(queue.next(false, true, false)), "nothing");
        assert_eq!(id_of(queue.next(false, false, false)), "build m");
        assert_eq!(id_of(queue.next(false, false, false)), "nothing");
    }

    #[test]
    fn a_second_install_waits_and_runs_in_order_before_the_one_build() {
        let mut queue = Queue::default();
        queue.push(one("a"));
        queue.push(Request::Many(vec!["x".into()], "Pack".into()));
        queue.push(one("b"));
        assert!(queue.waits("a") && queue.waits("b") && !queue.waits("c"));
        queue.build_after_install(vec!["m".into()]);
        // Nothing starts while one runs.
        assert_eq!(id_of(queue.next(true, false, false)), "nothing");
        assert_eq!(
            id_of(queue.next(false, true, false)),
            "nothing",
            "behind a build"
        );
        assert_eq!(id_of(queue.next(false, false, false)), "a");
        assert!(!queue.waits("a") && queue.is_waiting());
        assert_eq!(id_of(queue.next(false, false, false)), "many");
        assert_eq!(id_of(queue.next(false, false, false)), "b");
        assert!(!queue.is_waiting());
        assert_eq!(id_of(queue.next(false, false, false)), "build m");
    }

    #[test]
    fn a_waiting_install_stays_queued_while_the_game_runs() {
        let mut queue = Queue::default();
        queue.push(one("a"));
        assert_eq!(id_of(queue.next(false, false, true)), "nothing");
        assert!(queue.waits("a"));
        assert_eq!(id_of(queue.next(false, false, false)), "a");
    }

    #[test]
    fn the_build_is_told_which_mods_it_was_started_for_each_once() {
        let mut queue = Queue::default();
        queue.build_after_install(vec!["a".into(), "b".into()]);
        queue.build_after_install(vec!["b".into(), "c".into()]);
        assert_eq!(id_of(queue.next(false, false, false)), "build a,b,c");
    }
}
