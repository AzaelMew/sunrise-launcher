use super::*;

fn opaque(px: &Pixels) -> usize {
    px.0.as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] == 255)
        .count()
}

#[test]
fn the_cat_hides_comes_out_into_the_room_it_has_and_goes_back() {
    let mut outing = Outing::new(&TABBY_WAYS, 7, 1000.);
    outing.step(0., 400., false);
    assert!(outing.view(0.).is_none(), "it starts out of sight");
    let (mut now, mut furthest, mut visited) = (0., 0f32, false);
    // Until it has come out and is away again.
    while !(visited && matches!(outing.scene, Scene::Away { .. })) {
        now += 40.;
        assert!(now < 600_000., "it went home within ten minutes");
        outing.step(now, 400., false);
        if let Some(view) = outing.view(now) {
            furthest = furthest.max(view.out);
            visited |= view.pettable;
        }
    }
    assert!(
        furthest > WIDTH && furthest <= 400.,
        "wholly out, and never past the room: {furthest}"
    );
    assert!(outing.view(now).is_none());
}

#[test]
fn with_too_little_room_the_cat_only_peeks_and_with_none_it_stays_in() {
    let mut outing = Outing::new(&SIAMESE_WAYS, 3, 0.);
    for step in 0..200 {
        outing.step(step as f32 * 40., 0., false);
        assert!(outing.view(step as f32 * 40.).is_none());
    }
    let mut furthest = 0f32;
    for step in 200..2000 {
        let now = step as f32 * 40.;
        outing.step(now, 60., false);
        if let Some(view) = outing.view(now) {
            assert!(!view.pettable);
            furthest = furthest.max(view.out);
        }
    }
    assert!(
        furthest > PEEK_OUT - 1. && furthest <= PEEK_OUT,
        "head and chest, no further: {furthest}"
    );
}

#[test]
fn on_play_the_cat_sleeps_without_frames_and_drops_off_when_play_is_needed() {
    let mut outing = Outing::new(&TABBY_WAYS, 7, 0.);
    outing.perch_next = true;
    // Play is busy: it does not climb up.
    outing.step(0., 400., false);
    assert!(!matches!(outing.scene, Scene::Perch { .. }));
    let mut outing = Outing::new(&TABBY_WAYS, 7, 0.);
    outing.perch_next = true;
    outing.step(0., 0., true);
    let mut now = 0.;
    while now < CLIMB + CURL + 40. {
        now += 40.;
        outing.step(now, 0., true);
    }
    let view = outing.view(now).expect("it is on Play");
    assert_eq!(view.perch, Some(1.), "wholly up");
    assert!(
        !view.pettable,
        "a picture only: clicks are Play's and the page's"
    );
    assert_eq!(view.pose, rest(State::Sleep), "and curled up asleep");
    let wait = outing.next_change(now);
    assert!(wait >= Duration::from_secs(40), "no frames while it sleeps");
    assert_eq!(outing.view(now + 30_000.).map(|v| v.pose), Some(view.pose));
    // Play is pressed, or turns busy: off it goes, and it is gone within the drop.
    outing.step(now + 30_000., 0., false);
    assert!(matches!(outing.scene, Scene::Drop { .. }));
    outing.step(now + 30_000. + DROP, 0., false);
    assert!(outing.view(now + 30_000. + DROP).is_none());
}

#[test]
fn a_resting_cat_asks_for_no_frames_until_it_next_does_something() {
    let mut cat = Cat::new(&TABBY_WAYS, 7);
    cat.advance(0.);
    let next = cat.next_change().expect("a resting cat waits");
    assert!(
        next >= 4000.,
        "the first rest or blink is seconds away: {next}"
    );
    assert_eq!(
        cat.pose(0.),
        cat.pose(next - 1.),
        "and it holds still until then"
    );
    cat.advance(next);
    assert!(cat.next_change().is_none(), "then it moves, frame by frame");
}

#[test]
fn petting_wakes_a_meow_and_the_cat_settles_back_where_it_was() {
    let mut cat = Cat::new(&SIAMESE_WAYS, 3);
    cat.pet(100.);
    assert_eq!(cat.action().map(|(a, _)| a.name), Some("meow"));
    let loud = cat.pose(100. + 480.);
    assert!(
        loud[MOUTH] > 0.9 && loud[TURN] > 0.9,
        "mouth wide, facing you"
    );
    cat.advance(100. + 2701.);
    assert!(cat.action().is_none());
    assert_eq!(cat.pose(3000.), rest(State::Sit));
}

#[test]
fn a_cat_asleep_in_the_bar_hisses_when_petted_and_stays_asleep_after() {
    // In the bar a visit can end in a nap (`fall asleep`); on Play the cat is never petted.
    let mut cat = Cat::new(&TABBY_WAYS, 5);
    cat.state = State::Sleep;
    cat.pet(0.);
    assert_eq!(cat.action().map(|(a, _)| a.name), Some("grumpy"));
    assert!(cat.pose(700. * TABBY_WAYS.pace)[ANGER] > 0.9);
    cat.advance(2800. * TABBY_WAYS.pace + 1.);
    assert_eq!(cat.state, State::Sleep);
}

#[test]
fn the_cat_stays_away_when_switched_off_or_under_reduced_motion() {
    assert!(cat_out(true, false));
    assert!(!cat_out(false, false), "switched off");
    assert!(!cat_out(true, true), "reduced motion");
    assert!(!cat_out(false, true));
    // Staying away is the same state either way: nothing kept, no wake due.
    let mut state = CatState {
        cat: Some(Outing::new(&TABBY_WAYS, 7, 0.)),
        seen: Some(Instant::now()),
        wake: Some(Instant::now() + Duration::from_millis(40)),
        ..CatState::default()
    };
    assert!(state.switch_off().is_none(), "no picture was drawn yet");
    assert!(state.cat.is_none() && state.seen.is_none() && state.wake.is_none());
}

#[test]
fn a_cat_switched_off_keeps_nothing_and_schedules_nothing() {
    let mut state = CatState {
        cat: Some(Outing::new(&SIAMESE_WAYS, 1, 0.)),
        seen: Some(Instant::now()),
        wake: Some(Instant::now() + Duration::from_secs(5)),
        shown: Some((
            rest(State::Sit),
            1,
            false,
            Arc::new(RenderImage::new([image::Frame::new(
                image::RgbaImage::new(1, 1),
            )])),
        )),
        ..CatState::default()
    };
    assert!(state.switch_off().is_some());
    assert!(state.cat.is_none() && state.seen.is_none() && state.wake.is_none());
    assert!(state.shown.is_none());
    assert!(state.switch_off().is_none());
}

#[test]
fn each_coat_draws_a_whole_cat_with_its_own_eyes() {
    for (coat, eye) in [(&SIAMESE, [255, 178, 92]), (&TABBY, [78, 214, 196])] {
        let px = picture(coat, &SIT);
        assert!(opaque(&px) > 500, "a body, not a few pixels");
        assert!(
            px.0.as_chunks::<4>().0.iter().any(|p| p[..3] == eye),
            "its eye colour, as BGR"
        );
    }
}

#[test]
fn every_action_ends_where_its_state_rests() {
    for (i, action) in actions().iter().enumerate() {
        let mut cat = Cat::new(&SIAMESE_WAYS, 9);
        cat.state = action.from;
        cat.playing = Some((Playing::Own(i), 0.));
        let end = action.keys.last().map_or(0., Key::at);
        let to = action.to.unwrap_or(action.from);
        let (ends, rests) = (cat.pose(end), rest(to));
        assert!(
            ends.iter().zip(rests).all(|(a, b)| (a - b).abs() < 1e-4),
            "{} ends at rest",
            action.name
        );
    }
}
