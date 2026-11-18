//! The licence notices of what the launcher embeds and derives from. Packaging ships them in
//! each archive, AppImage and app bundle; Windows Setup writes them into its install. Only a
//! registered install refreshes them at runtime, using the running version's own notices.
//! `packaging/notices.txt` and these embedded notices must agree.

/// File name in the `LICENSES` folder, and the notice.
pub const FILES: &[(&str, &str)] = &[
    (
        "MIT-Horizon.txt",
        include_str!("../assets/LICENSES/MIT-Horizon.txt"),
    ),
    (
        "OFL-Figtree.txt",
        include_str!("../assets/fonts/OFL-Figtree.txt"),
    ),
    (
        "OFL-Geist.txt",
        include_str!("../crates/theme/fonts/OFL-Geist.txt"),
    ),
    (
        "OFL-BricolageGrotesque.txt",
        include_str!("../crates/theme/fonts/OFL-BricolageGrotesque.txt"),
    ),
    (
        "OFL-PixelifySans.txt",
        include_str!("../crates/theme/fonts/OFL-PixelifySans.txt"),
    ),
    (
        "MIT-Pixelarticons.txt",
        include_str!("../crates/theme/icons/pixel/LICENSE-pixelarticons.txt"),
    ),
    (
        "MIT-Hugeicons.txt",
        include_str!("../crates/theme/icons/glass/LICENSE-hugeicons.txt"),
    ),
];

/// Writes every notice into `dir/LICENSES`.
pub fn write_into(dir: &std::path::Path) -> std::io::Result<()> {
    let folder = dir.join("LICENSES");
    std::fs::create_dir_all(&folder)?;
    for (name, text) in FILES {
        std::fs::write(folder.join(name), text)?;
    }
    Ok(())
}

#[cfg(any(windows, test))]
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The install record must name this exact executable. Neither the setup copy, a portable
/// executable with the same name, nor a symlink to an executable proves ownership.
#[cfg(any(windows, test))]
pub(crate) fn installed_dir(exe: &Path, registered: Option<&Path>, setup: bool) -> Option<PathBuf> {
    let registered = registered?;
    if setup
        || !registered.is_absolute()
        || !fs::symlink_metadata(registered).ok()?.is_file()
        || !fs::symlink_metadata(exe).ok()?.is_file()
    {
        return None;
    }
    let installed = registered.canonicalize().ok()?;
    if exe.canonicalize().ok()? != installed {
        return None;
    }
    Some(installed.parent()?.to_path_buf())
}

/// Replaces the registered install's notices with this binary's exact set, or does nothing if
/// the folder already contains that set. The replacement is staged before touching the old set.
#[cfg(any(windows, test))]
pub(crate) fn refresh_installed(
    exe: &Path,
    registered: Option<&Path>,
    setup: bool,
) -> io::Result<()> {
    let Some(dir) = installed_dir(exe, registered, setup) else {
        return Ok(());
    };
    let folder = dir.join("LICENSES");
    let exists = match fs::symlink_metadata(&folder) {
        Ok(meta) if meta.is_dir() => {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
                if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    return Err(io::Error::other("LICENSES is a reparse point"));
                }
            }
            let mut matching = 0;
            for entry in fs::read_dir(&folder)? {
                let entry = entry?;
                let name = entry.file_name();
                let meta = entry.metadata()?;
                match FILES.iter().find(|(file, _)| name == *file) {
                    Some((_, text))
                        if meta.is_file()
                            && meta.len() == text.len() as u64
                            && fs::read(entry.path())? == text.as_bytes() =>
                    {
                        matching += 1
                    }
                    _ => {
                        matching = 0;
                        break;
                    }
                }
            }
            if matching == FILES.len() {
                return Ok(());
            }
            true
        }
        Ok(_) => return Err(io::Error::other("LICENSES is not a regular directory")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    let stage = tempfile::Builder::new()
        .prefix(".sunrise-notices-")
        .tempdir_in(&dir)?;
    write_into(stage.path())?;
    let previous = stage.path().join("previous");
    if exists {
        fs::rename(&folder, &previous)?;
    }
    if let Err(e) = fs::rename(stage.path().join("LICENSES"), &folder) {
        if exists && fs::rename(&previous, &folder).is_err() {
            // Don't discard the previous notices if even the rollback is refused.
            let _ = stage.keep();
        }
        return Err(e);
    }
    Ok(())
}

/// Removes the notices [`write_into`] wrote, and their folder when nothing else is in it.
#[cfg(any(windows, test))]
pub fn remove_from(dir: &std::path::Path) -> Vec<String> {
    let folder = dir.join("LICENSES");
    let mut left = Vec::new();
    for (name, _) in FILES {
        match std::fs::remove_file(folder.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => left.push(format!("{}: {e}", folder.join(name).display())),
        }
    }
    let _ = std::fs::remove_dir(&folder);
    left
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn setup_does_not_write_notices_beside_itself() {
        let download = tempfile::tempdir().unwrap();
        let exe = download.path().join(crate::identity::WINDOWS_EXECUTABLE);
        fs::write(&exe, "launcher started in Setup mode").unwrap();
        // Even the installed binary can be started in Setup mode. Setup takes precedence.
        refresh_installed(&exe, Some(&exe), true).unwrap();
        assert!(!download.path().join("LICENSES").exists());
    }

    #[test]
    fn portable_binaries_leave_their_folders_alone() {
        let root = tempfile::tempdir().unwrap();
        let registered = root.path().join(crate::identity::WINDOWS_EXECUTABLE);
        fs::write(&registered, "registered launcher").unwrap();
        for path in ["Downloads", ".local/bin", "bin", ".cargo/bin"] {
            let dir = root.path().join(path);
            fs::create_dir_all(&dir).unwrap();
            let exe = dir.join(crate::identity::WINDOWS_EXECUTABLE);
            fs::write(&exe, "portable launcher").unwrap();
            refresh_installed(&exe, None, false).unwrap();
            refresh_installed(&exe, Some(&registered), false).unwrap();
            assert!(!dir.join("LICENSES").exists(), "{path}");
        }
        assert!(!root.path().join("LICENSES").exists());
    }

    #[test]
    fn the_new_installed_binary_replaces_its_notices_with_its_exact_set() {
        let install = tempfile::tempdir().unwrap();
        let exe = install.path().join(crate::identity::WINDOWS_EXECUTABLE);
        let folder = install.path().join("LICENSES");
        fs::write(&exe, "new launcher").unwrap();
        write_into(install.path()).unwrap();
        fs::write(folder.join(FILES[0].0), "superseded licence text").unwrap();
        fs::write(folder.join("removed-dependency.txt"), "old notice").unwrap();
        refresh_installed(&exe, Some(&exe), false).unwrap();
        let assert_exact = || {
            assert_eq!(fs::read_dir(&folder).unwrap().count(), FILES.len());
            for (name, text) in FILES {
                assert_eq!(fs::read_to_string(folder.join(name)).unwrap(), *text);
            }
        };
        assert_exact();
        // An extra stale notice alone is enough to refresh the set.
        fs::write(folder.join("removed-dependency.txt"), "old notice").unwrap();
        refresh_installed(&exe, Some(&exe), false).unwrap();
        assert_exact();
    }

    #[cfg(unix)]
    #[test]
    fn the_registered_install_does_not_follow_a_notices_symlink() {
        let install = tempfile::tempdir().unwrap();
        let unrelated = tempfile::tempdir().unwrap();
        let exe = install.path().join(crate::identity::WINDOWS_EXECUTABLE);
        fs::write(&exe, "installed launcher").unwrap();
        std::os::unix::fs::symlink(unrelated.path(), install.path().join("LICENSES")).unwrap();
        assert!(refresh_installed(&exe, Some(&exe), false).is_err());
        assert_eq!(fs::read_dir(unrelated.path()).unwrap().count(), 0);
    }

    fn root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn listed() -> Vec<(String, String)> {
        std::fs::read_to_string(root().join("packaging/notices.txt"))
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(|l| {
                let (name, source) = l.split_once(' ').unwrap();
                (name.to_string(), source.to_string())
            })
            .collect()
    }

    #[test]
    fn the_packaging_list_and_the_embedded_notices_are_the_same_files() {
        let listed = listed();
        assert_eq!(listed.len(), FILES.len());
        for ((name, text), (listed_name, source)) in FILES.iter().zip(&listed) {
            assert_eq!(name, listed_name);
            let on_disk = std::fs::read_to_string(root().join(source)).unwrap();
            assert_eq!(*text, on_disk, "{name} differs from {source}");
        }
    }

    #[test]
    fn every_embedded_asset_has_its_notice_in_the_list() {
        let names: Vec<_> = FILES.iter().map(|(n, _)| *n).collect();
        // Fonts and icons bundled into the binary: Figtree here, Geist, Bricolage and Pixelify
        // Sans and both icon sets in the theme crate.
        for needed in [
            "OFL-Figtree.txt",
            "OFL-Geist.txt",
            "OFL-BricolageGrotesque.txt",
            "OFL-PixelifySans.txt",
            "MIT-Pixelarticons.txt",
            "MIT-Hugeicons.txt",
            "MIT-Horizon.txt",
        ] {
            assert!(names.contains(&needed), "{needed}");
        }
        let fonts = std::fs::read_to_string(root().join("crates/theme/src/lib.rs")).unwrap();
        for family in ["Geist", "Bricolage", "Pixelify"] {
            if fonts.contains(family) {
                assert!(names.iter().any(|n| n.contains(family)), "{family}");
            }
        }
    }

    #[test]
    fn every_route_copies_the_list() {
        let script = std::fs::read_to_string(root().join("packaging/copy-notices.sh")).unwrap();
        assert!(script.contains("packaging/notices.txt"));
        for route in [
            "packaging/linux/appimage.sh",
            "packaging/macos/package.sh",
            ".github/workflows/release.yml",
        ] {
            let text = std::fs::read_to_string(root().join(route)).unwrap();
            assert!(text.contains("copy-notices.sh"), "{route}");
            for (name, _) in FILES {
                assert!(
                    !text.contains(name),
                    "{route} names {name}: the list is the only place"
                );
            }
        }
    }

    #[test]
    fn setup_writes_and_uninstall_removes_every_notice() {
        let dir = std::env::temp_dir().join(format!("sunrise-notices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_into(&dir).unwrap();
        for (name, text) in FILES {
            assert_eq!(
                &std::fs::read_to_string(dir.join("LICENSES").join(name)).unwrap(),
                text
            );
        }
        assert!(remove_from(&dir).is_empty());
        assert!(!dir.join("LICENSES").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
