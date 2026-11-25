use super::*;

#[test]
fn confirmation_names_the_picked_build_only_when_switching() {
    let current = "dev-2026-11-22-7cfdf81";
    let picked = "nightly-2026-11-20-aaaaaaa";
    assert_eq!(confirm_label(current, Some(current)), "Keep this build");
    assert_eq!(
        confirm_label(picked, Some(current)),
        "Switch to Nightly · Nov 20"
    );
}

/// `sunrise-launcher benchmark`'s scroll phase in a real launcher window: the pump's tick
/// opens the build picker and schedules the wheel (`Launcher::drain`), the release list
/// arrives only after that, as on a cold start, and the timing waits for it. Then the
/// benchmark's own wheel events move the populated list, and the frames that moved it are
/// its samples.
///
/// Runs in a child of the test binary with scratch homes, no keychain and an unreachable
/// proxy, so the window it opens reads and writes no player's data and fetches nothing.
#[test]
fn the_benchmark_scrolls_the_populated_build_picker_once_its_rows_arrive() {
    const CHILD: &str = "SUNRISE_BENCH_PICKER_TEST";
    let Some(root) = std::env::var_os(CHILD).map(PathBuf::from) else {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let nowhere = "http://127.0.0.1:9";
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "sheets::versions::tests::the_benchmark_scrolls_the_populated_build_picker_once_its_rows_arrive",
                "--nocapture",
            ])
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
            .env("https_proxy", nowhere)
            .env("http_proxy", nowhere)
            .env("all_proxy", nowhere)
            .env_remove("NO_PROXY")
            .env_remove("no_proxy")
            .env_remove("SUNRISE_GITHUB_MIRROR")
            .env_remove("SUNRISE_LAUNCHER_BENCH")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    };
    let mut app = gpui::TestAppContext::single();
    // The window as the launcher opens it: the launcher drawn as a cached view of its root.
    let (shell, cx) = app.add_window_view(crate::window_root::Shell::new);
    let launcher = shell.read_with(cx, |shell, _| shell.launcher());
    let handle = cx.update(|window, _| window.window_handle());
    // The start-up fetch fails at once through the proxy; take its answer, so nothing
    // replaces the list this test gives the picker.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while launcher.read_with(cx, |l, _| matches!(l.releases, Load::Loading)) {
        assert!(
            std::time::Instant::now() < deadline,
            "the release fetch answered"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        launcher.update(cx, |l, cx| l.drain(handle, cx));
        cx.run_until_parked();
    }
    let out = root.join("bench.json");
    launcher.update(cx, |l, _| {
        assert!(
            l.selected_instance().is_some(),
            "a fresh home has a profile"
        );
        // A cold start: the picker opens while the release list is still on its way.
        l.releases = Load::Loading;
        l.bench = Some(crate::perf::Bench::switched_for_test(
            out.clone(),
            crate::perf::List::Builds,
        ));
    });
    // The tick that opens the picker and schedules the wheel, as the pump runs it.
    launcher.update(cx, |l, cx| l.drain(handle, cx));
    cx.run_until_parked();
    let read = |cx: &mut gpui::VisualTestContext| {
        launcher.read_with(cx, |l, _| {
            let bench = l.bench.as_ref().unwrap();
            let list = &l.ui.sheets.versions.scroll.0.borrow().base_handle;
            (
                bench.scrolling(),
                bench.timing_scroll(),
                bench.scroll_samples().len(),
                f32::from(list.offset().y),
            )
        })
    };
    // A frame from the platform: the wheel's callbacks run, then the window draws.
    let frame = |cx: &mut gpui::VisualTestContext| {
        let ran = cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
        ran
    };
    assert_eq!(
        launcher.read_with(cx, |l, _| l.ui.sheet),
        Some(Sheet::Versions)
    );
    for _ in 0..5 {
        assert_eq!(frame(cx), 1, "the wheel looks after every frame");
        let (scrolling, timed, samples, offset) = read(cx);
        assert!(scrolling && !timed, "waiting for the release list, untimed");
        assert_eq!((samples, offset), (0, 0.));
    }
    // The releases arrive, as the pump hands them over.
    let releases: Vec<Release> = (1..=28)
        .flat_map(|day| {
            [10, 11].map(|month| {
                let date = format!("2026-{month}-{day:02}");
                release(&format!("nightly-{date}-aaaaaaa"), &date, true)
            })
        })
        .collect();
    launcher.update(cx, |l, _| l.releases = Load::Ready(releases));
    // The platform's next frame runs the callbacks before it draws: the list is loaded
    // but still laid out empty, which is neither a list that fits nor the timing's start.
    frame(cx);
    let (scrolling, timed, _, _) = read(cx);
    assert!(
        scrolling && !timed,
        "waiting for the loaded rows to be drawn"
    );
    launcher.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    // The frame after the one that drew the long list: from here the phase is timed.
    frame(cx);
    let (scrolling, timed, samples, offset) = read(cx);
    assert!(scrolling && timed, "timed once the rows are drawn");
    assert_eq!(samples, 0, "drawing the rows is no scroll sample");
    assert!(offset < 0., "the first wheel step moved the list: {offset}");
    let mut last = offset;
    for _ in 0..3 {
        frame(cx);
        let (_, _, _, offset) = read(cx);
        assert!(
            offset < last,
            "the list keeps moving down: {offset} after {last}"
        );
        last = offset;
    }
    let (_, _, samples, _) = read(cx);
    assert!(
        samples >= 3,
        "each frame that moved the list is a sample: {samples}"
    );
}

#[test]
fn the_current_unready_build_can_be_downloaded_or_prepared_but_never_kept() {
    assert_eq!(
        version_action(false, || unreachable!(), false),
        Some(VersionAction::Download)
    );
    assert_eq!(
        version_action(true, || false, false),
        Some(VersionAction::Prepare)
    );
    assert_eq!(version_action(true, || true, false), None);
    assert_eq!(
        version_action(true, || true, true),
        Some(VersionAction::Choose)
    );
}

fn release(tag: &str, date: &str, nightly: bool) -> Release {
    Release {
        tag: tag.into(),
        name: tag.into(),
        body: String::new(),
        date: date.into(),
        time: String::new(),
        published_at: String::new(),
        asset: Some((String::new(), 1)),
        prerelease: false,
        nightly: nightly.then(|| "nightly".to_owned()),
    }
}

fn build(tag: &str) -> Line {
    Line::Build(Build {
        tag: tag.into(),
        name: tag.into(),
        about: String::new(),
        installed: false,
    })
}

#[test]
fn arrow_keys_step_over_headings_and_unusable_rows_and_stop_at_the_ends() {
    let lines = [
        Line::Head("November".into()),
        build("a"),
        build("b"),
        Line::Head("October".into()),
        build("c"),
        build("d"),
    ];
    let all = |_: &Build| true;
    assert_eq!(next_row(&lines, None, true, all), Some(1));
    assert_eq!(next_row(&lines, None, false, all), Some(5));
    assert_eq!(next_row(&lines, Some(2), true, all), Some(4));
    assert_eq!(next_row(&lines, Some(4), false, all), Some(2));
    assert_eq!(next_row(&lines, Some(5), true, all), None);
    assert_eq!(next_row(&lines, Some(1), false, all), None);
    let not_c = |b: &Build| b.tag != "c";
    assert_eq!(next_row(&lines, Some(2), true, not_c), Some(5));
    assert_eq!(next_row(&[], None, true, all), None);
}

fn shape(lines: &[Line]) -> Vec<String> {
    lines
        .iter()
        .map(|line| match line {
            Line::Head(text) => format!("# {text}"),
            Line::Build(b) => b.tag.clone(),
        })
        .collect()
}

#[test]
fn only_current_nightly_and_stable_builds_have_published_rows() {
    let mut releases = vec![
        release("nightly-2026-11-17-aaaaaaa", "2026-11-17", true),
        Release {
            prerelease: true,
            ..release("26.3-0.1.1", "2026-11-16", false)
        },
    ];
    assert_eq!(
        shape(&lines(&releases, &[], None, Channel::Nightly, "")),
        ["# Nightly · November 2026", "nightly-2026-11-17-aaaaaaa",]
    );
    releases.push(release("26.3-0.1.0", "2026-10-01", false));
    assert_eq!(
        shape(&lines(&releases, &[], None, Channel::Nightly, "26.3-0.1.0")),
        ["# Stable · October 2026", "26.3-0.1.0"]
    );
}

#[test]
fn dev_versions_are_listed_only_after_opt_in() {
    let tag = "dev-2026-11-21-aaaaaaa";
    let pruned = "dev-2026-11-20-bbbbbbb";
    let releases = [Release {
        prerelease: true,
        nightly: Some(tag.into()),
        ..release(tag, "2026-11-22", false)
    }];
    let installed = [pruned.to_owned()];
    assert!(lines(&releases, &installed, None, Channel::Stable, "").is_empty());
    assert_eq!(
        shape(&lines(&releases, &installed, None, Channel::Dev, "")),
        ["# Installed", pruned, "# Dev · November 2026", tag]
    );
}

#[test]
fn installed_builds_lead_then_the_rest_by_channel_and_month() {
    let releases = [
        release("nightly-2026-10-30-ccccccc", "2026-10-30", true),
        release("v2026.10.13", "2026-10-13", false),
        release("26.3-0.1.0", "2026-10-01", false),
    ];
    let installed = [
        "nightly-2026-11-17-aaaaaaa".to_owned(),
        "nightly-2026-11-16-bbbbbbb".to_owned(),
    ];
    let all = lines(
        &releases,
        &installed,
        Some("nightly-2026-11-16-bbbbbbb"),
        Channel::Nightly,
        "",
    );
    assert_eq!(
        shape(&all),
        [
            "# Installed",
            // The build kept or played leads, whatever its date.
            "nightly-2026-11-16-bbbbbbb",
            "nightly-2026-11-17-aaaaaaa",
            "# Nightly · October 2026",
            "nightly-2026-10-30-ccccccc",
            "# Stable · October 2026",
            "26.3-0.1.0",
        ]
    );
    // A commit or a day finds its build; a heading with nothing under it is gone.
    let found = lines(&releases, &installed, None, Channel::Nightly, "ccccccc");
    assert_eq!(
        shape(&found),
        ["# Nightly · October 2026", "nightly-2026-10-30-ccccccc"]
    );
    assert_eq!(
        shape(&lines(
            &releases,
            &installed,
            None,
            Channel::Nightly,
            "nov 17"
        ))
        .len(),
        2
    );
}
