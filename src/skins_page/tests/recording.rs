//! Recording a confirmed change: when a later profile shows it, when it is still awaited, and accounts.json.

use super::*;

#[test]
fn a_change_a_later_profile_shows_is_recorded_and_not_sent_again() {
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c2")));
    unconfirmed_save(&mut skins);
    // The next reading shows the cape and the skin, with the arms sent; the skin's picture
    // is still on its way.
    read(&mut skins, profile(NEW, true, Some("c1")));
    assert_eq!(
        skins.confirmed.cape,
        Some(Some("http://textures.minecraft.net/texture/c1".into()))
    );
    assert_eq!(
        skins.confirmed.skin, None,
        "not before its picture is checked"
    );
    skins.pictures.insert(NEW.into(), Picture::Pending);
    skins.picture(NEW.into(), Some(skin_png(5)));
    assert_eq!(
        skins.confirmed.skin,
        Some(Some((NEW.into(), "slim".into()))),
        "the picture served is the one uploaded"
    );
    assert!(skins.submit(false).is_none(), "nothing sent again");
}

#[test]
fn a_later_profile_that_doesnt_show_the_change_records_nothing() {
    // Another skin at a new URL, or the arms not sent, and no cape: something else is worn,
    // so both are dropped and nothing is recorded.
    for (url, slim, png) in [(NEW, true, skin_png(6)), (NEW, false, skin_png(5))] {
        let mut skins = Skins::new_for("a");
        read(&mut skins, profile(SKIN, false, Some("c2")));
        unconfirmed_save(&mut skins);
        read(&mut skins, profile(url, slim, None));
        skins.picture(url.into(), Some(png));
        assert_eq!(skins.confirmed, look::Record::default());
        assert_eq!(
            skins.awaited,
            Awaited::default(),
            "the profile is the truth"
        );
    }
    // A Save that changes the cape again: the earlier one's cape is not recorded after it.
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c2")));
    unconfirmed_save(&mut skins);
    choose(&mut skins, |edit| edit.cape = Some(CapePick::None));
    skins.submit(false).unwrap();
    assert_eq!(skins.awaited.cape, None);
}

#[test]
fn a_profile_still_behind_keeps_the_change_awaited_for_a_few_readings() {
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c2")));
    unconfirmed_save(&mut skins);
    // Readings that still show what was worn before: behind, kept.
    for _ in 1..AWAITED_READINGS {
        read(&mut skins, profile(SKIN, false, Some("c2")));
        assert!(skins.awaited.skin.is_some() && skins.awaited.cape.is_some());
    }
    // The next shows both: recorded.
    read(&mut skins, profile(NEW, true, Some("c1")));
    skins.picture(NEW.into(), Some(skin_png(5)));
    assert_eq!(
        skins.confirmed.cape,
        Some(Some("http://textures.minecraft.net/texture/c1".into()))
    );
    assert_eq!(
        skins.confirmed.skin,
        Some(Some((NEW.into(), "slim".into())))
    );
    // Behind for every reading allowed: dropped after the last.
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c2")));
    unconfirmed_save(&mut skins);
    for _ in 0..AWAITED_READINGS {
        read(&mut skins, profile(SKIN, false, Some("c2")));
    }
    assert_eq!(skins.awaited, Awaited::default());
    assert_eq!(skins.confirmed, look::Record::default());
}

#[test]
fn a_picture_that_couldnt_be_had_leaves_the_skin_awaited() {
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c2")));
    unconfirmed_save(&mut skins);
    read(&mut skins, profile(NEW, true, Some("c2")));
    skins.pictures.insert(NEW.into(), Picture::Pending);
    skins.picture(NEW.into(), None);
    assert!(
        skins.awaited.skin.is_some(),
        "kept: the picture is asked again"
    );
    assert_eq!(skins.confirmed.skin, None);
    // The next reading names it again, and this time its picture is had.
    read(&mut skins, profile(NEW, true, Some("c2")));
    skins.picture(NEW.into(), Some(skin_png(5)));
    assert_eq!(
        skins.confirmed.skin,
        Some(Some((NEW.into(), "slim".into())))
    );
}

#[test]
fn a_save_waits_for_what_an_earlier_one_confirmed_to_be_recorded() {
    let mut skins = Skins::new_for("a");
    skins.recording = 1;
    choose(&mut skins, |edit| edit.cape = Some(CapePick::None));
    assert!(skins.submit(false).is_none());
    skins.recording = 0;
    assert!(skins.submit(false).is_some());
}

#[test]
fn a_confirmed_change_is_written_to_accounts_json_and_shown() {
    in_a_scratch_launcher(
        "skins_page::tests::a_confirmed_change_is_written_to_accounts_json_and_shown",
        |l, _| {
            l.skins.offline_for_test = Some(false);
            l.accounts.save().unwrap();
            l.skins.show(UUID, |_| true);
            l.skins.profile = Some(Ok(profile(SKIN, false, Some("c2"))));
            pick(&mut l.skins, SkinPick::Default("alex"));
            l.skins.edit.cape = Some(CapePick::Minecraft("c1".into()));
            let save = l.skins.submit(false).unwrap();
            let mut stale = outcome(
                Some(Change::Unconfirmed("old".into())),
                Some(Change::Unconfirmed("old".into())),
                Some(profile(SKIN, false, Some("c2"))),
            );
            stale.uploaded = Some((skin_library::id_of_png(&skin_png(5)).unwrap(), true));
            l.look_saved(UUID.into(), save.id, stale);
            assert_eq!(l.accounts.accounts[0].skin, None, "not confirmed yet");
            // The picture at the new URL is here; the next reading shows both changes.
            l.skins.reading = None;
            l.skins
                .pictures
                .insert(NEW.into(), Picture::Ready(skin_png(5)));
            let reading = l.skins.read_asked();
            l.skins_profile(UUID.into(), reading, Ok(profile(NEW, true, Some("c1"))));
            assert_eq!(l.skins.recording, 1, "being recorded");
            assert!(l.skins.writing());
            assert_eq!(
                l.accounts.accounts[0].skin, None,
                "the chip waits until it is written"
            );
            let Msg::LookRecorded(uuid, record, recorded) =
                next(l, |msg| matches!(msg, Msg::LookRecorded(..)))
            else {
                unreachable!()
            };
            l.look_recorded(uuid, record, recorded);
            assert_eq!(l.skins.recording, 0);
            assert_eq!(
                l.accounts.accounts[0].skin,
                Some((NEW.into(), "slim".into())),
                "the chip shows it"
            );
            let on_disk = account::Accounts::load();
            assert_eq!(on_disk.accounts[0].skin, Some((NEW.into(), "slim".into())));
            assert_eq!(
                on_disk.accounts[0].cape.as_deref(),
                Some("http://textures.minecraft.net/texture/c1")
            );
            assert!(l.skins.edit.is_empty() && l.skins.submit(false).is_none());
        },
    );
}

#[test]
fn a_confirmed_change_the_queue_wont_take_stays_to_record() {
    in_a_scratch_launcher(
        "skins_page::tests::a_confirmed_change_the_queue_wont_take_stays_to_record",
        |l, _| {
            l.skins.show(UUID, |_| true);
            // The writes queue is full: a job holds its thread, the rest fill it.
            let (started, running) = std::sync::mpsc::channel::<()>();
            let (release, held) = std::sync::mpsc::channel::<()>();
            l.skins
                .writes
                .push(
                    "test",
                    Box::new(move || {
                        let _ = started.send(());
                        let _ = held.recv();
                    }),
                )
                .unwrap();
            running.recv().unwrap();
            while l.skins.writes.push("test", Box::new(|| {})).is_ok() {}
            l.skins.confirmed.cape = Some(None);
            l.record_confirmed(UUID);
            assert_eq!(l.skins.confirmed.cape, Some(None), "kept, to try again");
            assert_eq!(l.skins.recording, 0);
            release.send(()).unwrap();
        },
    );
}

#[test]
fn a_skin_kept_after_a_save_gave_it_other_arms_keeps_them() {
    in_a_scratch_launcher(
        "skins_page::tests::a_skin_kept_after_a_save_gave_it_other_arms_keeps_them",
        |l, _| {
            l.skins.offline_for_test = Some(false);
            let dir = skin_library::folder(UUID).unwrap();
            // An arm-only Save gave the worn skin slim arms ...
            let saved = skin_library::add(&dir, &skin_png(3), "", Some(true)).unwrap();
            // ... and a keep queued before it ends after it, with the classic arms the page
            // saw then.
            l.skins.show(UUID, |_| true);
            l.skins.profile = Some(Ok(profile(SKIN, false, None)));
            l.skins
                .pictures
                .insert(SKIN.into(), Picture::Ready(skin_png(3)));
            l.remember_worn();
            let Msg::SkinKept(uuid, url, kept, listed) =
                next(l, |msg| matches!(msg, Msg::SkinKept(..)))
            else {
                unreachable!()
            };
            l.skins_kept(uuid, url, kept, listed);
            let listed = skin_library::list(&dir).unwrap();
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].id, saved.id);
            assert!(listed[0].slim, "the arms the Save gave it");
        },
    );
}

#[test]
fn a_failed_texture_fetch_then_a_successful_one_records_the_new_skin() {
    in_a_scratch_launcher(
        "skins_page::tests::a_failed_texture_fetch_then_a_successful_one_records_the_new_skin",
        |l, _| {
            l.skins.offline_for_test = Some(false);
            l.accounts.save().unwrap();
            l.skins.show(UUID, |_| true);
            l.skins.profile = Some(Ok(profile(SKIN, false, None)));
            pick(&mut l.skins, SkinPick::Default("alex"));
            let save = l.skins.submit(false).unwrap();
            let uploaded_png = skin_png(7);
            let mut unconfirmed = outcome(
                Some(Change::Unconfirmed("couldn't check".into())),
                None,
                // Mojang already returned the new skin URL, only fetching its PNG failed.
                Some(profile(NEW, true, None)),
            );
            unconfirmed.uploaded = Some((skin_library::id_of_png(&uploaded_png).unwrap(), true));
            l.look_saved(UUID.into(), save.id, unconfirmed);
            assert_eq!(
                l.skins.awaited.skin_before,
                Some((SKIN.into(), "classic".into())),
                "keeps the pre-upload skin, not the post-upload URL"
            );

            // First texture fetch at NEW fails.
            l.skins_picture(NEW.into(), None);
            assert_eq!(l.skins.confirmed.skin, None);

            // Later profile read returns the same NEW profile; then the PNG fetch succeeds.
            l.skins.reading = None;
            let reading = l.skins.read_asked();
            l.skins_profile(UUID.into(), reading, Ok(profile(NEW, true, None)));
            assert_eq!(
                l.skins.awaited.skin_at,
                Some((NEW.into(), "slim".into())),
                "NEW is not mistaken for the pre-upload skin"
            );
            l.skins_picture(NEW.into(), Some(uploaded_png));
            let Msg::LookRecorded(uuid, record, recorded) =
                next(l, |msg| matches!(msg, Msg::LookRecorded(..)))
            else {
                unreachable!()
            };
            l.look_recorded(uuid, record, recorded);
            assert_eq!(
                account::Accounts::load().accounts[0].skin,
                Some((NEW.into(), "slim".into())),
                "new skin recorded without uploading again"
            );
        },
    );
}

#[test]
fn a_newer_accepted_save_supersedes_a_pending_confirmed_record() {
    in_a_scratch_launcher(
        "skins_page::tests::a_newer_accepted_save_supersedes_a_pending_confirmed_record",
        |l, _| {
            l.skins.offline_for_test = Some(false);
            l.accounts.save().unwrap();
            l.skins.show(UUID, |_| true);
            l.skins.profile = Some(Ok(profile(SKIN, false, Some("c1"))));

            // Fill the write queue so an older confirmed cape record stays pending.
            let (release, held) = std::sync::mpsc::channel::<()>();
            l.skins
                .writes
                .push(
                    "test",
                    Box::new(move || {
                        let _ = held.recv();
                    }),
                )
                .unwrap();
            while l.skins.writes.push("test", Box::new(|| {})).is_ok() {}
            l.skins.confirmed.cape = Some(Some("http://textures.minecraft.net/texture/c1".into()));
            l.record_confirmed(UUID);
            assert!(l.skins.confirmed.cape.is_some());
            assert_eq!(l.skins.recording, 0);

            // Release the queue before the newer Save lands, and simulate the newer Save
            // writing cape c2 to accounts.json directly (as look::save does on Change::Done).
            release.send(()).unwrap();
            let newer_cape = Some("http://textures.minecraft.net/texture/c2".to_string());
            account::Accounts::save_look(UUID, None, Some(newer_cape.clone())).unwrap();

            l.skins.edit.cape = Some(CapePick::Minecraft("c2".into()));
            let save = l.skins.submit(false).unwrap();
            let done = outcome(
                None,
                Some(Change::Done),
                Some(profile(SKIN, false, Some("c2"))),
            );
            l.look_saved(UUID.into(), save.id, done);
            assert_eq!(
                l.skins.confirmed.cape, None,
                "superseded by the newer accepted Save"
            );
            assert_eq!(l.skins.recording, 0);

            // Drain the queue by waiting on a sentinel job on the same worker. The worker
            // was just released above, so wait briefly for a slot if the queue is still full.
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let tx = done_tx.clone();
                match l.skins.writes.push(
                    "drain",
                    Box::new(move || {
                        let _ = tx.send(());
                    }),
                ) {
                    Ok(()) => break,
                    Err(_) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(err) => panic!("write queue never drained: {err:?}"),
                }
            }
            done_rx.recv().unwrap();

            assert_eq!(
                account::Accounts::load().accounts[0].cape,
                newer_cape,
                "newer cape stays in accounts.json after the queue drains"
            );
        },
    );
}
