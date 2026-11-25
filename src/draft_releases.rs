//! `SUNRISE_LAUNCHER_DRAFTS`: test a Dev build from its draft release's files on disk, before it is
//! public. The folder holds one subfolder per release, named for its tag and laid out as GitHub's
//! download addresses are (`<folder>/<tag>/<file>`, as `gh release download <tag> -D <folder>/<tag>`
//! makes it): the game's `dev-<date>-<commit>` and its SDK's `sdk-<commit>` side by side.
//!
//! While the variable names a folder, every file of Sunrise's releases is read from that folder and
//! never from GitHub; one whose release has no subfolder there is refused, not downloaded. The files
//! are checked exactly as downloaded ones are, against the same signed `SHA256SUMS`. Only an exact
//! tag installs from it (`sunrise-launcher install dev-<date>-<commit>`), and the installed build
//! keeps a mark ([`label`]) that leaves it out of every automatic choice of the newest Dev build.
//! Unset or empty, nothing here reads a file.
use crate::i18n::trf;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use sunrise_launcher_release_verify::Channel;

pub const VAR: &str = "SUNRISE_LAUNCHER_DRAFTS";

/// Left in an installed build's folder by an install from the folder: the build's mark.
const MARK: &str = "draft-release.json";

/// The folder [`VAR`] names: None when it is unset or empty.
pub(crate) fn folder() -> Result<Option<PathBuf>, String> {
    folder_in(std::env::var_os(VAR))
}

fn folder_in(value: Option<OsString>) -> Result<Option<PathBuf>, String> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let folder = PathBuf::from(value);
    let shown = folder.display().to_string();
    if !folder.is_absolute() {
        return Err(trf(
            "{var} must be an absolute path, not {folder}.",
            &[("var", VAR), ("folder", &shown)],
        ));
    }
    if !folder.is_dir() {
        return Err(trf(
            "{var} names {folder}, which isn't a folder.",
            &[("var", VAR), ("folder", &shown)],
        ));
    }
    Ok(Some(folder))
}

/// The file read instead of the release download `url` while [`VAR`] names a folder; None when it
/// is unset, or for an address that isn't a file of Sunrise's releases.
pub(crate) fn local(url: &str) -> Result<Option<PathBuf>, String> {
    local_with(std::env::var_os(VAR), url)
}

fn local_with(value: Option<OsString>, url: &str) -> Result<Option<PathBuf>, String> {
    let Some(folder) = folder_in(value)? else {
        return Ok(None);
    };
    let prefix = format!(
        "https://github.com/{}/releases/download/",
        crate::backend::GAME_REPO
    );
    let Some((tag, name)) = url
        .strip_prefix(&prefix)
        .and_then(|rest| rest.split_once('/'))
    else {
        return Ok(None);
    };
    // `signed::asset_url` escapes everything but plain names, so an escape or a dot name is refused.
    let plain =
        |part: &str| !part.is_empty() && part != "." && part != ".." && !part.contains(['%', '/']);
    if !plain(tag) || !plain(name) {
        return Err(trf(
            "{url} is not a release file {var} can stand in for.",
            &[("url", url), ("var", VAR)],
        ));
    }
    Ok(Some(release_folder(&folder, tag)?.join(name)))
}

fn release_folder(folder: &Path, tag: &str) -> Result<PathBuf, String> {
    let release = folder.join(tag);
    if !release.is_dir() {
        return Err(trf(
            "{var} is set, and {release} isn't a folder: put the release's files there, or unset {var}.",
            &[("var", VAR), ("release", &release.display().to_string())],
        ));
    }
    Ok(release)
}

/// The file at `path`, whole up to `limit` bytes; None when there is none, as a missing download is.
pub(crate) fn read(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let mut body = Vec::new();
    file.take(limit)
        .read_to_end(&mut body)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Some(body))
}

/// GitHub's answer for the Dev release `tag`, made from its subfolder while [`VAR`] names a folder:
/// its title names the tag's date and commit, as a published Dev build's does, so its
/// `SHA256SUMS.v2.sig` must sign for exactly `tag`.
pub(crate) fn release(tag: &str) -> Result<Option<serde_json::Value>, String> {
    release_with(std::env::var_os(VAR), tag)
}

fn release_with(value: Option<OsString>, tag: &str) -> Result<Option<serde_json::Value>, String> {
    let Some(folder) = folder_in(value)? else {
        return Ok(None);
    };
    let Some(title) = title(tag) else {
        return Ok(None);
    };
    let release = release_folder(&folder, tag)?;
    let mut assets = Vec::new();
    for entry in
        fs::read_dir(&release).map_err(|error| format!("{}: {error}", release.display()))?
    {
        let entry = entry.map_err(|error| format!("{}: {error}", release.display()))?;
        let (Some(name), Ok(meta)) = (
            entry.file_name().to_str().map(str::to_owned),
            fs::metadata(entry.path()),
        ) else {
            continue;
        };
        if meta.is_file() {
            assets.push(serde_json::json!({"name": name, "size": meta.len()}));
        }
    }
    Ok(Some(serde_json::json!({
        "tag_name": tag,
        "name": title,
        "body": "",
        "draft": false,
        "prerelease": true,
        "published_at": "",
        "assets": assets,
    })))
}

/// The title a published Dev build `tag` carries (`Dev <date> · <commit>`); None for any other tag.
fn title(tag: &str) -> Option<String> {
    sunrise_launcher_release_verify::is_build(Channel::Dev, tag).then(|| {
        let date = &tag[Channel::Dev.tag().len() + 1..][..10];
        let commit = &tag[Channel::Dev.tag().len() + 12..];
        format!("Dev {date} · {commit}")
    })
}

/// Marks the installed build `tag` as installed from `folder`.
pub(crate) fn mark(tag: &str, folder: &Path) -> Result<(), String> {
    let path = crate::backend::version_dir(tag).join(MARK);
    fs::write(&path, serde_json::json!({ "folder": folder }).to_string())
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// Whether the installed build `tag` came from a draft's files: chosen only by its exact tag.
pub fn is_test_build(tag: &str) -> bool {
    crate::backend::version_dir(tag).join(MARK).is_file()
}

/// The installed build `tag`'s name, marked as a test build, when it came from a draft's files.
pub fn label(tag: &str) -> Option<String> {
    is_test_build(tag).then(|| {
        trf(
            "{build} · Test build from local files",
            &[("build", &title(tag).unwrap_or_else(|| tag.to_owned()))],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAG: &str = "dev-2026-11-25-bdad86d";

    fn url(tag: &str, name: &str) -> String {
        crate::signed::asset_url(tag, name)
    }

    #[test]
    fn unset_or_empty_reads_no_folder() {
        for value in [None, Some(OsString::new())] {
            assert_eq!(folder_in(value.clone()), Ok(None));
            assert_eq!(local_with(value.clone(), &url(TAG, "SHA256SUMS")), Ok(None));
            assert_eq!(release_with(value, TAG), Ok(None));
        }
    }

    #[test]
    fn the_folder_is_an_existing_absolute_one() {
        let relative = folder_in(Some("drafts".into())).unwrap_err();
        assert!(relative.contains("absolute"), "{relative}");
        let missing = tempfile::tempdir().unwrap().path().join("gone");
        let missing = folder_in(Some(missing.into())).unwrap_err();
        assert!(missing.contains("isn't a folder"), "{missing}");
    }

    #[test]
    fn release_files_come_only_from_their_subfolder() {
        let drafts = tempfile::tempdir().unwrap();
        fs::create_dir(drafts.path().join(TAG)).unwrap();
        let value = || Some(drafts.path().as_os_str().to_owned());
        assert_eq!(
            local_with(value(), &url(TAG, "SHA256SUMS")),
            Ok(Some(drafts.path().join(TAG).join("SHA256SUMS")))
        );
        // No subfolder: refused, never downloaded instead.
        let other = local_with(value(), &url("sdk-0123456", "sdk.json")).unwrap_err();
        assert!(other.contains("sdk-0123456"), "{other}");
        assert!(local_with(value(), &url("..", "SHA256SUMS")).is_err());
        assert!(local_with(value(), &url(TAG, "a/b")).is_err());
        // Anything but Sunrise's release files is untouched.
        assert_eq!(
            local_with(
                value(),
                &crate::signed::launcher_asset_url("v1.3.3", "SHA256SUMS")
            ),
            Ok(None)
        );
        assert_eq!(
            format!("{}/", sunrise_launcher_addresses::SDK_RELEASES),
            url(TAG, "x").strip_suffix(&format!("{TAG}/x")).unwrap(),
            "the SDK's files are read through the same addresses"
        );
    }

    #[test]
    fn a_draft_reads_as_its_published_release() {
        let drafts = tempfile::tempdir().unwrap();
        let release = drafts.path().join(TAG);
        fs::create_dir(&release).unwrap();
        fs::write(release.join("SHA256SUMS"), "12345").unwrap();
        fs::create_dir(release.join("not-a-file")).unwrap();
        let json = release_with(Some(drafts.path().as_os_str().to_owned()), TAG)
            .unwrap()
            .unwrap();
        assert_eq!(json["name"], "Dev 2026-11-25 · bdad86d");
        assert_eq!(
            crate::signed::title_version(TAG, json["name"].as_str().unwrap()).as_deref(),
            Some(TAG)
        );
        assert_eq!(json["prerelease"], true);
        assert_eq!(json["draft"], false);
        assert_eq!(
            json["assets"],
            serde_json::json!([{"name": "SHA256SUMS", "size": 5}])
        );
        let missing = release_with(
            Some(drafts.path().as_os_str().to_owned()),
            "dev-2026-11-26-abcdef0",
        );
        assert!(missing.is_err());
    }
}
