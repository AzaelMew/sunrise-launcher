//! A server's files: its settings, player lists and backups, and its background service.

use super::*;

/// The settings, as `sunrise-server config --json` prints them (`None` before `init`, or from a
/// `sunrise-server` older than the command).
pub fn config(dir: &Path) -> Option<Value> {
    if !dir.join("server.toml").is_file() {
        return None;
    }
    let out = run(dir, &["config", "--json"]).ok()?;
    serde_json::from_str(out.trim()).ok()
}

/// What `config set` did: the settings applied now, and those waiting for a restart.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Applied {
    #[serde(default)]
    pub applied: Vec<String>,
    #[serde(default)]
    pub restart: Vec<String>,
    #[serde(default)]
    pub running: bool,
}

/// Changes settings through `sunrise-server config set` (checked there; nothing is written if one is
/// wrong).
pub fn set_config(dir: &Path, changes: &[(&str, String)]) -> Result<Applied, Problem> {
    let pairs: Vec<String> = changes.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut args = vec!["config", "set"];
    args.extend(pairs.iter().map(String::as_str));
    args.push("--json");
    let out = run(dir, &args)?;
    serde_json::from_str(out.trim()).map_err(|e| Problem::plain(e.to_string()))
}

/// A list file's names (`whitelist.json`, `ops.json`, `banned-players.json`), as vanilla writes them.
pub fn names(dir: &Path, file: &str) -> Vec<String> {
    fs::read_to_string(dir.join(file))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(|e| e["name"].as_str().map(str::to_string))
        .collect()
}

/// A list file's names with their UUIDs (empty when the file has none).
pub fn entries(dir: &Path, file: &str) -> Vec<(String, String)> {
    fs::read_to_string(dir.join(file))
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Value>>(&t).ok())
        .unwrap_or_default()
        .iter()
        .filter_map(|e| {
            let name = e["name"].as_str()?.to_string();
            Some((name, e["uuid"].as_str().unwrap_or_default().to_string()))
        })
        .collect()
}

/// A backup, as `sunrise-server backups --json` lists it.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Backup {
    pub name: String,
    pub bytes: u64,
    #[serde(default)]
    pub modified: Option<u64>,
}

pub fn backups(dir: &Path) -> Result<Vec<Backup>, Problem> {
    let out = run(dir, &["backups", "--json"])?;
    serde_json::from_str(out.trim()).map_err(|e| Problem::plain(e.to_string()))
}

/// The address friends on the same network type: this computer's LAN address. No packet is sent: the
/// system only picks the interface it would use.
pub fn lan_address() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

/// Whether the server starts at login: the service definition `sunrise-server service print` names exists.
pub fn service_installed(dir: &Path) -> bool {
    if cfg!(windows) {
        let name = format!("sunrise-server-{}", service_suffix(dir));
        return crate::process::command("schtasks")
            .args(["/Query", "/TN", &name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    }
    run(dir, &["service", "print"])
        .ok()
        .and_then(|out| {
            out.lines()
                .next()
                .and_then(|l| l.strip_prefix("# "))
                .map(PathBuf::from)
        })
        .is_some_and(|p| p.is_file())
}

/// sunrise-server's service name for a folder, after `sunrise-server-`.
pub(super) fn service_suffix(dir: &Path) -> String {
    let folder = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let clean: String = folder
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let clean = clean.trim_matches('-');
    if clean.is_empty() {
        "server".into()
    } else {
        clean.into()
    }
}
