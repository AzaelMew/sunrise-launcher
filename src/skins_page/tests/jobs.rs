//! The page's background work: the bounded queue, panicking jobs, and fetching pictures and textures.

use super::*;

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
