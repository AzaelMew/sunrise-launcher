//! Who owns an installed package (a v2 project, a v1 row or the player), revoked releases, and the v2 inventory of a profile.

use super::*;

/// Classify an installed package's v2 ownership strictly from verified evidence.
///
/// A legacy v1 `mod-browser.json` row is adopted ONLY when its on-disk kind, version, byte size,
/// and SHA-256 uniquely match one `(Project, Release)` in signed v2 history. Any manual file,
/// GitHub source mod, unmatched v1 row, tampered file, or ambiguous digest stays
/// [`V2Ownership::Local`].
pub fn classify_v2_ownership(
    index: &Index,
    home: &Path,
    entry: Option<&installed::Entry>,
    package: &ScannedPackage,
    digest: &str,
    size: u64,
    has_github_origin: bool,
) -> V2Ownership {
    if package.directory || has_github_origin {
        return V2Ownership::Local;
    }
    let Some(row) = entry else {
        return V2Ownership::Local;
    };
    if row.source != SourceId::Sunrise
        || row.kind != Kind::Mod
        || row.path(home).as_deref() != Some(package.path.as_path())
    {
        return V2Ownership::Local;
    }
    let expected_kind = if package.manifest.native.is_some() {
        V2Kind::NativeMod
    } else {
        V2Kind::Mod
    };
    if row.is_v2_registry() && row.mod_id.as_deref() == Some(package.manifest.id.as_str()) {
        let Some(project) = index.project(&row.project) else {
            return V2Ownership::Local;
        };
        if project.mod_id != package.manifest.id || project.kind != expected_kind {
            return V2Ownership::Local;
        }
        let Some(release) = project.releases.iter().find(|release| {
            release.file.sha256.eq_ignore_ascii_case(digest) && release.file.size == size
        }) else {
            return V2Ownership::Local;
        };
        let Some(repository) = row.repository.as_ref() else {
            return V2Ownership::Local;
        };
        if repository.id != project.release_repository(release) {
            return V2Ownership::Local;
        }
        return V2Ownership::Verified {
            project_id: project.id.clone(),
            slug: project.slug.clone(),
            version: release.version.clone(),
            repository: repository.clone(),
        };
    }
    // Legacy v1 receipt: project field holds the in-game mod id, mod_id/repository are absent.
    if row.mod_id.is_none() && row.project == package.manifest.id {
        let matches: Vec<(&Project, &Release)> = index
            .releases_by_sha256(&package.manifest.id, digest)
            .into_iter()
            .filter(|(project, release)| {
                project.kind == expected_kind
                    && release.version == package.manifest.version
                    && release.file.size == size
            })
            .collect();
        if let [(project, release)] = matches.as_slice() {
            return V2Ownership::AdoptedV1 {
                project_id: project.id.clone(),
                slug: project.slug.clone(),
                version: release.version.clone(),
                repository: installed::VerifiedRepository {
                    id: project.release_repository(release),
                    name: project.repository.name.clone(),
                    path: project.repository.path.clone(),
                    workflow: release.workflow.clone(),
                },
            };
        }
    }
    V2Ownership::Local
}

/// Returns the active takedown affecting an installed package, if any.
///
/// Exact-digest takedowns (`revoked[].sha256 = Some(...)`) apply even to local/unadopted files.
/// Whole-project takedowns (`revoked[].sha256 = None`) apply ONLY when ownership of that project
/// is proved ([`V2Ownership::Verified`] or [`V2Ownership::AdoptedV1`]).
pub fn revoked_for_package<'a>(
    index: &'a Index,
    ownership: &V2Ownership,
    digest: &str,
) -> Option<&'a Revoked> {
    if let Some(revoked) = index.revoked_by_sha256(digest) {
        return Some(revoked);
    }
    ownership
        .project_id()
        .and_then(|project_id| index.revoked(project_id, Some(digest)))
}

/// True when `package` has a non-registry `.origin.json` sidecar (GitHub or local-folder install),
/// or when reading sidecar metadata fails (fail closed so source mods are never adopted as v2).
fn has_source_origin(home: &Path, package: &ScannedPackage) -> bool {
    if package.directory {
        return false;
    }
    let present = |path: &Path| {
        !matches!(
            fs::symlink_metadata(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound
        )
    };
    let beside = package.path.with_extension("origin.json");
    let native = crate::github_mods::origin_path(home, &package.manifest.id);
    present(&beside) || present(&native)
}

/// Convenience helper for pre-Play checks: classifies `package` against `rows` and checks v2
/// revocations for `digest`.
pub(crate) fn revoked_for_installed_package<'a>(
    index: &'a Index,
    rows: &installed::Installed,
    home: &Path,
    package: &ScannedPackage,
    digest: &str,
) -> Option<&'a Revoked> {
    let size = fs::metadata(&package.path).ok()?.len();
    let entry = rows
        .entries
        .iter()
        .find(|row| row.in_game_mod_id() == Some(package.manifest.id.as_str()));
    let has_github = has_source_origin(home, package);
    let ownership = classify_v2_ownership(index, home, entry, package, digest, size, has_github);
    revoked_for_package(index, &ownership, digest)
}

/// Turn off any enabled mods in `home` revoked by the signed v2 index, leaving their files on disk.
pub fn turn_off_revoked_v2(index: &Index, home: &Path) -> Result<Vec<String>, String> {
    let _lock = crate::browser::lock_home(home).map_err(|e| e.to_string())?;
    installed::recover_game_install(home)?;
    let packages = managed_play::scan_packages(home)?;
    let rows = installed::Installed::read(home)?;
    let mut notices = Vec::new();
    for package in &packages {
        if package.directory {
            continue;
        }
        let digest = index::sha256_file(&package.path)?;
        let Some(revoked) = revoked_for_installed_package(index, &rows, home, package, &digest)
        else {
            continue;
        };
        if mod_list::is_enabled(home, &package.manifest.id) {
            mod_list::set_enabled(home, &package.manifest.id, false)?;
            notices.push(trf(
                "{mod} was turned off because it was taken down: {reason}",
                &[("mod", &package.manifest.name), ("reason", &revoked.reason)],
            ));
        }
    }
    for entry in &rows.entries {
        if entry.source != SourceId::Sunrise
            || !matches!(entry.kind, Kind::ResourcePack | Kind::Shader)
        {
            continue;
        }
        let revoked = if index_v2::valid_project_id(&entry.project) {
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

/// Scan every installed package in `home` and classify its verified v2 ownership against `index`.
pub fn inventory_v2(
    home: &Path,
    index: &Index,
) -> Result<BTreeMap<String, InstalledV2Mod>, String> {
    let packages = managed_play::scan_packages(home)?;
    let rows = installed::Installed::read(home)?;
    let mut by_slot: BTreeMap<String, &installed::Entry> = BTreeMap::new();
    for row in &rows.entries {
        if let Some(slot) = row.in_game_mod_id() {
            if by_slot.insert(slot.to_owned(), row).is_some() {
                return Err(trf(
                    "Several entries in {manifest} name {id}.",
                    &[("manifest", installed::MANIFEST), ("id", slot)],
                ));
            }
        }
    }
    let mut out = BTreeMap::new();
    for package in packages {
        let id = package.manifest.id.clone();
        let enabled = mod_list::is_enabled(home, &id);
        let (digest, size) = if package.directory {
            (String::new(), 0)
        } else {
            (
                index::sha256_file(&package.path)?,
                fs::metadata(&package.path)
                    .map_err(|e| at(&package.path, e))?
                    .len(),
            )
        };
        let has_github = has_source_origin(home, &package);
        let ownership = classify_v2_ownership(
            index,
            home,
            by_slot.get(&id).copied(),
            &package,
            &digest,
            size,
            has_github,
        );
        out.insert(
            id,
            InstalledV2Mod {
                package,
                digest,
                size,
                enabled,
                ownership,
            },
        );
    }
    Ok(out)
}
