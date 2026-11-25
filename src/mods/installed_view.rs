//! Mods › Installed: collection sections, one switch per row, and scoped bulk actions.
use super::model::{self, Row, Show};
use super::ui::*;
use super::*;
use crate::paint::palette;
use std::{collections::BTreeMap, rc::Rc};
use sunrise_launcher::browser::images;

mod items;
mod page;
mod row;
mod unfit;

pub(super) use items::remove_names;

#[cfg(test)]
mod tests;
