//! Switch plans against an exact prepared build and keeps the result private through approval.
use super::*;
use crate::{
    managed_play::{Active, CandidateImage, Held, HeldReason, HeldRoot, HeldSource},
    worlds::switch::{Changed, Fence, Question, TargetAge},
};
use std::{io, path::PathBuf};
use sunrise_launcher_smod::dependencies::parse_version;

pub struct SwitchHooks<'a> {
    pub cancellation: &'a native_mods::Cancellation,
    pub progress: &'a dyn Fn(InstallProgress),
    pub registry_crates: Option<&'a dyn Fn(&native_mods::Consent) -> bool>,
    pub approve: &'a dyn Fn(&SwitchQuestion) -> Result<bool, String>,
    pub at_cutoff: &'a dyn Fn() -> Result<(), String>,
}

/// The complete decision for one prepared candidate, never a request to update saves.
#[derive(Clone, Debug)]
pub struct SwitchQuestion {
    pub build: String,
    pub worlds: Question,
    pub held: Vec<String>,
    pub removed: Vec<(String, String)>,
}

pub enum Switched {
    AlreadySelected,
    Stayed,
    Changed(Box<Active>),
}

/// A positive picker note is a read-only check of the currently installed content
/// artifacts. Native SDK fit, possible replacements and ambiguous input stay unknown.
pub fn checked_switch_fit(home: &Path, tag: &str) -> bool {
    let prove = || -> Option<()> {
        let source = location();
        let local = super::super::local();
        if source != super::super::INDEX_URL && !(cfg!(debug_assertions) && local) {
            return None;
        }
        let (catalogue, _) = super::super::complete_index(&source, local).ok()?;
        let active = managed_play::display_active(home).ok()??;
        let game = backend::switch_game_version(tag).ok()?;
        let exact = Originals::read(home, Some(&active), &catalogue).ok()?;
        if !exact.removed.is_empty() {
            return None;
        }
        for (id, old) in &exact.mods {
            if old.held || old.seen.package.manifest.native.is_some() || !old.seen.owned {
                return None;
            }
            let digest = old.seen.digest.as_deref()?;
            let project = catalogue.project(id)?;
            let version = project.versions.iter().find(|v| {
                v.file.sha256 == digest
                    && v.game == game
                    && v.side != Some(Side::Server)
                    && v.capabilities.unknown().is_empty()
            })?;
            if project.kind != index::Kind::Mod || catalogue.revoked(id, digest).is_some() {
                return None;
            }
            index::check_file_with_facts(&catalogue, project, version, &old.seen.package.path)
                .ok()?;
        }
        validate_enabled(home, &catalogue).ok()?;
        if !active.matches_inputs(home)
            || managed_play::display_active(home).ok()?.as_ref() != Some(&active)
        {
            return None;
        }
        Some(())
    };
    prove().is_some()
}

pub fn switch_build(
    instance: &Instance,
    identity: Identity,
    tag: &str,
    hooks: SwitchHooks<'_>,
) -> Result<Switched, String> {
    if instance.kind == InstanceKind::Pack {
        return Err(tr("Choose an ordinary profile to switch builds.").into());
    }
    (hooks.progress)(InstallProgress::Checking);
    let mut profile =
        Profile::open_for(&instance.home, None, Some(identity)).map_err(String::from)?;
    let Some(edit) = profile.begin_switch(tag)? else {
        return Ok(Switched::AlreadySelected);
    };
    let game = backend::switch_game_version(edit.base())?;
    let source = location();
    let local = super::super::local();
    if source != super::super::INDEX_URL && !(cfg!(debug_assertions) && local) {
        return Err(tr("Connect to the Sunrise registry before switching builds.").into());
    }
    let (catalogue, evidence) = super::super::complete_index(&source, local)?;
    let old = edit.candidate()?;
    let original = Originals::read(old.path(), edit.current(), &catalogue)?;
    let download = tempfile::Builder::new()
        .prefix("switch-download-")
        .tempdir_in(old.path())
        .map_err(|e| at(old.path(), e))?;
    let choices = choices(
        &original,
        &catalogue,
        local,
        &game,
        download.path(),
        hooks.progress,
    )?;
    let mut selected = None;
    best_survivor_set(&choices, &original.mods, |set| {
        (hooks.progress)(InstallProgress::Preparing);
        let Some(enabled) = enabled_set(&set, &original.mods) else {
            return Ok(false);
        };
        let next = edit.candidate()?;
        apply(next.path(), &original, &set, &enabled)?;
        validate_enabled(next.path(), &catalogue).map_err(|e| e.to_string())?;
        let prove = |home: &Path| {
            edit.prove(
                home,
                instance.native_profile,
                hooks.cancellation,
                |step| {
                    super::build_progress(step, hooks.progress);
                },
                hooks.registry_crates,
            )
        };
        let Some(proof) = exact_image(prove(next.path()), &set)? else {
            return Ok(false);
        };
        // An intentionally off native choice still needs proof for its legitimate activation.
        // It may activate its required off dependencies, but never evict another enabled mod.
        for (&id, package) in &set {
            if package.manifest.native.is_none() || enabled.contains(id) {
                continue;
            }
            let mut activated = enabled.clone();
            if !activate(id, &set, &mut activated) || !fits(&set, &activated) {
                return Ok(false);
            }
            let trial = edit.candidate()?;
            apply(trial.path(), &original, &set, &activated)?;
            if exact_image(prove(trial.path()), &set)?.is_none() {
                return Ok(false);
            }
        }
        selected = Some((next, proof, set, enabled));
        Ok(true)
    })?;
    let (next, proof, set, enabled) = selected.ok_or_else(unproved_switch)?;
    let mut holds = Vec::new();
    let mut held = Vec::new();
    let mut changed = Vec::new();
    for (id, old) in &original.mods {
        let manifest = &old.seen.package.manifest;
        let chosen = set.get(id.as_str());
        if chosen.is_none() && !original.removed.contains_key(id) && !old.held {
            let path = &old.seen.package.path;
            let directory = path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str());
            let root = [HeldRoot::Native, HeldRoot::Content]
                .into_iter()
                .find(|root| Some(root.directory()) == directory)
                .ok_or_else(unproved_switch)?;
            let leaf = path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(unproved_switch)?;
            let digest = old.seen.digest.as_deref().ok_or_else(|| {
                trf(
                    "Pack {mod} as an .smod before switching so its original bytes can be kept.",
                    &[("mod", &manifest.name)],
                )
            })?;
            holds.push(Held::new(
                id.clone(),
                manifest.name.replace(char::is_control, " "),
                sha256_bytes(digest)?,
                (root, leaf.into()),
                old.source.clone(),
                old.on,
                HeldReason::NoFit { build: tag.into() },
            )?);
            held.push(manifest.name.clone());
        }
        if old.on {
            let loses_content = match chosen {
                Some(package) if enabled.contains(id.as_str()) => {
                    if package.digest == old.seen.digest {
                        false
                    } else if manifest.native.is_some() {
                        true
                    } else {
                        let path = next
                            .path()
                            .join(relative(&old.seen.package.path, original.home)?);
                        !super::super::content_loss::dropped(&old.seen.package.path, &path)?
                            .is_empty()
                    }
                }
                _ => true,
            };
            if loses_content {
                changed.push(Changed {
                    id,
                    title: &manifest.name,
                    namespaces: &manifest.namespaces,
                });
            }
        }
    }
    let resources = edit.switch_resources()?;
    let mut order = None;
    let mut compare = |commit: &str| {
        order
            .get_or_insert_with(|| backend::WorldBuildOrder::for_target(tag))
            .compare_saved(commit)
    };
    let (fence, worlds) = Fence::hold(
        &instance.home,
        &changed,
        Some(TargetAge {
            data_version: backend::prepared_world_version(&resources),
            compare_saved: &mut compare,
        }),
    )?;
    let question = SwitchQuestion {
        build: tag.into(),
        worlds,
        held,
        removed: original.removed.values().cloned().collect(),
    };
    if question.worlds.needed() && !(hooks.approve)(&question)? {
        return Ok(Switched::Stayed);
    }
    let active = edit.publish_switch(proof, hooks.cancellation, holds, original.removed, || {
        let (_, current) = super::super::complete_index(&source, local)?;
        if current != evidence {
            return Err(tr("The mod registry changed while the switch was being prepared. Nothing was switched.").into());
        }
        fence.revalidate(&instance.home)
    }, || {
        (hooks.at_cutoff)()?;
        (hooks.progress)(InstallProgress::Publishing);
        Ok(())
    })?;
    Ok(Switched::Changed(Box::new(active)))
}

struct Original {
    seen: SeenPackage,
    source: HeldSource,
    on: bool,
    held: bool,
}

struct Originals<'a> {
    home: &'a Path,
    mods: BTreeMap<String, Original>,
    removed: BTreeMap<String, (String, String)>,
}

impl<'a> Originals<'a> {
    fn read(
        home: &'a Path,
        active: Option<&Active>,
        catalogue: &index::Index,
    ) -> Result<Self, String> {
        let selection = home.join(sunrise_launcher_smod::mod_list::FILE);
        match fs::read_to_string(&selection) {
            Ok(_) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(at(&selection, e)),
        }
        let doc = Document::read(home)?;
        let mut mods = BTreeMap::new();
        let mut removed = BTreeMap::new();
        for (id, seen) in inventory(home)? {
            let manifest = &seen.package.manifest;
            let origin_at = crate::github_mods::origin_path(home, &id);
            let github = match fs::symlink_metadata(&origin_at) {
                Ok(meta) if meta.is_file() => {
                    Some(crate::github_mods::origin(home, &id).ok_or_else(unproved_switch)?)
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                _ => return Err(unproved_switch()),
            };
            if doc.row(&id)?.is_some() != seen.owned || (seen.owned && github.is_some()) {
                return Err(unproved_switch());
            }
            let source = if seen.owned {
                HeldSource::Registry {
                    project: id.clone(),
                    version: manifest.version.clone(),
                }
            } else if let Some(origin) = github {
                if origin.id != id
                    || origin.version != manifest.version
                    || seen.digest.as_deref() != Some(&origin.sha256)
                {
                    return Err(unproved_switch());
                }
                HeldSource::Github {
                    owner: origin.owner,
                    repo: origin.repo,
                    commit: origin.commit,
                    version: origin.version,
                }
            } else {
                HeldSource::Local {
                    version: manifest.version.clone(),
                }
            };
            let on = mod_list::is_enabled(home, &id);
            let held = active.and_then(|active| active.holds().find(|held| held.id() == id));
            if let Some(hold) = held {
                if on
                    || &source != hold.source()
                    || home.join(hold.path()) != seen.package.path
                    || seen
                        .digest
                        .as_deref()
                        .map(sha256_bytes)
                        .transpose()?
                        .as_ref()
                        != Some(hold.digest())
                {
                    return Err(unproved_switch());
                }
            }
            if let Some(revoked) = seen.digest.as_ref().and_then(|digest| {
                catalogue.revoked.iter().find(|revoked| {
                    revoked.id == id
                        && match &revoked.sha256 {
                            Some(hash) => hash.eq_ignore_ascii_case(digest),
                            None => seen.owned,
                        }
                })
            }) {
                removed.insert(id.clone(), (manifest.name.clone(), revoked.reason.clone()));
            }
            mods.insert(
                id,
                Original {
                    seen,
                    source,
                    on,
                    held: held.is_some(),
                },
            );
        }
        if active.is_some_and(|active| active.holds().any(|hold| !mods.contains_key(hold.id()))) {
            return Err(unproved_switch());
        }
        Ok(Self {
            home,
            mods,
            removed,
        })
    }
}

struct Package {
    manifest: Manifest,
    digest: Option<String>,
    replacement: Option<(PathBuf, installed::Entry)>,
}

type Choices = BTreeMap<String, Vec<Package>>;
type Set<'a> = BTreeMap<&'a str, &'a Package>;

fn choices(
    original: &Originals<'_>,
    catalogue: &index::Index,
    local: bool,
    game: &str,
    root: &Path,
    progress: &dyn Fn(InstallProgress),
) -> Result<Choices, String> {
    let mut todo: BTreeSet<_> = original.mods.keys().cloned().collect();
    let mut slots = BTreeMap::new();
    let mut fetched = 0;
    let doc = Document::read(original.home)?;
    while let Some(id) = todo.pop_first() {
        if slots.contains_key(&id) {
            continue;
        }
        if slots.len() == 128 {
            return Err(too_many());
        }
        let old = original.mods.get(&id);
        let mut available = Vec::new();
        if original.removed.contains_key(&id) || old.is_some_and(|old| old.held) {
            // Only an explicit Update on the current base can restore a prior hold.
        } else if let Some(old) = old.filter(|old| !old.seen.owned) {
            let manifest = &old.seen.package.manifest;
            if manifest.side != Side::Server && manifest.api == sunrise_launcher_smod::manifest::API
            {
                available.push(Package {
                    manifest: manifest.clone(),
                    digest: old.seen.digest.clone(),
                    replacement: None,
                });
            }
        } else if let Some(project) = catalogue.project(&id) {
            if !matches!(project.kind, index::Kind::Mod | index::Kind::NativeMod) {
                return Err(unproved_switch());
            }
            let root_name = if project.kind == index::Kind::NativeMod {
                HeldRoot::Native
            } else {
                HeldRoot::Content
            }
            .directory();
            for version in &project.versions {
                if version.side == Some(Side::Server)
                    || project.kind != index::Kind::NativeMod && version.game != game
                    || catalogue.revoked(&id, &version.file.sha256).is_some()
                {
                    continue;
                }
                if !version.capabilities.unknown().is_empty() {
                    return Err(unproved_switch());
                }
                let same =
                    old.filter(|old| old.seen.digest.as_deref() == Some(&version.file.sha256));
                let (file, replacement) = if let Some(old) = same {
                    (old.seen.package.path.clone(), None)
                } else {
                    fetched += 1;
                    if fetched > MAX_TRIALS {
                        return Err(too_many());
                    }
                    let dir = root.join(fetched.to_string());
                    let path = old
                        .map(|old| relative(&old.seen.package.path, original.home))
                        .transpose()?
                        .unwrap_or_else(|| Path::new(root_name).join(format!("{id}.smod")));
                    let filename = path
                        .to_str()
                        .ok_or_else(unproved_switch)?
                        .replace('\\', "/");
                    if !path.starts_with(root_name) || old.is_none() && doc.row(&id)?.is_some() {
                        return Err(unproved_switch());
                    }
                    doc.rows
                        .check_other_claims(original.home, &id, &filename)
                        .map_err(|e| e.render(trf))?;
                    let file = fetch(
                        &version_of(catalogue, project, version).files[0],
                        &dir,
                        local,
                        &|f| progress(InstallProgress::Downloading(f)),
                    )?;
                    let entry = installed::Entry {
                        source: SourceId::Sunrise,
                        project: id.clone(),
                        slug: id.clone(),
                        title: project.name.clone(),
                        kind: Kind::Mod,
                        version: version.version.clone(),
                        number: version.version.clone(),
                        file: filename,
                        checksum: format!("SHA-256:{}", version.file.sha256),
                        page: format!("{}/{}", super::super::SITE, id),
                        dependency: old.is_none()
                            || doc
                                .row(&id)?
                                .is_some_and(|row| row["dependency"].as_bool() == Some(true)),
                        verdict: None,
                        install_hold: None,
                    };
                    (file.clone(), Some((file, entry)))
                };
                let (trust, facts) =
                    index::check_file_with_facts(catalogue, project, version, &file)?;
                if version.dependencies.is_none()
                    && facts
                        .dependencies
                        .values()
                        .any(|need| !need.details().optional)
                {
                    return Err(unproved_switch());
                }
                let manifest = sunrise_launcher_smod::manifest::Package::read(&file)
                    .map_err(|e| at(&file, e))?
                    .manifest;
                let replacement = replacement.map(|(file, mut entry)| {
                    entry.verdict = Some(trust_label(trust).into());
                    (file, entry)
                });
                available.push(Package {
                    manifest,
                    digest: Some(version.file.sha256.clone()),
                    replacement,
                });
            }
        } else if old.is_some()
            && !catalogue
                .revoked
                .iter()
                .any(|revoked| revoked.id == id && revoked.sha256.is_none())
        {
            // A complete response can still omit a project. Omission doesn't prove
            // an installed archive incompatible (and must not turn it off).
            return Err(unproved_switch());
        }
        available.sort_by(|a, b| {
            parse_version(&b.manifest.version)
                .ok()
                .cmp(&parse_version(&a.manifest.version).ok())
                .then_with(|| a.digest.cmp(&b.digest))
        });
        for choice in &available {
            todo.extend(required(&choice.manifest).map(|(id, _)| id.clone()));
        }
        slots.insert(id, available);
    }
    Ok(slots)
}

fn relative(path: &Path, home: &Path) -> Result<PathBuf, String> {
    path.strip_prefix(home)
        .map(Path::to_owned)
        .map_err(|_| unproved_switch())
}

pub(super) fn sha256_bytes(hex: &str) -> Result<[u8; 32], String> {
    let mut bytes = [0; 32];
    if hex.len() != 64 || !hex.is_ascii() {
        return Err(unproved_switch());
    }
    for (i, out) in bytes.iter_mut().enumerate() {
        *out = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).map_err(|_| unproved_switch())?;
    }
    Ok(bytes)
}

fn required(
    manifest: &Manifest,
) -> impl Iterator<Item = (&String, &sunrise_launcher_smod::manifest::Dependency)> {
    manifest.dependencies.iter().filter(|(_, need)| {
        let need = need.details();
        !need.optional && need.side != Side::Server
    })
}

fn best_survivor_set<'a>(
    slots: &'a Choices,
    original: &BTreeMap<String, Original>,
    mut trial: impl FnMut(Set<'a>) -> Result<bool, String>,
) -> Result<(), String> {
    struct Search<'a> {
        slots: Vec<(&'a str, &'a [Package], bool)>,
        remaining: Vec<usize>,
        checked: usize,
    }
    impl<'a> Search<'a> {
        fn rank(
            &mut self,
            at: usize,
            retained: usize,
            chosen: &mut Set<'a>,
            trial: &mut impl FnMut(Set<'a>) -> Result<bool, String>,
        ) -> Result<bool, String> {
            if retained > self.remaining[at] {
                return Ok(false);
            }
            self.checked += 1;
            if self.checked > MAX_TRIALS * 128 {
                return Err(too_many());
            }
            let Some(&(id, choices, original)) = self.slots.get(at) else {
                let mut reachable = BTreeSet::new();
                for &(id, _, was) in &self.slots {
                    if was && chosen.contains_key(id) && !activate(id, chosen, &mut reachable) {
                        return Ok(false);
                    }
                }
                return if reachable.len() == chosen.len() {
                    trial(chosen.clone())
                } else {
                    Ok(false)
                };
            };
            if let Some(left) = retained.checked_sub(usize::from(original)) {
                for choice in choices {
                    chosen.insert(id, choice);
                    if self.rank(at + 1, left, chosen, trial)? {
                        return Ok(true);
                    }
                }
                chosen.remove(id);
            }
            self.rank(at + 1, retained, chosen, trial)
        }
    }
    let slots: Vec<_> = slots
        .iter()
        .map(|(id, choices)| (id.as_str(), choices.as_slice(), original.contains_key(id)))
        .collect();
    let mut remaining = vec![0; slots.len() + 1];
    for (i, (_, choices, was)) in slots.iter().enumerate().rev() {
        remaining[i] = remaining[i + 1] + usize::from(*was && !choices.is_empty());
    }
    let mut search = Search {
        slots,
        remaining,
        checked: 0,
    };
    let mut tried = 0;
    let mut checked_trial = |chosen| {
        tried += 1;
        if tried > MAX_TRIALS {
            return Err(too_many());
        }
        trial(chosen)
    };
    for retained in (0..=search.remaining[0]).rev() {
        if search.rank(0, retained, &mut BTreeMap::new(), &mut checked_trial)? {
            return Ok(());
        }
    }
    Err(unproved_switch())
}

fn activate<'a>(id: &'a str, set: &Set<'a>, selected: &mut BTreeSet<&'a str>) -> bool {
    if !selected.insert(id) {
        return true;
    }
    required(&set[id].manifest).all(|(dep, need)| {
        set.get_key_value(dep.as_str()).is_some_and(|(&key, pkg)| {
            parse_version(&pkg.manifest.version).is_ok_and(|v| need.details().version.matches(&v))
                && activate(key, set, selected)
        })
    })
}

fn enabled_set<'a>(
    set: &Set<'a>,
    original: &BTreeMap<String, Original>,
) -> Option<BTreeSet<&'a str>> {
    let mut enabled = BTreeSet::new();
    for &id in set.keys() {
        if original.get(id).is_some_and(|old| old.on) && !activate(id, set, &mut enabled) {
            return None;
        }
    }
    (enabled
        .iter()
        .all(|id| original.get(*id).is_none_or(|old| old.on))
        && fits(set, &enabled))
    .then_some(enabled)
}

fn fits(set: &Set<'_>, enabled: &BTreeSet<&str>) -> bool {
    let mut namespaces = BTreeSet::new();
    enabled.iter().all(|id| {
        let manifest = &set[id].manifest;
        manifest.owned_namespaces().all(|ns| namespaces.insert(ns))
            && manifest.incompatible.iter().all(|(other, rule)| {
                !enabled.contains(other.as_str())
                    || !parse_version(&set[other.as_str()].manifest.version)
                        .is_ok_and(|v| rule.details().version.matches(&v))
            })
    })
}

fn apply(
    home: &Path,
    original: &Originals<'_>,
    set: &Set<'_>,
    enabled: &BTreeSet<&str>,
) -> Result<(), String> {
    let mut doc = Document::read(home)?;
    for id in original.removed.keys() {
        let old = &original.mods[id];
        let target = home.join(relative(&old.seen.package.path, original.home)?);
        fs::remove_file(&target).map_err(|e| at(&target, e))?;
        if let HeldSource::Github { .. } = old.source {
            let path = crate::github_mods::origin_path(home, id);
            fs::remove_file(&path).map_err(|e| at(&path, e))?;
        }
        doc.remove(id)?;
    }
    for pkg in set.values() {
        if let Some((file, entry)) = &pkg.replacement {
            let target = home.join(&entry.file);
            let parent = target
                .parent()
                .expect("managed package destination has a root");
            fs::create_dir_all(parent).map_err(|e| at(parent, e))?;
            if !original.mods.contains_key(&entry.project)
                && target.try_exists().map_err(|e| at(&target, e))?
            {
                return Err(unproved_switch());
            }
            fs::copy(file, &target).map_err(|e| at(&target, e))?;
            doc.replace(entry)?;
        }
    }
    if !original.removed.is_empty() || set.values().any(|pkg| pkg.replacement.is_some()) {
        doc.write(home)?;
    }
    let off: Vec<_> = original
        .mods
        .iter()
        .filter(|(id, old)| old.on && !enabled.contains(id.as_str()))
        .map(|(id, _)| id.as_str())
        .collect();
    let on: Vec<_> = enabled
        .iter()
        .copied()
        .filter(|id| original.mods.get(*id).is_none_or(|old| !old.on))
        .collect();
    if !off.is_empty() {
        mod_list::set_all_enabled(home, &off, false)?
    }
    if !on.is_empty() {
        mod_list::set_all_enabled(home, &on, true)?
    }
    Ok(())
}

fn exact_image(
    result: Result<CandidateImage, native_mods::Failed>,
    set: &Set<'_>,
) -> Result<Option<CandidateImage>, String> {
    match result {
        Ok(image) => Ok(Some(image)),
        Err(native_mods::Failed::Mods {
            left,
            compiling: false,
            ..
        }) if left
            .iter()
            .any(|m| matches!(m.why, native_mods::Why::Unfit(_)))
            && left.iter().all(|m| {
                set.get(m.id.as_str())
                    .is_some_and(|p| p.manifest.version == m.version)
                    && matches!(
                        m.why,
                        native_mods::Why::Unfit(_) | native_mods::Why::Needs(_)
                    )
            }) =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

fn unproved_switch() -> String {
    tr("Sunrise couldn't verify the full mod set for this build. Nothing was switched.").into()
}

fn too_many() -> String {
    tr("Not every compatible mod set could be checked. Nothing was switched.").into()
}
