//! The instance dialog in the glass look (the mockup's `InstanceModal`, sunrise-site
//! `components/launcher/instance-modal.tsx`): one plate round at 28 over the dimmed page. On the
//! left, what the instance will be (its icon, name and what it plays) and its kind; on the right,
//! what it plays, its name and group, and its icon; Cancel and Create at the foot. Dark
//! appearance only: the light look keeps `instance_dialog`.
use super::*;
use crate::glass_ui::{self as ui, white};
use new_instance::{ VersionList};


use sunrise_launcher::instances::{ stable_builds, Channel, StableBuild};
use sunrise_launcher::loaders::Kind as Loader;
use sunrise_launcher_theme::glass::{fill, ink, radius};

/// The loaders Prism Launcher offers, in the mockup's order. Those `loaders::Kind` can install
/// are shown; the others join when the launcher learns them.
const LOADERS: [&str; 5] = ["fabric", "quilt", "neoforge", "forge", "liteloader"];

/// The loaders a new modded instance can use now.
pub(super) fn loaders() -> Vec<Loader> {
    LOADERS
        .iter()
        .filter_map(|id| Loader::parse(id).ok())
        .collect()
}

/// A choice card (`CHOICE`): round at 16, white at 12 % when chosen, else 4 %, 7 % under the
/// pointer.
pub(super) fn choice(id: impl Into<SharedString>, on: bool, t: &Theme) -> Stateful<Div> {
    keyboard(
        div()
            .id(id.into())
            .relative()
            .flex()
            .rounded(px(crate::glass_ui::rad(radius::POPOVER)))
            .cursor_pointer()
            .bg(white(if on { fill::ACTIVE } else { 0.04 }))
            .when(!on, |d| {
                d.child(ui::back_tint(radius::POPOVER, white(0.03)))
            }),
        t,
    )
}

/// A `Section`: its title at 13 px, white at 60 %, anything `aside` at its right, then `body`.
pub(super) fn section(title: SharedString, aside: Option<Div>, body: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .child(
            div()
                .flex()
                .items_end()
                .justify_between()
                .gap(px(16.))
                .mb(px(12.))
                .child(ui::text(13., FontWeight::MEDIUM, ink::NOTE).child(title))
                .children(aside),
        )
        .child(body)
}

/// A list's row of choices: round at 12, white at 11 % when chosen, 5 % under the pointer.
pub(super) fn list_choice(id: impl Into<SharedString>, on: bool, t: &Theme) -> Stateful<Div> {
    keyboard(
        div()
            .id(id.into())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(16.))
            .px(px(16.))
            .rounded(px(crate::glass_ui::rad(radius::ROW)))
            .cursor_pointer()
            .when(on, |d| d.bg(white(0.11)))
            .when(!on, |d| {
                d.child(ui::back_tint(radius::ROW, white(fill::HOVER)))
            }),
        t,
    )
}

/// The tick at a list row's end, the accent's soft shade, in a 20 px column.
pub(super) fn tick(on: bool, soft: u32) -> Div {
    div()
        .flex()
        .flex_none()
        .justify_center()
        .w(px(20.))
        .children(on.then(|| ui::icon("tick-02", 18., ui::color(soft))))
}

/// A list of rows on white at 4 %, round at 16, scrolling past `max` px.
pub(super) fn list_box(id: &'static str, max: f32) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_col()
        .max_h(px(max))
        .overflow_y_scroll()
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .p(px(6.))
        .rounded(px(crate::glass_ui::rad(radius::POPOVER)))
        .bg(white(0.04))
}

impl Launcher {

    /// The Minecraft version a Java instance plays (or will), once Mojang's list says.
    pub(super) fn draft_minecraft(&self) -> Option<String> {
        let versions = match &self.instances.versions {
            VersionList::Ready(v) => Some(v),
            _ => None,
        };
        self.instances.dialog.as_ref()?.java.minecraft(versions)
    }

    /// The Sunrise version an instance plays: the one it keeps, else the one it follows.
    pub(super) fn draft_sunrise(&self) -> Option<String> {
        let d = self.instances.dialog.as_ref()?;
        d.pin.clone().or_else(|| self.followed_version(d.channel))
    }

    /// The version an instance that keeps none plays: on Stable the newest stable build (Create
    /// installs it), on Nightly the launcher's, or with none installed yet, the newest Nightly.
    fn followed_version(&self, channel: Channel) -> Option<String> {
        let nightly = || {
            self.launcher_version()
                .cloned()
                .or_else(|| self.sunrise_choices().into_iter().next())
        };
        match channel {
            Channel::Stable => self
                .stable_builds()
                .into_iter()
                .next()
                .map(|b| b.tag)
                .or_else(nightly),
            Channel::Nightly => nightly(),
        }
    }

    /// The Stable channel's builds, newest first (none until the release list is here).
    pub(super) fn stable_builds(&self) -> Vec<StableBuild> {
        match &self.releases {
            Load::Ready(r) => stable_builds(r),
            _ => Vec::new(),
        }
    }

    /// The Sunrise versions the dialog offers (the mockup's `data.nightlies`): see
    /// [`sunrise_choices`].
    pub(super) fn sunrise_choices(&self) -> Vec<String> {
        let releases = match &self.releases {
            Load::Ready(r) => r.as_slice(),
            _ => &[],
        };
        sunrise_choices(releases, &self.installed)
    }

    /// The release to download for the Sunrise version `tag`, when it isn't installed.
    pub(super) fn release_to_install(&self, tag: &str) -> Option<Release> {
        let Load::Ready(releases) = &self.releases else {
            return None;
        };
        (!self.installed.iter().any(|t| t == tag))
            .then(|| releases.iter().find(|r| r.tag == tag && r.asset.is_some()))
            .flatten()
            .cloned()
    }

}

/// Every Nightly published with a build for this platform, newest first, then the installed
/// versions that list lacks (older Nightlies, imported builds). Picking one not installed installs
/// it on Create.
pub(super) fn sunrise_choices(releases: &[Release], installed: &[String]) -> Vec<String> {
    let published = releases
        .iter()
        .filter(|r| r.nightly.is_some() && r.asset.is_some())
        .map(|r| &r.tag);
    let mut out: Vec<String> = Vec::new();
    for tag in published.chain(installed) {
        if !out.contains(tag) {
            out.push(tag.clone());
        }
    }
    out
}


impl Launcher {
    /// A version as a player reads it: the Versions page's name for a build of the game ("Nov 13
    /// update", with its time when another came out that day; its commit stays in the details),
    /// "Imported · 2026-10-30", a developer build's name, or the tag. Players who don't know what
    /// a nightly is see the day it came out, not the channel's jargon.
    pub(super) fn version_label(&self, tag: &str) -> String {
        if let Some(developer) = sunrise_launcher::developer_install::label(tag) {
            return developer;
        }
        // A game built with mods reads as what it was built on: "Nov 13 update, with your mods".
        if native_mods::is_native(tag) {
            return match native_base(tag) {
                Some(base) => trf(
                    "{version}, with your mods",
                    &[("version", &self.version_label(&base))],
                ),
                None => tr("A build with your mods").to_string(),
            };
        }
        match sunrise_launcher::nightly::date_of(tag) {
            Some(_) => self.version_name(tag),
            None => build_import::label(tag).unwrap_or_else(|| tag.to_string()),
        }
    }
}

/// The release a native build started from, read once per build: a new build has a new tag.
pub(crate) fn native_base(tag: &str) -> Option<String> {
    thread_local! {
        static BASES: std::cell::RefCell<std::collections::HashMap<String, Option<String>>> =
            Default::default();
    }
    BASES.with(|bases| {
        bases
            .borrow_mut()
            .entry(tag.to_string())
            .or_insert_with(|| native_mods::about(tag).map(|about| about.base))
            .clone()
    })
}


#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, nightly: bool, asset: bool) -> Release {
        Release {
            tag: tag.into(),
            name: String::new(),
            body: String::new(),
            date: String::new(),
            time: String::new(),
            asset: asset.then(|| ("sunrise.tar.gz".into(), 1)),
            prerelease: false,
            nightly: nightly.then(|| "nightly".into()),
        }
    }

    #[test]
    fn stable_is_a_build_a_week_until_real_releases_exist() {
        // 2026-11-09 is a Monday, 2026-11-08 the Sunday before it.
        let mut releases = vec![
            release("nightly-2026-11-13-aaaaaaa", true, true),
            release("nightly-2026-11-12-bbbbbbb", true, true),
            release("nightly-2026-11-10-ccccccc", true, true),
            release("nightly-2026-11-08-ddddddd", true, true),
            release("nightly-2026-11-03-eeeeeee", true, true),
            // No build for this platform: not a candidate.
            release("nightly-2026-11-14-fffffff", true, false),
        ];
        let weekly = |tag: &str, week: &str| StableBuild {
            tag: tag.into(),
            week: Some(week.into()),
        };
        assert_eq!(
            stable_builds(&releases),
            [
                weekly("nightly-2026-11-13-aaaaaaa", "2026-11-09"),
                weekly("nightly-2026-11-08-ddddddd", "2026-11-02"),
            ]
        );
        releases.insert(0, release("v0.3.0", false, true));
        assert_eq!(
            stable_builds(&releases),
            [StableBuild {
                tag: "v0.3.0".into(),
                week: None
            }]
        );
    }

    #[test]
    fn the_dialog_offers_every_published_nightly_then_what_only_this_computer_has() {
        let releases = [
            release("nightly-2026-11-03-aaaaaaa", true, true),
            // No build for this platform: nothing to install.
            release("nightly-2026-11-02-bbbbbbb", true, false),
            release("nightly-2026-11-01-ccccccc", true, true),
            // The old Alpha channel's releases aren't Nightlies.
            release("v0.2.1", false, true),
        ];
        let installed = [
            "nightly-2026-11-01-ccccccc".to_string(),
            "nightly-2026-10-20-ddddddd".to_string(),
            "imported-2026-10-30".to_string(),
        ];
        assert_eq!(
            sunrise_choices(&releases, &installed),
            [
                "nightly-2026-11-03-aaaaaaa",
                "nightly-2026-11-01-ccccccc",
                "nightly-2026-10-20-ddddddd",
                "imported-2026-10-30",
            ]
        );
        // Offline, or before the list arrives: what is installed.
        assert_eq!(sunrise_choices(&[], &installed), installed);
    }
}
