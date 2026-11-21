//! Two independently published batches, with one review of the exact proposed world changes.
use super::*;
use crate::worlds::WorldRisk;

type JointReview<'a> = dyn Fn(&ChangeImpact, Option<&ChangeImpact>) -> bool + 'a;

pub struct CombinedUpdate {
    pub registry: Result<InstalledBatch, InstallError>,
    pub sources: Result<(), SourceError>,
}

/// Run registry work first. Only its verified publication can advance the source batch's input
/// evidence. Neither a later state read nor accepting a review authorizes a different generation.
#[allow(clippy::too_many_arguments)]
pub fn update_registry_and_sources(
    instance: &Instance,
    identity: Identity,
    requested: Vec<String>,
    prepared: &mut [PreparedSource],
    choose: impl Fn() -> Result<FirstBuild, String>,
    registry_hooks: InstallHooks<'_>,
    source_hooks: InstallHooks<'_>,
    cancellation: &native_mods::Cancellation,
    authorize_source: &dyn Fn(&[PreparedSource]) -> Result<bool, String>,
    review: &JointReview<'_>,
    start_sources: impl FnOnce(),
) -> CombinedUpdate {
    let mut mixed = MixedSources {
        prepared,
        review,
        checked: None,
        denied: false,
        published: None,
    };
    let registry = registry_edit(
        instance,
        identity,
        || Action::Update(UpdateRequest::Bulk(requested), &|_| false),
        &choose,
        registry_hooks,
        super::super::super::fetch_with_official,
        Some(&mut mixed),
    );
    if mixed.denied {
        return CombinedUpdate {
            registry,
            sources: Err(SourceError::Cancelled),
        };
    }
    start_sources();
    let sources = (|| {
        let reviewed = match mixed.checked {
            Some(Ok(impacts)) => Some(impacts),
            Some(Err(error)) => return Err(error),
            None => None, // Registry declined or failed before either batch was reviewed.
        };
        if let Some((active, inputs)) = mixed.published {
            for source in &mut *mixed.prepared {
                source.before = inputs;
                source.active = Some(active.clone());
            }
        }
        let changed = std::cell::Cell::new(false);
        let result = apply_sources(
            instance,
            identity,
            mixed.prepared,
            choose,
            SourceHooks {
                install: source_hooks,
                cancellation,
                authorize: authorize_source,
                approve_worlds: &|impact| {
                    if let Some(checked) = &reviewed {
                        let same = impact.matches_world_review(checked);
                        changed.set(!same);
                        same
                    } else {
                        !impact.risks_worlds() || review(impact, None)
                    }
                },
            },
        );
        if changed.get() {
            Err(changed_source_target())
        } else {
            result
        }
    })();
    CombinedUpdate { registry, sources }
}

pub(in crate::browser::sunrise::registry_install) struct MixedSources<'a> {
    prepared: &'a mut [PreparedSource],
    review: &'a JointReview<'a>,
    checked: Option<Result<Vec<WorldRisk>, SourceError>>,
    denied: bool,
    published: Option<(managed_play::Active, [u8; 32])>,
}

impl MixedSources<'_> {
    pub(in crate::browser::sunrise::registry_install) fn combine(
        &mut self,
        impact: &ChangeImpact,
        instance: &Instance,
        identity: Identity,
        edit: &managed_play::Edit<'_>,
        original: &Path,
    ) -> bool {
        let source_impact = (|| {
            let before = managed_play::inputs(original, None)?;
            if self.prepared.iter().any(|p| {
                p.identity != identity
                    || p.before != before
                    || p.base != edit.base()
                    || p.active.as_ref() != edit.current()
                    || !p.is_update()
            }) {
                return Err(changed_source_target());
            }
            let entries = self
                .prepared
                .iter()
                .map(PreparedSource::entry)
                .collect::<Vec<_>>();
            // Registry publication is not authority for the source predecessor. Verify both that
            // it was reviewed on this generation and that this separate trial keeps exact bytes.
            let origins = github::predecessors(original, &entries)?;
            let staged = edit.candidate()?;
            github::stage_entries(
                staged.path(),
                &entries,
                &origins,
                edit.current(),
                edit.base(),
            )?;
            Ok(
                github::source_review(&instance.home, original, staged.path(), &origins, &entries)?
                    .0,
            )
        })();
        let allowed = (!impact.risks_worlds()
            && !source_impact.as_ref().is_ok_and(ChangeImpact::risks_worlds))
            || (self.review)(impact, source_impact.as_ref().ok());
        self.checked = Some(source_impact.map(ChangeImpact::into_evidence));
        self.denied = !allowed;
        allowed
    }

    pub(in crate::browser::sunrise::registry_install) fn published(
        &mut self,
        active: managed_play::Active,
        inputs: [u8; 32],
    ) {
        self.published = Some((active, inputs));
    }
}
