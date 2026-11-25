//! What a plan refuses before the cutoff or the seal: clashing, non-portable or unowned destinations and files that can't be replaced.

use super::*;

#[test]
fn composed_and_decomposed_destinations_cannot_enter_one_plan() {
    let composed = PathBuf::from("sunrise-mods/café.smod");
    let decomposed = PathBuf::from("sunrise-mods/cafe\u{301}.smod");
    assert_ne!(composed, decomposed);
    for existing in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        initialized(home, existing);
        if existing {
            fs::write(home.join(&composed), package("1.0.0")).unwrap();
            // On normalization-insensitive volumes the second spelling can already open
            // the first file. On other volumes it is a prospective across-volume alias.
            if let Some(other) = Identity::file_at(&home.join(&decomposed)).unwrap() {
                assert!(Identity::file_at(&home.join(&composed)).unwrap() == Some(other));
            }
        }
        let mut owner = Admission::open(home, None).unwrap();
        let stage = owner.reserve(Intent::Install, BTreeSet::new()).unwrap();
        let one = stage.path().join("one");
        let two = stage.path().join("two");
        fs::write(&one, package("2.0.0")).unwrap();
        fs::write(&two, package("3.0.0")).unwrap();
        assert!(stage
            .seal(
                INSTALLED.to_vec(),
                &[
                    (composed.clone(), Some(one)),
                    (decomposed.clone(), Some(two))
                ]
            )
            .is_err());
        assert_eq!(fs::read(home.join(POINTER)).unwrap(), OLD);
        if existing {
            assert_eq!(fs::read(home.join(&composed)).unwrap(), package("1.0.0"));
        } else {
            assert!(!home.join("sunrise-mods").exists());
        }
    }
}

#[test]
fn absent_nonportable_leaves_and_ascii_case_aliases_are_refused_before_cutoff() {
    for names in [
        vec!["lan\u{034f}tern.smod"], // Default ignorable, not just a non-ASCII letter.
        vec!["LANTER~1.smod"],
        vec!["CONOUT$.smod"],
        vec!["lantern.smod", "Lantern.smod"],
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        initialized(home, false);
        let mut owner = Admission::open(home, None).unwrap();
        let stage = owner.reserve(Intent::Install, BTreeSet::new()).unwrap();
        let changes = names
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                let source = stage.path().join(i.to_string());
                fs::write(&source, package("2.0.0")).unwrap();
                (Path::new("sunrise-mods").join(name), Some(source))
            })
            .collect::<Vec<_>>();
        assert!(stage.seal(INSTALLED.to_vec(), &changes).is_err());
        assert!(!home.join("sunrise-mods").exists());
        assert_eq!(fs::read(home.join(POINTER)).unwrap(), OLD);
    }
}

#[test]
fn different_basenames_that_gain_the_same_file_identity_are_refused_before_cutoff() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let first = home.join(PACK);
    let second = PathBuf::from("sunrise-mods/second.smod");
    fs::write(home.join(&second), package("1.0.0")).unwrap();
    let mut owner = Admission::open(home, None).unwrap();
    let stage = owner.reserve(Intent::Install, BTreeSet::new()).unwrap();
    let sealed = stage
        .seal(
            INSTALLED.to_vec(),
            &[(PACK.into(), None), (second.clone(), None)],
        )
        .unwrap();
    // Preserve both before-hashes but alias the second entry just before publish.
    fs::remove_file(home.join(&second)).unwrap();
    fs::hard_link(&first, home.join(&second)).unwrap();
    assert!(sealed
        .publish::<()>(|| panic!("alias must be refused before cutoff"))
        .is_err());
    assert_eq!(fs::read(&first).unwrap(), package("1.0.0"));
    assert_eq!(fs::read(home.join(&second)).unwrap(), package("1.0.0"));
    assert_eq!(fs::read(home.join(POINTER)).unwrap(), OLD);
}

#[test]
fn unowned_bytes_and_unknown_records_are_never_replaced_or_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, false);
    fs::write(home.join(RECORD), b"{\"version\":1}").unwrap();
    assert!(Admission::open(home, None).is_err());
    assert_eq!(fs::read(home.join(RECORD)).unwrap(), b"{\"version\":1}");
    fs::remove_file(home.join(RECORD)).unwrap();
    let mut admission = Admission::open(home, None).unwrap();
    let sealed = seal(&mut admission, true);
    fs::create_dir(home.join("sunrise-mods")).unwrap();
    fs::write(home.join(PACK), package("9.0.0")).unwrap();
    assert!(sealed
        .publish::<()>(|| panic!("occupied target must be rejected before cutoff"))
        .is_err());
    assert_eq!(fs::read(home.join(PACK)).unwrap(), package("9.0.0"));
    assert_eq!(fs::read(home.join(POINTER)).unwrap(), OLD);
    let staging = admission.reserve(Intent::Install, BTreeSet::new()).unwrap();
    let outside = home.join("outside");
    fs::write(&outside, b"keep").unwrap();
    assert!(staging
        .seal(
            INSTALLED.to_vec(),
            &[(("../outside").into(), Some(outside.clone()))]
        )
        .is_err());
    assert_eq!(fs::read(outside).unwrap(), b"keep");
}

#[cfg(windows)]
#[test]
fn a_read_only_file_that_cannot_be_replaced_is_refused_before_seal() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    initialized(home, true);
    let pointer = home.join(POINTER);
    let original = fs::metadata(&pointer).unwrap().permissions();
    let mut restricted = original.clone();
    restricted.set_readonly(true);
    fs::set_permissions(&pointer, restricted).unwrap();
    let mut owner = Admission::open(home, None).unwrap();
    assert!(owner.reserve(Intent::Install, BTreeSet::new()).is_err());
    assert!(!home.join(RECORD).exists());
    pair(home, false, false);
    fs::set_permissions(pointer, original).unwrap();
}

#[cfg(windows)]
#[test]
fn an_existing_named_data_stream_is_refused_before_seal_and_left_intact() {
    for zone in [&b"[ZoneTransfer]\r\nZoneId=3\r\n"[..], &b""[..]] {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        initialized(home, true);
        let mut stream = home.join(PACK).into_os_string();
        stream.push(":Zone.Identifier");
        fs::write(&stream, zone).unwrap();
        let mut owner = Admission::open(home, None).unwrap();
        let stage = owner.reserve(Intent::Install, BTreeSet::new()).unwrap();
        let source = stage.path().join("replace.smod");
        fs::write(&source, package("2.0.0")).unwrap();
        assert_eq!(
            stage
                .seal(INSTALLED.to_vec(), &[(PACK.into(), Some(source))])
                .err(),
            Some(at(
                &home.join(PACK),
                io::Error::from(io::ErrorKind::Unsupported)
            ))
        );
        pair(home, false, false);
        assert_eq!(fs::read(&stream).unwrap(), zone);
    }
}
