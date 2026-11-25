use super::*;
use std::path::{Path, PathBuf};

/// `SUNRISE_LAUNCHER_HOME` is the launch script's (and every test's) way to keep the launcher away
/// from the player's folder; `data_dir` must read the override set in this process, every call.
#[test]
fn the_launcher_home_override_is_read_in_this_process() {
    let previous = std::env::var_os("SUNRISE_LAUNCHER_HOME");
    let home = tempfile::tempdir().expect("a scratch home");
    std::env::set_var("SUNRISE_LAUNCHER_HOME", home.path());
    let resolved = data_dir();
    let production_path = resolve_data_dir();
    if let Some(previous) = previous {
        std::env::set_var("SUNRISE_LAUNCHER_HOME", previous);
    } else {
        std::env::remove_var("SUNRISE_LAUNCHER_HOME");
    }
    assert_eq!(resolved, home.path().to_path_buf());
    assert_eq!(production_path, home.path().to_path_buf());
}

#[test]
fn with_every_name_taken_the_fallback_is_never_a_claimed_folder() {
    let temp = Path::new("/scratch/tmp");
    let short = Some(PathBuf::from("C:/PROGRA~3/ok"));
    assert_eq!(
        build_fallback(short, 7, temp),
        PathBuf::from("C:/PROGRA~3/ok")
    );
    // No short name, or one that is no better than the long one.
    let expected = temp.join("sunrise-launcher-7");
    assert_eq!(build_fallback(None, 7, temp), expected);
    assert_eq!(
        build_fallback(Some(PathBuf::from("C:/Users/Ж/x")), 7, temp),
        expected
    );
}

#[test]
fn build_folder_candidates_try_the_base_then_the_numbered_alternatives() {
    let base = Path::new("C:/ProgramData/sunrise-launcher-1a2b3c4d");
    let mut candidates = build_dir_candidates(base);
    assert_eq!(candidates.next(), Some(base.to_path_buf()));
    for n in 2..=9 {
        assert_eq!(
            candidates.next(),
            Some(base.with_file_name(format!("sunrise-launcher-1a2b3c4d-{n}")))
        );
    }
    assert_eq!(candidates.next(), None);
}

/// A canonical path loses Windows' verbatim prefix on a drive (horizon-mixins' output
/// check stats `\\?\C:`, the volume itself, which Windows refuses, os error 1), and keeps
/// it where there is no drive form.
#[test]
fn a_canonical_windows_path_loses_only_a_drives_verbatim_prefix() {
    let plain = |p: &str| plain(PathBuf::from(p));
    assert_eq!(
        plain(r"\\?\C:\Users\me\AppData\Roaming\sunrise-launcher\native"),
        PathBuf::from(r"C:\Users\me\AppData\Roaming\sunrise-launcher\native")
    );
    // A share has no drive form to fall back to; other paths have no prefix.
    assert_eq!(
        plain(r"\\?\UNC\server\share\native"),
        PathBuf::from(r"\\?\UNC\server\share\native")
    );
    assert_eq!(
        plain("/home/me/.local/share/sunrise-launcher/native"),
        PathBuf::from("/home/me/.local/share/sunrise-launcher/native")
    );
}

/// [#5189](https://github.com/horizon-rust/horizon/issues/5189): MinGW-w64's tools can't
/// read a non-ASCII path, so the build root is the data folder's resolved ASCII spelling —
/// which a link's target or a short (8.3) name already is.
#[test]
fn the_build_dir_is_a_path_the_tools_can_read() {
    assert!(ascii(Path::new(
        r"C:\Users\me\AppData\Roaming\sunrise-launcher"
    )));
    assert!(!ascii(Path::new(
        r"C:\Users\Кириченко Павло\AppData\Roaming\sunrise-launcher"
    )));
    assert!(!ascii(Path::new("/høme/me/sunrise-launcher")));
    // A space splits windres' include folders, which matters on Windows only.
    let spaced = Path::new(r"C:\Users\Jane Doe\AppData\Roaming\sunrise-launcher");
    assert!(ascii(spaced));
    assert_eq!(tool_safe(spaced), !cfg!(windows));
    let built = build_dir();
    assert!(ascii(&built) || cfg!(windows), "{}", built.display());
    // Resolved: the build never sees a link in the path (where the answer is the data
    // folder itself, so already resolved — Windows' other spellings aren't its own).
    if ascii(&built) {
        assert_eq!(
            built,
            built
                .canonicalize()
                .map(plain)
                .unwrap_or_else(|_| built.clone())
        );
    }
}

/// [#6009](https://github.com/horizon-rust/horizon/issues/6009): with a Cyrillic user name the
/// build folder is a real ASCII folder of ProgramData's (not a short name of the data folder,
/// which rustc resolves back to the Cyrillic one), its own for each data folder.
#[test]
fn a_cyrillic_data_folder_builds_in_an_ascii_folder_of_its_own() {
    let pavlo = Path::new(r"C:\Users\Кириченко Павло\AppData\Roaming\sunrise-launcher");
    let other = Path::new(r"C:\Users\Олена\AppData\Roaming\sunrise-launcher");
    let public = PathBuf::from(if cfg!(windows) { r"D:\Data" } else { "/data" });
    let dir = ascii_build_dir(pavlo, Some(public.clone()));
    assert!(ascii(&dir), "{}", dir.display());
    assert_eq!(dir.parent(), Some(public.as_path()));
    let name = dir.file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with("sunrise-launcher-") && name.len() == 25,
        "{name}"
    );
    assert_eq!(dir, ascii_build_dir(pavlo, Some(public.clone())), "stable");
    assert_ne!(dir, ascii_build_dir(other, Some(public)), "one per user");
    // %ProgramData% unset, relative or itself not ASCII: C:\ProgramData.
    let default = PathBuf::from(r"C:\ProgramData");
    for pd in [None, Some("ProgramData".into()), Some(r"C:\Дані".into())] {
        let dir = ascii_build_dir(pavlo, pd);
        assert_eq!(dir.parent(), Some(default.as_path()));
        assert!(ascii(&dir));
    }
}
