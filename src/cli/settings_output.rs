use super::*;

pub(super) fn settings_json(s: &backend::Settings) -> serde_json::Value {
    settings_json_in(
        s,
        &backend::data_dir(),
        &sunrise_launcher::theme_files::dir(),
    )
}

/// The settings as the command prints them, with the look's folders under `data` and `themes`.
/// A folder goes out as the text a player would read: a path that isn't Unicode (any path is
/// allowed on Unix) is shown with its odd bytes replaced, never a reason to fail after a
/// setting was already saved.
fn settings_json_in(s: &backend::Settings, data: &Path, themes: &Path) -> serde_json::Value {
    serde_json::json!({
        "selected_version": backend::resolve_version(None).ok(),
        "game_architecture": s.game_architecture.name(),
        "minecraft_dir": s.minecraft_dir,
        "server_dir": backend::server_dir(None),
        "install_new_versions": s.install_new_versions.unwrap_or(true),
        "prune_old_versions": s.auto_prune(),
        "keep_versions": s.keep_newest(),
        "opening_animation": !s.skip_opening,
        "cat": s.show_cat(),
        "game_args": s.game_args,
        "proxy": s.proxy,
        "desktop_entry": s.desktop_entry.unwrap_or(true),
        "while_playing": s.while_playing.name(),
        "language": s.language.map_or("system", |l| l.code()),
        "theme": sunrise_launcher::appearance::effective_id(s),
        "theme_choice": s.theme,
        "accent": sunrise_launcher::appearance::accent(s).name(),
        "scenery": s.scenery(),
        "themes_dir": themes.display().to_string(),
        "background": sunrise_launcher::themes::background(data, s.background.as_deref())
            .map(|kept| kept.display().to_string()),
        "cache_limit_gib": s.cache_limit_gib.unwrap_or(sunrise_launcher::storage::policy::DEFAULT_LIMIT_GIB),
        "no_ai": backend::no_ai(),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt as _;

    /// The data folder may be any path the system allows: one that isn't Unicode is reported,
    /// with a kept picture in it, and nothing panics.
    #[test]
    fn folders_that_are_not_unicode_are_reported_not_a_panic() {
        let root = tempfile::tempdir().unwrap();
        let data = root
            .path()
            .join(std::ffi::OsStr::from_bytes(b"launcher-\xff\xfe"));
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("background-0123456789ab.png"), b"kept").unwrap();
        let s = backend::Settings::parse(r#"{"background": "background-0123456789ab.png"}"#);
        let shown = settings_json_in(&s, &data, &data.join("themes"));
        let themes = shown["themes_dir"].as_str().unwrap();
        assert!(
            themes.contains("launcher-") && themes.ends_with("themes"),
            "{themes}"
        );
        assert!(shown["background"]
            .as_str()
            .unwrap()
            .ends_with("background-0123456789ab.png"));
        // With no kept picture the field is null, as before.
        let none = settings_json_in(&backend::Settings::parse("{}"), &data, &data.join("themes"));
        assert!(none["background"].is_null());
    }
}
