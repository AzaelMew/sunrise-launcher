use super::*;

/// Signed in, a build installed, nothing under way: Play.
fn idle() -> Facts {
    Facts {
        signed_in: true,
        installed: true,
        located: true,
        ..Facts::default()
    }
}

/// Takes the winning state away, so the next one shows.
type Next = fn(&mut Facts);

const LONG: &str = "The build failed. The compiler said a great deal more than fits here.";

#[test]
fn play_says_each_state_in_the_order_they_win() {
    let all = Facts {
        installing: Some(0.42),
        preparing: true,
        building: true,
        starting: true,
        mutating: true,
        running: true,
        ..Facts::default()
    };
    // Each step takes the winning state away: the next one shows.
    let steps: [(Main, Next); 9] = [
        (Main::Installing(0.42), |f| f.installing = None),
        (Main::Preparing, |f| f.preparing = false),
        (Main::Building, |f| f.building = false),
        (Main::Starting, |f| f.starting = false),
        (Main::ChangingMods, |f| f.mutating = false),
        (Main::Stop, |f| f.running = false),
        (Main::SignIn, |f| f.signed_in = true),
        (Main::Install, |f| f.installed = true),
        (Main::Play, |_| {}),
    ];
    let mut facts = all;
    for (main, next) in steps {
        assert_eq!(facts.view().main, main);
        next(&mut facts);
    }
    assert_eq!(Main::Play.label(), "Play");
    assert_eq!(Main::Installing(0.42).label(), "Installing · 42%");
}

#[test]
fn play_can_be_pressed_unless_something_is_under_way_or_the_data_is_lost() {
    assert!(idle().view().enabled);
    // Stop, sign-in and the first install are presses too.
    for facts in [
        Facts {
            running: true,
            ..idle()
        },
        Facts {
            signed_in: false,
            ..idle()
        },
        Facts {
            installed: false,
            ..idle()
        },
    ] {
        assert!(facts.view().enabled, "{facts:?}");
    }
    for facts in [
        Facts {
            installing: Some(0.),
            ..idle()
        },
        Facts {
            preparing: true,
            ..idle()
        },
        Facts {
            building: true,
            ..idle()
        },
        Facts {
            starting: true,
            ..idle()
        },
        Facts {
            mutating: true,
            ..idle()
        },
        Facts {
            located: false,
            ..idle()
        },
    ] {
        assert!(!facts.view().enabled, "{facts:?}");
    }
}

#[test]
fn a_build_under_way_has_stop_and_keeps_its_old_error_quiet() {
    let view = Facts {
        building: true,
        native_error: Some(LONG.into()),
        held: Some("Nov 17".into()),
        update: Some("Nov 17".into()),
        mods_off: 2,
        ..idle()
    }
    .view();
    assert_eq!(view.act, Some(Act::StopBuild));
    assert_eq!(view.act.as_ref().map(Act::label), Some("Stop"));
    assert!(!view.failed);
    assert_eq!(view.mods_off, None);
    assert_eq!(
        view.strip(),
        Some(("Building mods…".into(), StripAct::Act(Act::StopBuild)))
    );
}

#[test]
fn a_problem_is_one_sentence_with_details_when_there_is_more() {
    let view = Facts {
        native_error: Some(LONG.into()),
        ..idle()
    }
    .view();
    assert_eq!(view.said.as_deref(), Some("The build failed."));
    assert!(view.failed);
    let details = Act::Notice {
        text: LONG.into(),
        way_out: None,
    };
    assert_eq!(view.act.as_ref().map(Act::label), Some("Details…"));
    assert_eq!(view.act, Some(details.clone()));
    assert_eq!(
        view.strip(),
        Some(("The build failed.".into(), StripAct::Act(details)))
    );
}

#[test]
fn a_short_problem_has_no_action_and_the_strip_leads_home() {
    let view = Facts {
        client_error: Some("The game is already running.".into()),
        ..idle()
    }
    .view();
    assert_eq!(view.said.as_deref(), Some("The game is already running."));
    assert_eq!(view.act, None);
    assert_eq!(
        view.strip(),
        Some(("The game is already running.".into(), StripAct::Home))
    );
    assert_eq!(StripAct::Home.label(), "Go to Home");
}

#[test]
fn missing_game_files_are_fixed_by_choosing_the_folder() {
    let view = Facts {
        prepare_error: Some("Minecraft was not found".into()),
        // The channel's way out is the build's, never this problem's.
        build_gone: Some(instances::Channel::Stable),
        ..idle()
    }
    .view();
    let fix = Act::Notice {
        text: "Minecraft was not found".into(),
        way_out: Some(WayOut::MinecraftFolder),
    };
    assert_eq!(fix.label(), "Fix…");
    assert_eq!(view.act, Some(fix.clone()));
    assert_eq!(
        view.strip(),
        Some(("Minecraft was not found".into(), StripAct::Act(fix)))
    );
}

#[test]
fn a_build_that_is_gone_is_fixed_by_the_newest_of_its_channel_once_idle() {
    let gone = Facts {
        native_error: Some("No mod tools".into()),
        build_gone: Some(instances::Channel::Nightly),
        ..idle()
    };
    assert_eq!(
        gone.view().act,
        Some(Act::Notice {
            text: "No mod tools".into(),
            way_out: Some(WayOut::Newest(instances::Channel::Nightly)),
        })
    );
    // While something else runs the channel must not change under it.
    let busy = Facts {
        mutating: true,
        ..gone
    };
    assert_eq!(busy.view().act, None);
}

#[test]
fn problems_win_in_the_order_the_launch_meets_them() {
    let mut facts = Facts {
        client_error: Some("client".into()),
        prepare_error: Some("prepare".into()),
        native_error: Some("native".into()),
        install_error: Some("install".into()),
        crash: Some("crash".into()),
        ..idle()
    };
    let steps: [(&str, Next); 5] = [
        ("client", |f| f.client_error = None),
        ("prepare", |f| f.prepare_error = None),
        ("native", |f| f.native_error = None),
        ("install", |f| f.install_error = None),
        ("crash", |f| f.crash = None),
    ];
    for (said, next) in steps {
        assert_eq!(facts.view().said.as_deref(), Some(said));
        next(&mut facts);
    }
    assert_eq!(facts.view().said, None);
}

#[test]
fn a_held_update_speaks_for_the_build_and_is_fixed_in_its_sheet() {
    let held = Facts {
        native_error: Some(LONG.into()),
        held: Some("Nov 17".into()),
        update: Some("Nov 17".into()),
        ..idle()
    };
    let view = held.view();
    assert_eq!(
        view.said.as_deref(),
        Some("The update to Nov 17 is on hold")
    );
    assert!(!view.failed);
    assert_eq!(view.act, Some(Act::Resolve));
    assert_eq!(Act::Resolve.label(), "Fix…");
    assert_eq!(
        view.strip(),
        Some((
            "The update to Nov 17 is on hold".into(),
            StripAct::Act(Act::Resolve)
        ))
    );
    // Nothing is resolved while something runs; another problem is still said first.
    let busy = Facts {
        mutating: true,
        ..held.clone()
    };
    assert_eq!(busy.view().act, None);
    let other = Facts {
        crash: Some("The game stopped".into()),
        ..held
    };
    assert_eq!(other.view().said.as_deref(), Some("The game stopped"));
    assert_eq!(other.view().act, Some(Act::Resolve));
}

#[test]
fn a_newer_build_is_offered_only_to_an_idle_installed_game_and_only_on_home() {
    let newer = Facts {
        update: Some("Nov 18".into()),
        ..idle()
    };
    let view = newer.view();
    assert_eq!(view.said.as_deref(), Some("Nov 18 is available"));
    assert_eq!(view.act, Some(Act::Update));
    assert_eq!(Act::Update.label(), "Update game");
    assert_eq!(view.strip(), None);
    for facts in [
        Facts {
            running: true,
            ..newer.clone()
        },
        Facts {
            installed: false,
            ..newer.clone()
        },
        Facts {
            preparing: true,
            ..newer
        },
    ] {
        let view = facts.view();
        assert_eq!((view.said, view.act), (None, None), "{facts:?}");
    }
}

#[test]
fn mods_left_out_are_one_quiet_line_when_nothing_else_is_said() {
    let off = Facts {
        mods_off: 2,
        ..idle()
    };
    assert_eq!(off.view().mods_off, Some(2));
    assert_eq!(off.view().strip(), None);
    let said = Facts {
        crash: Some("The game stopped".into()),
        ..off
    };
    assert_eq!(said.view().mods_off, None);
}

#[test]
fn the_strip_shows_work_under_way_then_a_running_game_and_nothing_when_idle() {
    assert_eq!(idle().view().strip(), None);
    assert_eq!(
        Facts {
            signed_in: false,
            installed: false,
            ..idle()
        }
        .view()
        .strip(),
        None
    );
    let home = |facts: Facts, says: &str| {
        assert_eq!(
            facts.view().strip(),
            Some((says.to_string().into(), StripAct::Home)),
            "{facts:?}"
        );
    };
    home(
        Facts {
            installing: Some(0.5),
            ..idle()
        },
        "Installing · 50%",
    );
    home(
        Facts {
            preparing: true,
            ..idle()
        },
        "Preparing…",
    );
    home(
        Facts {
            starting: true,
            ..idle()
        },
        "Starting…",
    );
    home(
        Facts {
            mutating: true,
            ..idle()
        },
        "Changing mods…",
    );
    let running = Facts {
        running: true,
        ..idle()
    };
    assert_eq!(
        running.view().strip(),
        Some(("The game is running".into(), StripAct::StopGame))
    );
    assert_eq!(StripAct::StopGame.label(), "Stop");
    // What failed is said before the game that still runs.
    let failed = Facts {
        client_error: Some(LONG.into()),
        ..running
    };
    assert!(matches!(
        failed.view().strip(),
        Some((_, StripAct::Act(Act::Notice { .. })))
    ));
}

#[test]
fn the_first_sentence_is_what_is_said() {
    assert_eq!(first_sentence("  One. Two.\nThree"), "One.");
    assert_eq!(first_sentence("One line\nand another"), "One line");
    assert_eq!(first_sentence(""), "");
}
