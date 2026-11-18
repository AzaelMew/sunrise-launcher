//! One deferred native-code action. Declining never invokes its installer or builder.
use std::path::Path;
use sunrise_launcher::{
    backend::Settings,
    browser::{installed::Entry, mod_list, Project, SourceId},
    github_mods::Resolved,
    native_status::BuildInput,
};

pub(super) enum PendingConsent {
    /// A build of these packages, named when the question opened from the files it takes in.
    Build(Vec<BuildInput>),
    Toggle(Entry),
    Install(SourceId, String, Option<Project>),
    Github(Resolved, Option<String>),
}

impl PendingConsent {
    pub(super) fn name(&self) -> String {
        match self {
            Self::Build(inputs) => inputs
                .iter()
                .map(build_label)
                .collect::<Vec<_>>()
                .join(", "),
            Self::Toggle(entry) => entry.title.clone(),
            // The mod as its page names it ("JEI"), not its id ("jei").
            Self::Install(_, id, project) => project
                .as_ref()
                .map(|p| p.title.clone())
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| id.clone()),
            Self::Github(found, _) => found.label(),
        }
    }
}

/// A build's package as the question names it: the registry's name only for a file the registry
/// lists (`native_status::build_inputs`), else its id and file, so a package put in the folder
/// under a registry mod's id never reads as that mod.
fn build_label(input: &BuildInput) -> String {
    match &input.registry {
        Some(name) if !name.is_empty() => name.clone(),
        _ => crate::i18n::trf(
            "{id} ({file}, not from the registry)",
            &[("id", &input.id), ("file", &input.file)],
        ),
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
            PendingConsent::Build(inputs) => {
                // One rewrite: a failure can never leave some declined mods on.
                let ids: Vec<&str> = inputs.iter().map(|i| i.id.as_str()).collect();
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
    fn an_install_is_named_as_its_page_names_the_mod() {
        let project = |title: &str| Project {
            source: SourceId::Sunrise,
            id: "jei".into(),
            slug: "jei".into(),
            title: title.into(),
            summary: String::new(),
            author: String::new(),
            downloads: 0,
            page: String::new(),
            icon: None,
            license: String::new(),
            kind: sunrise_launcher::browser::Kind::Mod,
            game_versions: Vec::new(),
            loaders: Vec::new(),
            blocked: None,
        };
        let install = |p| PendingConsent::Install(SourceId::Sunrise, "jei".into(), p);
        assert_eq!(install(Some(project("JEI"))).name(), "JEI");
        assert_eq!(install(Some(project(""))).name(), "jei");
        assert_eq!(install(None).name(), "jei");
    }

    #[test]
    fn a_build_names_a_package_as_the_registry_mod_only_when_its_file_is_the_registrys() {
        let input = |registry: Option<&str>| BuildInput {
            id: "jei".into(),
            file: "jei.smod".into(),
            registry: registry.map(Into::into),
        };
        let build = |i| PendingConsent::Build(vec![i]).name();
        assert_eq!(build(input(Some("JEI"))), "JEI");
        assert_eq!(build(input(None)), "jei (jei.smod, not from the registry)");
        assert_eq!(
            build(input(Some(""))),
            "jei (jei.smod, not from the registry)"
        );
    }

    #[test]
    fn cancel_escape_and_allow_consume_only_the_pending_toggle() {
        // Settings is process-global: exercise this transition in its own isolated process.
        let input = |id: &str| BuildInput {
            id: id.into(),
            file: format!("{id}.smod"),
            registry: None,
        };
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
            let mut pending = Some(PendingConsent::Build(vec![input("sample")]));
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
            input("sample"),
            input("second"),
        ]));
        resolve(&mut pending, false, true, &mut settings, home.path()).unwrap();
        assert!(
            !mod_list::is_enabled(home.path(), "sample")
                && !mod_list::is_enabled(home.path(), "second"),
            "NoConsent cancellation turns every one of the build's mods off"
        );
        assert!(mod_list::is_enabled(home.path(), "unrelated"));
        let mut unsupported = Some(PendingConsent::Build(vec![input("sample")]));
        assert!(resolve(&mut unsupported, true, false, &mut settings, home.path()).is_err());
        assert!(unsupported.is_some());
        assert!(Settings::load().native_mods_allowed_at.is_none());
        if sunrise_launcher::rust_toolchain::host().is_some() {
            let mut pending = Some(PendingConsent::Build(vec![input("sample")]));
            let action = resolve(&mut pending, true, true, &mut settings, home.path())
                .unwrap()
                .unwrap();
            assert!(Settings::load().native_mods_allowed_at.is_some());
            match action {
                PendingConsent::Build(inputs) => {
                    mod_list::set_enabled(home.path(), &inputs[0].id, true).unwrap()
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
