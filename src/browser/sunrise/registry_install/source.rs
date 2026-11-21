//! Plan source bytes without compiling, then pass the exact choice to the ordinary GitHub edit.
use super::{
    github::{self, Entry},
    *,
};
use crate::{
    browser::sunrise::sources::{Failure, SourceCommit},
    github_mods::{self, Origin, Packed},
};

mod mixed;
pub(super) use mixed::MixedSources;
pub use mixed::{update_registry_and_sources, CombinedUpdate};

pub enum SourceRequest {
    Add(SourceCommit),
    Update {
        pin: SourceCommit,
        predecessor: Box<Origin>,
    },
}

pub struct PreparedSource {
    identity: Identity,
    base: String,
    active: Option<managed_play::Active>,
    before: [u8; 32],
    checked_base: FirstBuild,
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
    pub fn base(&self) -> &str {
        &self.base
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
    Dependency {
        required: Option<String>,
        detail: String,
    },
    DoesNotFit(String),
    Unproved(String),
    Changed(String),
    Tools(String),
    Build {
        detail: String,
        log: Option<Arc<str>>,
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
            Self::Dependency { detail, .. }
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
        let (package, origin) =
            github_mods::pack_pin(&pin, cancellation).map_err(SourceError::Github)?;
        if let Some(previous) = &predecessor {
            if !github_mods::forward(previous, &origin).map_err(SourceError::Github)? {
                return Err(SourceError::Changed(
                    tr("There is no newer source on the same tracker.").into(),
                ));
            }
        }
        prepare_package(
            instance,
            identity,
            base,
            (package, origin),
            predecessor,
            cancellation,
        )
    })();
    if cancellation.is_stopped() {
        Err(SourceError::Cancelled)
    } else {
        result
    }
}

/// Read and check all eligible native bundles in a pasted link's exact snapshot.
pub fn prepare_pasted_sources(
    instance: &Instance,
    identity: Identity,
    base: &str,
    link: &github_mods::Link,
    found: &github_mods::Resolved,
    cancellation: &native_mods::Cancellation,
) -> Result<Vec<PreparedSource>, SourceError> {
    let result = (|| {
        let packages =
            github_mods::pack_pasted(link, found, cancellation).map_err(SourceError::Github)?;
        packages
            .into_iter()
            .map(|package| prepare_package(instance, identity, base, package, None, cancellation))
            .collect()
    })();
    if cancellation.is_stopped() {
        Err(SourceError::Cancelled)
    } else {
        result
    }
}

fn prepare_package(
    instance: &Instance,
    identity: Identity,
    base: &str,
    (package, origin): (Packed, Origin),
    predecessor: Option<Origin>,
    cancellation: &native_mods::Cancellation,
) -> Result<PreparedSource, SourceError> {
    if instance.kind == InstanceKind::Pack {
        return Err(SourceError::Changed(
            tr("This pack controls which mods it installs.").into(),
        ));
    }
    cancellation.check()?;
    let checked_base = FirstBuild::installed(base)?;
    if checked_base.base_tag() != base {
        return Err(changed_source_target());
    }
    let stage = tempfile::tempdir().map_err(|e| e.to_string())?;
    let (before, active, origins) = {
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
        let entry = Entry {
            package: &package,
            origin: &origin,
            prior: predecessor
                .as_ref()
                .map_or(github::Prior::Fresh, github::Prior::Source),
        };
        let origins = github::predecessors(&instance.home, &[entry])?;
        managed_play::copy_install_inputs(&instance.home, stage.path())?;
        if managed_play::inputs(stage.path(), None)? != before
            || managed_play::inputs(&instance.home, None)? != before
        {
            return Err(changed_source_target());
        }
        (before, active, origins)
    };
    let result = PreparedSource {
        identity,
        base: base.into(),
        active,
        before,
        checked_base,
        package,
        origin,
        predecessor,
    };
    let trial = github::stage_entries(
        stage.path(),
        &[result.entry()],
        &origins,
        result.active.as_ref(),
        base,
    )?;
    check_source_set(stage.path(), &trial.off, &result.base, cancellation)?;
    Ok(result)
}

pub struct SourceHooks<'a> {
    pub install: InstallHooks<'a>,
    pub cancellation: &'a native_mods::Cancellation,
    pub authorize: &'a dyn Fn(&[PreparedSource]) -> Result<bool, String>,
    pub approve_worlds: &'a dyn Fn(&ChangeImpact) -> bool,
}

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
    if prepared.iter().any(|entry| {
        entry.identity != identity
            || entry.base != first.base
            || entry.before != first.before
            || entry.active != first.active
            || !entry.checked_base.same_cached_base(&first.checked_base)
    }) {
        return Err(changed_source_target());
    }
    if !first
        .checked_base
        .same_cached_base(&FirstBuild::installed(&first.base)?)
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
            if !first.checked_base.same_cached_base(&first_build) {
                return Err(changed_source_target().to_string());
            }
            Ok(first_build)
        },
        hooks.install,
        Some(hooks.approve_worlds),
        Some(github::SourceBatch {
            before: first.before,
            base: &first.base,
            active: first.active.as_ref(),
            cancellation,
            authorize: &|| (hooks.authorize)(prepared),
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
        if let Some(conflict) = manifest_conflict(home, catalogue.as_deref(), None, |_| false)? {
            return Err(SourceError::Dependency {
                required: None,
                detail: conflict.reason,
            });
        }
        let routes =
            native_mods::source_routes(home, base, cancellation).map_err(|why| match why {
                native_mods::Failed::Mods { error, .. } => SourceError::DoesNotFit(error),
                why => SourceError::Unproved(why.to_string()),
            })?;
        for (_, route) in routes.mods {
            match route {
                native_mods::Route::Prebuilt | native_mods::Route::Fits { .. } => (),
                native_mods::Route::Unfit { line, .. } => {
                    return Err(SourceError::DoesNotFit(line))
                }
                native_mods::Route::Unchecked { reason }
                | native_mods::Route::Invalid { reason } => {
                    return Err(SourceError::Unproved(reason))
                }
            }
        }
        Ok(())
    })
}
