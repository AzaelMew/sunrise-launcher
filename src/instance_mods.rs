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


use build_mode::ModKind;

use std::collections::{HashMap, HashSet};
use std::path::{ PathBuf};
use std::sync::Arc;
use sunrise_launcher::browser::{ installed};


/// A mod as the tab lists it.
#[derive(Clone)]
pub(super) struct ModRow {
    /// Its file in the instance, as it is when on (`mods/lumen-0.6.2.jar`).
    pub(super) file: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) on: bool,
    /// What the browser knows of it; None: a file put in by hand.
    pub(super) entry: Option<installed::Entry>,
    /// Its validated package icon or jar icon, kept between draws.
    pub(super) icon: Option<Arc<gpui::Image>>,
    pub(super) error: Option<String>,
    pub(super) native: Option<native_status::Installed>,
    pub(super) internals: Vec<String>,
    /// What the package is and its mod id, for a Sunrise mod; None for a Java jar.
    pub(super) kind: Option<(ModKind, String)>,
}

/// Modrinth projects' icon URLs by project id, kept on disk beside the image cache so the icons
/// show offline too; each id is asked for once a session.
#[derive(Default)]
pub(super) struct ProjectIcons {
    urls: HashMap<String, String>,
    loaded: bool,
    asked: HashSet<String>,
}

#[derive(Default)]
pub(super) struct InstanceMods {
    home: Option<PathBuf>,
    pub(super) fresh: bool,
    reading: bool,
    pub(super) list: Vec<ModRow>,
    /// The browser was installing at the last draw: its end changes the list.
    was_busy: bool,
    /// Only the rows near the view are built (a modpack has hundreds), measured again when the
    /// shown list changes (its length and the filter).
    pub(super) cull: cull::Cull,
    pub(super) shown: (usize, String),
    fingerprint: String,
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

