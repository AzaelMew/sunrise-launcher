use super::{inputs::browser_repair, scan_packages};
use crate::{
    browser::{installed::Installed, mod_list, Kind, SourceId},
    i18n::{tr, trf},
};
use std::{path::Path, sync::Arc};
use sunrise_launcher_smod::index::{self, Index};

/// The caller owns this input epoch through staging. Use the same proved row postimage
/// that refresh/commit will publish, not the inventory's stale display checksum.
/// Managed images link exactly the enabled native inputs, so test their archive bytes
/// here, before any compiler or cached image can be used.
pub(super) fn inspect(
    home: &Path,
    load: impl FnOnce() -> Option<Arc<Index>>,
) -> Result<Vec<String>, String> {
    let packages = scan_packages(home)?;
    let entries = match browser_repair(home, &packages)? {
        Some(postimage) => serde_json::from_slice(&postimage).map_err(|e| e.to_string())?,
        None => Installed::read(home)?,
    };
    let entries: Vec<_> = entries
        .entries
        .into_iter()
        .filter(|e| {
            e.source == SourceId::Sunrise
                && (e.kind != Kind::Mod || packages.iter().any(|p| p.path == home.join(&e.file)))
        })
        .collect();
    // An untracked native package may share a registry ID without being that mod's bytes.
    // Check only hash-specific takedowns here; proved registry rows are checked below.
    let native = packages.iter().filter(|package| {
        package.manifest.native.is_some()
            && mod_list::is_enabled(home, &package.manifest.id)
            && !entries
                .iter()
                .any(|row| row.kind == Kind::Mod && row.project == package.manifest.id)
    });
    if entries.is_empty() && native.clone().next().is_none() {
        return Ok(Vec::new());
    }
    let Some(index) = load() else {
        return Ok(vec![
            tr("Mods were not checked for registry takedowns.").into()
        ]);
    };
    let refused = |title: &str, reason: &str| {
        trf(
            "{mod} was taken down. Turn it off before playing.\n{reason}",
            &[("mod", title), ("reason", reason)],
        )
    };
    for package in native {
        let hash = index::sha256_file(&package.path)?;
        if let Some(revoked) = index.revoked.iter().find(|row| {
            row.id == package.manifest.id
                && row
                    .sha256
                    .as_ref()
                    .is_some_and(|listed| listed.eq_ignore_ascii_case(&hash))
        }) {
            return Err(refused(&package.manifest.name, &revoked.reason));
        }
    }
    let mut notices = Vec::new();
    for entry in entries {
        let hash = entry
            .checksum
            .split_once(':')
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("SHA-256"))
            .map_or("", |(_, hash)| hash);
        let Some(revoked) = index.revoked(&entry.project, hash) else {
            continue;
        };
        if entry.kind == Kind::Mod && mod_list::is_enabled(home, &entry.project) {
            return Err(refused(&entry.title, &revoked.reason));
        }
        if matches!(entry.kind, Kind::ResourcePack | Kind::Shader) {
            notices.push(trf(
                "{mod} was taken down: {reason} Remove it.",
                &[("mod", &entry.title), ("reason", &revoked.reason)],
            ));
        }
    }
    Ok(notices)
}
