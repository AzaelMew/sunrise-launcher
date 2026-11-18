//! Home's Mods tab (docs/design-system.md, "Night"; plan R0, screens 5 and 6): every mod of the
//! selected instance, the browser's and those put in its folder by hand, with its version, where
//! it comes from, an update when the browser found one, and a switch. A Java instance turns a mod
//! off by renaming its jar (`.disabled`, as `java_instance` does); a Sunrise instance's mods are
//! the registry's, switched as the game's Options > Mods does. Adding mods is the Mods page's.
//!
//! Each row shows the mod's own icon: Modrinth's for what the browser installed from it (the URLs
//! asked for in one request and kept beside the image cache, the images through the Mods page's
//! cache at the row's size), else the package's icon or the one its jar names, else a generic glyph.
use super::*;

/// A mod as the tab lists it.
#[derive(Clone)]
pub(super) struct ModRow {}

#[derive(Default)]
pub(super) struct InstanceMods {
    pub(super) fresh: bool,
    pub(super) list: Vec<ModRow>,
    /// Only the rows near the view are built (a modpack has hundreds), measured again when the
    /// shown list changes (its length and the filter).
    pub(super) cull: cull::Cull,
    pub(super) shown: (usize, String),
}

impl InstanceMods {
    fn invalidate(&mut self) {
        self.fresh = false;
    }
}

impl Launcher {
    pub(super) fn instance_mods_changed(&mut self) {
        self.instance_mods.invalidate();
        self.instance_mods.cull.reset();
    }
}
