use super::scan_packages;
use crate::{
    browser::{installed::Installed, mod_list, Kind, SourceId},
    i18n::{tr, trf},
};
use std::{path::Path, sync::Arc};
use sunrise_launcher_smod::index::{self, Index};

/// Both registries' takedown checks before Play. A notice both give (neither index could be read)
/// shows once.
pub(super) fn inspect_both(
    home: &Path,
    load: impl FnOnce() -> Option<Arc<Index>>,
    load_v2: impl FnOnce() -> Option<Arc<sunrise_launcher_smod::index_v2::Index>>,
) -> Result<Vec<String>, String> {
    let mut notices = inspect(home, load)?;
    for notice in inspect_v2(home, load_v2)? {
        if !notices.contains(&notice) {
            notices.push(notice);
        }
    }
    Ok(notices)
}

/// Check the actual package bytes before any compiler or cached image can be used.
/// A source-wide takedown only applies to a proved registry archive; hash-specific
/// takedowns can also identify a hand-copied archive by those exact bytes.
pub(super) fn inspect(
    home: &Path,
    load: impl FnOnce() -> Option<Arc<Index>>,
) -> Result<Vec<String>, String> {
    let packages = scan_packages(home)?;
    let rows = Installed::read(home)?;
    if !rows
        .entries
        .iter()
        .any(|row| row.source == SourceId::Sunrise)
        && !packages.iter().any(|package| {
            package.manifest.native.is_some() && mod_list::is_enabled(home, &package.manifest.id)
        })
    {
        return Ok(Vec::new());
    }
    let Some(index) = load() else {
        return Ok(vec![
            tr("Mods were not checked for registry takedowns.").into()
        ]);
    };
    let registry = crate::browser::sunrise::fetched_official_index(&index);
    for package in &packages {
        let id = &package.manifest.id;
        if package.directory || !mod_list::is_enabled(home, id) {
            continue;
        }
        let hash = index::sha256_file(&package.path)?;
        let owned = registry
            .as_ref()
            .is_some_and(|registry| registry.proves_archive(id, &hash));
        let revoked = if owned {
            index.revoked(id, &hash)
        } else {
            index.revoked.iter().find(|row| {
                &row.id == id
                    && row
                        .sha256
                        .as_ref()
                        .is_some_and(|listed| listed.eq_ignore_ascii_case(&hash))
            })
        };
        if let Some(revoked) = revoked {
            return Err(trf(
                "{mod} was taken down. Turn it off before playing.\n{reason}",
                &[("mod", &package.manifest.name), ("reason", &revoked.reason)],
            ));
        }
    }
    let mut notices = Vec::new();
    for entry in rows.entries {
        if entry.source != SourceId::Sunrise
            || !matches!(entry.kind, Kind::ResourcePack | Kind::Shader)
        {
            continue;
        }
        let hash = entry
            .checksum
            .split_once(':')
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("SHA-256"))
            .map_or("", |(_, hash)| hash);
        if let Some(revoked) = index.revoked(&entry.project, hash) {
            notices.push(trf(
                "{mod} was taken down: {reason} Remove it.",
                &[("mod", &entry.title), ("reason", &revoked.reason)],
            ));
        }
    }
    Ok(notices)
}

/// Check installed package bytes against the signed schema-2 registry index before Play.
/// Exact-digest takedowns apply even to unowned/local files; whole-project takedowns apply only
/// when project ownership is proved.
pub(super) fn inspect_v2(
    home: &Path,
    load: impl FnOnce() -> Option<Arc<sunrise_launcher_smod::index_v2::Index>>,
) -> Result<Vec<String>, String> {
    let packages = scan_packages(home)?;
    let rows = Installed::read(home)?;
    if !rows
        .entries
        .iter()
        .any(|row| row.source == SourceId::Sunrise)
        && !packages.iter().any(|package| {
            package.manifest.native.is_some() && mod_list::is_enabled(home, &package.manifest.id)
        })
    {
        return Ok(Vec::new());
    }
    let Some(index) = load() else {
        return Ok(vec![
            tr("Mods were not checked for registry takedowns.").into()
        ]);
    };
    for package in &packages {
        let id = &package.manifest.id;
        if package.directory || !mod_list::is_enabled(home, id) {
            continue;
        }
        let hash = index::sha256_file(&package.path)?;
        if let Some(revoked) = crate::browser::sunrise::revoked_for_installed_package(
            &index, &rows, home, package, &hash,
        ) {
            return Err(trf(
                "{mod} was taken down. Turn it off before playing.\n{reason}",
                &[("mod", &package.manifest.name), ("reason", &revoked.reason)],
            ));
        }
    }
    let mut notices = Vec::new();
    for entry in rows.entries {
        if entry.source != SourceId::Sunrise
            || !matches!(entry.kind, Kind::ResourcePack | Kind::Shader)
        {
            continue;
        }
        let revoked = if sunrise_launcher_smod::index_v2::valid_project_id(&entry.project) {
            index.revoked(&entry.project, entry.sha256())
        } else {
            entry
                .sha256()
                .and_then(|hash| index.revoked_by_sha256(hash))
        };
        if let Some(revoked) = revoked {
            notices.push(trf(
                "{mod} was taken down: {reason} Remove it.",
                &[("mod", &entry.title), ("reason", &revoked.reason)],
            ));
        }
    }
    Ok(notices)
}
