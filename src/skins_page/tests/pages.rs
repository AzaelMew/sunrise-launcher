//! One page per account, offline mode, and what a save holds off: data moves, restarts and switching profiles.

use super::*;

#[test]
fn a_data_move_waits_for_skins_being_written() {
    let mut pages = SkinsPages::default();
    pages.show("a", |_| true);
    assert!(!pages.writing());
    pages.disk_writes += 1;
    assert!(pages.writing(), "an import or a kept skin");
    pages.disk_writes -= 1;
    pick(&mut pages, SkinPick::Default("alex"));
    pages.submit(false).unwrap();
    assert!(pages.writing(), "a Save");
}

#[test]
fn a_page_is_forgotten_with_its_account_but_not_during_a_save() {
    let mut pages = SkinsPages::default();
    pages.show("a", |_| true);
    pick(&mut pages, SkinPick::Default("alex"));
    pages.submit(false).unwrap();
    pages.show("b", |_| true);
    pages.show("c", |uuid| uuid != "a");
    assert!(
        pages.page_mut("a").is_some(),
        "its Save still to be answered"
    );
    pages.page_mut("a").unwrap().saving = None;
    pages.show("b", |uuid| uuid != "a");
    assert!(pages.page_mut("a").is_none());
    assert!(pages.page_mut("c").is_some());
}

#[test]
fn offline_now_the_page_takes_no_answer_asks_nothing_and_sends_nothing() {
    // Same account: a reading asked online, answered once the offline view is shown.
    let mut skins = Skins::new_for("a");
    let reading = skins.read_asked();
    assert!(!skins.read_answered("a", reading, Ok(profile(SKIN, false, Some("c1"))), true));
    assert_eq!(skins.profile, None);
    assert!(
        !skins.ask_picture(SKIN, true),
        "no texture requests offline"
    );
    skins.edit.skin = Some(SkinPick::Default("alex"));
    assert!(skins.submit(true).is_none(), "no edits are sent");
    // Online again, all of it works.
    assert!(skins.ask_picture(SKIN, false));
    assert!(skins.submit(false).is_some());
}

#[test]
fn a_save_on_any_page_holds_off_a_data_move_and_a_launcher_restart() {
    in_a_scratch_launcher(
        "skins_page::tests::a_save_on_any_page_holds_off_a_data_move_and_a_launcher_restart",
        |l, _| {
            let before = (l.location_busy(), l.launcher_update_wait());
            l.skins.show(UUID, |_| true);
            l.skins.edit.cape = Some(CapePick::None);
            l.skins.submit(false).unwrap();
            // Another account's page shown: the Save is still under way.
            l.skins.show("b09600dff4354d4fb56776eec1e55083", |_| true);
            assert!(l.location_busy());
            assert_eq!(
                l.launcher_update_wait(),
                Some(crate::launcher_update::Wait::Work)
            );
            l.skins.page_mut(UUID).unwrap().saving = None;
            assert_eq!((l.location_busy(), l.launcher_update_wait()), before);
            l.skins.disk_writes += 1;
            assert!(l.location_busy(), "a saved skin being written");
        },
    );
}

#[test]
fn switching_the_profiles_build_while_skins_is_shown_works_the_offline_mode_out_again() {
    in_a_scratch_launcher(
        "skins_page::tests::switching_the_profiles_build_while_skins_is_shown_works_the_offline_mode_out_again",
        |l, _| {
            l.installed = vec!["developer-a".into(), "26.3-0.1.20261101".into()];
            let pin = |l: &mut Launcher, tag: &str| {
                let id = l.selected_instance().unwrap().id.clone();
                let instance = l.instances.list.iter_mut().find(|i| i.id == id).unwrap();
                instance.version = Some(tag.into());
            };
            pin(l, "developer-a");
            assert_eq!(l.selected().map(String::as_str), Some("developer-a"));
            assert!(l.offline_cached(|_| true), "worked out");
            assert!(l.offline_cached(|_| false), "kept while nothing changed");
            // The profile now plays another build: the mode is worked out again.
            pin(l, "26.3-0.1.20261101");
            assert!(!l.offline_cached(|_| false));
            pin(l, "developer-a");
            assert!(l.offline_cached(|_| true));
            // The played build's install record changes (reinstalled, removed).
            let dir = backend::version_dir("developer-a");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(".installed"), "").unwrap();
            assert!(!l.offline_cached(|_| false));
            // An imported build: its base's records count, read through its own record.
            let imported = "imported-base-test";
            let own = backend::version_dir(imported);
            std::fs::create_dir_all(&own).unwrap();
            std::fs::write(
                own.join("imported.json"),
                r#"{"base": "developer-b", "file": "x.jar"}"#,
            )
            .unwrap();
            l.installed.push(imported.into());
            pin(l, imported);
            assert_eq!(l.selected().map(String::as_str), Some(imported));
            assert!(l.offline_cached(|_| true));
            assert!(l.offline_cached(|_| false), "kept");
            // The base reinstalled under the same tag.
            let base = backend::version_dir("developer-b");
            std::fs::create_dir_all(&base).unwrap();
            std::fs::write(base.join(".installed"), "").unwrap();
            assert!(!l.offline_cached(|_| false), "worked out again");
        },
    );
}

#[test]
fn offline_now_a_save_that_ends_starts_nothing_and_choices_are_ignored() {
    in_a_scratch_launcher(
        "skins_page::tests::offline_now_a_save_that_ends_starts_nothing_and_choices_are_ignored",
        |l, _| {
            l.skins.offline_for_test = Some(false);
            l.skins.show(UUID, |_| true);
            l.skins.edit.skin = Some(SkinPick::Default("alex"));
            let save = l.skins.submit(false).unwrap();
            // The offline developer view comes on while the Save runs.
            l.skins.offline_for_test = Some(true);
            let mut done = outcome(
                Some(Change::Done),
                None,
                Some(Profile {
                    id: UUID.into(),
                    ..profile(ALEX, false, Some("c1"))
                }),
            );
            done.kept = kept(SKIN);
            l.look_saved(UUID.into(), save.id, done);
            assert!(l.skins.saving.is_none(), "reconciled");
            assert_eq!(
                l.accounts.accounts[0]
                    .skin
                    .as_ref()
                    .map(|(u, _)| u.as_str()),
                Some(ALEX),
                "recorded"
            );
            assert!(l.skins.heads_stale, "no profile head fetched offline");
            assert!(l.skins.reading.is_none(), "no profile read");
            assert!(l.skins.pictures.is_empty(), "no picture asked for");
            assert!(l.skins.keeping.is_empty(), "no skin kept");
            assert!(l.skins.writes.jobs.is_none(), "no saved skins listed");
            // A reading answered, a choice and a Save: none of them do anything offline.
            let reading = l.skins.read_asked();
            l.skins_profile(UUID.into(), reading, Ok(profile(SKIN, false, None)));
            assert!(l.skins.pictures.is_empty());
            l.choose(|edit| edit.cape = Some(CapePick::None));
            assert!(l.skins.edit.is_empty());
            l.skins.edit.cape = Some(CapePick::None);
            l.look_save();
            assert!(l.skins.saving.is_none(), "nothing sent");
            l.skins_add_for(UUID.into(), vec![PathBuf::from("x.png")]);
            assert_eq!(l.skins.disk_writes, 0, "nothing added");
        },
    );
}

#[test]
fn a_reading_for_an_account_not_shown_settles_on_its_own_page() {
    in_a_scratch_launcher(
        "skins_page::tests::a_reading_for_an_account_not_shown_settles_on_its_own_page",
        |l, _| {
            l.skins.offline_for_test = Some(false);
            l.skins.show(UUID, |_| true);
            let reading = l.skins.read_asked();
            let other = "b09600dff4354d4fb56776eec1e55083";
            l.skins.show(other, |_| true);
            // The page shown has pictures of its own not asked for yet: an answer for another
            // account starts nothing for it either.
            l.skins.profile = Some(Ok(profile(ALEX, false, None)));
            l.skins_profile(UUID.into(), reading, Ok(profile(SKIN, false, Some("c1"))));
            let page = l.skins.page_mut(UUID).unwrap();
            assert_eq!(page.reading, None, "answered: the next opening reads again");
            assert!(matches!(page.profile, Some(Ok(_))));
            assert!(
                page.pictures.is_empty(),
                "nothing asked for a page not shown"
            );
            assert_eq!(
                l.skins.profile,
                Some(Ok(profile(ALEX, false, None))),
                "the page shown is left alone"
            );
            assert!(l.skins.pictures.is_empty(), "and asks for nothing");
            // In the offline view an answer is not taken in, but the reading is over.
            l.skins.offline_for_test = Some(true);
            l.skins.show(UUID, |_| true);
            let reading = l.skins.read_asked();
            l.skins_profile(UUID.into(), reading, Ok(profile(ALEX, false, None)));
            assert_eq!(l.skins.reading, None);
        },
    );
}

#[test]
fn a_restart_waits_for_a_skin_being_written() {
    in_a_scratch_launcher(
        "skins_page::tests::a_restart_waits_for_a_skin_being_written",
        |l, cx| {
            assert!(l.restart_blocked_by_skins().is_none());
            // A data folder that can't be retried or given up, so neither path can restart
            // this test even if it didn't wait: the pointer is damaged, and its backup's
            // place is taken by a folder that can't be removed as a file.
            let pointer =
                PathBuf::from(std::env::var_os("SUNRISE_LAUNCHER_LOCATION_FILE").unwrap());
            std::fs::write(&pointer, "{ damaged").unwrap();
            let backup = pointer.with_file_name("location.json.old");
            std::fs::create_dir_all(backup.join("kept")).unwrap();
            l.skins.disk_writes += 1;
            let wait = l.restart_blocked_by_skins().unwrap();
            l.retry_location(cx);
            assert_eq!(
                l.location_error.as_deref(),
                Some(wait.as_str()),
                "Retry waits"
            );
            l.location_error = None;
            l.use_default_location(cx);
            assert_eq!(
                l.location_error.as_deref(),
                Some(wait.as_str()),
                "Use the default folder waits"
            );
            assert!(pointer.is_file(), "the pointer left as it was");
        },
    );
}

#[test]
fn switching_profiles_from_the_rail_refreshes_the_texture_offline_flag() {
    in_a_scratch_launcher(
        "skins_page::tests::switching_profiles_from_the_rail_refreshes_the_texture_offline_flag",
        |l, cx| {
            static FETCHED: AtomicU64 = AtomicU64::new(0);
            fn fetch(_: &str) -> Option<Vec<u8>> {
                FETCHED.fetch_add(1, Ordering::Relaxed);
                Some(vec![1])
            }
            l.skins.offline_for_test = Some(false);
            assert!(!l.skins_offline());
            assert!(!l.skins.offline_flag.load(Ordering::Relaxed));

            // Queue a texture job while online.
            let (tx, rx) = std::sync::mpsc::channel();
            let job = texture_job(tx, SKIN.into(), l.skins.offline_flag.clone(), fetch);

            // Switch from the rail to another profile whose build is in offline mode.
            // Switching from the rail opens Home, not Skins, yet must refresh the flag.
            let other = instances::create("Offline", None).unwrap();
            l.reload_instances();
            l.skins.offline_for_test = Some(true);
            l.switch_profile(&other, cx);
            assert!(matches!(l.ui.page, crate::frame::Page::Home));
            assert!(
                l.skins.offline_flag.load(Ordering::Relaxed),
                "offline flag refreshed on profile switch outside Skins"
            );

            // Running the queued job now makes no online request.
            job();
            assert!(matches!(rx.recv().unwrap(), Msg::Picture(url, None) if url == SKIN));
            assert_eq!(FETCHED.load(Ordering::Relaxed), 0);
        },
    );
}
