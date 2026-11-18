//! The Servers page in the glass look (the mockup's `ServersView`, sunrise-site
//! `components/launcher/launcher-views.tsx`): the open server's glass with its state, name, numbers,
//! Start or Restart and Stop, its console and command line; beside it the list of servers and how
//! friends reach the open one. Below, what the mockup has no room for: the server's Players,
//! Settings and Backups. Dark appearance only.
use super::*;
use crate::glass_ui::{ white, Tone};
use sunrise_launcher_theme::glass::{fill, ink, radius};

/// The right column's width (`lg:grid-cols-[minmax(0,1fr)_320px]`).
const SIDE: f32 = 320.;

/// A dot's tone for a server's state.
fn tone(status: &Status) -> Tone {
    match status.phase.as_str() {
        "running" if status.crashes > 0 => Tone::Warn,
        "running" => Tone::Live,
        "stopped" => Tone::Off,
        _ => Tone::Busy,
    }
}


