//! Adding a source's mods: its target, the queue, the question, the activity.
use super::*;

impl Launcher {
    pub(crate) fn source_target(&self) -> Result<SourceTarget, String> {
        self.github_target()?.source()
    }

    pub(crate) fn queue_sources(
        &mut self,
        target: SourceTarget,
        inputs: Vec<PreparedSource>,
    ) -> Result<(), String> {
        if inputs.is_empty() {
            return Ok(());
        }
        let current = self.source_target()?;
        if current.identity != target.identity
            || current.base != target.base
            || current.profile.home != target.profile.home
        {
            return Err(tr(
                "The profile or game build changed. Check the source again; nothing was changed.",
            )
            .into());
        }
        let ids = inputs
            .iter()
            .map(|source| source.origin().id.clone())
            .collect::<Vec<_>>();
        for id in &ids {
            self.mods
                .installs
                .source_outcomes
                .remove(&(target.identity, id.clone()));
        }
        self.mods.installs.submit(Request {
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
            local: None,
            sources: inputs,
            picks: None,
        });
        self.start_registry_queue();
        Ok(())
    }

    pub(crate) fn source_activity(&self) -> Option<SourceActivity<'_>> {
        let run = self.mods.installs.running.as_ref()?;
        if let Some(source) = &run.source {
            return Some(SourceActivity {
                ids: &source.ids,
                names: &source.names,
                progress: run.stage,
                stopping: source.cancellation.is_stopped(),
            });
        }
        let local = run.local.as_ref()?;
        Some(SourceActivity {
            ids: &run.requests[0].ids,
            names: &local.names,
            progress: run.stage,
            stopping: run.cancellation.is_stopped(),
        })
    }

    pub(crate) fn stop_source(&mut self) {
        let Some(cancellation) = self
            .mods
            .installs
            .running
            .as_ref()
            .filter(|run| run.source.is_some() || run.local.is_some())
            .map(|run| run.cancellation.clone())
        else {
            return;
        };
        cancellation.stop();
        if cancellation.is_stopped() {
            self.answer_source_question(Ok(false));
            if let Some((_, reply)) = self.mods.installs.world.take() {
                let _ = reply.send(false);
            }
        }
    }

    pub(crate) fn source_outcome(&self, id: &str) -> Option<&Result<(), SourceError>> {
        let home = &self.selected_instance()?.home;
        self.mods
            .installs
            .source_outcomes
            .get(&(Identity::of(home).ok()?, id.to_owned()))
    }

    pub(crate) fn source_question(&self) -> Option<&SourceQuestion> {
        self.mods
            .installs
            .source_question
            .as_ref()
            .map(|(question, _)| question)
    }

    pub(crate) fn answer_source_question(&mut self, allow: Result<bool, String>) {
        if let Some((_, reply)) = self.mods.installs.source_question.take() {
            let _ = reply.send(allow);
        }
    }

    pub(in crate::mods_page) fn receive_source_question(
        &mut self,
        question: SourceQuestion,
        reply: Sender<Result<bool, String>>,
    ) {
        if self
            .source_activity()
            .is_none_or(|activity| activity.stopping)
        {
            let _ = reply.send(Ok(false));
        } else if question.dependencies.is_empty() && self.source_already_allowed(&question) {
            let _ = reply.send(Ok(true));
        } else {
            self.reset_install_question_sheet();
            self.mods.installs.source_question = Some((question, reply));
        }
    }

    pub(crate) fn add_sources(&mut self, picks: SourcePicks) {
        let target = match self.source_target() {
            Ok(target) => target,
            Err(error) => {
                self.open_notice(error, None);
                return;
            }
        };
        let ids: Vec<String> = picks
            .listings()
            .iter()
            .map(|source| source.id().into())
            .collect();
        let Some(card) = ids.first().cloned() else {
            return;
        };
        for id in &ids {
            self.mods
                .installs
                .source_outcomes
                .remove(&(target.identity, id.clone()));
        }
        self.mods.installs.submit(Request {
            profile: target.profile,
            identity: target.identity,
            target: Some(target.base),
            release: None,
            displayed_names: BTreeMap::new(),
            view: RequestView {
                kind: Kind::Source,
                card,
                ids,
            },
            github: None,
            local: None,
            sources: Vec::new(),
            picks: Some(picks),
        });
        self.start_registry_queue();
    }
}
