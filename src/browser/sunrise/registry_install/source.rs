//! Plan source bytes without compiling, then pass the exact choice to the ordinary GitHub edit.
use super::{
    github::{self, Entry},
    *,
};
use crate::{
    browser::sunrise::sources::{Failure, SourceCommit},
    github_mods::{self, AddedFrom, Origin, Packed},
};

mod dependencies;
pub use dependencies::SourceDependency;
pub(super) use dependencies::{complete_source_add, SourceAdd};
#[cfg(test)]
mod graph_tests;
mod mixed;
#[cfg(all(test, target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
pub(super) use mixed::update_registry_and_sources_with_registry;
pub(super) use mixed::MixedSources;
pub use mixed::{update_registry_and_sources, CombinedUpdate};

pub enum SourceRequest {
    Add(SourceCommit),
    Update {
        pin: SourceCommit,
        predecessor: Box<Origin>,
    },
}

/// The exact profile identity, active state, mod inputs and cached base build that a
/// prepared source or update check was proved against.
#[derive(Clone, Debug)]
pub struct SourceProof {
    pub(super) identity: Identity,
    pub(super) base: String,
    pub(super) active: Option<managed_play::Active>,
    pub(super) before: [u8; 32],
    pub(super) checked_base: Arc<FirstBuild>,
}

impl PartialEq for SourceProof {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.base == other.base
            && self.before == other.before
            && self.active == other.active
            && self.checked_base.same_cached_base(&other.checked_base)
    }
}

impl Eq for SourceProof {}

impl SourceProof {
    pub fn capture(
        instance: &Instance,
        identity: Identity,
        base: &str,
    ) -> Result<Self, SourceError> {
        if instance.kind == InstanceKind::Pack {
            return Err(SourceError::Changed(
                tr("This pack controls which mods it installs.").into(),
            ));
        }
        let checked_base = Arc::new(FirstBuild::installed(base)?);
        if checked_base.base_tag() != base {
            return Err(changed_source_target());
        }
        let _lock = crate::browser::lock_home(&instance.home).map_err(|e| e.to_string())?;
        let active = managed_play::display_active(&instance.home)?;
        if Identity::of(&instance.home)? != identity
            || active
                .as_ref()
                .is_some_and(|current| current.base() != base)
        {
            return Err(changed_source_target());
        }
        let before = managed_play::inputs(&instance.home, None)?;
        Ok(Self {
            identity,
            base: base.into(),
            active,
            before,
            checked_base,
        })
    }
}

pub struct PreparedSource {
    pub(super) proof: SourceProof,
    package: Packed,
    origin: Origin,
    predecessor: Option<Origin>,
}

impl PreparedSource {
    pub fn origin(&self) -> &Origin {
        &self.origin
    }
    pub fn name(&self) -> &str {
        &self.package.name
    }
    pub fn sha256(&self) -> &str {
        &self.package.sha256
    }
    pub fn base(&self) -> &str {
        &self.proof.base
    }
    pub fn proof(&self) -> &SourceProof {
        &self.proof
    }
    pub fn is_update(&self) -> bool {
        self.predecessor.is_some()
    }
    fn entry(&self) -> Entry<'_> {
        Entry {
            package: &self.package,
            origin: &self.origin,
            prior: self
                .predecessor
                .as_ref()
                .map_or(github::Prior::Fresh, github::Prior::Source),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceError {
    Cancelled,
    Github(Failure),
    DependencyGithub {
        name: String,
        failure: Failure,
    },
    Dependency {
        required: Option<String>,
        detail: String,
    },
    /// A public catalogue or resolver explanation, not a raw filesystem or network error.
    Catalogue(String),
    DoesNotFit(String),
    Unproved(String),
    Changed(String),
    Tools(String),
    Build {
        detail: String,
        log: Option<Arc<str>>,
        failed: Vec<String>,
    },
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => f.write_str(tr("Source build canceled. Nothing was changed.")),
            Self::Github(Failure::RepositoryUnavailable) => {
                f.write_str(tr("This repository is private, removed or moved."))
            }
            Self::Github(Failure::RefUnavailable) => {
                f.write_str(tr("Tracked branch or release unavailable."))
            }
            Self::Github(Failure::RateLimited { .. }) => {
                f.write_str(tr("GitHub is limiting requests. Try again later."))
            }
            Self::Github(Failure::NoManifest) => f.write_str(tr("No Sunrise mod at this commit.")),
            Self::Github(Failure::Offline(detail) | Failure::Other(detail)) => f.write_str(detail),
            Self::DependencyGithub {
                failure: Failure::Offline(detail),
                ..
            } => f.write_str(detail),
            Self::DependencyGithub { name, failure } => {
                let why = Self::Github(failure.clone()).to_string();
                f.write_str(&trf(
                    "{mod} couldn't be read from GitHub. {why}",
                    &[("mod", name), ("why", &why)],
                ))
            }
            Self::Dependency { detail, .. }
            | Self::Catalogue(detail)
            | Self::Build { detail, .. }
            | Self::DoesNotFit(detail)
            | Self::Unproved(detail)
            | Self::Changed(detail)
            | Self::Tools(detail) => f.write_str(detail),
        }
    }
}

impl From<String> for SourceError {
    fn from(detail: String) -> Self {
        Self::Unproved(detail)
    }
}

impl From<InstallError> for SourceError {
    fn from(error: InstallError) -> Self {
        match error {
            InstallError::Cancelled => Self::Cancelled,
            InstallError::NotForVersion(why) => Self::DoesNotFit(why),
            InstallError::Source(error) => error,
            error => Self::Unproved(error.to_string()),
        }
    }
}

pub(super) fn changed_source_target() -> SourceError {
    SourceError::Changed(
        tr("The profile or game build changed. Check the source again; nothing was changed.")
            .into(),
    )
}

pub fn prepare_source(
    instance: &Instance,
    identity: Identity,
    base: &str,
    request: SourceRequest,
    cancellation: &native_mods::Cancellation,
) -> Result<PreparedSource, SourceError> {
    let (pin, predecessor) = match request {
        SourceRequest::Add(pin) => (pin, None),
        SourceRequest::Update { pin, predecessor } => (pin, Some(*predecessor)),
    };
    let result = (|| {
        cancellation.check()?;
        let (package, mut origin) =
            github_mods::pack_pin(&pin, cancellation).map_err(SourceError::Github)?;
        if let Some(previous) = &predecessor {
            if !github_mods::forward(previous, &origin).map_err(SourceError::Github)? {
                return Err(SourceError::Changed(
                    tr("There is no newer source on the same tracker.").into(),
                ));
            }
            origin.added_from.clone_from(&previous.added_from);
        } else {
            origin.added_from = Some(AddedFrom::Discover);
        }
        prepare_packages(
            instance,
            identity,
            base,
            vec![(PastedSource { package, origin }, predecessor)],
            false,
            false,
            cancellation,
        )
        .map(|mut prepared| prepared.pop().expect("one source was checked"))
    })();
    if cancellation.is_stopped() {
        Err(SourceError::Cancelled)
    } else {
        result
    }
}

/// One pinned listing's exact bytes, to check with others through [`prepare_pasted_sources`].
pub fn pack_sources(
    pins: &[SourceCommit],
    added_from: AddedFrom,
    cancellation: &native_mods::Cancellation,
) -> Vec<Result<PastedSource, SourceError>> {
    github_mods::pack_pins(pins, cancellation)
        .into_iter()
        .map(|result| {
            result
                .map(|(package, mut origin)| {
                    origin.added_from = Some(added_from.clone());
                    PastedSource { package, origin }
                })
                .map_err(SourceError::Github)
        })
        .collect()
}

/// Uncompiled, exact bytes from a single pasted GitHub snapshot. Being listed is not a fit proof.
#[derive(Clone)]
pub struct PastedSource {
    package: Packed,
    origin: Origin,
}

impl PastedSource {
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    pub fn name(&self) -> &str {
        &self.package.name
    }
}

pub fn read_pasted_sources(
    link: &github_mods::Link,
    found: &github_mods::Resolved,
    cancellation: &native_mods::Cancellation,
) -> Result<Vec<PastedSource>, SourceError> {
    let result = github_mods::pack_pasted(link, found, cancellation)
        .map(|choices| {
            choices
                .into_iter()
                .map(|(package, origin)| PastedSource { package, origin })
                .collect()
        })
        .map_err(SourceError::Github);
    if cancellation.is_stopped() {
        Err(SourceError::Cancelled)
    } else {
        result
    }
}

/// Check the selected set with the profile's catalogue. The queued worker repeats this
/// fit check against the same profile inputs, and only its exact staged set goes to consent.
pub fn prepare_pasted_sources(
    instance: &Instance,
    identity: Identity,
    base: &str,
    selected: Vec<PastedSource>,
    cancellation: &native_mods::Cancellation,
) -> Result<Vec<PreparedSource>, SourceError> {
    let result = prepare_packages(
        instance,
        identity,
        base,
        selected
            .into_iter()
            .map(|package| (package, None))
            .collect(),
        true,
        true,
        cancellation,
    );
    if cancellation.is_stopped() {
        Err(SourceError::Cancelled)
    } else {
        result
    }
}

fn prepare_packages(
    instance: &Instance,
    identity: Identity,
    base: &str,
    packages: Vec<(PastedSource, Option<Origin>)>,
    follow_catalogue: bool,
    refresh_manual: bool,
    cancellation: &native_mods::Cancellation,
) -> Result<Vec<PreparedSource>, SourceError> {
    let stage = tempfile::tempdir().map_err(|e| e.to_string())?;
    let (result, origins) = bind_packages(
        instance,
        identity,
        base,
        packages,
        Some(stage.path()),
        refresh_manual,
        cancellation,
    )?;
    let entries = result.iter().map(PreparedSource::entry).collect::<Vec<_>>();
    let trial = github::stage_entries(
        stage.path(),
        &entries,
        &origins,
        result[0].proof.active.as_ref(),
        base,
    )?;
    if follow_catalogue && origins.is_empty() {
        complete_source_add(
            instance,
            stage.path(),
            base,
            &entries,
            result[0].proof.active.as_ref(),
            cancellation,
            &|_| {},
        )?;
    } else {
        check_source_set(stage.path(), &trial.off, base, cancellation)?;
    }
    Ok(result)
}

/// Install exact source bytes after one fit check and consent, leaving the build for Play.
pub fn add_sources(
    instance: &Instance,
    identity: Identity,
    base: &str,
    selected: Vec<PastedSource>,
    hooks: SourceHooks<'_>,
) -> Result<(), SourceError> {
    let (prepared, _) = bind_packages(
        instance,
        identity,
        base,
        selected.into_iter().map(|entry| (entry, None)).collect(),
        None,
        false,
        hooks.cancellation,
    )?;
    if prepared.is_empty() {
        return hooks.cancellation.check().map_err(SourceError::from);
    }
    apply_sources(
        instance,
        identity,
        &prepared,
        || FirstBuild::installed(base),
        hooks,
    )
}

fn same_manual_source(installed: &Origin, next: &Origin) -> bool {
    installed.is_manual()
        && installed.local_folder_path().is_none()
        && installed.id == next.id
        && installed.owner.eq_ignore_ascii_case(&next.owner)
        && installed.repo.eq_ignore_ascii_case(&next.repo)
        && match (&installed.source, &next.source) {
            (Some(old), Some(new)) => old.repo_id == new.repo_id && old.manifest == new.manifest,
            _ => false,
        }
}

fn bind_packages(
    instance: &Instance,
    identity: Identity,
    base: &str,
    packages: Vec<(PastedSource, Option<Origin>)>,
    copy_to: Option<&Path>,
    refresh_manual: bool,
    cancellation: &native_mods::Cancellation,
) -> Result<(Vec<PreparedSource>, BTreeMap<String, Origin>), SourceError> {
    if packages.is_empty() {
        return Err(SourceError::Cancelled);
    }
    if instance.kind == InstanceKind::Pack {
        return Err(SourceError::Changed(
            tr("This pack controls which mods it installs.").into(),
        ));
    }
    cancellation.check()?;
    let checked_base = Arc::new(FirstBuild::installed(base)?);
    if checked_base.base_tag() != base {
        return Err(changed_source_target());
    }
    {
        let _lock = crate::browser::lock_home(&instance.home).map_err(|e| e.to_string())?;
        let active = managed_play::display_active(&instance.home)?;
        if Identity::of(&instance.home)? != identity
            || active
                .as_ref()
                .is_some_and(|current| current.base() != base)
        {
            return Err(changed_source_target());
        }
        let before = managed_play::inputs(&instance.home, None)?;
        let mut selected = Vec::with_capacity(packages.len());
        for (pasted, mut predecessor) in packages {
            // An earlier Add only satisfies a pick after publishing these exact bytes.
            // A checked preview/update has predecessor rules of its own and never skips here.
            if copy_to.is_none() && source_is_published(instance, active.as_ref(), &pasted.origin) {
                continue;
            }
            if refresh_manual
                && predecessor.is_none()
                && pasted.origin.added_from == Some(AddedFrom::Github)
            {
                if let Ok(existing) =
                    github::installed_origin(&instance.home, &pasted.origin.id, |_| true)
                {
                    if same_manual_source(&existing, &pasted.origin) {
                        if existing.sha256.eq_ignore_ascii_case(&pasted.origin.sha256) {
                            return Err(SourceError::from(InstallError::Refused(
                                tr("That GitHub archive is already installed. Nothing was changed.")
                                    .into(),
                            )));
                        }
                        predecessor = Some(existing);
                    }
                }
            }
            selected.push(PreparedSource {
                proof: SourceProof {
                    identity,
                    base: base.into(),
                    active: active.clone(),
                    before,
                    checked_base: checked_base.clone(),
                },
                package: pasted.package,
                origin: pasted.origin,
                predecessor,
            });
        }
        let origins = if let Some(stage) = copy_to {
            let entries = selected
                .iter()
                .map(PreparedSource::entry)
                .collect::<Vec<_>>();
            let origins = github::predecessors(&instance.home, &entries)?;
            managed_play::copy_install_inputs(&instance.home, stage)?;
            if managed_play::inputs(stage, None)? != before
                || managed_play::inputs(&instance.home, None)? != before
            {
                return Err(changed_source_target());
            }
            origins
        } else {
            BTreeMap::new()
        };
        Ok((selected, origins))
    }
}

fn source_is_published(
    instance: &Instance,
    active: Option<&managed_play::Active>,
    origin: &Origin,
) -> bool {
    let home = &instance.home;
    let Some(recorded) = github_mods::origin(home, &origin.id) else {
        return false;
    };
    // A queued add from another page may ask for exactly the same bytes. Leave the first
    // successful add's display origin alone; it doesn't change the installed source pin.
    let expected = Origin {
        added_from: recorded.added_from.clone(),
        ..origin.clone()
    };
    recorded == expected
        && active.is_none_or(|active| active.holds().all(|held| held.id() != origin.id))
        && github::installed_origin(home, &origin.id, |installed| *installed == recorded).is_ok()
}

pub struct SourceHooks<'a> {
    pub install: InstallHooks<'a>,
    pub cancellation: &'a native_mods::Cancellation,
    pub authorize: &'a SourceAuthorization<'a>,
    pub approve_worlds: &'a dyn Fn(&ChangeImpact) -> bool,
}

pub type SourceAuthorization<'a> =
    dyn Fn(&[PreparedSource], &[SourceDependency]) -> Result<bool, String> + 'a;

pub fn apply_sources(
    instance: &Instance,
    identity: Identity,
    prepared: &[PreparedSource],
    choose: impl FnOnce() -> Result<FirstBuild, String>,
    hooks: SourceHooks<'_>,
) -> Result<(), SourceError> {
    let cancellation = hooks.cancellation;
    let Some(first) = prepared.first() else {
        return Err(SourceError::Cancelled);
    };
    if prepared
        .iter()
        .any(|entry| entry.proof != first.proof || entry.proof.identity != identity)
    {
        return Err(changed_source_target());
    }
    if !first
        .proof
        .checked_base
        .same_cached_base(&FirstBuild::installed(&first.proof.base)?)
    {
        return Err(changed_source_target());
    }
    let result = github::edit_github(
        instance,
        identity,
        &prepared
            .iter()
            .map(PreparedSource::entry)
            .collect::<Vec<_>>(),
        || {
            let first_build = choose()?;
            if !first.proof.checked_base.same_cached_base(&first_build) {
                return Err(changed_source_target().to_string());
            }
            Ok(first_build)
        },
        hooks.install,
        Some(hooks.approve_worlds),
        github::Check::Source(github::SourceBatch {
            before: first.proof.before,
            base: &first.proof.base,
            active: first.proof.active.as_ref(),
            cancellation,
            authorize: &|dependencies| (hooks.authorize)(prepared, dependencies),
        }),
    )
    .map_err(SourceError::from);
    if cancellation.is_stopped() {
        Err(SourceError::Cancelled)
    } else {
        result
    }
}

pub(super) fn check_source_set(
    home: &Path,
    off_trial: &[String],
    base: &str,
    cancellation: &native_mods::Cancellation,
) -> Result<(), SourceError> {
    github::with_trial_selection(home, off_trial, || {
        let packages = inventory(home, None)?;
        let enabled = mutations::enabled(home)?;
        let manifests = packages
            .iter()
            .filter(|(id, _)| enabled.contains(*id))
            .map(|(id, p)| (id.as_str(), &p.package.manifest))
            .collect::<BTreeMap<_, _>>();
        for manifest in manifests.values() {
            if let Some(required) = missing_required(manifest, &manifests) {
                return Err(SourceError::Dependency {
                    required: Some(required.into()),
                    detail: trf(
                        "{mod} needs {required}. Install a fitting version first.",
                        &[("mod", &manifest.name), ("required", required)],
                    ),
                });
            }
        }
        let catalogue = super::super::fetched_index();
        if let Some(conflict) =
            manifest_conflict(home, catalogue.as_deref(), None, Side::Client, |_| false)?
        {
            return Err(SourceError::Dependency {
                required: None,
                detail: conflict.reason,
            });
        }
        source_fit(home, base, cancellation)?
            .mods
            .into_iter()
            .try_for_each(|(_, route)| checked_source_route(route))
    })
}

fn source_fit(
    home: &Path,
    base: &str,
    cancellation: &native_mods::Cancellation,
) -> Result<native_mods::Routes, SourceError> {
    // A source check waits for another build quietly, under the step it already shows.
    native_mods::source_routes(home, base, cancellation, &|| ()).map_err(|why| match why {
        // Stopped while it waited for another build.
        _ if cancellation.is_stopped() => SourceError::Cancelled,
        native_mods::SourceCheckError::Dependency(detail) => SourceError::Dependency {
            required: None,
            detail,
        },
        native_mods::SourceCheckError::Fit(native_mods::Failed::Mods { error, .. }) => {
            SourceError::DoesNotFit(error)
        }
        native_mods::SourceCheckError::Fit(why) => SourceError::Unproved(why.to_string()),
    })
}

fn checked_source_route(route: native_mods::Route) -> Result<(), SourceError> {
    match route {
        native_mods::Route::Prebuilt | native_mods::Route::Fits { .. } => Ok(()),
        native_mods::Route::Unfit { line, .. } | native_mods::Route::Unproven { line } => {
            Err(SourceError::DoesNotFit(line))
        }
        native_mods::Route::Unchecked { reason } | native_mods::Route::Invalid { reason } => {
            Err(SourceError::Unproved(reason))
        }
    }
}
