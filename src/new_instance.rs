//! New instance's kinds (docs/design-system.md, "Night"; plan R0, screen 4): Sunrise, Vanilla
//! (Minecraft: Java Edition as Mojang ships it) and Modded (with Fabric, Quilt, Forge or NeoForge). A Java kind picks
//! its version from Mojang's public list (`minecraft_versions`), fetched once when the dialog first
//! needs it; the game itself comes with the first Play, for an account that owns it.
use super::*;



use sunrise_launcher::loaders::{self, Kind as Loader};
use sunrise_launcher::minecraft_versions::Versions;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NewKind {
    Sunrise,
    Vanilla,
    Modded,
}

/// What a new Java instance plays.
#[derive(Clone, Debug)]
pub(super) struct JavaChoice {
    pub(super) kind: NewKind,
    pub(super) loader: Loader,
    /// None: the newest release.
    pub(super) version: Option<String>,
    /// The loader's version; None: its newest stable one.
    pub(super) loader_version: Option<String>,
}

impl Default for JavaChoice {
    fn default() -> Self {
        JavaChoice {
            kind: NewKind::Sunrise,
            loader: Loader::Fabric,
            version: None,
            loader_version: None,
        }
    }
}

impl JavaChoice {
    /// The Minecraft version it makes: the chosen one, else the newest release.
    pub(super) fn minecraft(&self, versions: Option<&Versions>) -> Option<String> {
        self.version
            .clone()
            .or_else(|| versions.map(|v| v.latest_release.clone()))
    }

    /// The name it gets when none is typed: "Vanilla 1.21.4", "Fabric 1.21.4".
    pub(super) fn default_name(&self, minecraft: &str) -> String {
        match self.kind {
            NewKind::Modded => format!("{} {minecraft}", self.loader.name()),
            _ => format!("Vanilla {minecraft}"),
        }
    }
}

/// Mojang's list as the dialog has it.
#[derive(Default)]
pub(super) enum VersionList {
    #[default]
    NotAsked,
    Loading,
    Ready(Versions),
    Failed(String),
}


/// Makes a Java instance: `name`, playing `minecraft`, with the loader version chosen for Modded
/// (none: its newest stable one), asked of its service now, so a version it doesn't support
/// says so here.
pub(super) fn create_java(
    choice: &JavaChoice,
    name: &str,
    minecraft: &str,
) -> Result<instances::Instance, String> {
    let loader = (choice.kind == NewKind::Modded)
        .then(|| {
            loaders::choose(
                choice.loader.id(),
                minecraft,
                choice.loader_version.as_deref(),
            )
        })
        .transpose()?;
    let made = instances::create_java(name, minecraft)?;
    match loader {
        Some(loader) => instances::edit(&made.id, |m| {
            m.java.get_or_insert_with(Default::default).loader = Some(loader)
        }),
        None => Ok(made),
    }
}

/// `to`, with the newest stable version of a loader it names without one (asked of its service
/// now, so a Minecraft version the loader doesn't support says so here).
pub(super) fn with_loader(to: instances::Plays) -> Result<instances::Plays, String> {
    match to {
        instances::Plays::Java {
            minecraft,
            loader: Some(l),
        } if l.version.is_empty() => Ok(instances::Plays::Java {
            loader: Some(loaders::choose(l.kind.id(), &minecraft, None)?),
            minecraft,
        }),
        other => Ok(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_java_instance_is_named_after_what_it_plays() {
        let mut choice = JavaChoice {
            kind: NewKind::Vanilla,
            ..Default::default()
        };
        assert_eq!(choice.default_name("1.21.4"), "Vanilla 1.21.4");
        choice.kind = NewKind::Modded;
        choice.loader = Loader::Quilt;
        assert_eq!(choice.default_name("1.21.4"), "Quilt 1.21.4");
        assert_eq!(choice.minecraft(None), None);
        choice.version = Some("1.20.1".into());
        assert_eq!(choice.minecraft(None).as_deref(), Some("1.20.1"));
    }

}
