use super::*;
use sunrise_launcher::account::capes::Cape;

const SKIN: &str = "http://textures.minecraft.net/texture/worn";
const ALEX: &str = "http://textures.minecraft.net/texture/alex";
const KEPT: &str = "0123456789abcdef";

fn cape(id: &str, active: bool) -> Cape {
    Cape {
        id: id.into(),
        alias: id.into(),
        url: format!("http://textures.minecraft.net/texture/{id}"),
        active,
    }
}

fn profile(skin: &str, slim: bool, worn_cape: Option<&str>) -> Profile {
    Profile {
        id: "069a79f444e94726a5befca90e38aaf5".into(),
        skin: Some((skin.into(), if slim { "slim" } else { "classic" }.into())),
        capes: ["c1", "c2"]
            .into_iter()
            .map(|id| cape(id, worn_cape == Some(id)))
            .collect(),
    }
}

fn worn(skins: &Skins) -> Worn {
    skins.worn(None, false)
}

/// Chooses on the page the way a tile does: the edit, then forgetting what is worn already.
fn choose(skins: &mut Skins, change: impl FnOnce(&mut Edit)) {
    let worn = worn(skins);
    skins.choose(change, &worn);
}

fn pick(skins: &mut Skins, skin: SkinPick) {
    choose(skins, |edit| edit.skin = Some(skin));
}

/// A Save answered and settled, as `Launcher::look_saved` does it.
fn answer_save(skins: &mut Skins, id: u64, outcome: &look::Outcome) -> bool {
    let answered = skins.save_answered(id, outcome);
    let worn = worn(skins);
    skins.settle(&worn);
    answered
}

fn outcome(skin: Option<Change>, cape: Option<Change>, profile: Option<Profile>) -> look::Outcome {
    look::Outcome {
        kept: Kept::Nothing,
        skin,
        cape,
        profile,
        arms: None,
        recorded: None,
        uploaded: None,
    }
}

fn kept(url: &str) -> Kept {
    Kept::Kept {
        url: url.into(),
        saved: Saved {
            id: KEPT.into(),
            name: String::new(),
            slim: false,
        },
    }
}

fn read(skins: &mut Skins, profile: Profile) {
    let id = skins.read_asked();
    assert!(skins.read_answered("a", id, Ok(profile), false));
}

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
fn a_picture_reaches_the_page_that_asked_for_it_while_another_is_shown() {
    let mut pages = SkinsPages::default();
    pages.show("a", |_| true);
    assert!(pages.ask_picture(SKIN, false));
    pages.show("b", |_| true);
    pages.picture(SKIN, Some(vec![1, 2, 3]));
    assert!(!pages.pictures.contains_key(SKIN), "B didn't ask for it");
    pages.show("a", |_| true);
    assert_eq!(
        pages.pictures.get(SKIN),
        Some(&Picture::Ready(vec![1, 2, 3]))
    );
}

#[test]
fn a_picture_that_couldnt_be_had_is_asked_for_again() {
    let mut skins = Skins::new_for("a");
    assert!(skins.ask_picture(SKIN, false));
    assert!(!skins.ask_picture(SKIN, false), "on its way already");
    assert!(skins.unasked([&SKIN.to_owned()]).is_empty());
    skins.picture(SKIN.into(), None);
    assert!(matches!(skins.picture_png(SKIN), Png::Missing));
    // The next reading of the profile asks for it again.
    assert_eq!(skins.unasked([&SKIN.to_owned()]), [SKIN]);
    assert!(skins.ask_picture(SKIN, false));
    skins.picture(SKIN.into(), Some(vec![1]));
    assert!(!skins.ask_picture(SKIN, false), "here now");
    assert!(skins.unasked([&SKIN.to_owned()]).is_empty());
}

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
fn a_queue_holds_a_bounded_amount_of_work_on_one_thread() {
    let mut queue = Queue::default();
    let (release, held) = mpsc::channel::<()>();
    let (done, finished) = mpsc::channel::<usize>();
    // The first job holds the thread until released; the queue fills behind it.
    queue
        .push(
            "test-queue",
            Box::new(move || {
                let _ = held.recv();
            }),
        )
        .unwrap();
    let mut taken = 0;
    let mut refused = 0;
    for n in 0..QUEUED + 8 {
        let done = done.clone();
        match queue.push("test-queue", Box::new(move || done.send(n).unwrap())) {
            Ok(()) => taken += 1,
            Err(Refused::Full) => refused += 1,
            Err(Refused::NoThread) => panic!("the thread is there"),
        }
    }
    assert!((QUEUED - 1..=QUEUED).contains(&taken), "{taken}");
    assert!(refused >= 8, "refused, not queued without end: {refused}");
    // A job that panics doesn't take the queue down with it.
    release.send(()).unwrap();
    for _ in 0..taken {
        finished.recv().unwrap();
    }
    queue
        .push("test-queue", Box::new(|| panic!("a bad job")))
        .unwrap();
    let done2 = done.clone();
    queue
        .push("test-queue", Box::new(move || done2.send(999).unwrap()))
        .unwrap();
    assert_eq!(finished.recv().unwrap(), 999);
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
fn the_worn_look_comes_from_one_reading_of_the_profile() {
    let mut skins = Skins::new_for("a");
    let recorded = (SKIN.to_owned(), "slim".to_owned());
    assert_eq!(
        skins.worn(Some(&recorded), false),
        Worn {
            skin: SkinPick::Texture(SKIN.into()),
            slim: true,
            cape: None,
            texture: Some(SKIN.into()),
        }
    );
    read(
        &mut skins,
        profile(
            "http://textures.minecraft.net/texture/new",
            false,
            Some("c1"),
        ),
    );
    let worn = skins.worn(Some(&recorded), false);
    assert!(!worn.slim);
    assert_eq!(worn.cape, Some(CapePick::Minecraft("c1".into())));
    let again = skins.read_asked();
    skins.read_answered("a", again, Err("offline".into()), false);
    assert!(matches!(skins.profile, Some(Ok(_))));
}

#[test]
fn the_worn_skins_tile_uses_the_profiles_arms() {
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, true, None));
    skins.kept.insert(SKIN.into(), KEPT.into());
    let worn = worn(&skins);
    assert_eq!(worn.skin, SkinPick::Saved(KEPT.into()));
    skins.edit.skin = Some(worn.skin.clone());
    skins.edit.slim = Some(worn.slim);
    skins.settle(&worn);
    assert!(skins.edit.is_empty());
    // Removed from the saved skins, it is the skin at its URL again.
    skins.removed(KEPT);
    assert_eq!(
        super::tests::worn(&skins).skin,
        SkinPick::Texture(SKIN.into())
    );
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
fn a_job_that_panics_still_answers() {
    let (tx, rx) = std::sync::mpsc::channel();
    answer(
        &tx,
        || panic!("a bad job"),
        || Msg::SkinFile("k".into(), 7, None),
    );
    assert!(matches!(rx.recv().unwrap(), Msg::SkinFile(key, 7, None) if key == "k"));
    answer(
        &tx,
        || Msg::SkinFile("k".into(), 8, Some(vec![1])),
        || panic!("not the fallback"),
    );
    assert!(matches!(rx.recv().unwrap(), Msg::SkinFile(_, 8, Some(_))));
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

#[test]
fn the_account_default_skin_comes_with_the_arms_shown() {
    let resources = tempfile::tempdir().unwrap();
    // An id the game gives a slim default skin, then the same skin with wide arms.
    let uuid = (0..64u32)
        .map(|n| format!("{n:032x}"))
        .find(|uuid| {
            sunrise_launcher::heads::default_skin_file(resources.path(), uuid)
                .is_some_and(|(_, slim)| slim)
        })
        .unwrap();
    let (slim_file, _) =
        sunrise_launcher::heads::default_skin_file(resources.path(), &uuid).unwrap();
    let name = slim_file.file_stem().unwrap().to_str().unwrap().to_owned();
    assert_eq!(
        account_default_path(resources.path(), &uuid, true),
        Some(slim_file)
    );
    assert_eq!(
        account_default_path(resources.path(), &uuid, false),
        Some(default_skin_path(resources.path(), &name, false)),
        "the wide picture of the same skin"
    );
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

/// The launcher itself, in a child of the test binary with scratch homes, no keychain and an
/// unreachable proxy, so it reads and writes no player's data and fetches nothing.
fn in_a_scratch_launcher(name: &str, test: impl FnOnce(&mut Launcher, &mut Context<Launcher>)) {
    const CHILD: &str = "SUNRISE_SKINS_LAUNCHER_TEST";
    if std::env::var_os(CHILD).is_none() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let nowhere = "http://127.0.0.1:9";
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env(CHILD, root)
            .env("SUNRISE_LAUNCHER_HOME", root.join("launcher"))
            .env("SUNRISE_HOME", root.join("game"))
            .env("SUNRISE_LAUNCHER_LOCATION_FILE", root.join("location.json"))
            .env("SUNRISE_NO_KEYCHAIN", "1")
            .env("SUNRISE_TELEMETRY_URL", "off")
            .env("SUNRISE_LAUNCHER_NO_SELF_UPDATE", "1")
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("HTTPS_PROXY", nowhere)
            .env("HTTP_PROXY", nowhere)
            .env("ALL_PROXY", nowhere)
            .env_remove("NO_PROXY")
            .env_remove("SUNRISE_GITHUB_MIRROR")
            .env_remove("SUNRISE_LAUNCHER_BENCH")
            .env_remove("SUNRISE_LAUNCHER_DEVELOPER_OFFLINE")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let mut app = gpui::TestAppContext::single();
    let (shell, cx) = app.add_window_view(crate::window_root::Shell::new);
    let launcher = shell.read_with(cx, |shell, _| shell.launcher());
    launcher.update(cx, |l, cx| {
        l.accounts.accounts.push(account::Account {
            uuid: UUID.into(),
            name: "Player".into(),
            skin: None,
            cape: None,
        });
        test(l, cx)
    });
}

const UUID: &str = "069a79f444e94726a5befca90e38aaf5";

/// The launcher's next message that `wanted` picks, the others (release lists, heads) let go.
fn next(l: &mut Launcher, wanted: impl Fn(&Msg) -> bool) -> Msg {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let msg = l.rx.recv_timeout(left).expect("the message came");
        if wanted(&msg) {
            return msg;
        }
    }
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

/// A 64 by 64 skin, one texel of it `n`.
fn skin_png(n: u8) -> Vec<u8> {
    let image = image::RgbaImage::from_fn(64, 64, |x, y| {
        image::Rgba(if (x, y) == (0, 0) {
            [n, 1, 1, 255]
        } else {
            [x as u8, y as u8, 7, 255]
        })
    });
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

const NEW: &str = "http://textures.minecraft.net/texture/new";

/// A Save the services took, whose own profile was the one from before it.
fn unconfirmed_save(skins: &mut Skins) {
    pick(skins, SkinPick::Default("alex"));
    choose(skins, |edit| {
        edit.cape = Some(CapePick::Minecraft("c1".into()))
    });
    let submitted = skins.submit(false).unwrap();
    let mut stale = outcome(
        Some(Change::Unconfirmed("old".into())),
        Some(Change::Unconfirmed("old".into())),
        Some(profile(SKIN, false, Some("c2"))),
    );
    stale.uploaded = Some((skin_library::id_of_png(&skin_png(5)).unwrap(), true));
    answer_save(skins, submitted.id, &stale);
    assert_eq!(
        skins.confirmed,
        look::Record::default(),
        "nothing shown yet"
    );
}

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
fn a_queued_texture_job_fetches_nothing_once_offline() {
    static FETCHED: AtomicU64 = AtomicU64::new(0);
    fn fetch(_: &str) -> Option<Vec<u8>> {
        FETCHED.fetch_add(1, Ordering::Relaxed);
        Some(vec![1])
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let offline = Arc::new(AtomicBool::new(false));
    let job = texture_job(tx.clone(), SKIN.into(), offline.clone(), fetch);
    // The view comes on after the job was queued, before it starts.
    offline.store(true, Ordering::Relaxed);
    job();
    assert!(matches!(rx.recv().unwrap(), Msg::Picture(url, None) if url == SKIN));
    assert_eq!(FETCHED.load(Ordering::Relaxed), 0, "no request");
    offline.store(false, Ordering::Relaxed);
    texture_job(tx, SKIN.into(), offline, fetch)();
    assert!(matches!(rx.recv().unwrap(), Msg::Picture(_, Some(_))));
    assert_eq!(FETCHED.load(Ordering::Relaxed), 1);
}

#[test]
fn the_texture_jobs_go_by_the_pages_own_answer() {
    in_a_scratch_launcher(
        "skins_page::tests::the_texture_jobs_go_by_the_pages_own_answer",
        |l, _| {
            l.skins.offline_for_test = Some(true);
            assert!(l.skins_offline());
            assert!(l.skins.offline_flag.load(Ordering::Relaxed));
            l.skins.offline_for_test = Some(false);
            assert!(!l.skins_offline());
            assert!(!l.skins.offline_flag.load(Ordering::Relaxed));
        },
    );
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
