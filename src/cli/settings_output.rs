use super::*;

pub(super) fn settings_json(s: &backend::Settings) -> serde_json::Value {
    serde_json::json!({
        "selected_version": backend::resolve_version(None).ok(),
        "game_architecture": s.game_architecture.name(),
        "minecraft_dir": s.minecraft_dir,
        "server_dir": backend::server_dir(None),
        "install_new_versions": s.install_new_versions.unwrap_or(true),
        "prune_old_versions": s.auto_prune(),
        "keep_versions": s.keep_newest(),
        "opening_animation": !s.skip_opening,
        "game_args": s.game_args,
        "proxy": s.proxy,
        "desktop_entry": s.desktop_entry.unwrap_or(true),
        "while_playing": s.while_playing.name(),
        "language": s.language.map_or("system", |l| l.code()),
        "theme": sunrise_launcher::appearance::effective_id(s),
        "theme_choice": s.theme,
        "accent": sunrise_launcher::appearance::accent(s).name(),
        "scenery": s.scenery(),
        "themes_dir": sunrise_launcher::theme_files::dir(),
        "background": sunrise_launcher::themes::background(&backend::data_dir(), s.background.as_deref()),
        "cache_limit_gib": s.cache_limit_gib.unwrap_or(sunrise_launcher::storage::policy::DEFAULT_LIMIT_GIB),
        "no_ai": backend::no_ai(),
    })
}
