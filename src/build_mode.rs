//! The Mods tab's build card and the per-mod route chips (docs/modding/src/how-to/native-mods.md).
//! The card picks the build mode, shows where a build is (a percentage, one pill per stage) and
//! cancels it. A chip on each native mod says how it reaches the game; its menu gives the reason
//! and the actions.
use super::*;

impl Launcher {
    /// A build stopped for worlds at stake: the world question is put to the player (Stay is
    /// the default; Switch anyway builds again, leaving the mods out), and the mods that
    /// failed to compile stay marked past it.
    pub(super) fn keep_stop(&mut self, stake: &native_mods::AtStake) {
        self.mods.world_ask = self
            .native_for
            .clone()
            .map(|instance| mods_page::WorldAsk::from_stop(instance, stake));
        self.mods.unbuilt = self
            .native_for
            .clone()
            .map(|instance| (instance, stake.unbuilt.clone()));
    }

    /// The mods `instance`'s native build left out, as the build recorded them.
    pub(super) fn native_left_out_of(
        &self,
        instance: &instances::Instance,
    ) -> Vec<native_mods::LeftOut> {
        let Some(version) = self.version_of(instance) else {
            return Vec::new();
        };
        match self.native_build_of(version) {
            Some(build) => build.left_out,
            // A game that needed no build because every mod with native code was left out.
            None => native_mods::kept_left_out(&instance.home, version),
        }
    }
}
