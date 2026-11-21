//! Resolve a published release's channel or exact installed version.
use super::{Release, RELEASE_FILE};
use chrono::NaiveDate;
use std::{cmp::Ordering, collections::BTreeMap};
use sunrise_launcher_release_verify::Channel;

/// Look up an explicit Dev build by its exact tag, independently of the catalog, or return None
/// for a different channel. Its title and SHA256SUMS.v2.sig must both name that exact build.
pub(super) fn dev(tag: &str) -> Result<Option<Release>, String> {
    if Channel::of(tag) != Some(Channel::Dev) {
        return Ok(None);
    }
    if !sunrise_launcher_release_verify::is_build(Channel::Dev, tag) {
        return Err(crate::i18n::trf(
            "{tag} is not a full Dev tag (dev-<date>-<commit>).",
            &[("tag", tag)],
        ));
    }
    let missing = || crate::i18n::trf("No Sunrise Dev build is tagged {tag}.", &[("tag", tag)]);
    let release = crate::github::json_optional(&format!(
        "{}/repos/{}/releases/tags/{tag}",
        crate::github::API,
        super::GAME_REPO
    ))?
    .ok_or_else(missing)?;
    let settings = super::Settings::load();
    let assets = super::game_assets_for(
        std::env::consts::OS,
        super::native_windows_architecture(),
        std::env::consts::ARCH,
        settings.game_architecture,
    );
    let (asset, fallback) = assets
        .map_or((super::ASSET, super::GAME_FALLBACK), |(asset, fallback)| {
            (Some(asset), fallback)
        });
    dev_in(&serde_json::json!([release]), tag, asset, fallback)
        .map(Some)
        .ok_or_else(missing)
}

/// The Dev build tagged `tag` in `list` (GitHub's answer for it) with `asset`, else `fallback`.
fn dev_in(
    list: &serde_json::Value,
    tag: &str,
    asset: Option<&str>,
    fallback: Option<&str>,
) -> Option<Release> {
    let keep = |tag: &str| Channel::of(tag) == Some(Channel::Dev);
    let mut found = super::parse_releases(list, asset, keep)
        .into_iter()
        .chain(super::parse_releases(list, fallback, keep))
        .filter(|r| r.tag == tag && r.prerelease);
    let mut dev = found.next()?;
    if dev.asset.is_none() {
        dev.asset = found.find_map(|r| r.asset);
    }
    // Its GitHub tag is its version: its files are there, and it installs and verifies as that.
    dev.nightly = Some(dev.tag.clone());
    Some(dev)
}

pub(super) fn by_tag<'a>(releases: &'a [Release], tag: &str) -> Option<&'a Release> {
    releases
        .iter()
        .find(|release| release.tag == tag || release.nightly.as_deref() == Some(tag))
}

/// The signed build name covers the UTC build day, not a mutable GitHub publication time.
struct SignedIdentity {
    commit: String,
    day: NaiveDate,
}

impl SignedIdentity {
    fn read(release: &Release) -> Option<Self> {
        let source = release.nightly.as_deref()?;
        let bytes = super::fetch_small(&crate::signed::asset_url(source, RELEASE_FILE)).ok()??;
        let digest = crate::signed::build_digest(
            source,
            &release.tag,
            RELEASE_FILE,
            &sunrise_launcher_release_verify::trusted_keys_for(super::GAME_REPO),
            super::fetch_small,
        )
        .ok()?;
        if sunrise_launcher_release_verify::sha256_hex(&bytes) != digest {
            return None;
        }
        Self::decode(&release.tag, &bytes)
    }

    fn decode(tag: &str, verified: &[u8]) -> Option<Self> {
        #[derive(serde::Deserialize)]
        struct Facts {
            contract: u32,
            build: String,
            commit: String,
        }
        let facts: Facts = serde_json::from_slice(verified).ok()?;
        let channel = Channel::of(tag)?;
        if facts.contract != 1
            || facts.build != tag
            || !sunrise_launcher_release_verify::is_build(channel, tag)
            || !full_commit(&facts.commit)
        {
            return None;
        }
        let (_, suffix) = tag.split_once('-')?;
        let (day, abbreviated) = suffix.rsplit_once('-')?;
        if !facts.commit.starts_with(abbreviated) {
            return None;
        }
        Some(Self {
            commit: facts.commit,
            day: NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?,
        })
    }

    fn compare(&self, other: &Self) -> Option<Ordering> {
        if self.commit == other.commit {
            return Some(Ordering::Equal);
        }
        let order = self.day.cmp(&other.day);
        // The signed release contract has no sequence within a UTC day.
        (order != Ordering::Equal).then_some(order)
    }
}

fn full_commit(commit: &str) -> bool {
    commit.len() == 40
        && commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(crate) struct WorldBuildOrder {
    target: Option<SignedIdentity>,
    releases: Vec<Release>,
    comparisons: BTreeMap<String, Option<Ordering>>,
}

impl WorldBuildOrder {
    pub(crate) fn for_target(tag: &str) -> Self {
        let mut releases = super::release_list()
            .map(|list| world_order_releases(&list))
            .unwrap_or_default();
        if !releases.iter().any(|r| r.tag == tag) {
            releases.extend(dev(tag).unwrap_or_default());
        }
        let target = releases
            .iter()
            .find(|r| r.tag == tag)
            .and_then(SignedIdentity::read);
        Self {
            target,
            releases,
            comparisons: BTreeMap::new(),
        }
    }

    /// Abbreviated tags are only lookup hints, never the saved world's identity.
    pub(crate) fn compare_saved(&mut self, commit: &str) -> Option<Ordering> {
        self.compare_saved_with(commit, SignedIdentity::read)
    }

    fn compare_saved_with(
        &mut self,
        commit: &str,
        read_signed: impl FnMut(&Release) -> Option<SignedIdentity>,
    ) -> Option<Ordering> {
        let target = self.target.as_ref()?;
        if commit == target.commit {
            return Some(Ordering::Equal);
        }
        if !full_commit(commit) {
            return None;
        }
        *self.comparisons.entry(commit.into()).or_insert_with(|| {
            let mut signed = self
                .releases
                .iter()
                .filter(|r| {
                    r.nightly.is_some()
                        && r.tag
                            .rsplit_once('-')
                            .is_some_and(|(_, hint)| commit.starts_with(hint))
                })
                .filter_map(read_signed)
                .filter(|identity| identity.commit == commit);
            let saved = signed.next()?;
            if signed.any(|identity| identity.day != saved.day) {
                return None;
            }
            saved.compare(target)
        })
    }
}

fn world_order_releases(list: &serde_json::Value) -> Vec<Release> {
    let mut releases = super::game_releases(list);
    // Use each Dev source once for provenance, independent of the picker and update filters.
    releases.retain(|r| Channel::of(&r.tag) != Some(Channel::Dev));
    releases.extend(
        super::parse_releases(list, None, |tag| {
            sunrise_launcher_release_verify::is_build(Channel::Dev, tag)
        })
        .into_iter()
        .map(|mut dev| {
            dev.nightly = Some(dev.tag.clone());
            dev
        }),
    );
    releases
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::game_releases_with;

    #[test]
    fn a_world_saved_by_dev_gets_signed_order_against_a_nightly_target() {
        let saved = "b".repeat(40);
        let tag = "dev-2026-11-19-bbbbbbb";
        let raw = serde_json::json!([{
            "tag_name": tag,
            "name": "Dev 2026-11-19 · bbbbbbb",
            "published_at": "2026-11-19T00:00:00Z",
            "prerelease": true,
            "assets": []
        }]);
        let releases = world_order_releases(&raw);
        assert_eq!(releases.len(), 1);
        let mut order = WorldBuildOrder {
            target: Some(SignedIdentity {
                commit: "a".repeat(40),
                day: NaiveDate::from_ymd_opt(2026, 11, 18).unwrap(),
            }),
            releases,
            comparisons: BTreeMap::new(),
        };
        assert_eq!(
            order.compare_saved_with(&saved, |release| {
                assert_eq!(release.nightly.as_deref(), Some(tag));
                let signed = serde_json::json!({ "contract": 1, "build": tag, "commit": saved });
                SignedIdentity::decode(&release.tag, signed.to_string().as_bytes())
            }),
            Some(Ordering::Greater)
        );
    }

    #[test]
    fn world_order_uses_full_signed_identity_and_has_no_order_within_one_day() {
        let decode = |build: &str, commit: &str| {
            SignedIdentity::decode(
                build,
                &serde_json::to_vec(&serde_json::json!({
                    "contract": 1, "build": build, "commit": commit,
                }))
                .unwrap(),
            )
        };
        let a = &"a".repeat(40);
        let b = &"b".repeat(40);
        let first = decode("nightly-2026-11-18-aaaaaaa", a).unwrap();
        let same_day = decode("dev-2026-11-18-bbbbbbb", b).unwrap();
        let newer = decode("nightly-2026-11-19-bbbbbbb", b).unwrap();
        assert_eq!(first.compare(&same_day), None);
        assert_eq!(first.compare(&newer), Some(Ordering::Less));
        assert_eq!(newer.compare(&same_day), Some(Ordering::Equal));
        assert!(decode("nightly-2026-11-18-aaaaaaa", "aaaaaaa").is_none());
        assert!(decode("nightly-2026-11-18-aaaaaaa", b).is_none());
        assert!(decode("snapshot-2026-11-18-bbbbbbb", b).is_none());
    }

    fn published(channel: &str) -> Vec<Release> {
        game_releases_with(
            &serde_json::json!([{
                "tag_name": channel,
                "name": "Nightly 2026-11-10 · 9f13812",
                "published_at": "2026-11-10T17:07:46Z",
                "prerelease": true,
                "assets": [{"name": "sunrise-linux-x86_64.tar.gz", "size": 123}]
            }]),
            Some("sunrise-linux-x86_64.tar.gz"),
            None,
        )
    }

    #[test]
    fn channel_and_exact_version_select_the_same_published_build() {
        let channel = "nightly";
        let releases = published(channel);
        for requested in [channel, "nightly-2026-11-10-9f13812"] {
            let release = by_tag(&releases, requested).unwrap();
            assert_eq!(release.tag, "nightly-2026-11-10-9f13812");
            assert_eq!(release.nightly.as_deref(), Some(channel));
            assert_eq!(
                    release.asset,
                    Some((format!("https://github.com/sunrise-org/sunrise/releases/download/{channel}/sunrise-linux-x86_64.tar.gz"), 123))
                );
        }
    }

    #[test]
    fn a_dev_build_is_found_by_its_tag_and_installs_as_it() {
        const DEV: &str = "dev-2026-11-13-bbbbbbb";
        let (arm, x64) = ("sunrise-windows-arm64.zip", "sunrise-windows-x64.zip");
        let list = serde_json::json!([{
            "tag_name": DEV,
            "name": "Dev 2026-11-13 · bbbbbbb",
            "published_at": "2026-11-13T10:00:00Z",
            "prerelease": true,
            "assets": [{"name": x64, "size": 1}]
        }]);
        let found = dev_in(&list, DEV, Some(arm), Some(x64)).unwrap();
        assert_eq!(found.tag, DEV);
        assert_eq!(found.nightly.as_deref(), Some(DEV));
        assert!(found.prerelease);
        assert!(found.asset.unwrap().0.ends_with(&format!("/{DEV}/{x64}")));
        assert_eq!(
            crate::signed::title_version(DEV, &found.name).as_deref(),
            Some(DEV),
            "what install() checks before verifying"
        );
        assert!(dev_in(&list, "dev-2026-11-12-aaaaaaa", Some(x64), None).is_none());
        let mut full_release = list;
        full_release[0]["prerelease"] = serde_json::Value::Bool(false);
        assert!(dev_in(&full_release, DEV, Some(x64), None).is_none());
        let nightly = serde_json::json!([{"tag_name": "nightly", "name": "Nightly 2026-11-13 · bbbbbbb",
                                           "assets": [{"name": x64, "size": 1}]}]);
        assert!(dev_in(&nightly, "nightly", Some(x64), None).is_none());
        // Other tags are not looked up here; only a full Dev tag may go to GitHub.
        for tag in [
            "nightly",
            "26.3-0.1.0",
            "snapshot",
            "snapshot-2026-11-13-bbbbbbb",
        ] {
            assert!(dev(tag).unwrap().is_none(), "{tag}");
        }
        for tag in ["dev", "dev-2026-11-13", "dev-../../x"] {
            assert!(dev(tag).is_err(), "{tag}");
        }
    }

    #[test]
    fn an_unknown_tag_never_falls_back_to_an_available_build() {
        assert!(published("nightly-2026-11-10").is_empty());
        let releases = published("nightly");
        assert!(by_tag(&releases, "nightly-2026-11-09").is_none());
        assert!(by_tag(&releases, "nightly-2026-11-10-deadbee").is_none());
        assert!(by_tag(&[], "nightly").is_none());
    }
}
