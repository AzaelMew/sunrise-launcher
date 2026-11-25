//! Whether a release fits the exact build, from its generated fit lists only, and the newest fitting one.
use super::*;

/// Compatibility state of a project for the selected profile's exact build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) enum CardFit {
    /// At least one non-revoked release lists the exact build in `fits`.
    /// Carries the newest fitting release's version.
    Fits { version: String },
    /// No non-revoked release fits, and the newest non-revoked release lists the exact build in
    /// `does_not_fit`.
    DoesNotFit { build_label: String },
    /// No non-revoked release fits, and the exact build is in neither `fits` nor `does_not_fit`.
    NotCheckedYet { build_label: String },
    /// The index says nothing of the build, and this computer's check found that a release
    /// fits it. Carries the newest such release's version.
    FitsHere { version: String },
    /// The index says nothing of the build, and this computer's check found that the newest
    /// release does not fit it.
    DoesNotFitHere { build_label: String },
    /// The index says nothing of the build, and this computer is checking the newest release.
    Checking { build_label: String },
    /// The project or every one of its releases is taken down in `index.revoked`.
    Revoked(String),
    /// No release can be installed, and the newest one fits but lists original jars that
    /// Modrinth no longer serves and this computer has not cached.
    JarUnavailable(Vec<JarRef>),
    /// No release can be installed, and the newest one fits but needs mods that have no release
    /// for the build, as the index lists in `dependency_unavailable`.
    DependencyUnavailable {
        build_label: String,
        mods: Vec<String>,
    },
    /// The selected profile has no game build yet.
    NoBuild,
}

impl CardFit {
    pub(in crate::mods) fn fits(&self) -> bool {
        matches!(self, CardFit::Fits { .. } | CardFit::FitsHere { .. })
    }

    pub(in crate::mods) fn fitting_version(&self) -> Option<&str> {
        match self {
            CardFit::Fits { version, .. } | CardFit::FitsHere { version } => Some(version),
            _ => None,
        }
    }

    /// The fit pill a fitting card shows.
    pub(in crate::mods) fn fit_pill(&self) -> Option<String> {
        match self {
            CardFit::Fits { version } => {
                Some(trf("{version} · Fits build", &[("version", version)]))
            }
            CardFit::FitsHere { version } => Some(trf(
                "{version} · Fits build (checked on this computer)",
                &[("version", version)],
            )),
            _ => None,
        }
    }

    /// Whether a card that can't be installed is drawn faded: not while its fit is being
    /// checked.
    pub(in crate::mods) fn faded(&self) -> bool {
        !self.fits() && !matches!(self, CardFit::Checking { .. })
    }

    /// The reason a card is greyed out and cannot be installed on the selected profile.
    pub(in crate::mods) fn reason(&self) -> Option<String> {
        match self {
            CardFit::Fits { .. } | CardFit::FitsHere { .. } => None,
            CardFit::DoesNotFit { build_label } => {
                Some(trf("Doesn't fit {build}", &[("build", build_label)]))
            }
            CardFit::NotCheckedYet { build_label } => Some(trf(
                "Not checked for {build} yet",
                &[("build", build_label)],
            )),
            CardFit::DoesNotFitHere { build_label } => Some(trf(
                "Doesn't fit {build} (checked on this computer)",
                &[("build", build_label)],
            )),
            CardFit::Checking { build_label } => {
                Some(trf("Checking fit for {build}…", &[("build", build_label)]))
            }
            CardFit::Revoked(why) => Some(trf("Taken down: {reason}", &[("reason", why)])),
            CardFit::JarUnavailable(jars) => Some(jars_gone(jars)),
            CardFit::DependencyUnavailable { build_label, mods } => {
                Some(dependencies_gone(mods, build_label))
            }
            CardFit::NoBuild => Some(tr("No game build selected").to_owned()),
        }
    }
}

/// One line per jar Modrinth no longer serves.
fn jars_gone(jars: &[JarRef]) -> String {
    jars.iter()
        .map(|jar| {
            trf(
                "The original file {id} {version} is no longer on Modrinth.",
                &[("id", jar.id()), ("version", jar.version())],
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One line per required mod that has no release for the build.
fn dependencies_gone(mods: &[String], build_label: &str) -> String {
    mods.iter()
        .map(|mod_id| {
            trf(
                "Needs {mod}, which has no release for Sunrise {build}.",
                &[("mod", mod_id), ("build", build_label)],
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The required mods the index lists as having no release for `build`, for `release`.
pub(super) fn unavailable_dependencies(release: &Release, build: &str) -> Vec<String> {
    release
        .dependency_unavailable
        .iter()
        .filter(|entry| entry.build == build)
        .map(|entry| entry.mod_id.clone())
        .collect()
}

/// Compatibility verdict of one individual release for the selected profile's exact build.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mods) enum ReleaseVerdict {
    Fits,
    DoesNotFit(String),
    NotCheckedYet(String),
    /// Fits by this computer's check; the index says nothing of the build.
    FitsHere,
    DoesNotFitHere(String),
    Checking(String),
    Revoked(String),
    /// Fits, but lists original jars Modrinth no longer serves that this computer has not cached.
    JarUnavailable(Vec<JarRef>),
    /// Fits, but needs mods that have no release for the build.
    DependencyUnavailable {
        build_label: String,
        mods: Vec<String>,
    },
    NoBuild,
}

/// This computer's fit checks for one build: what was found, and what is being checked now.
#[derive(Clone, Debug, Default)]
pub(in crate::mods) struct LocalFits {
    pub(super) found: local_fit::Found,
    /// `.smod` SHA-256s (lowercase) a running check will answer.
    pub(super) checking: BTreeSet<String>,
    /// The original-jar cache; `None` holds no jar.
    pub(super) jar_cache: Option<PathBuf>,
}

impl LocalFits {
    /// What this computer found for `build`, listed in the index or not, with every unanswered
    /// gap marked as being checked when `running`, and the original jars the profile at `home`
    /// holds.
    pub(in crate::mods) fn read(
        index: &Index,
        build: Option<&str>,
        running: bool,
        home: &Path,
    ) -> Self {
        let Some(build) = build else {
            return Self::default();
        };
        let found = local_fit::commit_of(index, build)
            .map(|commit| local_fit::kept(&commit))
            .unwrap_or_default();
        let checking = if running {
            local_fit::gaps(index, build)
                .into_iter()
                .map(|r| r.file.sha256.to_ascii_lowercase())
                .filter(|sha256| !found.contains_key(sha256))
                .collect()
        } else {
            BTreeSet::new()
        };
        let jar_cache = Some(home.join("jars"));
        Self {
            found,
            checking,
            jar_cache,
        }
    }

    /// The jars `release` lists that Modrinth no longer serves and the cache doesn't hold. A
    /// cached jar still works, so it is not named.
    fn unavailable_jars(&self, release: &Release) -> Vec<JarRef> {
        release
            .jar_unavailable
            .iter()
            .filter(|jar| {
                !self
                    .jar_cache
                    .as_ref()
                    .is_some_and(|cache| jar.cache_path(cache).is_file())
            })
            .cloned()
            .collect()
    }

    /// The verdict for a release the index says nothing of on `build`.
    fn verdict(&self, release: &Release, label: String) -> ReleaseVerdict {
        let sha256 = release.file.sha256.to_ascii_lowercase();
        match self.found.get(&sha256) {
            Some(local_fit::Local::Fits) => ReleaseVerdict::FitsHere,
            Some(local_fit::Local::DoesNotFit) => ReleaseVerdict::DoesNotFitHere(label),
            Some(local_fit::Local::Unknown) => ReleaseVerdict::NotCheckedYet(label),
            None if self.checking.contains(&sha256) => ReleaseVerdict::Checking(label),
            None => ReleaseVerdict::NotCheckedYet(label),
        }
    }
}

impl ReleaseVerdict {
    pub(in crate::mods) fn fits(&self) -> bool {
        matches!(self, ReleaseVerdict::Fits | ReleaseVerdict::FitsHere)
    }

    /// What a fitting release's row says of its fit.
    pub(in crate::mods) fn fit_label(&self) -> &'static str {
        match self {
            ReleaseVerdict::FitsHere => tr("Fits this build (checked on this computer)"),
            _ => tr("Fits this build"),
        }
    }

    pub(in crate::mods) fn reason(&self) -> Option<String> {
        match self {
            ReleaseVerdict::Fits | ReleaseVerdict::FitsHere => None,
            ReleaseVerdict::DoesNotFitHere(build) => Some(trf(
                "Doesn't fit {build} (checked on this computer)",
                &[("build", build)],
            )),
            ReleaseVerdict::Checking(build) => {
                Some(trf("Checking fit for {build}…", &[("build", build)]))
            }
            ReleaseVerdict::DoesNotFit(build) => {
                Some(trf("Doesn't fit {build}", &[("build", build)]))
            }
            ReleaseVerdict::NotCheckedYet(build) => {
                Some(trf("Not checked for {build} yet", &[("build", build)]))
            }
            ReleaseVerdict::Revoked(why) => Some(trf("Taken down: {reason}", &[("reason", why)])),
            ReleaseVerdict::JarUnavailable(jars) => Some(jars_gone(jars)),
            ReleaseVerdict::DependencyUnavailable { build_label, mods } => {
                Some(dependencies_gone(mods, build_label))
            }
            ReleaseVerdict::NoBuild => Some(tr("No game build selected").to_owned()),
        }
    }
}

pub(super) fn compare_releases_newest_first(a: &Release, b: &Release) -> Ordering {
    let va = semver::Version::parse(&a.version).ok();
    let vb = semver::Version::parse(&b.version).ok();
    vb.cmp(&va)
        .then_with(|| b.published.cmp(&a.published))
        .then_with(|| b.version.cmp(&a.version))
}

pub(super) fn release_verdict(
    index: &Index,
    project: &Project,
    release: &Release,
    exact_build: Option<&str>,
    build_label: &dyn Fn(&str) -> String,
    local: &LocalFits,
) -> ReleaseVerdict {
    if let Some(revoked) = index.revoked(&project.id, Some(&release.file.sha256)) {
        return ReleaseVerdict::Revoked(revoked.reason.clone());
    }
    let Some(build) = exact_build else {
        return ReleaseVerdict::NoBuild;
    };
    let verdict = match release.fit_verdict(build) {
        FitVerdict::Fits => ReleaseVerdict::Fits,
        FitVerdict::DoesNotFit => ReleaseVerdict::DoesNotFit(build_label(build)),
        FitVerdict::Unknown => local.verdict(release, build_label(build)),
    };
    let gone = local.unavailable_jars(release);
    if verdict.fits() && !gone.is_empty() {
        return ReleaseVerdict::JarUnavailable(gone);
    }
    let missing = unavailable_dependencies(release, build);
    if verdict.fits() && !missing.is_empty() {
        return ReleaseVerdict::DependencyUnavailable {
            build_label: build_label(build),
            mods: missing,
        };
    }
    verdict
}

pub(super) fn project_fit(
    index: &Index,
    project: &Project,
    sorted_releases: &[&Release],
    exact_build: Option<&str>,
    build_label: &dyn Fn(&str) -> String,
    local: &LocalFits,
) -> (CardFit, String) {
    if let Some(revoked) = index.revoked(&project.id, None) {
        let updated = sorted_releases
            .first()
            .map(|r| r.published.clone())
            .unwrap_or_default();
        return (CardFit::Revoked(revoked.reason.clone()), updated);
    }

    let mut active: Vec<&Release> = Vec::new();
    let mut first_revoked: Option<&str> = None;
    for release in sorted_releases {
        if let Some(revoked) = index.revoked(&project.id, Some(&release.file.sha256)) {
            first_revoked.get_or_insert(revoked.reason.as_str());
        } else {
            active.push(*release);
        }
    }

    if active.is_empty() {
        let reason = first_revoked.unwrap_or("taken down").to_owned();
        let updated = sorted_releases
            .first()
            .map(|r| r.published.clone())
            .unwrap_or_default();
        return (CardFit::Revoked(reason), updated);
    }

    let newest_active = active[0];
    let Some(build) = exact_build else {
        return (CardFit::NoBuild, newest_active.published.clone());
    };

    if let Some(fitting) = active.iter().copied().find(|r| {
        r.fit_verdict(build) == FitVerdict::Fits
            && local.unavailable_jars(r).is_empty()
            && unavailable_dependencies(r, build).is_empty()
    }) {
        return (
            CardFit::Fits {
                version: fitting.version.clone(),
            },
            fitting.published.clone(),
        );
    }

    if let Some(fitting) = active
        .iter()
        .copied()
        .find(|r| release_verdict(index, project, r, exact_build, build_label, local).fits())
    {
        return (
            CardFit::FitsHere {
                version: fitting.version.clone(),
            },
            fitting.published.clone(),
        );
    }

    let fit = match release_verdict(
        index,
        project,
        newest_active,
        exact_build,
        build_label,
        local,
    ) {
        ReleaseVerdict::DoesNotFit(build_label) => CardFit::DoesNotFit { build_label },
        ReleaseVerdict::DoesNotFitHere(build_label) => CardFit::DoesNotFitHere { build_label },
        ReleaseVerdict::Checking(build_label) => CardFit::Checking { build_label },
        ReleaseVerdict::JarUnavailable(jars) => CardFit::JarUnavailable(jars),
        ReleaseVerdict::DependencyUnavailable { build_label, mods } => {
            CardFit::DependencyUnavailable { build_label, mods }
        }
        _ => CardFit::NotCheckedYet {
            build_label: build_label(build),
        },
    };
    (fit, newest_active.published.clone())
}
