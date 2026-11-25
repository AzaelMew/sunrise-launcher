//! Servers hosted from the launcher. Every server is a `sunrise-server` folder and the launcher drives
//! `sunrise-server` (docs/server.md in the repository) rather than its own copy of the logic: `init`,
//! `start`, `stop`, `config set`, `backup`, `restore`, `update`, `service`, and the control API the running
//! supervisor serves on 127.0.0.1 for the live status, the log and console commands.
//!
//! The programs come from an installed Sunrise version, laid out as `install.sh` lays them out
//! (`server-runtime/releases/<tag>`, `current`, `game-files/`), so `sunrise-server update` and its
//! rollback work on them unchanged. The game files are the player's own, copied from the version the
//! launcher prepared: nothing of Mojang's is downloaded or shipped.

use crate::backend;
use crate::i18n::{tr, trf};
use crate::worlds;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

pub mod network;

const PROGRAM: &str = if cfg!(windows) {
    "sunrise-server.exe"
} else {
    "sunrise-server"
};

/// The Minecraft EULA a server owner agrees to.
pub const EULA_URL: &str = "https://aka.ms/MinecraftEULA";

pub mod remote;

mod list;
pub use list::*;
mod programs;
pub use programs::*;
mod runner;
pub use runner::*;
mod new_server;
pub use new_server::*;
mod files;
pub use files::*;
mod api;
pub use api::*;

#[cfg(test)]
mod tests;
