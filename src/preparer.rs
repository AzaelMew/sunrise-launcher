//! What the launcher reads or runs of a version's own preparer (`sunrise-prepare`, docs/api/prepare-extras.md), without linking it.

use serde_json::Value;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// The game's cache of prepared data in a resources folder (`sunrise_core::prepared_files::FILE_NAME`).
pub const DATA_FILE: &str = "sunrise-data.bin";

/// The `format` of the preparer's JSON replies this launcher reads.
const FORMAT: u64 = 1;

fn home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Python's `Path.expanduser` for the forms players type: `~` and `~/...`.
fn expand_user(path: &Path) -> PathBuf {
    let mut components = path.components();
    match components.next() {
        Some(Component::Normal(first)) if first == "~" => home().join(components.as_path()),
        _ => path.to_path_buf(),
    }
}

/// The official launcher's game directory, which holds the player's own copy of the game (as `sunrise-prepare` finds it).
pub fn minecraft_dir(chosen: Option<&Path>) -> PathBuf {
    if let Some(dir) = chosen.filter(|d| !d.as_os_str().is_empty()) {
        return expand_user(dir);
    }
    if let Some(dir) = std::env::var_os("SUNRISE_MINECRAFT_DIR").filter(|d| !d.is_empty()) {
        return expand_user(Path::new(&dir));
    }
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/minecraft")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home)
            .join(".minecraft")
    } else {
        home().join(".minecraft")
    }
}

fn clone_file(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        let source = fs::File::open(from)?;
        let target = fs::File::create(to)?;
        // SAFETY: FICLONE takes a source fd, both descriptors stay open for the call.
        if unsafe { libc::ioctl(target.as_raw_fd(), libc::FICLONE, source.as_raw_fd()) } == 0 {
            return Ok(());
        }
        Err(io::Error::last_os_error())
    }
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let path = |p: &Path| {
            CString::new(p.as_os_str().as_bytes())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
        };
        let (source, target) = (path(from)?, path(to)?);
        // SAFETY: both paths are valid NUL-terminated strings for the duration of the call.
        if unsafe { libc::clonefile(source.as_ptr(), target.as_ptr(), 0) } == 0 {
            return Ok(());
        }
        Err(io::Error::last_os_error())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (from, to);
        Err(io::ErrorKind::Unsupported.into())
    }
}

/// A copy-on-write clone where the file system has one, else a plain copy.
pub fn clone_or_copy(from: &Path, to: &Path) -> io::Result<()> {
    clone_file(from, to).or_else(|_| fs::copy(from, to).map(|_| ()))
}

/// The version folder's `sunrise-prepare`, when it ships one.
pub fn binary(dir: &Path) -> Option<PathBuf> {
    let binary = dir.join(if cfg!(windows) {
        "sunrise-prepare.exe"
    } else {
        "sunrise-prepare"
    });
    binary.is_file().then_some(binary)
}

/// The JSON reply on the last stdout line of `command`, when the preparer has the subcommand (exit 2 means it doesn't).
fn reply(mut command: Command, line: &dyn Fn(String)) -> Result<Option<Value>, String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("sunrise-prepare: {e}"))?;
    let mut last = None;
    for text in BufReader::new(child.stdout.take().expect("piped")).lines() {
        let text = text.map_err(|e| format!("sunrise-prepare: {e}"))?;
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        match value["message"].as_str() {
            Some(message) => line(message.to_owned()),
            None => last = Some(value),
        }
    }
    let status = child.wait().map_err(|e| format!("sunrise-prepare: {e}"))?;
    if status.code() == Some(2) {
        return Ok(None);
    }
    let value = last.ok_or_else(|| format!("sunrise-prepare ended ({status}) without a reply"))?;
    if value["format"]
        .as_u64()
        .is_none_or(|format| format > FORMAT)
    {
        return Err("This Sunrise build's preparer is newer than the launcher understands: update the launcher.".into());
    }
    if let Some(error) = value.get("error") {
        return Err(error["message"]
            .as_str()
            .unwrap_or("sunrise-prepare failed")
            .to_owned());
    }
    Ok(Some(value))
}

fn json_command(binary: &Path, root: &Path, subcommand: &str) -> Command {
    let mut command = crate::process::command(binary);
    command
        .current_dir(root)
        .arg(subcommand)
        .arg("--root")
        .arg(root)
        .arg("--json");
    command
}

/// What preparing `root` needs beyond the player's Minecraft folder (`needs --json`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Needs {
    /// No verified import of the pinned jars is there to reuse (a sibling's is linked in when found).
    pub server_jar: bool,
    /// The version ships no matching exporters, so preparing compiles them with a JDK.
    pub jdk: bool,
}

/// [`Needs`] from the version's own preparer; `None` when it is too old to answer.
pub fn needs(binary: &Path, root: &Path, line: &dyn Fn(String)) -> Result<Option<Needs>, String> {
    Ok(
        reply(json_command(binary, root, "needs"), line)?.map(|value| Needs {
            server_jar: value["server_jar"].as_bool().unwrap_or(true),
            jdk: value["jdk"].as_bool().unwrap_or(true),
        }),
    )
}

/// Each extra's kind and what it is, as the preparer names them.
pub type Extras = Vec<(String, String)>;

fn extras(value: &Value) -> Extras {
    value
        .as_array()
        .into_iter()
        .flatten()
        .map(|extra| {
            (
                extra["kind"].as_str().unwrap_or_default().to_owned(),
                extra["what"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect()
}

/// The extras the prepared `resources` of `root` lack (`missing-extras --json`); `None` when the preparer is too old to answer.
pub fn missing_extras(
    binary: &Path,
    root: &Path,
    resources: &Path,
) -> Result<Option<Extras>, String> {
    let mut command = json_command(binary, root, "missing-extras");
    command.arg("--output").arg(resources);
    Ok(reply(command, &|_| {})?.map(|value| extras(&value["missing"])))
}

/// What `extras` did: the kinds added, each that failed with why, and what still lacks.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Added {
    pub added: Vec<String>,
    pub failed: Vec<(String, String)>,
    pub missing: Extras,
}

/// Adds the `only` extras `resources` lack, each from the first of `folders` that has it (`extras --json`); `None` when the preparer is too old.
pub fn add_extras(
    binary: &Path,
    root: &Path,
    resources: &Path,
    only: &[String],
    folders: &[PathBuf],
    line: &dyn Fn(String),
) -> Result<Option<Added>, String> {
    let mut command = json_command(binary, root, "extras");
    command
        .arg("--output")
        .arg(resources)
        .arg("--only")
        .arg(only.join(","));
    for folder in folders {
        command.arg("--minecraft-dir").arg(folder);
    }
    Ok(reply(command, line)?.map(|value| Added {
        added: value["added"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|kind| kind.as_str().map(str::to_owned))
            .collect(),
        failed: value["failed"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|failure| {
                (
                    failure["kind"].as_str().unwrap_or_default().to_owned(),
                    failure["error"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect(),
        missing: extras(&value["missing"]),
    }))
}
