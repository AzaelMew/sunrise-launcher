//! One deferred native-code action. Declining never invokes its installer or builder.
use std::path::Path;
use sunrise_launcher::{
    backend::Settings,
    browser::{installed::Entry, mod_list, Project, SourceId},
    github_mods::Resolved,
};

pub(super) enum PendingConsent {
    Build(Vec<String>),
    Toggle(Entry),
    Install(SourceId, String, Option<Project>),
    Github(Resolved, Option<String>),
}

impl PendingConsent {
    pub(super) fn name(&self) -> String {
        match self {
            Self::Build(ids) => ids.join(", "),
            Self::Toggle(entry) => entry.title.clone(),
            Self::Install(_, id, _) => id.clone(),
            Self::Github(found, _) => found.label(),
        }
    }
}

/// Consume exactly one request. Permission is persisted before returning an action to execute.
/// Cancel and Escape both use `allow = false`, and cannot return an executable action.
pub(super) fn resolve(
    pending: &mut Option<PendingConsent>,
    allow: bool,
    supported: bool,
    settings: &mut Settings,
    home: &Path,
) -> Result<Option<PendingConsent>, String> {
    if pending.is_none() {
        return Ok(None);
    }
    if allow {
        if !supported {
            return Err(crate::i18n::tr("Unavailable on this platform").into());
        }
        settings.allow_native_mods()?;
        return Ok(pending.take());
    }
    if let Some(action) = pending.take() {
        match action {
            PendingConsent::Build(ids) => {
                // One rewrite: a failure can never leave some declined mods on.
                let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
                mod_list::set_all_enabled(home, &ids, false)?;
            }
            PendingConsent::Toggle(entry) => {
                if let Some(id) = sunrise_launcher::browser::sunrise::mod_id(&entry) {
                    mod_list::set_enabled(home, id, false)?;
                }
            }
            PendingConsent::Install(..) | PendingConsent::Github(..) => {}
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancel_escape_and_allow_consume_only_the_pending_toggle() {
        // Settings is process-global: exercise this transition in its own isolated process.
        if std::env::var_os("SUNRISE_CONSENT_TEST_CHILD").is_none() {
            let home = tempfile::tempdir().unwrap();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "native_consent::tests::cancel_escape_and_allow_consume_only_the_pending_toggle"])
                .env("SUNRISE_CONSENT_TEST_CHILD", "1")
                .env("SUNRISE_LAUNCHER_HOME", home.path())
                .env("SUNRISE_HOME", home.path())
                .status().unwrap();
            assert!(status.success());
            return;
        }
        let home = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        // Decline returns no dispatchable action: no installer or builder can run.
        // The build layer separately tests its HTTP backstop with a mock proxy.
        for _ in ["Cancel", "Escape"] {
            let mut pending = Some(PendingConsent::Build(vec!["sample".into()]));
            assert!(
                resolve(&mut pending, false, true, &mut settings, home.path())
                    .unwrap()
                    .is_none()
            );
            assert!(pending.is_none());
            assert!(!mod_list::is_enabled(home.path(), "sample"));
            assert!(settings.native_mods_allowed_at.is_none());
        }
        mod_list::set_enabled(home.path(), "sample", true).unwrap();
        mod_list::set_enabled(home.path(), "second", true).unwrap();
        mod_list::set_enabled(home.path(), "unrelated", true).unwrap();
        let mut pending = Some(PendingConsent::Build(vec![
            "sample".into(),
            "second".into(),
        ]));
        resolve(&mut pending, false, true, &mut settings, home.path()).unwrap();
        assert!(
            !mod_list::is_enabled(home.path(), "sample")
                && !mod_list::is_enabled(home.path(), "second"),
            "NoConsent cancellation turns every one of the build's mods off"
        );
        assert!(mod_list::is_enabled(home.path(), "unrelated"));
        let mut unsupported = Some(PendingConsent::Build(vec!["sample".into()]));
        assert!(resolve(&mut unsupported, true, false, &mut settings, home.path()).is_err());
        assert!(unsupported.is_some());
        assert!(Settings::load().native_mods_allowed_at.is_none());
        if sunrise_launcher::rust_toolchain::host().is_some() {
            let mut pending = Some(PendingConsent::Build(vec!["sample".into()]));
            let action = resolve(&mut pending, true, true, &mut settings, home.path())
                .unwrap()
                .unwrap();
            assert!(Settings::load().native_mods_allowed_at.is_some());
            match action {
                PendingConsent::Build(ids) => {
                    mod_list::set_enabled(home.path(), &ids[0], true).unwrap()
                }
                _ => panic!("wrong pending action"),
            }
            assert!(mod_list::is_enabled(home.path(), "sample"));
            assert!(
                resolve(&mut pending, true, true, &mut settings, home.path())
                    .unwrap()
                    .is_none(),
                "the action cannot execute twice"
            );
        }
    }
}
