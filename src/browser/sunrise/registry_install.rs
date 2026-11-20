//! Registry and collection requests produce one admitted, fully proved same-base generation.
use super::{
    fetch, fetch_index, fits_game, index, installed, location, project_of, trust_label, version_of,
};
use crate::{
    backend,
    browser::{mod_list, Kind, SourceId},
    i18n::{tr, trf},
    instances::{Instance, Kind as InstanceKind},
    managed_play::{self, FirstBuild, Profile, ScannedPackage},
    native_mods,
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use sunrise_launcher_smod::{
    index::{browser_state, resolve},
    manifest::{Manifest, Side},
};

const MAX_TRIALS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InstallProgress {
    Checking,
    Downloading(f32),
    Preparing,
    Tools { done: u64, total: u64 },
    Building(Option<f32>),
    Publishing,
}

#[derive(Debug)]
pub enum InstallError {
    Refused(String),
    Unproved(String),
    Download(String),
    Cancelled,
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(text) | Self::Unproved(text) | Self::Download(text) => f.write_str(text),
            Self::Cancelled => f.write_str(tr("Install cancelled.")),
        }
    }
}

impl From<String> for InstallError {
    fn from(error: String) -> Self {
        Self::Unproved(error)
    }
}

#[derive(Clone, Debug)]
pub struct NativeQuestion {
    pub names: Vec<String>,
    pub reviewed: bool,
}

#[derive(Clone, Debug, Default)]
pub struct InstalledBatch {
    pub added: Vec<installed::Entry>,
    pub updated: Vec<(String, installed::Entry)>,
}

pub struct InstallHooks<'a> {
    pub progress: &'a dyn Fn(InstallProgress),
    pub native: &'a dyn Fn(&NativeQuestion) -> Result<bool, String>,
    pub registry_crates: Option<&'a dyn Fn(&native_mods::Consent) -> bool>,
}

/// Profile selection is captured at the click. A saved active/pending build wins over the
/// candidate in `choose`, just as in Play. This callback is never called again at cutoff.
pub fn install_registry(
    instance: &Instance,
    request: impl FnOnce() -> Vec<String>,
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: InstallHooks<'_>,
) -> Result<InstalledBatch, InstallError> {
    if instance.kind == InstanceKind::Pack {
        return Err(InstallError::Refused(
            tr("No registry mods to install in this profile.").into(),
        ));
    }
    (hooks.progress)(InstallProgress::Checking);
    let source = location();
    let local = super::local();
    if source != super::INDEX_URL && !(cfg!(debug_assertions) && local) {
        return Err(InstallError::Unproved(
            tr("Connect to the Sunrise registry before installing a mod and its dependencies.")
                .into(),
        ));
    }
    let (catalogue, source_bytes) = fetch_index(&source, local)?;
    if !catalogue.left_out.is_empty() {
        return Err(InstallError::Unproved(
            tr("The mod registry didn't send a complete index. Nothing was installed.").into(),
        ));
    }
    // Freeze the visible batch before starting any private planning. A later click must queue.
    let ids = request();
    if ids.is_empty() {
        return Err(InstallError::Cancelled);
    }
    // Keep the settings/profile choice stable through download, consent, compile and publish.
    if instance.kind == InstanceKind::Default {
        fs::create_dir_all(&instance.home).map_err(|e| at(&instance.home, e))?;
    }
    let mut profile = Profile::open(&instance.home, None)?;
    let edit = profile.begin_edit(
        choose,
        |f| (hooks.progress)(InstallProgress::Downloading(f)),
        &|_| (hooks.progress)(InstallProgress::Preparing),
    )?;
    let game = backend::game_version(edit.base());
    let cancelled = native_mods::Cancellation::default();
    let roots: Vec<_> = ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|id| resolve::Request { id, version: None })
        .collect();
    for need in &roots {
        if catalogue
            .project(&need.id)
            .is_none_or(|p| !matches!(p.kind, index::Kind::Mod | index::Kind::NativeMod))
        {
            return Err(InstallError::Refused(trf(
                "{mod} is not a mod in the Sunrise registry.",
                &[("mod", &need.id)],
            )));
        }
    }
    let old = edit.candidate()?;
    let before = inventory(old.path())?;
    for root in &roots {
        if let Some(package) = before.get(&root.id).filter(|package| !package.owned) {
            return Err(InstallError::Refused(trf(
                "{mod} is already installed from another source or location. Nothing was installed.",
                &[("mod", &package.package.manifest.name)],
            )));
        }
    }
    let held = before
        .iter()
        .filter(|(id, _)| mod_list::is_enabled(old.path(), id))
        .map(|(id, p)| {
            let managed = p.owned.then(|| p.digest.clone()).flatten();
            resolve::InstalledMod {
                id: id.clone(),
                version: p.package.manifest.version.clone(),
                sha256: managed,
            }
        })
        .collect::<Vec<_>>();
    let mut unfit = BTreeSet::new();
    let mut last_sdk_refusal = None;
    for _ in 0..MAX_TRIALS {
        let plan = resolve::install_whole(&catalogue, &roots, &held, &game, Side::Both, |p, v| {
            !unfit.contains(&(p.id.clone(), v.version.clone()))
        });
        let plan = match plan {
            Ok(plan) => plan,
            Err(resolve::WholeRefusal::Unproved(reason)) => {
                return Err(InstallError::Unproved(reason))
            }
            Err(resolve::WholeRefusal::Inconsistent(reason)) => {
                return Err(InstallError::Refused(last_sdk_refusal.unwrap_or(reason)))
            }
        };
        let versions = plan
            .install
            .iter()
            .map(|row| (row.id.as_str(), row.version.as_str()))
            .chain(
                plan.update
                    .iter()
                    .map(|row| (row.id.as_str(), row.to.as_str())),
            )
            .map(|(id, version)| {
                let project = catalogue.project(id).ok_or_else(unproved_mods)?;
                let version = project
                    .versions
                    .iter()
                    .find(|v| v.version == version && fits_game(v, &game))
                    .ok_or_else(unproved_mods)?;
                Ok((project, version))
            })
            .collect::<Result<Vec<_>, String>>()?;
        if versions.is_empty() {
            return Ok(InstalledBatch::default());
        }
        let need_native: Vec<_> = versions
            .iter()
            .filter(|(p, _)| p.kind == index::Kind::NativeMod)
            .map(|(p, _)| p.name.clone())
            .collect();
        if !need_native.is_empty() {
            if let Some(error) = backend::native_mod_architecture_error() {
                return Err(InstallError::Refused(error));
            }
        }
        let next = edit.candidate()?;
        let mut entries = BTreeMap::new();
        for (n, (project, version)) in versions.iter().enumerate() {
            let entry = stage_package(
                &catalogue,
                local,
                next.path(),
                &before,
                project,
                version,
                |f| {
                    (hooks.progress)(InstallProgress::Downloading(
                        (n as f32 + f) / versions.len() as f32,
                    ));
                },
            )?;
            entries.insert(entry.project.clone(), entry);
        }
        let mut doc = Document::read(next.path())?;
        for entry in entries.values_mut() {
            entry.dependency = !ids.contains(&entry.project);
            if let Some(old) = doc.row(&entry.project)? {
                entry.dependency &= old["dependency"].as_bool().unwrap_or(false);
            }
            doc.replace(entry)?;
        }
        doc.write(next.path())?;
        let enabled: Vec<_> = entries
            .keys()
            .filter(|id| !before.contains_key(*id) || mod_list::is_enabled(old.path(), id))
            .map(String::as_str)
            .collect();
        if !enabled.is_empty() {
            mod_list::set_all_enabled(next.path(), &enabled, true)?;
        }
        validate_enabled(next.path(), &catalogue)?;
        (hooks.progress)(InstallProgress::Preparing);
        if !need_native.is_empty() {
            if backend::NativeGrant::read()?.is_none()
                && !(hooks.native)(&NativeQuestion {
                    names: need_native,
                    reviewed: source == super::INDEX_URL,
                })?
            {
                return Err(InstallError::Cancelled);
            }
            // This comes from the strict saved grant, never from the dialog reply alone.
            cancelled
                .admit_native_work()
                .map_err(|e| InstallError::Unproved(e.into()))?;
        }
        let prove = || {
            edit.prove(
                next.path(),
                instance.native_profile,
                &cancelled,
                |step| {
                    let event = match step {
                        native_mods::Step::Tools { done, total, .. } => {
                            InstallProgress::Tools { done, total }
                        }
                        native_mods::Step::Install | native_mods::Step::Done { .. } => {
                            InstallProgress::Building(Some(1.))
                        }
                        _ => match step.progress() {
                            native_mods::Progress {
                                stage: native_mods::Stage::Compile,
                                fraction,
                            } => InstallProgress::Building(fraction),
                            _ => InstallProgress::Preparing,
                        },
                    };
                    (hooks.progress)(event);
                },
                hooks.registry_crates,
            )
        };
        let mut result = InstalledBatch::default();
        let proof = match prove() {
            Ok(proof) => proof,
            Err(native_mods::Failed::Mods {
                left,
                error,
                compiling: false,
            }) if !left.is_empty()
                && left.iter().all(|failure| match &failure.why {
                    native_mods::Why::Needs(_) => true,
                    native_mods::Why::Unfit(_) => entries
                        .get(&failure.id)
                        .is_some_and(|entry| entry.version == failure.version),
                    _ => false,
                }) =>
            {
                let first = unfit.len();
                unfit.extend(
                    left.into_iter()
                        .filter(|f| matches!(f.why, native_mods::Why::Unfit(_)))
                        .map(|f| (f.id, f.version)),
                );
                if first == unfit.len() {
                    return Err(InstallError::Unproved(error));
                }
                last_sdk_refusal = Some(error);
                continue;
            }
            Err(error) => return Err(InstallError::Unproved(error.into())),
        };
        // Refuse definition loss until the shared Update/Remove world approval is present.
        guard_replacements(&instance.home, next.path(), &before, &entries)?;
        (hooks.progress)(InstallProgress::Publishing);
        edit.publish(proof, &cancelled, || {
            let (_, current) = fetch_index(&source, local)?;
            if source_bytes != current {
                return Err(tr("The mod registry changed while the install was being prepared. Nothing was installed.").into());
            }
            guard_replacements(&instance.home, next.path(), &before, &entries)
        })?;
        for (id, entry) in entries {
            if let Some(previous) = before.get(&id) {
                result
                    .updated
                    .push((previous.package.manifest.version.clone(), entry));
            } else {
                result.added.push(entry);
            }
        }
        return Ok(result);
    }
    Err(InstallError::Unproved(
        tr("Not every compatible version could be checked. Nothing was installed.").into(),
    ))
}

struct SeenPackage {
    package: ScannedPackage,
    digest: Option<String>,
    owned: bool,
}

type Inventory = BTreeMap<String, SeenPackage>;

fn inventory(home: &Path) -> Result<Inventory, String> {
    let doc = Document::read(home)?;
    managed_play::scan_packages(home)?
        .into_iter()
        .map(|p| {
            let digest = (!p.directory)
                .then(|| index::sha256_file(&p.path))
                .transpose()?;
            let id = p.manifest.id.clone();
            let row: Option<installed::Entry> = doc
                .row(&id)?
                .and_then(|row| serde_json::from_value(row.clone()).ok());
            let owned = row.is_some_and(|row| {
                row.source == SourceId::Sunrise
                    && row.kind == Kind::Mod
                    && row.project == id
                    && row.version == p.manifest.version
                    && row.path(home).as_ref() == Some(&p.path)
                    && digest.as_ref().is_some_and(|digest| {
                        row.checksum
                            .eq_ignore_ascii_case(&format!("SHA-256:{digest}"))
                    })
            });
            Ok((
                id,
                SeenPackage {
                    package: p,
                    digest,
                    owned,
                },
            ))
        })
        .collect()
}

fn stage_package(
    catalogue: &index::Index,
    local: bool,
    home: &Path,
    before: &Inventory,
    p: &index::Project,
    v: &index::Version,
    progress: impl Fn(f32),
) -> Result<installed::Entry, InstallError> {
    let shown = project_of(catalogue, p, &v.game).ok_or_else(unproved_mods)?;
    let native = p.kind == index::Kind::NativeMod;
    let dir = if native {
        native_mods::INSTALLED
    } else {
        mod_list::MODS_DIR
    };
    let relative = format!("{dir}/{}.smod", p.id);
    let target = home.join(&relative);
    let claims = browser_state::Installed::read(home).map_err(|error| error.details(trf))?;
    let existing = before.get(&p.id);
    if existing.is_some_and(|old| !old.owned || !old.package.path.ends_with(&relative)) {
        return Err(InstallError::Refused(trf(
            "{mod} is already installed from another source or location. Nothing was installed.",
            &[("mod", &p.name)],
        )));
    }
    let row = claims.single(&p.id).map_err(|error| error.render(trf))?;
    if row.is_some() != existing.is_some()
        || existing.is_none() && target.try_exists().map_err(|e| at(&target, e))?
    {
        return Err(unproved_mods().into());
    }
    claims
        .check_other_claims(home, &p.id, &relative)
        .map_err(|error| error.render(trf))?;
    let staging = tempfile::Builder::new()
        .prefix("package-")
        .tempdir_in(home)
        .map_err(|e| at(home, e))?;
    let file = &version_of(catalogue, p, v).files[0];
    let fetched = fetch(file, staging.path(), local, &progress).map_err(InstallError::Download)?;
    let (checked, facts) =
        index::check_file_with_facts(catalogue, p, v, &fetched).map_err(InstallError::Download)?;
    if v.dependencies.is_none()
        && facts
            .dependencies
            .values()
            .any(|entry| !entry.details().optional)
    {
        return Err(InstallError::Unproved(trf(
            "The registry didn't list the required dependencies for {mod}. Nothing was installed.",
            &[("mod", &p.name)],
        )));
    }
    let dest = target.parent().expect("managed mod parent");
    fs::create_dir_all(dest).map_err(|e| at(dest, e))?;
    fs::rename(&fetched, &target).map_err(|e| at(&target, e))?;
    Ok(installed::Entry {
        source: SourceId::Sunrise,
        project: p.id.clone(),
        slug: p.id.clone(),
        title: p.name.clone(),
        kind: Kind::Mod,
        version: v.version.clone(),
        number: v.version.clone(),
        file: relative,
        checksum: format!("SHA-256:{}", v.file.sha256),
        page: shown.page,
        dependency: false,
        verdict: Some(trust_label(checked).into()),
    })
}

fn required_satisfied(manifest: &Manifest, enabled: &BTreeMap<&str, &Manifest>) -> bool {
    manifest.dependencies.iter().all(|(id, dependency)| {
        let need = dependency.details();
        need.optional
            || enabled.get(id.as_str()).is_some_and(|other| {
                semver::Version::parse(&other.version).is_ok_and(|v| need.version.matches(&v))
            })
    })
}

fn validate_enabled(home: &Path, catalogue: &index::Index) -> Result<(), InstallError> {
    let packages = inventory(home)?;
    let enabled: BTreeMap<_, _> = packages
        .iter()
        .filter(|(id, _)| mod_list::is_enabled(home, id))
        .map(|(id, p)| (id.as_str(), &p.package.manifest))
        .collect();
    let mut namespaces = BTreeSet::new();
    for (&id, &manifest) in &enabled {
        let package = &packages[id];
        let revocation = package.digest.as_ref().and_then(|hash| {
            if package.owned {
                catalogue.revoked(id, hash)
            } else if manifest.native.is_some() {
                catalogue.revoked.iter().find(|row| {
                    row.id == id
                        && row
                            .sha256
                            .as_ref()
                            .is_some_and(|listed| listed.eq_ignore_ascii_case(hash))
                })
            } else {
                None
            }
        });
        if let Some(revocation) = revocation {
            return Err(InstallError::Refused(trf(
                "{mod} was taken down: {reason}",
                &[("mod", &manifest.name), ("reason", &revocation.reason)],
            )));
        }
        if !required_satisfied(manifest, &enabled)
            || !manifest.owned_namespaces().all(|ns| namespaces.insert(ns))
            || manifest.incompatible.iter().any(|(id, rule)| {
                enabled.get(id.as_str()).is_some_and(|other| {
                    semver::Version::parse(&other.version)
                        .is_ok_and(|v| rule.details().version.matches(&v))
                })
            })
        {
            return Err(InstallError::Refused(trf(
                "The proposed mods don't work with {mod}. Nothing was installed.",
                &[("mod", &manifest.name)],
            )));
        }
    }
    Ok(())
}

fn guard_replacements(
    home: &Path,
    candidate: &Path,
    before: &Inventory,
    entries: &BTreeMap<String, installed::Entry>,
) -> Result<(), String> {
    for (id, entry) in entries {
        let Some(old) = before.get(id) else {
            continue;
        };
        let unknown = old.package.manifest.native.is_some();
        if unknown
            || !super::content_loss::dropped(&old.package.path, &candidate.join(&entry.file))?
                .is_empty()
        {
            let saves = home.join("saves");
            match fs::read_dir(&saves) {
                Ok(mut rows) => {
                    if rows
                        .next()
                        .transpose()
                        .map_err(|e| at(&saves, e))?
                        .is_some()
                    {
                        return Err(trf("Installing this version of {mod} could remove content from saved worlds. Nothing was installed.", &[("mod", &entry.title)]));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(at(&saves, error)),
            }
        }
    }
    Ok(())
}

struct Document {
    map: Map<String, Value>,
    rows: browser_state::Installed,
}
impl Document {
    fn read(home: &Path) -> Result<Self, String> {
        let path = home.join(installed::MANIFEST);
        let mut map: Map<String, Value> = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| at(&path, e))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Map::from_iter([("entries".into(), Value::Array(Vec::new()))])
            }
            Err(error) => return Err(at(&path, error)),
        };
        let Some(Value::Array(entries)) = map.remove("entries") else {
            return Err(unproved_mods());
        };
        Ok(Self {
            map,
            rows: browser_state::Installed { entries },
        })
    }
    fn position(&self, id: &str) -> Result<Option<usize>, String> {
        self.rows.single(id).map_err(|e| e.render(trf))
    }
    fn row(&self, id: &str) -> Result<Option<&Value>, String> {
        Ok(self.position(id)?.map(|i| &self.rows.entries[i]))
    }
    fn set(&mut self, id: &str, value: Value) -> Result<(), String> {
        if let Some(i) = self.position(id)? {
            self.rows.entries[i] = value;
        } else {
            self.rows.entries.push(value);
        }
        Ok(())
    }
    fn replace(&mut self, entry: &installed::Entry) -> Result<(), String> {
        let encoded = serde_json::to_value(entry).map_err(|e| e.to_string())?;
        let mut fields = match self.row(&entry.project)? {
            Some(Value::Object(previous)) => previous.clone(),
            Some(_) => return Err(unproved_mods()),
            None => Map::new(),
        };
        fields.remove("verdict");
        let Value::Object(encoded) = encoded else {
            unreachable!("Entry serializes as an object")
        };
        fields.extend(encoded);
        self.set(&entry.project, Value::Object(fields))
    }
    fn write(&self, home: &Path) -> Result<(), String> {
        let mut map = self.map.clone();
        map.insert("entries".into(), Value::Array(self.rows.entries.clone()));
        let bytes = serde_json::to_vec(&map).map_err(|e| e.to_string())?;
        let path = home.join(installed::MANIFEST);
        fs::write(&path, bytes).map_err(|e| at(&path, e))
    }
}

fn unproved_mods() -> String {
    tr("Sunrise couldn't verify the installed mod set. Nothing was installed.").into()
}
fn at(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}
