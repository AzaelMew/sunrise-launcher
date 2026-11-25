//! Saving a look: what is sent, what a partial or failed save leaves, and choices made while one runs.

use super::*;

#[test]
fn a_skin_picked_while_the_capes_load_sends_no_cape_change() {
    let mut skins = Skins::new_for("a");
    let id = skins.read_asked();
    pick(&mut skins, SkinPick::Default("alex"));
    assert_eq!(worn(&skins).cape, None, "the worn cape isn't known yet");
    assert!(skins.read_answered("a", id, Ok(profile(SKIN, false, Some("c2"))), false));
    let submitted = skins.submit(false).unwrap();
    assert_eq!(submitted.skin, Some(SkinPick::Default("alex")));
    assert_eq!(submitted.cape, None, "no DELETE nobody asked for");
}

#[test]
fn a_save_that_ends_before_the_reading_keeps_its_answer() {
    let mut skins = Skins::new_for("a");
    let reading = skins.read_asked();
    pick(&mut skins, SkinPick::Default("alex"));
    let submitted = skins.submit(false).unwrap();
    let after = profile(ALEX, false, Some("c2"));
    assert!(answer_save(
        &mut skins,
        submitted.id,
        &outcome(Some(Change::Done), None, Some(after.clone()))
    ));
    assert!(skins.said.iter().all(Result::is_ok), "{:?}", skins.said);
    // The reading asked before the Save says what was worn before it: it changes nothing.
    assert!(!skins.read_answered("a", reading, Ok(profile(SKIN, false, None)), false));
    assert_eq!(skins.profile, Some(Ok(after)));
    assert!(skins.edit.is_empty());
}

#[test]
fn a_reading_answered_during_a_save_leaves_it_alone() {
    let mut skins = Skins::new_for("a");
    let reading = skins.read_asked();
    skins.edit.cape = Some(CapePick::Minecraft("c1".into()));
    let submitted = skins.submit(false).unwrap();
    assert!(!skins.read_answered("a", reading, Ok(profile(SKIN, false, Some("c2"))), false));
    assert_eq!(skins.saving.as_ref().map(|s| s.id), Some(submitted.id));
    let after = profile(SKIN, false, Some("c1"));
    assert!(answer_save(
        &mut skins,
        submitted.id,
        &outcome(None, Some(Change::Done), Some(after.clone()))
    ));
    assert_eq!(skins.profile, Some(Ok(after)));
}

#[test]
fn an_explicit_no_cape_is_sent_and_an_unknown_one_is_not_taken_for_it() {
    let mut skins = Skins::new_for("a");
    choose(&mut skins, |edit| edit.cape = Some(CapePick::None));
    assert_eq!(skins.edit.cape, Some(CapePick::None), "kept while unknown");
    assert_eq!(skins.submit(false).unwrap().cape, Some(CapePick::None));
}

#[test]
fn a_partial_save_says_each_change_and_leaves_only_the_failed_one_to_do() {
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c2")));
    pick(&mut skins, SkinPick::Default("alex"));
    choose(&mut skins, |edit| {
        edit.cape = Some(CapePick::Minecraft("c1".into()))
    });
    let submitted = skins.submit(false).unwrap();
    let after = profile(ALEX, false, Some("c2"));
    answer_save(
        &mut skins,
        submitted.id,
        &outcome(
            Some(Change::Done),
            Some(Change::Failed("Too many cape changes.".into())),
            Some(after.clone()),
        ),
    );
    assert_eq!(skins.said.len(), 2, "{:?}", skins.said);
    assert!(skins.said[0].is_ok(), "the skin: {:?}", skins.said[0]);
    let cape = skins.said[1].clone().unwrap_err();
    assert!(cape.contains("Too many cape changes."), "{cape}");
    assert_eq!(
        skins.profile,
        Some(Ok(after)),
        "the new skin is the baseline"
    );
    assert_eq!(skins.edit.skin, None, "the skin is done: no second upload");
    assert_eq!(skins.edit.cape, Some(CapePick::Minecraft("c1".into())));
    let again = skins.submit(false).unwrap();
    assert_eq!((again.skin, again.slim), (None, None));
    assert_eq!(again.cape, Some(CapePick::Minecraft("c1".into())));
}

#[test]
fn an_unconfirmed_half_of_a_save_is_not_called_saved() {
    let mut lines = said(&outcome(
        Some(Change::Done),
        Some(Change::Unconfirmed("busy".into())),
        None,
    ));
    assert!(lines.remove(0).is_ok(), "the skin went through");
    let cape = lines.remove(0).unwrap_err();
    assert!(
        cape.contains("isn't confirmed") && cape.contains("busy"),
        "{cape}"
    );
    assert!(lines.is_empty());
    let lines = said(&outcome(
        Some(Change::Unconfirmed("busy".into())),
        None,
        None,
    ));
    assert!(lines[0].is_err(), "{lines:?}");
}

#[test]
fn a_failed_save_keeps_every_edit() {
    let mut skins = Skins::new_for("a");
    pick(&mut skins, SkinPick::Default("alex"));
    let submitted = skins.submit(false).unwrap();
    answer_save(
        &mut skins,
        submitted.id,
        &outcome(Some(Change::Failed("Sign in again.".into())), None, None),
    );
    let said = skins.said[0].clone().unwrap_err();
    assert!(said.contains("Sign in again."), "{said}");
    assert_eq!(skins.edit.skin, Some(SkinPick::Default("alex")));
    assert!(skins.saving.is_none(), "Save can be tried again");
}

#[test]
fn a_skin_not_replaced_because_its_predecessor_couldnt_be_kept_says_so() {
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, None));
    pick(&mut skins, SkinPick::Default("alex"));
    let submitted = skins.submit(false).unwrap();
    let mut failed = outcome(
        Some(Change::Failed("no picture".into())),
        None,
        Some(profile(SKIN, false, None)),
    );
    failed.kept = Kept::Failed("no picture".into());
    answer_save(&mut skins, submitted.id, &failed);
    let said = skins.said[0].clone().unwrap_err();
    assert!(
        said.contains("couldn't be kept") && said.contains("no picture"),
        "{said}"
    );
    assert_eq!(
        skins.edit.skin,
        Some(SkinPick::Default("alex")),
        "still to do"
    );
}

#[test]
fn a_choice_made_during_a_save_outlives_it() {
    let mut skins = Skins::new_for("a");
    pick(&mut skins, SkinPick::Default("alex"));
    let submitted = skins.submit(false).unwrap();
    pick(&mut skins, SkinPick::Default("steve"));
    answer_save(
        &mut skins,
        submitted.id,
        &outcome(Some(Change::Done), None, Some(profile(ALEX, false, None))),
    );
    assert_eq!(skins.edit.skin, Some(SkinPick::Default("steve")));
}

#[test]
fn the_worn_cape_clicked_while_another_saves_is_kept() {
    // c1 is worn; c2 is being saved; the player clicks c1 again.
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c1")));
    choose(&mut skins, |edit| {
        edit.cape = Some(CapePick::Minecraft("c2".into()))
    });
    let submitted = skins.submit(false).unwrap();
    choose(&mut skins, |edit| {
        edit.cape = Some(CapePick::Minecraft("c1".into()))
    });
    assert_eq!(
        skins.edit.cape,
        Some(CapePick::Minecraft("c1".into())),
        "not dropped as a no-op against the profile from before the Save"
    );
    let (_, _, shown) = skins.chosen(&worn(&skins));
    assert_eq!(
        shown,
        Some(CapePick::Minecraft("c1".into())),
        "the preview shows it"
    );
    // c2 went on: c1 is still to do, and the next Save sends it.
    answer_save(
        &mut skins,
        submitted.id,
        &outcome(
            None,
            Some(Change::Done),
            Some(profile(SKIN, false, Some("c2"))),
        ),
    );
    assert_eq!(skins.edit.cape, Some(CapePick::Minecraft("c1".into())));
    assert_eq!(
        skins.submit(false).unwrap().cape,
        Some(CapePick::Minecraft("c1".into()))
    );

    // The same click when c2 was refused: c1 is what is worn, so nothing is left to do.
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c1")));
    choose(&mut skins, |edit| {
        edit.cape = Some(CapePick::Minecraft("c2".into()))
    });
    let submitted = skins.submit(false).unwrap();
    choose(&mut skins, |edit| {
        edit.cape = Some(CapePick::Minecraft("c1".into()))
    });
    answer_save(
        &mut skins,
        submitted.id,
        &outcome(
            None,
            Some(Change::Failed("busy".into())),
            Some(profile(SKIN, false, Some("c1"))),
        ),
    );
    assert!(skins.edit.is_empty());
}

#[test]
fn the_worn_skin_clicked_during_a_save_stays_the_picture_clicked() {
    // The default skin is worn; Alex is being saved; the player clicks "Your skin". The
    // Save keeps nothing (a default skin isn't kept): the click is still the default skin,
    // not dropped, and the next Save sends it.
    let mut skins = Skins::new_for("a");
    skins.profile = Some(Ok(Profile {
        skin: None,
        ..profile(SKIN, false, None)
    }));
    assert_eq!(worn(&skins).skin, SkinPick::AccountDefault);
    pick(&mut skins, SkinPick::Default("alex"));
    let submitted = skins.submit(false).unwrap();
    pick(&mut skins, SkinPick::AccountDefault);
    answer_save(
        &mut skins,
        submitted.id,
        &outcome(Some(Change::Done), None, Some(profile(ALEX, false, None))),
    );
    assert_eq!(skins.edit.skin, Some(SkinPick::AccountDefault));
    assert_eq!(
        skins.submit(false).unwrap().skin,
        Some(SkinPick::AccountDefault)
    );

    // `worn` is worn and shown; the player clicks it while Alex saves. The Save keeps it:
    // the click is that saved skin.
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, None));
    pick(&mut skins, SkinPick::Default("alex"));
    let submitted = skins.submit(false).unwrap();
    pick(&mut skins, SkinPick::Texture(SKIN.into()));
    let mut done = outcome(Some(Change::Done), None, Some(profile(ALEX, false, None)));
    done.kept = kept(SKIN);
    answer_save(&mut skins, submitted.id, &done);
    assert_eq!(skins.edit.skin, Some(SkinPick::Saved(KEPT.into())));

    // A stale "Your skin" (the page showed `worn`, the Save found and kept another): the
    // click is still `worn`'s picture, not the one the Save kept.
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, None));
    pick(&mut skins, SkinPick::Default("alex"));
    let submitted = skins.submit(false).unwrap();
    pick(&mut skins, SkinPick::Texture(SKIN.into()));
    let mut done = outcome(Some(Change::Done), None, Some(profile(ALEX, false, None)));
    done.kept = kept("http://textures.minecraft.net/texture/other");
    answer_save(&mut skins, submitted.id, &done);
    assert_eq!(skins.edit.skin, Some(SkinPick::Texture(SKIN.into())));
}

#[test]
fn new_arms_alone_are_sent_as_such_with_the_skin_the_page_shows() {
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, None));
    choose(&mut skins, |edit| edit.slim = Some(true));
    assert_eq!(
        skins.skin_sent(&worn(&skins)),
        Some(SkinSent::Arms(Some(SKIN.into()))),
        "the worker checks `worn` is still worn, and sends its own copy of it"
    );
    // Kept among the saved skins, it is still the skin worn.
    skins.kept.insert(SKIN.into(), KEPT.into());
    assert_eq!(
        skins.skin_sent(&worn(&skins)),
        Some(SkinSent::Arms(Some(SKIN.into())))
    );
    // Another picture is sent as a picture.
    pick(&mut skins, SkinPick::Default("alex"));
    assert_eq!(
        skins.skin_sent(&worn(&skins)),
        Some(SkinSent::Picture(SkinPick::Default("alex")))
    );
    // The account's default skin.
    let mut skins = Skins::new_for("a");
    skins.profile = Some(Ok(Profile {
        skin: None,
        ..profile(SKIN, false, None)
    }));
    choose(&mut skins, |edit| edit.slim = Some(true));
    assert_eq!(skins.skin_sent(&worn(&skins)), Some(SkinSent::Arms(None)));
    // While an accepted skin stands in for the profile's, it is sent as the picture it is.
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, None));
    skins.unconfirmed.skin = Some(SkinPick::Default("alex"));
    choose(&mut skins, |edit| edit.slim = Some(true));
    assert_eq!(
        skins.skin_sent(&worn(&skins)),
        Some(SkinSent::Picture(SkinPick::Default("alex")))
    );
    // No skin edit: no skin sent.
    skins.edit = Edit {
        cape: Some(CapePick::None),
        ..Edit::default()
    };
    assert_eq!(skins.skin_sent(&worn(&skins)), None);
}

#[test]
fn a_save_during_a_switch_away_and_back_stays_with_its_account() {
    let mut pages = SkinsPages::default();
    let signed_in = |_: &str| true;
    pages.show("a", signed_in);
    let old_read = pages.read_asked();
    pick(&mut pages, SkinPick::Default("alex"));
    let save = pages.submit(false).unwrap();
    // A → B → A while the Save runs.
    pages.show("b", signed_in);
    assert_eq!(pages.uuid.as_deref(), Some("b"));
    assert!(pages.saving.is_none(), "B has a page of its own");
    pages.show("a", signed_in);
    assert_eq!(
        pages.saving.as_ref().map(|s| s.id),
        Some(save.id),
        "A's Save is still under way"
    );
    assert_eq!(pages.edit.skin, Some(SkinPick::Default("alex")));
    assert!(
        pages.submit(false).is_none(),
        "no second write while the first runs"
    );
    // A reading from before the switch is still no answer to a newer one.
    let read = pages.read_asked();
    assert_ne!(read, old_read);
    assert!(!pages.read_answered("a", old_read, Ok(profile(SKIN, false, None)), false));
    assert!(!pages.read_answered("b", read, Ok(profile(SKIN, false, None)), false));
    // The Save ends while B is shown: A's page is reconciled, B's left alone.
    pages.show("b", signed_in);
    assert!(pages.writing(), "A's Save, though B is shown");
    let after = profile(ALEX, false, Some("c1"));
    let done = outcome(Some(Change::Done), None, Some(after.clone()));
    assert!(!pages.save_answered(save.id, &done), "not B's Save");
    assert!(answer_save(pages.page_mut("a").unwrap(), save.id, &done));
    assert!(!pages.writing());
    assert!(
        pages.profile.is_none(),
        "B's page knows nothing of A's profile"
    );
    pages.show("a", signed_in);
    assert_eq!(pages.profile, Some(Ok(after)));
    assert!(pages.saving.is_none());
    assert!(pages.edit.is_empty(), "done, not sent again");
    assert!(pages.said.iter().all(Result::is_ok));
}

#[test]
fn an_accepted_but_unconfirmed_save_is_not_sent_again() {
    // The upload got a 204 and the reading after it failed.
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c1")));
    pick(&mut skins, SkinPick::Default("alex"));
    choose(&mut skins, |edit| edit.cape = Some(CapePick::None));
    let submitted = skins.submit(false).unwrap();
    answer_save(
        &mut skins,
        submitted.id,
        &outcome(
            Some(Change::Unconfirmed("busy".into())),
            Some(Change::Unconfirmed("busy".into())),
            Some(profile(SKIN, false, Some("c1"))),
        ),
    );
    assert_eq!(skins.said.len(), 2);
    assert!(
        skins.said.iter().all(Result::is_err),
        "not called saved: {:?}",
        skins.said
    );
    assert!(skins.edit.is_empty(), "out of the resend path");
    assert!(skins.submit(false).is_none(), "Save has nothing to send");
    // Until a reading says otherwise, the account is taken to wear what was accepted.
    let shown = worn(&skins);
    assert_eq!(shown.skin, SkinPick::Default("alex"));
    assert_eq!(shown.cape, Some(CapePick::None));
    // Clicking what was worn before is a change again, and is sent.
    choose(&mut skins, |edit| {
        edit.cape = Some(CapePick::Minecraft("c1".into()))
    });
    assert_eq!(skins.edit.cape, Some(CapePick::Minecraft("c1".into())));
    skins.edit = Edit::default();
    // The next reading is the truth.
    read(&mut skins, profile(ALEX, false, None));
    assert!(skins.unconfirmed.is_empty());
    assert_eq!(worn(&skins).skin, SkinPick::Texture(ALEX.into()));
}

#[test]
fn a_save_that_stopped_unexpectedly_is_neither_sent_again_nor_taken_as_done() {
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, false, Some("c1")));
    pick(&mut skins, SkinPick::Default("alex"));
    choose(&mut skins, |edit| edit.cape = Some(CapePick::None));
    let submitted = skins.submit(false).unwrap();
    answer_save(&mut skins, submitted.id, &stopped_outcome((true, true)));
    assert!(skins.saving.is_none());
    assert!(skins.edit.is_empty(), "not sent again");
    assert_eq!(skins.unconfirmed.skin, Some(SkinPick::Default("alex")));
    assert_eq!(skins.said.len(), 2);
    assert!(skins.said.iter().all(Result::is_err));
    assert_eq!(stopped_outcome((false, true)).skin, None);
}
