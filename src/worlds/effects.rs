//! Conservative proof for preserving saved mod definitions across a staged edit.
use crate::managed_play::{self, Active, ScannedPackage};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::Path,
};
use sunrise_launcher_smod::{dependencies, files, manifest, mod_list};

/// Previously available mod IDs and their namespaces whose effects are not proved unchanged.
pub type SuspectMods = BTreeMap<String, BTreeSet<String>>;

/// Compare admitted bases, the selected package environment, and the old packages explicitly
/// replaced or removed (which may already be off). A missing base is not evidence of equality.
/// Failure to prove equivalence leaves the old IDs suspect rather than clearing their warnings.
pub fn unproved_mod_effects<'a>(
    before: &Path,
    before_base: Option<&Active>,
    after: &Path,
    after_base: Option<&Active>,
    changed: impl IntoIterator<Item = &'a str>,
) -> Result<SuspectMods, String> {
    let previous = managed_play::scan_packages(before)?;
    let changed: BTreeSet<_> = changed.into_iter().collect();
    let old_on = enabled(before)?;
    let mut watched: SuspectMods = previous
        .iter()
        .filter(|p| old_on.contains(&p.manifest.id) || changed.contains(p.manifest.id.as_str()))
        .map(|p| {
            (
                p.manifest.id.clone(),
                p.manifest.owned_namespaces().map(str::to_owned).collect(),
            )
        })
        .collect();
    if watched.is_empty()
        || !before_base
            .zip(after_base)
            .is_some_and(|(a, b)| a.same_base(b))
    {
        return Ok(watched);
    }
    let next = managed_play::scan_packages(after)?;
    let new_on = enabled(after)?;
    let previous = previous
        .iter()
        .filter(|p| old_on.contains(&p.manifest.id))
        .collect::<Vec<_>>();
    let next = next
        .iter()
        .filter(|p| new_on.contains(&p.manifest.id))
        .collect::<Vec<_>>();
    if let Some(preserved) = preserved_mods(before, after, &previous, &next) {
        watched.retain(|id, _| !preserved.contains(id.as_str()));
    }
    Ok(watched)
}

fn enabled(home: &Path) -> Result<BTreeSet<String>, String> {
    let path = home.join(mod_list::FILE);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(mod_list::ModList::parse(&text).enabled),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn preserved_mods<'a>(
    before: &Path,
    after: &Path,
    previous: &[&'a ScannedPackage],
    next: &[&'a ScannedPackage],
) -> Option<BTreeSet<&'a str>> {
    let before_mods = before.join(mod_list::DIR);
    let after_mods = after.join(mod_list::DIR);
    if overrides_bytes(&before_mods)? != overrides_bytes(&after_mods)? {
        return None;
    }
    let overrides = dependencies::Overrides::read(&before_mods).ok()?;
    let old: Vec<_> = previous.iter().map(|p| &p.manifest).collect();
    let new: Vec<_> = next.iter().map(|p| &p.manifest).collect();
    let old_ids: BTreeSet<_> = old.iter().map(|m| m.id.as_str()).collect();
    let new_packages: BTreeMap<_, _> = next.iter().map(|p| (p.manifest.id.as_str(), *p)).collect();
    let mut preserved = BTreeSet::new();
    let mut changed: BTreeSet<_> = new_packages
        .keys()
        .copied()
        .filter(|id| !old_ids.contains(id))
        .collect();
    for a in previous {
        let id = a.manifest.id.as_str();
        let Some(b) = new_packages.get(id) else {
            changed.insert(id);
            continue;
        };
        let flat = |p: &ScannedPackage| {
            !p.directory && p.manifest.native.is_none() && p.manifest.jar.is_empty()
        };
        if flat(a)
            && flat(b)
            && a.path
                .strip_prefix(before)
                .ok()
                .zip(b.path.strip_prefix(after).ok())
                .is_some_and(|(a, b)| a == b)
            && equivalent_manifest(&a.manifest, &b.manifest)
            && definition_bytes(&a.path)
                .zip(definition_bytes(&b.path))
                .is_some_and(|(a, b)| a == b)
        {
            preserved.insert(id);
        }
        // A leaf can have a display-only new release. Its consumers still see a new version.
        if !preserved.contains(id) || a.manifest.version != b.manifest.version {
            changed.insert(id);
        }
    }
    for side in [manifest::Side::Client, manifest::Side::Server] {
        let a = dependencies::resolve_manifests(&old, side, &overrides).ok()?;
        let b = dependencies::resolve_manifests(&new, side, &overrides).ok()?;
        let ranks: BTreeMap<_, _> = b
            .order
            .into_iter()
            .map(|i| new[i].id.as_str())
            .filter(|id| old_ids.contains(id))
            .enumerate()
            .map(|(i, id)| (id, i))
            .collect();
        let mut prefix_max: Option<usize> = None;
        for (index, id) in a
            .order
            .into_iter()
            .map(|i| old[i].id.as_str())
            .filter(|id| ranks.contains_key(id))
            .enumerate()
        {
            let rank = ranks[id];
            // Ignore the positions of added/removed mods, but require the same set of
            // surviving predecessors; an equal numeric rank alone doesn't prove that.
            if rank != index || prefix_max.is_some_and(|previous| previous > rank) {
                preserved.remove(id);
                changed.insert(id);
            }
            prefix_max = Some(prefix_max.map_or(rank, |previous| previous.max(rank)));
        }
    }
    let mut consumers: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for m in old.into_iter().chain(new) {
        for target in m
            .dependencies
            .keys()
            .chain(m.incompatible.keys())
            .chain(m.discouraged.keys())
        {
            consumers.entry(target).or_default().push(&m.id);
        }
    }
    let mut pending: Vec<_> = changed.iter().copied().collect();
    while let Some(id) = pending.pop() {
        for consumer in consumers.get(id).into_iter().flatten().copied() {
            preserved.remove(consumer);
            if changed.insert(consumer) {
                pending.push(consumer);
            }
        }
    }
    Some(preserved)
}

fn equivalent_manifest(a: &manifest::Manifest, b: &manifest::Manifest) -> bool {
    let mut a = a.clone();
    // These fields describe a content archive to people. The dependency comparison
    // separately checks version uses. Comparing the rest keeps new semantic fields in scope.
    a.name.clone_from(&b.name);
    a.version.clone_from(&b.version);
    a.description.clone_from(&b.description);
    a.authors.clone_from(&b.authors);
    a.license.clone_from(&b.license);
    a.license_url.clone_from(&b.license_url);
    a.credits.clone_from(&b.credits);
    a.homepage.clone_from(&b.homepage);
    a.issues.clone_from(&b.issues);
    a == *b
}

fn overrides_bytes(mods: &Path) -> Option<Option<Vec<u8>>> {
    let path = mods.join(dependencies::OVERRIDES);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && metadata.len() <= manifest::MAX_MANIFEST => {
            let mut bytes = Vec::new();
            fs::File::open(path)
                .ok()?
                .take(manifest::MAX_MANIFEST + 1)
                .read_to_end(&mut bytes)
                .ok()?;
            (bytes.len() as u64 <= manifest::MAX_MANIFEST).then_some(Some(bytes))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Some(None),
        _ => None,
    }
}

fn definition_bytes(path: &Path) -> Option<BTreeMap<String, [u8; 32]>> {
    let mut zip = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let mut contents = BTreeMap::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).ok()?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().ok()?.into_owned();
        if matches!(name.as_str(), manifest::MANIFEST | "icon.png") {
            continue;
        }
        // Unknown package payloads may affect the game. Only compare the bounded flat inputs.
        files::check_path(&name).ok()?;
        if entry.size() > files::MAX_FILE || contents.len() >= files::MAX_FILES {
            return None;
        }
        let mut digest = Sha256::new();
        let n = io::copy(&mut entry.by_ref().take(files::MAX_FILE + 1), &mut digest).ok()?;
        total = total.checked_add(n)?;
        if n != entry.size()
            || total > files::MAX_TOTAL
            || contents.insert(name, digest.finalize().into()).is_some()
        {
            return None;
        }
    }
    Some(contents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_paths_and_compatible_ranges_do_not_prove_the_same_definitions() {
        let before = tempfile::tempdir().unwrap();
        let after = tempfile::tempdir().unwrap();
        let root = "root_mod";
        let dep = "required_mod";
        let defs = [(
            "data/root_mod/blocks/lamp.json",
            br#"{"properties":{"lit":[true,false]}}"#.as_slice(),
        )];
        let write = |home: &Path, id: &str, version: &str, data: &[(&str, &[u8])]| {
            let rule = if id == root {
                "[dependencies]\nrequired_mod='^1'\n"
            } else {
                ""
            };
            let bytes = manifest::Package::write_files(
                &format!("id='{id}'\nname='Test'\nversion='{version}'\napi='{}'\nside='both'\ncapabilities=['registries']\n{rule}", manifest::API),
                data,
            ).unwrap();
            fs::write(home.join(mod_list::DIR).join(format!("{id}.smod")), bytes).unwrap();
        };
        for home in [before.path(), after.path()] {
            fs::create_dir(home.join(mod_list::DIR)).unwrap();
            crate::browser::mod_list::set_all_enabled(home, &[root, dep], true).unwrap();
            write(home, root, "1.0.0", &defs);
            write(home, dep, "1.0.0", &[]);
        }
        let ids = BTreeSet::from([dep.to_owned(), root.to_owned()]);
        let watched: SuspectMods = ids
            .iter()
            .map(|id| (id.clone(), BTreeSet::from([id.clone()])))
            .collect();
        let compare = || {
            let old = managed_play::scan_packages(before.path()).unwrap();
            let new = managed_play::scan_packages(after.path()).unwrap();
            preserved_mods(
                before.path(),
                after.path(),
                &old.iter().collect::<Vec<_>>(),
                &new.iter().collect::<Vec<_>>(),
            )
            .map(|ids| ids.into_iter().map(str::to_owned).collect::<BTreeSet<_>>())
        };
        write(after.path(), root, "1.1.0", &defs);
        assert_eq!(compare(), Some(ids));
        write(after.path(), dep, "1.1.0", &[]);
        assert_eq!(compare(), Some(BTreeSet::from([dep.into()])));
        write(after.path(), dep, "1.0.0", &[]);
        let overrides = after
            .path()
            .join(mod_list::DIR)
            .join(dependencies::OVERRIDES);
        fs::write(&overrides, "[root_mod]\nrequired_mod='drop'\n").unwrap();
        assert_eq!(compare(), None);
        fs::remove_file(overrides).unwrap();
        write(after.path(), root, "1.1.0", &[(defs[0].0, b"{}")]);
        assert_eq!(compare(), Some(BTreeSet::from([dep.into()])));
        // Byte-equal archives still cannot testify to equivalence on an unproved base.
        assert_eq!(
            unproved_mod_effects(before.path(), None, before.path(), None, []).unwrap(),
            watched
        );
    }
}
