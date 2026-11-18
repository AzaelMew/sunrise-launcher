//! Checks a launcher release folder as a launcher would: its SHA256SUMS signed for the launcher's releases, every listed file's SHA-256, and no file the list leaves out.
use std::path::PathBuf;
use sunrise_launcher_release_verify::{
    SIGNATURE, SUMS, check, parse_keys, sha256_hex, trusted_launcher_keys, verify_launcher,
};

const USAGE: &str =
    "usage: verify-launcher-release --tag <vX.Y.Z> [--key <base64 Ed25519 public key>] <folder>";

fn main() {
    if let Err(error) = run(std::env::args().skip(1)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run(mut args: impl Iterator<Item = String>) -> Result<(), String> {
    let (mut tag, mut key, mut folder) = (None, None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--tag" => tag = args.next(),
            "--key" => key = args.next(),
            _ if folder.is_none() => folder = Some(PathBuf::from(arg)),
            _ => return Err(USAGE.into()),
        }
    }
    let (Some(tag), Some(folder)) = (tag, folder) else {
        return Err(USAGE.into());
    };
    // A dry run's throwaway key, else the key launchers trust for their own releases (the game repository's).
    let keys = match key {
        Some(key) => parse_keys(&key)?,
        None => trusted_launcher_keys(),
    };
    let read = |name: &str| {
        std::fs::read(folder.join(name))
            .map_err(|e| format!("{}: {e}", folder.join(name).display()))
    };
    let sums = read(SUMS)?;
    let signature = String::from_utf8_lossy(&read(SIGNATURE)?).into_owned();
    let listed = verify_launcher(&tag, &sums, &signature, &keys)?;
    for (name, expected) in &listed {
        check(name, expected, &sha256_hex(&read(name)?))?;
    }
    let entries = std::fs::read_dir(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    for entry in entries {
        let name = entry.map_err(|e| e.to_string())?.file_name();
        let name = name.to_string_lossy();
        if name != SUMS && name != SIGNATURE && !listed.iter().any(|(listed, _)| *listed == name) {
            return Err(format!(
                "{name} is in the folder but not in {SUMS}: nothing unsigned is published"
            ));
        }
    }
    println!(
        "{tag}: {} files, signed for the launcher and matching",
        listed.len()
    );
    Ok(())
}
