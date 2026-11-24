//! A native mod with no hooks: its one `[[native.mixin]]` row is all CI's Windows build needs.
use mod_api::Hooks;

#[derive(Default)]
struct Mod;

#[mod_api::sunrise_mod]
impl Hooks for Mod {}
