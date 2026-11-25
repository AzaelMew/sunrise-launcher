//! The skins on disk: reading files once, imports, regular files only, the latest listing.

use super::*;

#[test]
fn a_file_is_read_once_and_only_the_latest_read_of_it_counts() {
    let mut pages = SkinsPages::default();
    pages.show("a", |_| true);
    let path = || Some(PathBuf::from("x.png"));
    let asked = |pages: &SkinsPages, key: &str| {
        pages
            .wanted
            .borrow()
            .iter()
            .rev()
            .find(|(k, _, _)| k == key)
            .map(|(_, _, id)| *id)
            .unwrap()
    };
    assert!(matches!(pages.file("saved:x", path), Png::Pending));
    assert!(matches!(pages.file("saved:x", path), Png::Pending));
    assert_eq!(pages.wanted.borrow().len(), 1, "read once");
    assert!(matches!(pages.file("saved:none", || None), Png::Missing));
    let x = asked(&pages, "saved:x");
    pages.file_read("saved:x", x, Some(vec![7]));
    assert!(matches!(pages.file("saved:x", path), Png::Ready(png) if *png == [7]));
    // One that couldn't be read is missing, and read again once forgotten.
    assert!(matches!(pages.file("saved:y", path), Png::Pending));
    let first = asked(&pages, "saved:y");
    pages.file_read("saved:y", first, None);
    assert!(matches!(pages.file("saved:y", path), Png::Missing));
    // Forgotten while a read is under way (its picture written anew): the read from before
    // is no answer to the one after.
    pages.forget_file("saved:y");
    assert!(matches!(pages.file("saved:y", path), Png::Pending));
    let stale = asked(&pages, "saved:y");
    pages.forget_file("saved:y");
    assert!(matches!(pages.file("saved:y", path), Png::Pending));
    let latest = asked(&pages, "saved:y");
    pages.file_read("saved:y", stale, Some(vec![1]));
    assert!(matches!(pages.file("saved:y", path), Png::Pending));
    pages.file_read("saved:y", latest, Some(vec![2]));
    assert!(matches!(pages.file("saved:y", path), Png::Ready(png) if *png == [2]));
    // Each account's saved skins are its own files.
    assert_ne!(saved_key(Some("a"), "x"), saved_key(Some("b"), "x"));
}

#[test]
fn a_failed_import_stays_beside_the_skin_added_with_it() {
    let mut skins = Skins::new_for("a");
    let good = Saved {
        id: KEPT.into(),
        name: "Knight".into(),
        slim: false,
    };
    let last = skins.files_added(vec![
        Err("big.png couldn't be added: too big".into()),
        Ok(good.clone()),
    ]);
    assert_eq!(last, Some(good.clone()));
    // The good one is chosen, as `Launcher::skins_added` does: the failure stays.
    choose(&mut skins, |edit| {
        edit.skin = Some(SkinPick::Saved(good.id))
    });
    assert_eq!(skins.added, ["big.png couldn't be added: too big"]);
    // The next import starts afresh.
    assert_eq!(skins.files_added(Vec::new()), None);
    assert!(skins.added.is_empty());
}

#[test]
fn only_regular_files_are_read_as_skins() {
    let dir = tempfile::tempdir().unwrap();
    let added = add_files(
        "069a79f444e94726a5befca90e38aaf5",
        &[dir.path().to_path_buf()],
    );
    let why = added[0].clone().unwrap_err();
    let name = dir.path().file_name().unwrap().to_string_lossy();
    assert!(why.contains(name.as_ref()), "named: {why}");
    assert!(add_files("not-a-uuid", &[dir.path().join("x.png")])[0].is_err());
}

#[test]
fn only_the_latest_listing_counts_and_a_failed_keep_stays_shown() {
    let mut skins = Skins::new_for("a");
    let one = Saved {
        id: KEPT.into(),
        name: String::new(),
        slim: false,
    };
    skins.listing((5, Ok(vec![one.clone()])));
    skins.listing((3, Ok(Vec::new())));
    assert_eq!(
        skins.saved,
        std::slice::from_ref(&one),
        "an older listing changes nothing"
    );
    skins.keep_error = Some("disk full".into());
    skins.listing((6, Err("unreadable".into())));
    assert_eq!(skins.library_error.as_deref(), Some("unreadable"));
    skins.listing((7, Ok(vec![one])));
    assert_eq!(skins.library_error, None);
    assert_eq!(
        skins.keep_error.as_deref(),
        Some("disk full"),
        "a listing that works doesn't hide a keep that didn't"
    );
}
