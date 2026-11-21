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
    let watched: SuspectMods = previous
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
    if old_on != new_on
        || !same_effects(before, after, &old_on, &watched, &previous, &next).unwrap_or(false)
    {
        return Ok(watched);
    }
    Ok(BTreeMap::new())
}

fn enabled(home: &Path) -> Result<BTreeSet<String>, String> {
    let path = home.join(mod_list::FILE);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(mod_list::ModList::parse(&text).enabled),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn same_effects(
    before: &Path,
    after: &Path,
    enabled: &BTreeSet<String>,
    watched: &SuspectMods,
    previous: &[ScannedPackage],
    next: &[ScannedPackage],
) -> Option<bool> {
    let previous: Vec<_> = previous
        .iter()
        .filter(|p| watched.contains_key(&p.manifest.id))
        .collect();
    let next: Vec<_> = next
        .iter()
        .filter(|p| watched.contains_key(&p.manifest.id))
        .collect();
    if previous.len() != next.len() || !enabled.iter().all(|id| watched.contains_key(id)) {
        return Some(false);
    }
    // A native mod can observe package metadata and change registrations; jars and unpacked
    // mods have inputs outside the flat content we can compare. Don't guess about any of them.
    if previous
        .iter()
        .chain(&next)
        .any(|p| p.directory || p.manifest.native.is_some() || !p.manifest.jar.is_empty())
    {
        return Some(false);
    }
    let before_mods = before.join(mod_list::DIR);
    let after_mods = after.join(mod_list::DIR);
    let before_overrides = overrides_bytes(&before_mods)?;
    if before_overrides != overrides_bytes(&after_mods)? {
        return Some(false);
    }
    let overrides = dependencies::Overrides::read(&before_mods).ok()?;
    let mut old = Vec::new();
    let mut new = Vec::new();
    for (a, b) in previous.iter().zip(&next) {
        // The loader's order and the location of each package are also part of the proof.
        if a.path.strip_prefix(before).ok()? != b.path.strip_prefix(after).ok()?
            || !equivalent_manifest(&a.manifest, &b.manifest)
            || definition_bytes(&a.path)? != definition_bytes(&b.path)?
        {
            return Some(false);
        }
        if enabled.contains(&a.manifest.id) {
            old.push(&a.manifest);
            new.push(&b.manifest);
        }
    }
    for (a, b) in old.iter().zip(&new) {
        // Updating a dependency changes its consumer's environment, even if the two
        // versions satisfy the same range. A leaf's display version can still advance.
        if a.version != b.version
            && old.iter().any(|consumer| {
                consumer.dependencies.contains_key(&a.id)
                    || consumer.incompatible.contains_key(&a.id)
                    || consumer.discouraged.contains_key(&a.id)
            })
        {
            return Some(false);
        }
    }
    for side in [manifest::Side::Client, manifest::Side::Server] {
        let a = dependencies::resolve_manifests(&old, side, &overrides).ok()?;
        let b = dependencies::resolve_manifests(&new, side, &overrides).ok()?;
        if a.order != b.order || a.warnings != b.warnings {
            return Some(false);
        }
    }
    Some(true)
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
