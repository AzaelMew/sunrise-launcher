//! Presentation for a Nightly source request on the shared installer queue.
use super::*;
use crate::mods::build_run::{Run, State};
use sunrise_launcher::browser::sunrise::sources::{self, Source, SourceCommit};
use sunrise_launcher::native_mods::Step;

#[derive(Clone)]
pub(crate) enum SourcePicks {
    One(Box<SourceCommit>),
    Collection(Vec<Source>),
}

impl SourcePicks {
    pub(crate) fn listings(&self) -> &[Source] {
        match self {
            Self::One(pin) => std::slice::from_ref(pin.source()),
            Self::Collection(sources) => sources,
        }
    }

    pub(super) fn resolve(&self) -> Vec<Result<SourceCommit, sources::Failure>> {
        match self {
            Self::One(pin) => vec![Ok((**pin).clone())],
            Self::Collection(sources) => sources::resolve_batch(sources),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SourceBuild {
    pub(crate) run: Run,
    pub(crate) started: bool,
    pub(crate) stage: InstallProgress,
    pub(crate) stages: Vec<String>,
    pub(crate) output: Option<Arc<str>>,
    pub(crate) show_log: bool,
    target: SourceTarget,
    picks: SourcePicks,
}

impl SourceBuild {
    pub(super) fn progress(&mut self, stage: InstallProgress) {
        let label = match stage {
            InstallProgress::Rust(_) => tr("Downloading build tools").into(),
            _ => stage_label(stage),
        };
        if !matches!(stage, InstallProgress::Building(_)) || self.run.progress.is_none() {
            if std::mem::discriminant(&self.stage) != std::mem::discriminant(&stage) {
                self.stages.push(label.clone());
            }
            self.run.step = label;
        }
        self.stage = stage;
    }

    pub(super) fn step(&mut self, step: &Step, output: Option<Arc<str>>) {
        self.run.step(step);
        match step {
            Step::Mod { .. } => self.stages.push(self.run.step.clone()),
            Step::Install => {
                for line in &mut self.run.lines {
                    if line.state == State::Building {
                        line.state = State::Built;
                    }
                }
                self.stages.push(self.run.step.clone());
            }
            _ => (),
        }
        if output.is_some() {
            self.output = output;
        }
    }

    pub(super) fn fail_line(&mut self, id: &str, error: &SourceError) {
        if let Some(line) = self.run.lines.iter_mut().find(|line| line.id == id) {
            line.state = State::Failed(error.to_string());
        }
    }

    pub(super) fn finish(&mut self, result: &Result<(), SourceError>) {
        match result {
            Ok(()) => self.run.done(),
            Err(SourceError::Cancelled) => self.run.cancelled(),
            Err(error) => {
                let failed = match error {
                    SourceError::Build { failed, log, .. } => {
                        if log.is_some() {
                            self.output = log.clone();
                        }
                        failed.as_slice()
                    }
                    _ => &[],
                };
                if failed.is_empty() && !matches!(error, SourceError::Build { .. }) {
                    let ids: Vec<_> = self.run.lines.iter().map(|line| line.id.clone()).collect();
                    self.run.stopped(error.to_string(), &ids);
                } else {
                    self.run.stopped(error.to_string(), failed);
                }
            }
        }
        self.show_log |= result
            .as_ref()
            .is_err_and(|error| !matches!(error, SourceError::Cancelled))
            || self.run.counts().failed > 0;
        self.run.said_at = Some(Instant::now());
    }
}

impl Launcher {
    pub(crate) fn queue_source_build(&mut self, picks: SourcePicks) {
        match self.source_build_target() {
            Ok(target) => self.submit_source_build(target, picks),
            Err(error) => self.open_notice(error, None),
        }
    }

    fn submit_source_build(&mut self, target: SourceTarget, mut picks: SourcePicks) {
        let installs = &mut self.mods.installs;
        // Coalesce the same already-requested listing on the same profile/build. A collection
        // still queues any other picks together, not one request per row.
        let matching = |build: &SourceBuild, listing: &Source| {
            build.run.running()
                && build.target.same_destination(&target)
                && build.picks.listings().contains(listing)
        };
        let existing = installs.source_builds.iter().position(|build| {
            if let (SourcePicks::One(wanted), SourcePicks::One(current)) = (&picks, &build.picks) {
                if wanted.sha() != current.sha() {
                    return false;
                }
            }
            picks
                .listings()
                .iter()
                .any(|listing| matching(build, listing))
        });
        if let SourcePicks::Collection(listings) = &mut picks {
            listings.retain(|listing| {
                !installs
                    .source_builds
                    .iter()
                    .any(|build| matching(build, listing))
            });
        }
        if picks.listings().is_empty()
            || (matches!(picks, SourcePicks::One(_)) && existing.is_some())
        {
            if let Some(id) = existing {
                self.open_source_build(id);
            }
            return;
        }
        if !installs
            .source_builds
            .iter()
            .any(|build| build.run.running())
        {
            installs.source_builds.clear();
        }
        let id = installs.source_builds.len();
        let mut run = Run::start(
            target.profile.id.clone(),
            target.profile.name.clone(),
            crate::sheets::build_name(&target.base),
            picks
                .listings()
                .iter()
                .map(|source| {
                    (
                        source.id().into(),
                        source.name().into(),
                        String::new(),
                        None,
                    )
                })
                .collect(),
            false,
        );
        run.step = tr("Queued").into();
        let ids: Vec<String> = run.lines.iter().map(|line| line.id.clone()).collect();
        for mod_id in &ids {
            installs
                .source_outcomes
                .remove(&(target.identity, mod_id.clone()));
        }
        installs.source_builds.push(SourceBuild {
            run,
            started: false,
            stage: InstallProgress::Checking,
            stages: Vec::new(),
            output: None,
            show_log: false,
            target: target.clone(),
            picks: picks.clone(),
        });
        installs.waiting.push_back(Request {
            profile: target.profile,
            identity: target.identity,
            target: Some(target.base),
            release: None,
            displayed_names: BTreeMap::new(),
            view: RequestView {
                kind: Kind::Source,
                card: ids[0].clone(),
                ids,
            },
            github: None,
            sources: Vec::new(),
            mixed_registry: Vec::new(),
            source_build: Some((id, picks)),
        });
        self.start_registry_queue();
        // Show the active batch and the newly queued rows in the same window.
        let shown = self
            .mods
            .installs
            .running
            .as_ref()
            .and_then(|run| run.source_build)
            .unwrap_or(id);
        self.open_source_build(shown);
    }

    pub(crate) fn source_builds(&self) -> &[SourceBuild] {
        &self.mods.installs.source_builds
    }

    pub(crate) fn source_build_view(&self) -> Option<(usize, &SourceBuild)> {
        let selected = self.mods.installs.shown_source_build?;
        Some((selected, &self.source_builds()[selected]))
    }

    pub(crate) fn open_source_build(&mut self, id: usize) {
        self.mods.installs.shown_source_build = Some(id);
        self.open_sheet(crate::sheets::Sheet::SourceBuild);
    }

    pub(crate) fn source_build_for(&self, id: &str) -> Option<usize> {
        let selected = self.selected_instance()?;
        self.source_builds().iter().rposition(|job| {
            job.target.profile.id == selected.id
                && job.target.profile.home == selected.home
                && job.run.lines.iter().any(|line| line.id == id)
        })
    }

    pub(crate) fn active_source_build(&self) -> Option<usize> {
        self.mods
            .installs
            .running
            .as_ref()
            .and_then(|run| run.source_build)
    }

    pub(crate) fn recent_source_build(&self) -> Option<(usize, String)> {
        let (id, build) = self
            .source_builds()
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, build)| !build.run.running())
            .max_by_key(|(_, build)| build.run.said_at)?;
        build.run.said(Instant::now()).map(|said| (id, said))
    }

    pub(crate) fn toggle_source_build_log(&mut self, id: usize) {
        let build = &mut self.mods.installs.source_builds[id];
        build.show_log = !build.show_log;
    }

    pub(crate) fn cancel_source_build(&mut self, id: usize) {
        if self
            .mods
            .installs
            .running
            .as_ref()
            .is_some_and(|run| run.source_build == Some(id))
        {
            self.stop_source();
        } else {
            self.mods.installs.waiting.retain(|request| {
                request
                    .source_build
                    .as_ref()
                    .is_none_or(|(job, _)| *job != id)
            });
            self.mods.installs.source_builds[id].finish(&Err(SourceError::Cancelled));
        }
    }

    pub(crate) fn retry_source_build(&mut self, id: usize) {
        let build = &self.source_builds()[id];
        let target = build.target.clone();
        let picks = match &build.picks {
            SourcePicks::One(pin) => SourcePicks::One(pin.clone()),
            SourcePicks::Collection(listings) => SourcePicks::Collection(
                listings
                    .iter()
                    .filter(|listing| {
                        build
                            .run
                            .lines
                            .iter()
                            .any(|line| line.id == listing.id() && line.state != State::Built)
                    })
                    .cloned()
                    .collect(),
            ),
        };
        self.submit_source_build(target, picks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> (tempfile::TempDir, SourceBuild) {
        let root = tempfile::tempdir().unwrap();
        let profile = instances::selected();
        let picks = ["butterflies", "fireflies"].map(|id| {
            Source::for_origin(
                id,
                "Example/creatures",
                1,
                "sunrise-mod.toml",
                sources::Tracker::Branch("main".into()),
            )
            .unwrap()
        });
        let run = Run::start(
            "fixture".into(),
            "Fixture".into(),
            "Nightly".into(),
            picks
                .iter()
                .map(|source| (source.id().into(), source.name().into(), "1".into(), None))
                .collect(),
            false,
        );
        let target = SourceTarget {
            profile,
            identity: Identity::of(root.path()).unwrap(),
            base: "nightly".into(),
        };
        (
            root,
            SourceBuild {
                run,
                started: true,
                stage: InstallProgress::Checking,
                stages: Vec::new(),
                output: None,
                show_log: false,
                target,
                picks: SourcePicks::Collection(picks.to_vec()),
            },
        )
    }

    #[test]
    fn the_compiler_stage_names_the_current_row_and_failure_keeps_its_log() {
        let (_root, mut build) = job();
        build.step(
            &Step::Mod {
                id: "butterflies".into(),
                done: 1,
                total: 2,
            },
            None,
        );
        build.progress(InstallProgress::Building(Some(0.)));
        assert_eq!(build.run.lines[0].state, State::Building);
        assert_eq!(build.run.lines[1].state, State::Waiting);
        assert!(build.run.step.contains("butterflies"));
        build.step(
            &Step::Mod {
                id: "fireflies".into(),
                done: 2,
                total: 2,
            },
            None,
        );
        let log: Arc<str> = Arc::from("error: rejected compiler input");
        build.finish(&Err(SourceError::Build {
            detail: "compiler refused".into(),
            log: Some(log.clone()),
            failed: vec!["fireflies".into()],
        }));
        assert_eq!(
            build.run.lines[0].state,
            State::Waiting,
            "nothing was published"
        );
        assert_eq!(
            build.run.lines[1].state,
            State::Failed("compiler refused".into())
        );
        assert!(build.show_log);
        assert_eq!(build.output, Some(log));
    }

    #[test]
    fn a_rejected_package_does_not_become_built_when_the_other_pick_publishes() {
        let (_root, mut build) = job();
        build.fail_line(
            "butterflies",
            &SourceError::Github(sources::Failure::NoManifest),
        );
        build.finish(&Ok(()));
        assert!(matches!(build.run.lines[0].state, State::Failed(_)));
        assert_eq!(build.run.lines[1].state, State::Built);
        assert!(build.show_log);
    }

    #[test]
    fn a_failed_fit_check_marks_the_batch_and_opens_the_log() {
        let (_root, mut build) = job();
        build.finish(&Err(SourceError::DoesNotFit(
            "fit check refused this batch".into(),
        )));
        assert_eq!(build.run.counts().failed, 2);
        assert!(build.show_log);
    }
}
