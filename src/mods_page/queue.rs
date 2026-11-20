//! What waits behind the install on screen.

#[derive(Default)]
pub(super) struct Queue {
    /// An install put native code in: one build takes it up.
    build: bool,
}

impl Queue {
    /// An install put native code in. Installs that follow each other share the one build.
    pub(super) fn build_after_install(&mut self) {
        self.build = true;
    }

    /// Whether the build that follows the installs starts now: nothing installs and no build
    /// runs. Asking again after it started answers no.
    pub(super) fn build_due(&mut self, installing: bool, building: bool) -> bool {
        let due = self.build && !installing && !building;
        self.build &= !due;
        due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_that_followed_each_other_start_one_build() {
        let mut queue = Queue::default();
        assert!(!queue.build_due(false, false), "nothing was installed");
        queue.build_after_install();
        queue.build_after_install();
        assert!(queue.build_due(false, false));
        assert!(!queue.build_due(false, false), "one build, not two");
    }

    #[test]
    fn a_running_build_is_never_joined_the_follow_up_waits_for_it() {
        let mut queue = Queue::default();
        queue.build_after_install();
        assert!(!queue.build_due(false, true), "a build is running");
        assert!(!queue.build_due(true, false), "an install is running");
        // Another install finishes while the build still runs: still one follow-up.
        queue.build_after_install();
        assert!(!queue.build_due(false, true));
        assert!(queue.build_due(false, false), "the build ended");
        assert!(!queue.build_due(false, false));
    }
}
