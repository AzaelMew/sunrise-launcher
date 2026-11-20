use super::*;
use std::{fs, io::Write as _};

#[test]
fn build_consent_binds_an_unopened_mods_page_to_the_selected_profile() {
    const CHILD: &str = "SUNRISE_BUILD_CONSENT_PROFILE_TEST";
    if std::env::var_os(CHILD).is_none() {
        let root = tempfile::tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "mods_page::tests::build_consent_binds_an_unopened_mods_page_to_the_selected_profile",
            ])
            .env(CHILD, "1")
            .env("SUNRISE_LAUNCHER_HOME", root.path().join("launcher"))
            .env("SUNRISE_HOME", root.path().join("default"))
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }

    let default = backend::game_home();
    let selected = default.with_file_name("selected");
    for (home, id) in [(&default, "default_mod"), (&selected, "selected_mod")] {
        let dir = home.join(native_mods::INSTALLED);
        fs::create_dir_all(&dir).unwrap();
        let mut zip =
            zip::ZipWriter::new(fs::File::create(dir.join(format!("{id}.smod"))).unwrap());
        let pin = "f".repeat(40);
        let manifest = format!("id = '{id}'\nname = '{id}'\nversion = '1.0.0'\napi = '0.2'\ncapabilities = ['registries']\n[native]\ngame = '26.3'\nsunrise = '{pin}'\nmixins = '{pin}'\n");
        for (name, text) in [
            ("sunrise-mod.toml", manifest.as_str()),
            (
                "Cargo.toml",
                "[package]\nname = 'fixture'\nversion = '1.0.0'\n",
            ),
            ("src/lib.rs", "pub fn fixture() {}\n"),
        ] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        mod_list::set_enabled(home, id, true).unwrap();
    }
    let untouched = fs::read(default.join("sunrise-mods.txt")).unwrap();
    let mut settings = backend::Settings::default();
    for allow in [false, true] {
        mod_list::set_enabled(&selected, "selected_mod", true).unwrap();
        let mut home = ModsHome::default();
        assert_eq!(home.current(), default, "Mods has never opened");
        let read = home.begin_build_consent(selected.clone());
        let mut pending = Some(PendingConsent::ReadingBuild(read.clone()));
        assert!(
            native_consent::resolve(&mut pending, true, true, &mut settings, &home.current())
                .unwrap()
                .is_none()
        );
        assert!(settings.native_mods_allowed_at.is_none());
        pending
            .as_mut()
            .unwrap()
            .finish_read(&read, native_status::build_inputs(&read, None));
        assert_eq!(
            pending.as_ref().unwrap().rows()[0].label,
            "selected_mod (selected_mod.smod, not checked against the registry)"
        );
        assert_eq!(home.current(), selected);
        let action =
            native_consent::resolve(&mut pending, allow, true, &mut settings, &home.current())
                .unwrap();
        if allow {
            assert!(matches!(action, Some(PendingConsent::Build(inputs))
                if matches!(inputs.as_slice(), [one] if one.id == "selected_mod")));
            assert!(settings.native_mods_allowed_at.is_some());
        } else {
            assert!(action.is_none());
            assert!(settings.native_mods_allowed_at.is_none());
        }
        assert!(pending.is_none());
        assert_eq!(mod_list::is_enabled(&selected, "selected_mod"), allow);
        assert_eq!(
            fs::read(default.join("sunrise-mods.txt")).unwrap(),
            untouched
        );
    }
}

#[test]
fn a_build_ending_costs_the_install_only_what_it_could_not_keep() {
    let failure = |id: &str| native_mods::BuildFailure {
        id: id.into(),
        reason: "r".into(),
        log: "l".into(),
    };
    let brought = ["a".to_string(), "b".to_string()];
    assert_eq!(
        build_cost(&brought, false, vec![failure("b"), failure("old")]),
        BuildCost::Failures(vec![failure("b"), failure("old")]),
        "a switch's build lists the others too"
    );
    assert_eq!(build_cost(&brought, false, Vec::new()), BuildCost::Nothing);
    // The build stopped outside the mods: every mod it was started for goes.
    assert_eq!(
        build_cost(&brought, true, vec![failure("b")]),
        BuildCost::Rollback(vec!["a".into(), "b".into()])
    );
}

#[test]
fn a_builds_stop_for_worlds_becomes_the_world_question_that_builds_on_when_switched() {
    let stake = native_mods::AtStake {
        text: "t".into(),
        worlds: vec!["Home".into()],
        mods: vec!["greeter".into()],
        unbuilt: Vec::new(),
        stay: "Nov 14".into(),
        base: "nightly-2026-11-17-abcdef0".into(),
    };
    let ask = WorldAsk::from_stop("main".into(), &stake);
    assert_eq!(ask.instance, "main");
    assert_eq!(ask.question.mods, ["greeter"]);
    assert_eq!(ask.question.worlds, ["Home"]);
    assert!(matches!(&ask.then, Then::Build(base) if base == "nightly-2026-11-17-abcdef0"));
}

#[test]
fn only_a_build_that_stopped_for_another_reason_rolls_its_install_back() {
    use native_mods::Failed;
    let ok: Result<(), Failed> = Ok(());
    assert!(matches!(BuildEnd::of(&ok), BuildEnd::Finished));
    let nothing: Result<(), Failed> = Err(Failed::Nothing);
    assert!(matches!(BuildEnd::of(&nothing), BuildEnd::Finished));
    let consent: Result<(), Failed> = Err(Failed::NoConsent);
    assert!(
        matches!(BuildEnd::of(&consent), BuildEnd::Pending),
        "a question to answer is not a failure"
    );
    let tools: Result<(), Failed> = Err(Failed::Other("download failed".into()));
    assert!(matches!(BuildEnd::of(&tools), BuildEnd::Stopped(why) if why == "download failed"));
}

#[test]
fn a_mod_is_named_by_its_title_from_the_profiles_list_and_by_its_id_only_when_unlisted() {
    let entry = installed::Entry {
        source: SourceId::Sunrise,
        project: "seabound_boats".into(),
        slug: "seabound_boats".into(),
        title: "Seabound Boats".into(),
        kind: PackKind::Mod,
        version: "1.0.0".into(),
        number: "1.0.0".into(),
        file: "sunrise-mods/seabound_boats.smod".into(),
        checksum: "SHA-256:a".into(),
        page: String::new(),
        dependency: false,
        verdict: None,
    };
    let ids = ["seabound_boats".to_string(), "mystery".to_string()];
    assert_eq!(titles(&[entry], &ids), ["Seabound Boats", "mystery"]);
}

#[test]
fn a_card_gets_its_own_failure_for_a_compile_error_and_for_an_interrupted_build() {
    let compile = InstallFailure::ModsDidNotBuild(vec![native_mods::BuildFailure {
        id: "ruby".into(),
        reason: "it doesn't compile".into(),
        log: "error[E0425]".into(),
    }]);
    assert_eq!(
        build_failure_of(Some(&compile), "ruby"),
        Some(("it doesn't compile".into(), "error[E0425]".into()))
    );
    assert_eq!(build_failure_of(Some(&compile), "other"), None);
    let stopped = InstallFailure::Interrupted {
        ids: vec!["ruby".into()],
        reason: "Couldn't connect to the download".into(),
    };
    assert_eq!(
        build_failure_of(Some(&stopped), "ruby"),
        Some((
            "You're offline".into(),
            "Couldn't connect to the download".into()
        ))
    );
    assert_eq!(build_failure_of(Some(&stopped), "other"), None);
    assert_eq!(build_failure_of(None, "ruby"), None);
}

#[test]
fn a_switch_saves_its_pin_only_once_the_mods_moved_without_a_world_question() {
    let switching = || {
        (
            "main".to_string(),
            Some("v1".to_string()),
            instances::Channel::Stable,
        )
    };
    let mut quiet = AutoOutcome::default();
    assert_eq!(settle_switch(&mut quiet, switching()), Some(switching()));
    // The update found a world that would lose a mod: nothing is saved, and answering the
    // question runs the switch again (Switch anyway) instead of an update on the old pin.
    let mut asked = AutoOutcome {
        asks: vec![WorldAsk {
            instance: "main".into(),
            question: sunrise_launcher::worlds::Question {
                mods: vec!["ruby".into()],
                worlds: vec!["Home".into()],
            },
            then: Then::Update,
        }],
        ..AutoOutcome::default()
    };
    assert_eq!(settle_switch(&mut asked, switching()), None);
    assert!(matches!(
        &asked.asks[0].then,
        Then::Switch(Some(v), instances::Channel::Stable) if v == "v1"
    ));
}
