//! Ordinary client Play keeps one admission on its worker through the actual spawn.
use super::{first::assert_requested, unsettled, FirstBuild, Profile};
use crate::{
    account::{self, Accounts, NoOwner},
    backend::{self, RuntimeGuard},
    browser::installed::GameInstallProblem,
    developer_offline, instances, native_mods,
    process::child::Child,
};
use std::process::Command;

pub enum Progress {
    Download(f32),
    Prepare,
    Native(native_mods::Step),
    SignIn,
    Line(String),
}

#[derive(Debug)]
pub enum StartError {
    Journal(GameInstallProblem),
    Account(NoOwner),
    Native(native_mods::Failed),
    Other(String),
}

impl From<String> for StartError {
    fn from(error: String) -> Self {
        Self::Other(error)
    }
}

impl From<StartError> for String {
    fn from(error: StartError) -> Self {
        match error {
            StartError::Journal(error) => error.details(),
            StartError::Account(error) => match error.keyring.as_deref() {
                Some(why) => why.to_owned(),
                None => format!("{error} {}", crate::i18n::tr(backend::SIGN_IN_HINT)),
            },
            StartError::Native(error) => error.into(),
            StartError::Other(error) => error,
        }
    }
}

pub struct Client<'a> {
    pub instance: &'a instances::Instance,
    pub accepted_account: Option<&'a str>,
    /// An explicit command-line account still requires ownership in a developer build.
    pub explicit_account: bool,
    pub requested_build: Option<&'a str>,
    pub piped: bool,
}

pub struct Started {
    pub child: Child,
    pub runtime: RuntimeGuard,
    pub executable: String,
    pub notice: Option<String>,
    pub registry_notices: Vec<String>,
    pub owner: bool,
}

impl Client<'_> {
    pub fn start(
        self,
        choose: impl FnOnce() -> Result<FirstBuild, String>,
        architecture: impl Fn(&str) -> Option<String>,
        arguments: impl FnOnce(&mut Command) -> Result<(), String>,
        prepare_jars: impl FnOnce(&mut Command, &crate::launch::Launch) -> Result<(), String>,
        native_consent: Option<&dyn Fn(&native_mods::Consent) -> bool>,
        progress: impl Fn(Progress),
    ) -> Result<Started, StartError> {
        // A fresh default profile may not have a game directory yet. Do not recreate
        // a named profile that disappeared while its Play awaited this worker.
        if self.instance.kind == instances::Kind::Pack {
            return Err(unsettled().into());
        }
        if self.accepted_account.is_none() && !developer_offline::requested() {
            return Err(StartError::Account(NoOwner::default()));
        }
        if self.instance.kind == instances::Kind::Default {
            std::fs::create_dir_all(&self.instance.home)
                .map_err(|error| super::at(&self.instance.home, error))?;
        }
        let mut profile = Profile::open_checked(&self.instance.home, None)?;
        let cancellation = native_mods::Cancellation::default();
        let mode = self.instance.native_profile;
        let native = |step| progress(Progress::Native(step));
        let check_build = |tag: &str| {
            if self.accepted_account.is_none() && !developer_offline::enabled(tag) {
                return Err(StartError::Account(NoOwner::default()));
            }
            architecture(tag).map_or(Ok(()), |error| Err(StartError::Other(error)))
        };
        let registry = crate::browser::sunrise::PlayCatalogue::inspect(profile.home())?;
        let active = if let Some(old) = profile.active()? {
            assert_requested(self.requested_build, old.base(), old.executable())?;
            check_build(old.base())?;
            profile
                .refresh(mode, &cancellation, native, native_consent)
                .map_err(StartError::Native)?
                .ok_or_else(unsettled)?
        } else {
            let first = profile.begin_requested(self.requested_build, choose, |fraction| {
                progress(Progress::Download(fraction));
            })?;
            check_build(first.base())?;
            progress(Progress::Prepare);
            first.prepare(&|line| progress(Progress::Line(line)))?;
            let image = first
                .exact_image(mode, &cancellation, native, native_consent)
                .map_err(StartError::Native)?;
            first.commit(&image, &cancellation)?
        };
        let tag = active.executable();
        let registry_notices = registry.notices(tag)?;
        let offline = !self.explicit_account && developer_offline::enabled(tag);
        let mut cmd = backend::client_with_resources(tag, &active.resource_path());
        instances::apply(&mut cmd, self.instance)?;
        let vars = self.instance.vars();
        cmd.envs(vars.iter().map(|(key, value)| (key, value)));
        progress(Progress::SignIn);
        let notice;
        let session_file = if offline {
            developer_offline::require_capability(tag)?;
            developer_offline::mark(&mut cmd);
            notice = Some(crate::i18n::tr(developer_offline::NOTICE).into());
            None
        } else {
            let uuid = self
                .accepted_account
                .ok_or_else(|| StartError::Account(NoOwner::default()))?;
            let session = account::session_for_game_accepted(&backend::game_home(), uuid)
                .map_err(StartError::Account)?;
            let session_file = session.file.clone();
            notice = if let Some(reason) = session.apply(&mut cmd) {
                Some(crate::i18n::trf(
                    "Playing offline: {reason}",
                    &[("reason", &reason)],
                ))
            } else {
                account::take_unsaved()
            };
            session_file
        };
        let launch = &self.instance.launch;
        let started = (|| {
            arguments(&mut cmd)?;
            profile.spawn_checked_account(
                &active,
                &cancellation,
                cmd,
                (launch, self.piped),
                |cmd| prepare_jars(cmd, launch),
                || {
                    if let Some(uuid) = self.accepted_account.filter(|_| !offline) {
                        account::check_launch_account(&Accounts::load(), uuid)
                            .map_err(|refused| refused.in_window())?;
                    }
                    Ok(())
                },
            )
        })();
        match started {
            Ok((child, runtime)) => Ok(Started {
                child,
                runtime,
                executable: tag.into(),
                notice,
                registry_notices,
                owner: !offline,
            }),
            Err(error) => {
                account::discard_launch_session(session_file);
                Err(error.into())
            }
        }
    }
}

/// Resolve an ordinary headless first Play lazily. An existing committed or pending
/// build bypasses the catalogue completely; an explicit first selection remains exact.
pub fn headless_first(
    instance: &instances::Instance,
    requested: Option<&str>,
) -> Result<FirstBuild, String> {
    let installed = match instance.resolve_version(requested) {
        Ok(tag) => return FirstBuild::installed(&tag),
        Err(error) => error,
    };
    let releases = backend::releases()?;
    let release = if let Some(tag) = requested.or(instance.version.as_deref()) {
        releases
            .iter()
            .find(|release| release.tag == tag && release.asset.is_some())
    } else {
        instance
            .channel
            .following(&releases)
            .builds(&releases)
            .next()
    };
    FirstBuild::download(release.ok_or(installed)?)
}
