//! GitHub supplies checked native bundles instead of a registry plan. Publish them, their
//! provenance and the enabled list through the same profile transaction as registry installs.
use super::{fs, index, installed, mod_list, publish, stage_enabled, SourceId};
use crate::github_mods::{self, Prepared};
use crate::i18n::{tr, trf};
use crate::native_mods::{self, INSTALLED};
use std::io;
use std::path::{Path, PathBuf};

impl super::super::txn::Lease {
    pub fn install_github(
        &self,
        prepared: &Prepared,
        approvals: &[installed::Approval],
        progress: &dyn Fn(&str, f32),
    ) -> Result<Vec<String>, installed::InstallError> {
        let native_allowed = native_mods::require_consent().is_ok();
        install(self, prepared, approvals, native_allowed, progress)
    }
}

fn install(
    lease: &super::super::txn::Lease,
    prepared: &Prepared,
    approvals: &[installed::Approval],
    native_allowed: bool,
    progress: &dyn Fn(&str, f32),
) -> Result<Vec<String>, installed::InstallError> {
    let Some(first) = prepared.mods().first() else {
        return Ok(Vec::new());
    };
    if !native_allowed {
        return Err(installed::InstallError::NeedsNativeConsent {
            native: first.name.clone(),
        });
    }
    let home = lease.home();
    let _lock = crate::browser::lock_home(home).map_err(|e| e.to_string())?;
    super::super::txn::settle_owned(home)?;
    if super::super::txn::has_build(home)? {
        return Err(super::super::txn::unsettled().into());
    }
    let registry = installed::Installed::load(home);
    let local = crate::native_status::untracked(home);
    let staged = tempfile::Builder::new()
        .prefix(".installed-mods-")
        .tempdir_in(home)
        .map_err(|e| format!("{}: {e}", home.display()))?;
    let mut changes = Vec::new();
    let mut drops = Vec::new();
    let mut enable = Vec::new();
    let mut ids = Vec::new();
    for (n, mod_) in prepared.mods().iter().enumerate() {
        progress(&mod_.name, n as f32 / prepared.mods().len() as f32);
        let id = &mod_.id;
        let path = package_path(home, id);
        let sidecar = github_mods::origin_path(home, id);
        let origin = mod_.origin(prepared.source());
        let collision = || {
            trf("A mod with the id {mod} is already installed from another source. Remove it first.", &[("mod", id)])
        };
        if registry
            .entries
            .iter()
            .any(|e| e.source == SourceId::Sunrise && super::super::mod_id(e) == Some(id))
            || local
                .iter()
                .any(|l| l.id == *id && home.join(&l.file) != path)
            || home.join(mod_list::MODS_DIR).join(id).exists()
            || home
                .join(mod_list::MODS_DIR)
                .join(format!("{id}.smod"))
                .exists()
        {
            return Err(collision().into());
        }
        let replacing = match path.symlink_metadata() {
            Ok(meta) if meta.is_file() => {
                let old = github_mods::origin(home, id)
                    .filter(|old| {
                        old.id == *id
                            && old.owner.eq_ignore_ascii_case(&origin.owner)
                            && old.repo.eq_ignore_ascii_case(&origin.repo)
                    })
                    .ok_or_else(collision)?;
                if index::sha256_file(&path)? != old.sha256 {
                    return Err(collision().into());
                }
                true
            }
            Ok(_) => return Err(collision().into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(format!("{}: {e}", path.display()).into()),
        };
        let stage = staged.path().join(format!("{n}.smod"));
        fs::copy(&mod_.file, &stage).map_err(|e| format!("{}: {e}", stage.display()))?;
        if index::sha256_file(&stage)? != origin.sha256 {
            return Err(trf(
                "The downloaded package for {mod} changed. Fetch it again.",
                &[("mod", id)],
            )
            .into());
        }
        if replacing
            && !approvals.iter().any(|a| matches!(a, installed::Approval::DropContent { id: approved } if approved == id))
            && !super::super::auto::dropped(&path, &stage)?.is_empty()
        {
            drops.push(id.clone());
        }
        // Keep the player's off switch when the link updates a mod already in this profile.
        if !replacing {
            enable.push(id.clone());
        }
        let stage_origin = staged.path().join(format!("{n}.origin.json"));
        let json = serde_json::to_vec(&origin).map_err(|e| e.to_string())?;
        fs::write(&stage_origin, json).map_err(|e| format!("{}: {e}", stage_origin.display()))?;
        changes.extend([(Some(stage), path), (Some(stage_origin), sidecar)]);
        ids.push(id.clone());
    }
    if !drops.is_empty() {
        if let Some(question) = crate::worlds::question(home, &drops)? {
            return Err(installed::InstallError::NeedsWorlds {
                mods: question.mods,
            });
        }
    }
    if !enable.is_empty() {
        changes.push(stage_enabled(home, staged.path(), &enable, true)?);
    }
    progress(tr("Installing…"), 1.);
    let (staged, attempt) = publish(staged, changes, approvals)?;
    super::super::txn::commit(home, &attempt, staged, true)?;
    Ok(ids)
}

fn package_path(home: &Path, id: &str) -> PathBuf {
    home.join(INSTALLED).join(format!("{id}.smod"))
}

/// Only remove the exact GitHub file the player reviewed, never a registry or hand-copied mod.
pub(super) fn removal_paths(home: &Path, id: &str, checksum: &str) -> Option<[PathBuf; 2]> {
    if !index::valid_id(id) {
        return None;
    }
    let hash = checksum.strip_prefix("SHA-256:")?;
    let origin = github_mods::origin(home, id)?;
    let path = package_path(home, id);
    (origin.id == id
        && origin.sha256 == hash
        && path.symlink_metadata().is_ok_and(|m| m.is_file())
        && index::sha256_file(&path).is_ok_and(|sum| sum == hash))
    .then(|| [path, github_mods::origin_path(home, id)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::sunrise::txn::{recover, Lease};
    use crate::github_mods::tests::prepared_release;

    #[test]
    fn consent_and_collision_leave_the_profile_untouched() {
        let home = tempfile::tempdir().unwrap();
        let lease = Lease::admit(home.path()).unwrap();
        let prepared = prepared_release(&["keys", "zoom"], false);
        let result = install(&lease, &prepared, &[], false, &|_, _| {});
        assert!(matches!(
            result,
            Err(installed::InstallError::NeedsNativeConsent { .. })
        ));
        assert!(!package_path(home.path(), "keys").exists());
        assert!(!lease.outstanding());
        let taken = package_path(home.path(), "zoom");
        fs::create_dir_all(taken.parent().unwrap()).unwrap();
        fs::write(&taken, b"manual").unwrap();
        let refused = install(&lease, &prepared, &[], true, &|_, _| {}).unwrap_err();
        assert!(refused.to_string().contains("another source"));
        assert_eq!(fs::read(taken).unwrap(), b"manual");
        assert!(!package_path(home.path(), "keys").exists());
        assert!(!lease.outstanding());
    }

    #[test]
    fn a_build_failure_recovers_before_continue_and_removes_only_the_named_package_after_it() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let prepared = prepared_release(&["keys", "zoom"], false);
        let run = |lease: &Lease| {
            assert_eq!(
                install(lease, &prepared, &[], true, &|_, _| {}).unwrap(),
                ["keys", "zoom"]
            );
            assert!(mod_list::is_enabled(home, "keys"));
            assert!(github_mods::origin(home, "keys").is_some());
            lease.mark_build("v1").unwrap()
        };
        let lease = Lease::admit(home).unwrap();
        run(&lease);
        drop(lease);
        assert!(recover(home));
        assert!(github_mods::origins(home).is_empty());
        assert!(!package_path(home, "keys").exists());
        assert!(!mod_list::is_enabled(home, "zoom"));

        let lease = Lease::admit(home).unwrap();
        let sealed = run(&lease);
        let wanted = |sum: &str| [("keys".to_string(), format!("SHA-256:{sum}"))];
        let keys = github_mods::origin(home, "keys").unwrap();
        assert!(lease
            .remove_after_build(&wanted("not-the-hash"), &sealed)
            .is_err());
        assert!(lease
            .remove_after_build(&wanted(&keys.sha256), &["not-the-seal".into()])
            .is_err());
        assert!(package_path(home, "keys").is_file());
        assert_eq!(
            lease
                .remove_after_build(&wanted(&keys.sha256), &sealed)
                .unwrap(),
            ["keys"]
        );
        assert!(!lease.outstanding());
        drop(lease);
        assert!(recover(home));
        assert!(!mod_list::is_enabled(home, "keys"));
        assert!(!package_path(home, "keys").exists());
        assert!(github_mods::origin(home, "keys").is_none());
        assert!(github_mods::origin(home, "zoom").is_some());
        assert!(package_path(home, "zoom").is_file());
        assert!(mod_list::is_enabled(home, "zoom"));
    }

    #[test]
    fn dropping_content_from_a_world_needs_the_players_answer() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let lease = Lease::admit(home).unwrap();
        let first = prepared_release(&["keys"], true);
        install(&lease, &first, &[], true, &|_, _| {}).unwrap();
        lease.done(&lease.mark_build("v1").unwrap()).unwrap();
        let prior = github_mods::origin(home, "keys").unwrap();
        let save = home.join("saves/Home");
        fs::create_dir_all(&save).unwrap();
        // An unreadable save is protected too: we cannot prove it doesn't use keys.
        fs::write(save.join("level.dat"), b"not nbt").unwrap();
        let next = prepared_release(&["keys"], false);
        let refused = install(&lease, &next, &[], true, &|_, _| {}).unwrap_err();
        assert!(
            matches!(refused, installed::InstallError::NeedsWorlds { mods } if mods == ["keys"])
        );
        assert_eq!(github_mods::origin(home, "keys"), Some(prior.clone()));
        assert!(!lease.outstanding());
        install(
            &lease,
            &next,
            &[installed::Approval::DropContent { id: "keys".into() }],
            true,
            &|_, _| {},
        )
        .unwrap();
        assert_ne!(github_mods::origin(home, "keys"), Some(prior.clone()));
        lease.undo(&lease.mark_build("v1").unwrap()).unwrap();
        assert_eq!(github_mods::origin(home, "keys"), Some(prior));
        assert_eq!(fs::read(save.join("level.dat")).unwrap(), b"not nbt");
    }

    #[test]
    fn removal_checks_the_current_file_not_only_its_origin() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let prepared = prepared_release(&["keys"], false);
        let lease = Lease::admit(home).unwrap();
        install(&lease, &prepared, &[], true, &|_, _| {}).unwrap();
        lease.done(&lease.mark_build("v1").unwrap()).unwrap();
        drop(lease);
        let sum = github_mods::origin(home, "keys").unwrap().sha256;
        let wanted = [("keys".to_string(), format!("SHA-256:{sum}"))];
        fs::write(package_path(home, "keys"), b"changed").unwrap();
        assert!(super::super::remove_checked(home, &wanted)
            .unwrap()
            .is_empty());
        assert_eq!(fs::read(package_path(home, "keys")).unwrap(), b"changed");
        assert!(github_mods::origin(home, "keys").is_some());
    }
}
