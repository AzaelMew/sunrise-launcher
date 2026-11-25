//! Discover and Mod Page view models built strictly from the signed v2 registry index.
//!
//! Pure data in and out: no files, no network, no GPUI. Compatibility comes solely from each
//! release's generated `fits` and `does_not_fit` lists for the selected profile's exact build,
//! never inferred from `sunrise` or semver. Images are kept only when their path is
//! content-addressed (`pages/<project id>/<sha256>.<png|jpg>`).
mod cards;
mod collections;
mod mod_page;
mod release_builds;
mod release_fit;
mod search;
#[cfg(test)]
mod tests;

use super::{Make, SourceLink};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use sunrise_launcher::i18n::{tr, trf};
use sunrise_launcher::native_mods::local_fit;
use sunrise_launcher_smod::index::Kind;
use sunrise_launcher_smod::index_v2::{self, FitVerdict, Index, Project, Release};
use sunrise_launcher_smod::manifest::JarRef;

pub(in crate::mods) use cards::*;
pub(in crate::mods) use collections::*;
pub(in crate::mods) use mod_page::*;
pub(in crate::mods) use release_builds::*;
pub(in crate::mods) use release_fit::*;
pub(in crate::mods) use search::*;
