//! Competing ports sharing an in-game mod ID (mocks 03 and 04a).
//!
//! Projects in the v2 index that share a `mod` slot are grouped as alternatives labelled by
//! their GitHub publisher (`owner/repo`), never as a collection or a single install for both.
//! An installed mod's page lists other reviewed ports of the same in-game ID and launches the
//! staged provider switch (`Launcher::begin_provider_switch`).
use super::model::Row;
use super::ui::*;
use super::*;
use crate::paint::palette;
use std::collections::BTreeMap;
use sunrise_launcher::browser::{details::category_label, images, sunrise::v2};
use sunrise_launcher_smod::index_v2::{self, FitVerdict};

mod model;
mod page;
mod switch;

pub(crate) use model::{
    InstalledPort, OtherPortOption, OtherPorts, PortAction, PortGroup, PortOption, PortSwitch,
};
pub(crate) use switch::{group_ports, grouped_project_ids, other_ports_for};

#[cfg(test)]
mod tests;
