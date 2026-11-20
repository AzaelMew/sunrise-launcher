//! Explicit registry mutations on an admitted build. Download and compilation happen in
//! the reserved private candidate; the approval object has no request it could run again.
use super::{authoritative_index, dependencies, installed, registry_candidates, txn, Install};
use crate::{backend, i18n::tr, worlds};
use std::sync::Arc;

pub struct RegistryInstall {
    ready: txn::ReadyInstall,
    evidence: String,
    entries: Vec<installed::Entry>,
}

impl RegistryInstall {
    pub fn entries(&self) -> &[installed::Entry] {
        &self.entries
    }

    pub fn question(&self) -> &worlds::SwitchQuestion {
        self.ready.question()
    }

    /// This never re-runs a build. A different catalogue or changed live input refuses at
    /// cutoff, even after the player agreed to the world question.
    pub fn commit(self, approved: bool) -> Result<Vec<installed::Entry>, String> {
        self.ready.commit(approved, || {
            let (_, current) = authoritative_index()?;
            if current != self.evidence {
                return Err(tr("The mod registry changed while the install was being prepared. Nothing was installed.").into());
            }
            Ok(())
        })?;
        Ok(self.entries)
    }
}

pub fn prepare_registry_install(
    owner: Arc<txn::Lease>,
    instance: &crate::instances::Instance,
    ids: &[String],
    approvals: &[installed::Approval],
    progress: &dyn Fn(&str, f32),
) -> Result<RegistryInstall, installed::InstallError> {
    if owner.home() != instance.home {
        return Err(txn::unsettled().into());
    }
    let (index, evidence) = authoritative_index()?;
    let waiting = txn::waiting_packages(owner.home())?;
    let plan = match owner.active_build()? {
        Some(active) => txn::StagedInstall::begin(owner, active)?,
        None => {
            let bound = owner.bind_bootstrap(|| txn::Target::for_profile(instance))?;
            owner.prepare_bound(&bound, |fraction| progress(bound.base(), fraction))?;
            txn::StagedInstall::first(owner, &bound)?
        }
    };
    let game = backend::game_of_build(plan.base()).ok_or_else(backend::no_version_installed)?;
    let asked = registry_candidates(&index, ids.iter().map(String::as_str), &game, &waiting)?;
    let entries = dependencies::stage(
        &Install {
            index: &index,
            local: false,
            home: plan.stage(),
            approvals,
            progress,
        },
        &asked,
        backend::Settings::native_grant()?.is_some(),
        &waiting,
    )?;
    let ready = plan.prepare(&[], instance.native_profile, |step| {
        progress(&step.label(), 0.99)
    })?;
    Ok(RegistryInstall {
        ready,
        evidence,
        entries,
    })
}
