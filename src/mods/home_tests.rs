use super::*;

#[test]
fn home_finds_resource_packs_after_leaving_a_different_installed_shelf() {
    in_scratch_launcher(
        "mods::home_tests::home_finds_resource_packs_after_leaving_a_different_installed_shelf",
        |launcher| {
            for shelf in [Shelf::Shader, Shelf::Data] {
                launcher.ui.page = Page::Home;
                launcher.ui.mods.shelf = shelf;
                launcher.ui.mods.packs.status = pack_discover::Status::Idle;
                let before = launcher.ui.mods.packs.seq;
                launcher.show_resource_pack_discover();
                assert!(launcher.ui.page == Page::PackFind);
                assert_eq!(launcher.ui.mods.packs.scope, pack_discover::Scope::Resource);
                assert_eq!(
                    launcher.ui.mods.packs.seq,
                    before + 2,
                    "one scope change and search"
                );
            }
        },
    );
}

// Launcher startup uses process settings and starts workers. Run it with its own folders,
// no keychain, and a proxy that can't reach the registry or update services.
fn in_scratch_launcher(test: &str, run: impl FnOnce(&mut Launcher)) {
    const CHILD: &str = "SUNRISE_HOME_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let root = tempfile::tempdir().unwrap();
        let nowhere = "http://127.0.0.1:9";
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env(CHILD, "1")
            .env("SUNRISE_LAUNCHER_HOME", root.path().join("launcher"))
            .env("SUNRISE_HOME", root.path().join("game"))
            .env(
                "SUNRISE_LAUNCHER_LOCATION_FILE",
                root.path().join("location.json"),
            )
            .env("SUNRISE_NO_KEYCHAIN", "1")
            .env("SUNRISE_TELEMETRY_URL", "off")
            .env("SUNRISE_LAUNCHER_NO_SELF_UPDATE", "1")
            .env("XDG_DATA_HOME", root.path().join("data"))
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .env("HTTPS_PROXY", nowhere)
            .env("HTTP_PROXY", nowhere)
            .env("ALL_PROXY", nowhere)
            .env_remove("NO_PROXY")
            .env_remove("SUNRISE_GITHUB_MIRROR")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let mut app = gpui::TestAppContext::single();
    let (shell, cx) = app.add_window_view(crate::window_root::Shell::new);
    let launcher = shell.read_with(cx, |shell, _| shell.launcher());
    launcher.update(cx, |launcher, _| run(launcher));
}
