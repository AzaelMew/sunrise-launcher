//! Switch plans against an exact prepared build and keeps the result private through approval.
use super::*;
use crate::{
    managed_play::{Active, CandidateImage, Held, HeldReason, HeldRoot, HeldSource},
    worlds::switch::{Fence, Question, TargetAge},
};
use std::{io, path::PathBuf};
use sunrise_launcher_smod::dependencies::parse_version;

pub struct SwitchHooks<'a> {
    pub cancellation: &'a native_mods::Cancellation,
    pub progress: &'a dyn Fn(InstallProgress),
    pub registry_crates: Option<&'a dyn Fn(&native_mods::Consent) -> bool>,
    pub approve: &'a dyn Fn(&SwitchQuestion) -> Result<SwitchDecision, String>,
    pub at_cutoff: &'a dyn Fn() -> Result<(), String>,
}

/// The complete decision for one prepared candidate, never a request to update saves.
#[derive(Clone, Debug)]
pub struct SwitchQuestion {
    pub build: String,
    pub worlds: Question,
    pub held: Vec<String>,
    pub unproved: Vec<String>,
    pub removed: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwitchDecision {
    Cancel,
    TurnOff,
    Remove,
}

impl SwitchQuestion {
    pub fn needed(&self) -> bool {
        self.worlds.needed() || self.has_omissions() || !self.removed.is_empty()
    }

    pub fn has_omissions(&self) -> bool {
        !self.held.is_empty() || !self.unproved.is_empty()
    }

    pub fn omission_summary(&self) -> String {
        let count = self.held.len() + self.unproved.len();
        if count == 1 {
            tr("1 mod can’t move to this build yet. You can keep it installed but turned off, or remove it.").into()
        } else {
            trf(
                "{count} mods can’t move to this build yet. You can keep them installed but turned off, or remove them.",
                &[("count", &count.to_string())],
            )
        }
    }

    pub fn hold_lines(&self) -> impl Iterator<Item = String> + '_ {
        self.held
            .iter()
            .map(|name| trf("{mod} — Not compatible with this build.", &[("mod", name)]))
            .chain(
                self.unproved
                    .iter()
                    .map(|name| trf("{mod} — Fit could not be verified.", &[("mod", name)])),
            )
    }

    pub fn world_lines(&self) -> Vec<String> {
        #[derive(Default)]
        struct Summary<'a> {
            newer: bool,
            unknown_order: bool,
            mods: Option<&'a [String]>,
        }
        let mut by_world: BTreeMap<&str, Summary<'_>> = BTreeMap::new();
        for risk in &self.worlds.affected {
            if !risk.mods.is_empty() {
                by_world.entry(risk.world.as_str()).or_default().mods = Some(&risk.mods);
            }
        }
        for world in &self.worlds.newer {
            by_world.entry(world.as_str()).or_default().newer = true;
        }
        for world in &self.worlds.unknown_order {
            by_world.entry(world.as_str()).or_default().unknown_order = true;
        }
        let omitted: BTreeSet<&str> = self
            .held
            .iter()
            .chain(&self.unproved)
            .map(String::as_str)
            .collect();
        by_world
            .into_iter()
            .filter_map(|(world, summary)| {
                let age = if summary.newer {
                    Some(tr("last played on a newer game"))
                } else if summary.unknown_order {
                    Some(tr("last game build is unknown"))
                } else {
                    None
                };
                let mods = summary.mods.map(|list| {
                    let listed: BTreeSet<&str> = list.iter().map(String::as_str).collect();
                    if !omitted.is_empty() && listed == omitted {
                        tr("content from the mods above may be lost").into()
                    } else if !omitted.is_empty() && omitted.is_subset(&listed) {
                        let extra: Vec<_> = list
                            .iter()
                            .map(String::as_str)
                            .filter(|name| !omitted.contains(*name))
                            .collect();
                        trf(
                            "content from the mods above and {mods} may be lost",
                            &[("mods", &extra.join(", "))],
                        )
                    } else {
                        trf(
                            "content from these mods may be lost: {mods}",
                            &[("mods", &list.join(", "))],
                        )
                    }
                });
                match (age, mods) {
                    (Some(age), Some(mods)) => Some(trf(
                        "{world}: {age}; {mods}.",
                        &[("world", world), ("age", age), ("mods", &mods)],
                    )),
                    (Some(age), None) => {
                        Some(trf("{world}: {age}.", &[("world", world), ("age", age)]))
                    }
                    (None, Some(mods)) => Some(trf(
                        "{world}: {mods}.",
                        &[("world", world), ("mods", &mods)],
                    )),
                    (None, None) => None,
                }
            })
            .collect()
    }
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
        let (catalogue, _, official) = super::super::complete_index(&source, local).ok()?;
        let active = managed_play::display_active(home).ok()??;
        let game = backend::switch_game_version(tag).ok()?;
        let exact = Originals::read(home, Some(&active), &catalogue, official.as_ref()).ok()?;
        if !exact.removed.is_empty() {
            return None;
        }
        for (id, old) in &exact.mods {
            if old.held
                || old.withheld
                || old.seen.package.manifest.native.is_some()
                || !old.seen.registry_claim
            {
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
        validate_enabled(home, &catalogue, official.as_ref()).ok()?;
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
    (hooks.progress)(InstallProgress::PreparingBuild);
    let mut profile =
        Profile::open_for(&instance.home, None, Some(identity)).map_err(String::from)?;
    let Some(edit) = profile.begin_switch(tag)? else {
        return Ok(Switched::AlreadySelected);
    };
    (hooks.progress)(InstallProgress::Checking);
    let game = backend::switch_game_version(edit.base())?;
    let source = location();
    let local = super::super::local();
    if source != super::super::INDEX_URL && !(cfg!(debug_assertions) && local) {
        return Err(tr("Connect to the Sunrise registry before switching builds.").into());
    }
    let (catalogue, evidence, official) = super::super::complete_index(&source, local)?;
    let old = edit.candidate()?;
    let original = Originals::read(old.path(), edit.current(), &catalogue, official.as_ref())?;
    let download = tempfile::Builder::new()
        .prefix("switch-download-")
        .tempdir_in(old.path())
        .map_err(|e| at(old.path(), e))?;
    let mut choices = choices(
        &original,
        &catalogue,
        local,
        &game,
        download.path(),
        hooks.progress,
    )?;
    let mut omissions = Omissions {
        unproved: prune_missing_uses(&mut choices, &original, tag)?,
        ..Omissions::default()
    };
    let prove = |home: &Path| {
        edit.prove(
            home,
            instance.native_profile,
            hooks.cancellation,
            |step| super::build_progress(step, hooks.progress),
            hooks.registry_crates,
            None,
        )
    };
    let mut selected = None;
    best_survivor_set(&mut choices, &original.mods, |set| {
        let mut rejected = BTreeSet::new();
        (hooks.progress)(InstallProgress::Preparing);
        let Some(enabled) = enabled_set(&set, &original.mods) else {
            return Ok(Trial::Conflict);
        };
        let next = edit.candidate()?;
        apply(next.path(), &original, &set, &enabled)?;
        validate_enabled(next.path(), &catalogue, official.as_ref()).map_err(|e| e.to_string())?;
        let Some(proof) = exact_image(
            prove(next.path()),
            &set,
            &enabled,
            hooks.cancellation,
            &mut omissions,
            &mut rejected,
        )?
        else {
            return Ok(Trial::reject(rejected));
        };
        // An intentionally off native choice still needs proof for its legitimate activation.
        // It may activate its required off dependencies, but never evict another enabled mod.
        for (&id, package) in &set {
            if package.manifest.native.is_none() || enabled.contains(id) {
                continue;
            }
            let mut activated = enabled.clone();
            if !activate(id, &set, &mut activated) || !fits(&set, &activated) {
                return Ok(Trial::Conflict);
            }
            let trial = edit.candidate()?;
            apply(trial.path(), &original, &set, &activated)?;
            if exact_image(
                prove(trial.path()),
                &set,
                &activated,
                hooks.cancellation,
                &mut omissions,
                &mut rejected,
            )?
            .is_none()
            {
                return Ok(Trial::reject(rejected));
            }
        }
        let kept = set.keys().map(|&id| id.to_owned()).collect::<BTreeSet<_>>();
        let replaced = set
            .iter()
            .filter(|(_, package)| package.replacement.is_some())
            .map(|(&id, _)| id.to_owned())
            .collect::<Vec<_>>();
        selected = Some((next, proof, kept, replaced));
        Ok(Trial::Fit)
    })?;
    let (next, mut proof, kept, replaced) = selected.ok_or_else(unproved_switch)?;
    let mut holds = Vec::new();
    for (id, old) in &original.mods {
        let manifest = &old.seen.package.manifest;
        if !kept.contains(id) && !original.removed.contains_key(id) && !old.held {
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
            let reason = match omissions.known.remove(id) {
                Some(KnownFailure::NoFit(detail)) => HeldReason::NoFit {
                    build: tag.into(),
                    detail: Some(detail),
                },
                Some(KnownFailure::BuildFailed) => HeldReason::BuildFailed { build: tag.into() },
                None if omissions.unproved.contains(id) => {
                    HeldReason::Unproved { build: tag.into() }
                }
                None => HeldReason::NoFit {
                    build: tag.into(),
                    detail: None,
                },
            };
            holds.push(Held::new(
                id.clone(),
                manifest.name.replace(char::is_control, " "),
                sha256_bytes(digest)?,
                (root, leaf.into()),
                old.source.clone(),
                old.on,
                reason,
            )?);
        }
    }
    let (held, unknown, hold_labels) = omission_labels(&holds);
    let affected = crate::worlds::unproved_mod_effects(
        old.path(),
        edit.current(),
        next.path(),
        // The staged image proves compatibility with the target, not that the old
        // game and the new one interpret saved definitions in the same way.
        None,
        original
            .removed
            .keys()
            .chain(&replaced)
            .map(String::as_str)
            .chain(holds.iter().map(Held::id)),
    )?;
    let resources = edit.switch_resources()?;
    let mut order = None;
    let mut compare = |commit: &str| {
        order
            .get_or_insert_with(|| backend::WorldBuildOrder::for_target(tag))
            .compare_saved(commit)
    };
    let (fence, worlds) = Fence::hold(
        &instance.home,
        affected,
        |id| {
            hold_labels
                .get(id)
                .map(String::as_str)
                .unwrap_or(&original.mods[id].seen.package.manifest.name)
        },
        Some(TargetAge {
            data_version: backend::prepared_world_version(&resources),
            compare_saved: &mut compare,
        }),
    )?;
    let question = SwitchQuestion {
        build: tag.into(),
        worlds,
        held,
        unproved: unknown,
        removed: original.removed.values().cloned().collect(),
    };
    let mut removed = original.removed;
    let mut removal = None;
    if question.needed() {
        match (hooks.approve)(&question)? {
            SwitchDecision::Cancel => return Ok(Switched::Stayed),
            SwitchDecision::TurnOff => (),
            SwitchDecision::Remove if !holds.is_empty() => {
                let present = inventory(next.path(), None)?;
                let staged = mutations::stage_removal(
                    &instance.home,
                    next.path(),
                    edit.current(),
                    holds.iter().map(Held::id),
                    &present,
                )
                .map_err(|error| error.to_string())?;
                removed.extend(staged.archives.iter().map(|archive| {
                    (
                        archive.target.id.clone(),
                        (
                            archive.target.name.clone(),
                            tr("Removed when switching builds").into(),
                        ),
                    )
                }));
                holds.clear();
                proof = prove(next.path()).map_err(String::from)?;
                removal = Some(staged);
            }
            SwitchDecision::Remove => (),
        }
    }
    let active = edit.publish_switch(proof, hooks.cancellation, holds, removed, || {
        let (_, current, proof) = super::super::complete_index(&source, local)?;
        if current != evidence || proof.is_some() != official.is_some() {
            return Err(tr("The mod registry changed while the switch was being prepared. Nothing was switched.").into());
        }
        if let Some(removal) = removal {
            removal.revalidate(&instance.home)?;
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
    built_from_source: bool,
    on: bool,
    held: bool,
    withheld: bool,
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
        official: Option<&super::super::OfficialRegistry>,
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
        for (id, seen) in inventory(home, official)? {
            let manifest = &seen.package.manifest;
            let github = seen
                .verified_github_origin(home)
                .map_err(|()| unproved_switch())?;
            let has_row = doc.row(&id)?.is_some();
            if has_row != seen.registry_claim || (has_row && github.is_some()) {
                return Err(unproved_switch());
            }
            // The registry row selects a checked destination, not its publisher. Only the
            // actual digest in the official catalogue can authorize destroying old bytes.
            let built_from_source = github
                .as_ref()
                .is_some_and(|origin| origin.source.is_some());
            let source = if let Some(origin) = github {
                HeldSource::Github {
                    owner: origin.owner,
                    repo: origin.repo,
                    commit: origin.commit,
                    version: origin.version,
                }
            } else if seen.owned {
                HeldSource::Registry {
                    project: id.clone(),
                    version: manifest.version.clone(),
                }
            } else {
                HeldSource::Local {
                    version: manifest.version.clone(),
                }
            };
            let on = mod_list::is_enabled(home, &id);
            let held = active.and_then(|active| active.holds().find(|held| held.id() == id));
            if let Some(hold) = held {
                // Registry authorization can disappear or arrive on a later request. It
                // doesn't change the immutable identity of an already held exact archive.
                let same_source = &source == hold.source()
                    || matches!(
                        (&source, hold.source()),
                        (HeldSource::Registry { version: a, .. }, HeldSource::Local { version: b })
                            | (HeldSource::Local { version: a }, HeldSource::Registry { version: b, .. }) if a == b
                    );
                if on
                    || !same_source
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
            let revoked = seen.digest.as_deref().and_then(|digest| {
                let listed = catalogue.revoked(&id, digest)?;
                let authorized = !built_from_source
                    && (seen.owned
                        || (!seen.registry_claim
                            && official.is_some_and(|proof| proof.proves_archive(&id, digest))));
                (authorized || seen.registry_claim || listed.sha256.is_some())
                    .then_some((listed, authorized))
            });
            if let Some((revoked, true)) = revoked {
                removed.insert(id.clone(), (manifest.name.clone(), revoked.reason.clone()));
            }
            mods.insert(
                id,
                Original {
                    seen,
                    source,
                    built_from_source,
                    on,
                    held: held.is_some(),
                    withheld: revoked.is_some_and(|(_, authorized)| !authorized),
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
    unlisted: bool,
    built_from_source: bool,
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
        if original.removed.contains_key(&id) || old.is_some_and(|old| old.held || old.withheld) {
            // Preserve unproved takedown bytes, off. Only an explicit Update on the
            // current base can restore a prior hold.
        } else if let Some(old) = old.filter(|old| !old.seen.owned) {
            let manifest = &old.seen.package.manifest;
            // A complete index can prove fit for unchanged listed content. Absence of these
            // exact bytes from a present project instead means they're a pinned local input.
            let mut pinned = true;
            if old.seen.registry_claim {
                let project = catalogue.project(&id).ok_or_else(unproved_switch)?;
                let mut exact = project
                    .versions
                    .iter()
                    .filter(|version| old.seen.digest.as_deref() == Some(&version.file.sha256))
                    .peekable();
                pinned = exact.peek().is_none();
                if !pinned {
                    let release = exact.find(|version| {
                        version.side != Some(Side::Server)
                            && (manifest.native.is_some() || version.game == game)
                    });
                    let Some(version) = release else {
                        slots.insert(id, available);
                        continue;
                    };
                    index::check_file_with_facts(
                        catalogue,
                        project,
                        version,
                        &old.seen.package.path,
                    )?;
                }
            }
            if manifest.side != Side::Server && manifest.api == sunrise_launcher_smod::manifest::API
            {
                available.push(Package {
                    manifest: manifest.clone(),
                    digest: old.seen.digest.clone(),
                    replacement: None,
                    unlisted: pinned,
                    built_from_source: old.built_from_source,
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
                    unlisted: false,
                    built_from_source: false,
                });
            }
        } else if old.is_some() {
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
            todo.extend(
                choice
                    .manifest
                    .required_client_dependencies()
                    .map(|(id, _)| id.clone()),
            );
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

fn omission_labels(holds: &[Held]) -> (Vec<String>, Vec<String>, BTreeMap<String, String>) {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for hold in holds {
        *counts.entry(hold.title().to_lowercase()).or_default() += 1;
    }
    let mut held = Vec::new();
    let mut unproved = Vec::new();
    let mut labels = BTreeMap::new();
    for hold in holds {
        let label = if counts[&hold.title().to_lowercase()] > 1 {
            format!("{} ({})", hold.title(), hold.id())
        } else {
            hold.title().to_owned()
        };
        labels.insert(hold.id().to_owned(), label.clone());
        if hold.reason().unproved() {
            unproved.push(label);
        } else {
            held.push(label);
        }
    }
    (held, unproved, labels)
}

// A bare native archive pinned to a different build has no cross-build proof to try.
// Keep alternate versions eligible and leave any supplied uses list to the target SDK.
// Local developer builds require the exact checkout revision, so a mismatch there is a
// known incompatibility rather than an unverified fit.
fn prune_missing_uses(
    slots: &mut Choices,
    original: &Originals<'_>,
    tag: &str,
) -> Result<BTreeSet<String>, String> {
    let developer = crate::developer_install::uses_developer(tag);
    let mut revision = None;
    let mut omitted = BTreeSet::new();
    for (id, choices) in slots.iter_mut().filter(|(_, choices)| !choices.is_empty()) {
        for choice in std::mem::take(choices) {
            if let Some(native) = &choice.manifest.native {
                // If the full revision isn't known, leave the verdict to the exact builder.
                let target = revision.get_or_insert_with(|| native_mods::revision_of(tag).ok());
                if target
                    .as_ref()
                    .is_some_and(|target| *target != native.sunrise)
                {
                    let path = match &choice.replacement {
                        Some((file, _)) => file,
                        None => &original.mods[id].seen.package.path,
                    };
                    let archive =
                        zip::ZipArchive::new(fs::File::open(path).map_err(|e| at(path, e))?)
                            .map_err(|e| at(path, e))?;
                    if archive
                        .index_for_name(sunrise_launcher_smod::manifest::USES_FILE)
                        .is_none()
                    {
                        continue;
                    }
                }
            }
            choices.push(choice);
        }
        if choices.is_empty() && !developer {
            omitted.insert(id.clone());
        }
    }
    Ok(prune_dependents(slots, &omitted))
}

fn prune_dependents(slots: &mut Choices, unproved: &BTreeSet<String>) -> BTreeSet<String> {
    let variants = || {
        slots
            .iter()
            .filter(|(_, choices)| !choices.is_empty())
            .map(|(id, choices)| (id.as_str(), choices.iter()))
    };
    let mut unfit: BTreeSet<_> = slots
        .iter()
        .filter(|(id, choices)| choices.is_empty() && !unproved.contains(*id))
        .map(|(id, _)| id.as_str())
        .collect();
    spread_to_dependents(variants(), &mut unfit);
    let mut unknown: BTreeSet<_> = unproved.iter().map(String::as_str).collect();
    spread_to_dependents(variants(), &mut unknown);
    let unknown = unknown
        .difference(&unfit)
        .map(|&id| id.to_owned())
        .collect();
    let mut unavailable: BTreeSet<_> = slots
        .iter()
        .filter(|(_, choices)| choices.is_empty())
        .map(|(id, _)| id.as_str())
        .collect();
    spread_to_dependents(variants(), &mut unavailable);
    let unavailable: BTreeSet<_> = unavailable.into_iter().map(str::to_owned).collect();
    slots.retain(|id, choices| {
        if unavailable.contains(id) {
            return false;
        }
        choices.retain(|choice| {
            !choice
                .manifest
                .required_client_dependencies()
                .any(|(dep, _)| unavailable.contains(dep))
        });
        true
    });
    unknown
}

type Rejected = BTreeSet<(String, String)>;

enum Trial {
    Fit,
    Conflict,
    Rejected(Rejected),
}

impl Trial {
    fn reject(rejected: Rejected) -> Self {
        if rejected.is_empty() {
            Self::Conflict
        } else {
            Self::Rejected(rejected)
        }
    }
}

fn best_survivor_set(
    slots: &mut Choices,
    original: &BTreeMap<String, Original>,
    mut trial: impl FnMut(Set<'_>) -> Result<Trial, String>,
) -> Result<(), String> {
    struct Search<'a, 'budget> {
        slots: Vec<(&'a str, &'a [Package], bool)>,
        remaining: Vec<usize>,
        checked: &'budget mut usize,
    }
    impl<'a> Search<'a, '_> {
        fn rank(
            &mut self,
            at: usize,
            retained: usize,
            chosen: &mut Set<'a>,
            trial: &mut impl FnMut(Set<'a>) -> Result<Trial, String>,
        ) -> Result<Trial, String> {
            if retained > self.remaining[at] {
                return Ok(Trial::Conflict);
            }
            *self.checked += 1;
            if *self.checked > MAX_TRIALS * 128 {
                return Err(too_many());
            }
            let Some(&(id, choices, original)) = self.slots.get(at) else {
                let mut reachable = BTreeSet::new();
                for &(id, _, was) in &self.slots {
                    if was && chosen.contains_key(id) && !activate(id, chosen, &mut reachable) {
                        return Ok(Trial::Conflict);
                    }
                }
                return if reachable.len() == chosen.len() {
                    trial(chosen.clone())
                } else {
                    Ok(Trial::Conflict)
                };
            };
            if let Some(left) = retained.checked_sub(usize::from(original)) {
                for choice in choices {
                    chosen.insert(id, choice);
                    match self.rank(at + 1, left, chosen, trial)? {
                        Trial::Conflict => (),
                        result => return Ok(result),
                    }
                }
                chosen.remove(id);
            }
            self.rank(at + 1, retained, chosen, trial)
        }
    }
    let mut tried = 0;
    let mut checked = 0;
    'rerank: loop {
        let mut checked_trial = |chosen| {
            tried += 1;
            if tried > MAX_TRIALS {
                return Err(too_many());
            }
            trial(chosen)
        };
        let variants = slots
            .iter()
            .map(|(id, choices)| (id.as_str(), choices.as_slice(), original.contains_key(id)))
            .collect::<Vec<_>>();
        let mut remaining = vec![0; variants.len() + 1];
        for (i, (_, _, was)) in variants.iter().enumerate().rev() {
            remaining[i] = remaining[i + 1] + usize::from(*was);
        }
        let mut search = Search {
            slots: variants,
            remaining,
            checked: &mut checked,
        };
        for retained in (0..=search.remaining[0]).rev() {
            match search.rank(0, retained, &mut BTreeMap::new(), &mut checked_trial)? {
                Trial::Fit => return Ok(()),
                Trial::Conflict => (),
                Trial::Rejected(rejected) => {
                    // The SDK verdict is about these exact bytes, not this combination.
                    // Re-rank with the still-eligible versions, not subsets of failed ones.
                    for (id, digest) in rejected {
                        slots
                            .get_mut(&id)
                            .expect("trial choice")
                            .retain(|choice| choice.digest.as_ref() != Some(&digest));
                    }
                    prune_dependents(slots, &BTreeSet::new());
                    continue 'rerank;
                }
            }
        }
        return Err(unproved_switch());
    }
}

fn activate<'a>(id: &'a str, set: &Set<'a>, selected: &mut BTreeSet<&'a str>) -> bool {
    if !selected.insert(id) {
        return true;
    }
    set[id]
        .manifest
        .required_client_dependencies()
        .all(|(dep, need)| {
            set.get_key_value(dep.as_str()).is_some_and(|(&key, pkg)| {
                parse_version(&pkg.manifest.version)
                    .is_ok_and(|v| need.details().version.matches(&v))
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

/// Evidence accumulated while searching. A specific result about exact source bytes
/// takes precedence over an earlier trial that could not prove them.
#[derive(Default)]
struct Omissions {
    known: BTreeMap<String, KnownFailure>,
    unproved: BTreeSet<String>,
}

enum KnownFailure {
    NoFit(String),
    BuildFailed,
}

fn exact_image(
    result: Result<CandidateImage, native_mods::Failed>,
    set: &Set<'_>,
    enabled: &BTreeSet<&str>,
    cancellation: &native_mods::Cancellation,
    omissions: &mut Omissions,
    rejected: &mut Rejected,
) -> Result<Option<CandidateImage>, String> {
    use native_mods::{Failed, Why};
    let variants = || set.iter().map(|(&id, &pkg)| (id, std::iter::once(pkg)));
    match result {
        Ok(image) => Ok(Some(image)),
        Err(Failed::Mods {
            left, compiling, ..
        }) => {
            cancellation.check()?;
            let mut unfit = BTreeSet::new();
            let mut unknown = BTreeSet::new();
            for omission in left {
                let (&id, package) = set
                    .get_key_value(omission.id.as_str())
                    .filter(|(id, p)| {
                        enabled.contains(*id) && p.manifest.version == omission.version
                    })
                    .ok_or_else(unproved_switch)?;
                if !compiling && matches!(omission.why, Why::Unfit(_) | Why::Unproved(_)) {
                    if let Some(digest) = &package.digest {
                        rejected.insert((id.into(), digest.clone()));
                    }
                }
                match omission.why {
                    Why::Unfit(detail) if !compiling => {
                        unfit.insert(id);
                        if package.built_from_source {
                            omissions
                                .known
                                .insert(id.into(), KnownFailure::NoFit(detail));
                        }
                    }
                    Why::Unproved(_) if !compiling => {
                        unknown.insert(id);
                    }
                    Why::DidNotBuild if compiling && package.built_from_source => {
                        unfit.insert(id);
                        omissions
                            .known
                            .entry(id.into())
                            .or_insert(KnownFailure::BuildFailed);
                    }
                    Why::Needs(_) => (),
                    _ => return Err(unproved_switch()),
                }
            }
            if unfit.is_empty() && unknown.is_empty() {
                return Err(unproved_switch());
            }
            // The builder only sees native bundles and reports one cause per omitted
            // dependent. Use the entire trial's required client closure, including content
            // and all of its causes. A known incompatibility alone defeats that trial.
            spread_to_dependents(variants(), &mut unfit);
            spread_to_dependents(variants(), &mut unknown);
            omissions
                .unproved
                .extend(unknown.difference(&unfit).map(|&id| id.to_owned()));
            Ok(None)
        }
        Err(Failed::Sdk {
            refusal,
            mod_id: Some(id),
        }) => {
            cancellation.check()?;
            let Some((&id, _)) = set
                .get_key_value(id.as_str())
                .filter(|(id, package)| enabled.contains(*id) && package.built_from_source)
            else {
                return Err(refusal.to_string());
            };
            // Only this source's original SDK is missing. Keep trying the remaining
            // packages against the target SDK, including listed native mods.
            let mut unknown = BTreeSet::from([id]);
            spread_to_dependents(variants(), &mut unknown);
            omissions
                .unproved
                .extend(unknown.into_iter().map(str::to_owned));
            Ok(None)
        }
        Err(error @ (Failed::NoConsent | Failed::Sdk { mod_id: None, .. })) => {
            cancellation.check()?;
            // A global refusal proves no native input. As with lost native consent,
            // do not omit a listed mod without trying its exact build. Only source
            // receipts get the SDK fallback; other native paths keep their old rules.
            let no_consent = matches!(error, Failed::NoConsent);
            let mut unknown = BTreeSet::new();
            for (&id, p) in set {
                if enabled.contains(id) && p.manifest.native.is_some() {
                    let may_hold = if no_consent {
                        p.unlisted
                    } else {
                        p.built_from_source
                    };
                    if !may_hold {
                        return Err(error.into());
                    }
                    unknown.insert(id);
                }
            }
            if unknown.is_empty() {
                return Err(unproved_switch());
            }
            spread_to_dependents(variants(), &mut unknown);
            omissions
                .unproved
                .extend(unknown.into_iter().map(str::to_owned));
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}

fn spread_to_dependents<'a>(
    variants: impl Iterator<Item = (&'a str, impl Iterator<Item = &'a Package>)> + Clone,
    omitted: &mut BTreeSet<&'a str>,
) {
    loop {
        let before = omitted.len();
        for (id, mut choices) in variants.clone() {
            if choices.all(|package| {
                package
                    .manifest
                    .required_client_dependencies()
                    .any(|(dep, _)| omitted.contains(dep.as_str()))
            }) {
                omitted.insert(id);
            }
        }
        if before == omitted.len() {
            return;
        }
    }
}

fn unproved_switch() -> String {
    tr("Sunrise couldn't verify the full mod set for this build. Nothing was switched.").into()
}

fn too_many() -> String {
    tr("There are too many possible mod combinations to check. Remove mods you no longer need from this profile, then try switching again. Nothing was switched.").into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_official_historical_bytes_allow_a_staged_replacement_or_takedown() {
        let id = "coords_hud";
        for (takedown, official) in [(false, false), (false, true), (true, false), (true, true)] {
            let before = tempfile::tempdir().unwrap();
            let candidate = tempfile::tempdir().unwrap();
            let download = tempfile::tempdir().unwrap();
            let row =
                super::super::tests::archive(before.path(), id, "Coordinates HUD", "0.0.1", false);
            let archive = before.path().join(&row.file);
            let old = fs::read(&archive).unwrap();
            let mut catalogue = super::super::super::tests::sample();
            let project = catalogue.projects.iter_mut().find(|p| p.id == id).unwrap();
            let mut historical = project.versions[0].clone();
            historical.game = "26.2".into();
            historical.version = "0.0.1".into();
            historical.file.sha256 = index::sha256(&old);
            project.versions.push(historical);
            if takedown {
                catalogue.revoked.push(index::Revoked {
                    id: id.into(),
                    sha256: Some(index::sha256(&old)),
                    reason: "Fixture withdrawal".into(),
                    date: "2026-11-20".into(),
                });
            }
            // The claim locates the file; neither its recorded version nor its checksum proves
            // the archive. An untracked exact takedown is also allowed by the official proof.
            fs::create_dir_all(candidate.path().join(mod_list::MODS_DIR)).unwrap();
            fs::write(candidate.path().join(&row.file), &old).unwrap();
            mod_list::set_enabled(before.path(), id, true).unwrap();
            mod_list::set_enabled(candidate.path(), id, true).unwrap();
            if !takedown {
                let mut stale = row.clone();
                stale.version = "0.1.0".into();
                stale.checksum = format!("SHA-256:{}", "0".repeat(64));
                let mut doc = Document::read(before.path()).unwrap();
                doc.replace(&stale).unwrap();
                doc.write(before.path()).unwrap();
                doc.write(candidate.path()).unwrap();
            }
            let authority = official.then(|| OfficialRegistry::fixture(&catalogue));
            let originals =
                Originals::read(before.path(), None, &catalogue, authority.as_ref()).unwrap();
            let mut options = choices(
                &originals,
                &catalogue,
                true,
                "26.3",
                download.path(),
                &|_| {},
            )
            .unwrap();
            prune_dependents(&mut options, &BTreeSet::new());
            best_survivor_set(&mut options, &originals.mods, |set| {
                apply(
                    candidate.path(),
                    &originals,
                    &set,
                    &enabled_set(&set, &originals.mods).unwrap(),
                )?;
                Ok(Trial::Fit)
            })
            .unwrap();
            let changed = candidate.path().join(&row.file);
            match (takedown, official) {
                (true, true) => assert!(!changed.exists()),
                (false, true) => assert_eq!(
                    index::sha256_file(&changed).unwrap(),
                    catalogue.project(id).unwrap().versions[0].file.sha256
                ),
                (_, false) => {
                    assert_eq!(fs::read(&changed).unwrap(), old);
                    assert!(!mod_list::is_enabled(candidate.path(), id));
                }
            }
            assert_eq!(fs::read(archive).unwrap(), old);
        }
    }

    #[test]
    fn reranks_to_a_fitting_version_and_keeps_dependents_enabled_after_sdk_rejection() {
        let before = tempfile::tempdir().unwrap();
        let candidate = tempfile::tempdir().unwrap();
        let download = tempfile::tempdir().unwrap();
        let mut catalogue = super::super::super::tests::sample();
        let template = catalogue.projects[0].clone();
        catalogue.projects.clear();

        let pack_native = |version: &str, sunrise_pin: char| {
            let manifest = format!(
                "id='light_core'\nname='Light Core'\nversion='{version}'\napi='{}'\nside='both'\ncapabilities=['registries']\n[native]\nsunrise='{}'\nmixins='{}'\ngame='26.3'\n",
                sunrise_launcher_smod::manifest::API,
                sunrise_pin.to_string().repeat(40),
                "c".repeat(40),
            );
            let cargo = format!("[package]\nname='light_core'\nversion='{version}'\n");
            sunrise_launcher_smod::Package::write_files(
                &manifest,
                &[
                    ("Cargo.toml", cargo.as_bytes()),
                    ("src/lib.rs", b""),
                    (sunrise_launcher_smod::manifest::USES_FILE, b"{}"),
                ],
            )
            .unwrap()
        };
        let bytes_v1 = pack_native("1.0.0", 'a');
        let bytes_v2 = pack_native("2.0.0", 'a');
        let file_v1 = download.path().join("light_core-1.0.0.smod");
        let file_v2 = download.path().join("light_core-2.0.0.smod");
        fs::write(&file_v1, &bytes_v1).unwrap();
        fs::write(&file_v2, &bytes_v2).unwrap();

        let make_version = |kind: &index::Kind,
                            version: &str,
                            file: &Path,
                            bytes: &[u8],
                            deps: Vec<index::Dependency>| {
            let facts = index::inspect(kind, file).unwrap();
            let mut row = template.versions[0].clone();
            row.version = version.into();
            row.game = "26.3".into();
            row.api = facts.api;
            row.side = facts.side;
            row.capabilities =
                serde_json::from_value(serde_json::json!(facts.capabilities)).unwrap();
            row.dependencies = Some(deps);
            row.file = index::File {
                name: file.file_name().unwrap().to_str().unwrap().to_owned(),
                url: format!("file://{}", file.display()),
                size: bytes.len() as u64,
                sha256: index::sha256(bytes),
                signature: None,
            };
            row
        };

        let dep_bytes = sunrise_launcher_smod::Package::write_files(
            &format!(
                "id='lanterns'\nname='Lanterns'\nversion='1.0.0'\napi='{}'\nside='both'\ncapabilities=['registries']\n[dependencies]\nlight_core='>=1.0.0'\n",
                sunrise_launcher_smod::manifest::API
            ),
            &[],
        )
        .unwrap();
        let dep_file = download.path().join("lanterns-1.0.0.smod");
        fs::write(&dep_file, &dep_bytes).unwrap();

        catalogue.projects.push(index::Project {
            id: "light_core".into(),
            name: "Light Core".into(),
            kind: index::Kind::NativeMod,
            key: None,
            versions: vec![
                make_version(
                    &index::Kind::NativeMod,
                    "2.0.0",
                    &file_v2,
                    &bytes_v2,
                    Vec::new(),
                ),
                make_version(
                    &index::Kind::NativeMod,
                    "1.0.0",
                    &file_v1,
                    &bytes_v1,
                    Vec::new(),
                ),
            ],
            ..template.clone()
        });
        catalogue.projects.push(index::Project {
            id: "lanterns".into(),
            name: "Lanterns".into(),
            kind: index::Kind::Mod,
            key: None,
            versions: vec![make_version(
                &index::Kind::Mod,
                "1.0.0",
                &dep_file,
                &dep_bytes,
                vec![index::Dependency {
                    id: "light_core".into(),
                    version: ">=1.0.0".into(),
                    optional: false,
                    side: None,
                }],
            )],
            ..template
        });

        let install_entry = |id: &str, name: &str, version: &str, dir: &str, bytes: &[u8]| {
            let rel = format!("{dir}/{id}.smod");
            for root in [before.path(), candidate.path()] {
                fs::create_dir_all(root.join(dir)).unwrap();
                fs::write(root.join(&rel), bytes).unwrap();
            }
            installed::Entry {
                source: SourceId::Sunrise,
                project: id.into(),
                slug: id.into(),
                title: name.into(),
                kind: Kind::Mod,
                version: version.into(),
                number: version.into(),
                file: rel,
                checksum: format!("SHA-256:{}", index::sha256(bytes)),
                page: String::new(),
                dependency: false,
                verdict: None,
                install_hold: None,
            }
        };
        let core_row = install_entry(
            "light_core",
            "Light Core",
            "2.0.0",
            native_mods::INSTALLED,
            &bytes_v2,
        );
        let dep_row = install_entry(
            "lanterns",
            "Lanterns",
            "1.0.0",
            mod_list::MODS_DIR,
            &dep_bytes,
        );
        for root in [before.path(), candidate.path()] {
            let mut doc = Document::read(root).unwrap();
            doc.replace(&core_row).unwrap();
            doc.replace(&dep_row).unwrap();
            doc.write(root).unwrap();
            mod_list::set_all_enabled(root, &["light_core", "lanterns"], true).unwrap();
        }

        let authority = OfficialRegistry::fixture(&catalogue);
        let originals = Originals::read(before.path(), None, &catalogue, Some(&authority)).unwrap();
        let mut options = choices(
            &originals,
            &catalogue,
            true,
            "26.3",
            download.path(),
            &|_| {},
        )
        .unwrap();
        let cancellation = native_mods::Cancellation::default();
        let mut omissions = Omissions::default();
        let mut seen_versions = Vec::new();
        best_survivor_set(&mut options, &originals.mods, |set| {
            let enabled = enabled_set(&set, &originals.mods).unwrap();
            let core_version = set["light_core"].manifest.version.clone();
            seen_versions.push((core_version.clone(), set.contains_key("lanterns")));
            if core_version == "2.0.0" {
                let mut rejected = BTreeSet::new();
                let outcome = exact_image(
                    Err(native_mods::Failed::Mods {
                        left: vec![native_mods::LeftOut {
                            id: "light_core".into(),
                            version: "2.0.0".into(),
                            why: native_mods::Why::Unfit("hook changed".into()),
                            detail: None,
                        }],
                        error: "Light Core 2.0.0 does not fit".into(),
                        compiling: false,
                        log: None,
                    }),
                    &set,
                    &enabled,
                    &cancellation,
                    &mut omissions,
                    &mut rejected,
                )?;
                assert!(outcome.is_none());
                return Ok(Trial::reject(rejected));
            }
            apply(candidate.path(), &originals, &set, &enabled)?;
            Ok(Trial::Fit)
        })
        .unwrap();

        assert_eq!(
            seen_versions,
            vec![("2.0.0".into(), true), ("1.0.0".into(), true)]
        );
        assert_eq!(
            fs::read(candidate.path().join(&core_row.file)).unwrap(),
            bytes_v1
        );
        assert!(mod_list::is_enabled(candidate.path(), "light_core"));
        assert!(mod_list::is_enabled(candidate.path(), "lanterns"));
    }

    #[test]
    fn omission_labels_append_mod_ids_when_display_names_collide() {
        let hold = |id: &str, title: &str, unproved: bool| {
            Held::new(
                id.into(),
                title.into(),
                [7; 32],
                (HeldRoot::Native, format!("{id}.smod")),
                HeldSource::Local {
                    version: "1.0.0".into(),
                },
                true,
                if unproved {
                    HeldReason::Unproved {
                        build: "dev-2026-11-22".into(),
                    }
                } else {
                    HeldReason::NoFit {
                        build: "dev-2026-11-22".into(),
                        detail: None,
                    }
                },
            )
            .unwrap()
        };
        let holds = vec![
            hold("alpha_lantern", "Lanterns", false),
            hold("beetles", "Beetles", true),
            hold("beta_lantern", "lanterns", true),
        ];
        let (held, unproved, _) = omission_labels(&holds);
        assert_eq!(held, ["Lanterns (alpha_lantern)"]);
        assert_eq!(unproved, ["Beetles", "lanterns (beta_lantern)"]);
        let question = SwitchQuestion {
            build: "dev-2026-11-22".into(),
            worlds: Question::default(),
            held,
            unproved,
            removed: Vec::new(),
        };
        let lines: Vec<_> = question.hold_lines().collect();
        assert!(lines[0].contains("Lanterns (alpha_lantern)"), "{lines:?}");
        assert!(
            lines[1].contains("Beetles") && !lines[1].contains("(beetles)"),
            "{lines:?}"
        );
        assert!(lines[2].contains("lanterns (beta_lantern)"), "{lines:?}");
    }

    #[test]
    fn world_lines_group_per_world_and_refer_to_omitted_mods_above() {
        use crate::worlds::switch::WorldRisk;
        let question = SwitchQuestion {
            build: "dev-2026-11-22-bbbbbbb".into(),
            worlds: Question {
                affected: vec![
                    WorldRisk {
                        world: "Exact Match".into(),
                        mods: vec!["Beetles".into(), "Creature Library".into()],
                    },
                    WorldRisk {
                        world: "Glass Basin".into(),
                        mods: vec![
                            "Creature Library".into(),
                            "Beetles".into(),
                            "Fireflies".into(),
                        ],
                    },
                    WorldRisk {
                        world: "Subset Only".into(),
                        mods: vec!["Fireflies".into()],
                    },
                ],
                newer: vec!["Glass Basin".into(), "Newer Only".into()],
                unknown_order: vec!["Unknown Build".into()],
            },
            held: vec!["Creature Library".into()],
            unproved: vec!["Beetles".into()],
            removed: Vec::new(),
        };
        assert_eq!(
            question.world_lines(),
            [
                "Exact Match: content from the mods above may be lost.",
                "Glass Basin: last played on a newer game; content from the mods above and Fireflies may be lost.",
                "Newer Only: last played on a newer game.",
                "Subset Only: content from these mods may be lost: Fireflies.",
                "Unknown Build: last game build is unknown.",
            ]
        );
    }
}
