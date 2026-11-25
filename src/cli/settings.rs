//! `settings`: the keys it sets and how their values are read.
use super::*;

/// The keys `settings` sets.
pub(super) const SETTING_KEYS: [&str; 19] = [
    "selected-version",
    "minecraft-dir",
    "server-dir",
    "prune-old-versions",
    "keep-versions",
    "opening-animation",
    "cat",
    "game-architecture",
    "proxy",
    "while-playing",
    "worlds",
    "language",
    "theme",
    "accent",
    "background",
    "scenery",
    "panel-opacity",
    "reduced-graphics",
    "cache-limit-gib",
];

pub(super) fn on_off(value: &str) -> Result<bool, String> {
    match value {
        "on" | "true" => Ok(true),
        "off" | "false" => Ok(false),
        _ => Err(format!("{value}: say on or off")),
    }
}

pub(super) fn set_setting(s: &mut backend::Settings, key: &str, value: &str) -> Result<(), String> {
    let folder = |value: &str| (value != "default").then(|| PathBuf::from(value));
    match key {
        "selected-version" => s.selected = Some(backend::resolve_version(Some(value))?),
        "minecraft-dir" => s.minecraft_dir = folder(value),
        "server-dir" => s.server_dir = folder(value),
        "prune-old-versions" => s.prune_old_versions = Some(on_off(value)?),
        "keep-versions" => {
            s.keep_versions = match value {
                "default" => None,
                n => Some(
                    n.parse::<usize>()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| format!("{value}: say a number from 1, or default"))?,
                ),
            }
        }
        "opening-animation" => s.skip_opening = !on_off(value)?,
        "cat" => s.cat = Some(on_off(value)?),
        "game-architecture" => {
            let choice = backend::GameArchitecture::parse(value).ok_or_else(|| {
                tr("Say auto, x64 or arm64 for the game architecture.").to_string()
            })?;
            if choice != backend::GameArchitecture::Auto && !backend::can_choose_game_architecture()
            {
                return Err(
                    tr("Manual game architectures are available on Windows ARM only.").into(),
                );
            }
            s.game_architecture = choice;
        }
        "while-playing" => {
            s.while_playing = backend::WhilePlaying::parse(value)
                .ok_or_else(|| format!("{value}: say stay, minimize or hide"))?
        }
        "worlds" => {
            s.world_layout = backend::WorldLayout::parse(value).ok_or_else(|| {
                trf(
                    "{value}: say {one} or {other}",
                    &[("value", value), ("one", "rows"), ("other", "tiles")],
                )
            })?
        }
        "theme" => sunrise_launcher::appearance::select_theme(s, value)?,
        "accent" => sunrise_launcher::appearance::type_accent(s, value)?,
        "background" => match value.trim() {
            "default" | "" => s.background = None,
            file => sunrise_launcher::appearance::set_background(
                s,
                &backend::data_dir(),
                std::path::Path::new(file),
            )?,
        },
        "scenery" => s.scenery_on = Some(on_off(value)?),
        "panel-opacity" => {
            let percent =
                match value.trim() {
                    "default" => None,
                    value => Some(value.parse::<u8>().ok().filter(|n| *n <= 100).ok_or_else(
                        || {
                            tr("Enter a whole panel opacity percentage from 0 to 100, or default.")
                                .to_string()
                        },
                    )?),
                };
            s.set_panel_opacity(percent);
        }
        "reduced-graphics" => s.reduced_graphics = Some(on_off(value)?),
        "language" => {
            use sunrise_launcher::i18n::Language;
            s.language = match value.trim() {
                "system" | "default" => None,
                code => Some(Language::parse(code).ok_or_else(|| {
                    let codes: Vec<&str> = Language::ALL.iter().map(|l| l.code()).collect();
                    format!("{value}: say system or one of {}", codes.join(", "))
                })?),
            }
        }
        "cache-limit-gib" => {
            s.cache_limit_gib = match value {
                "default" => None,
                n => Some(
                    n.parse::<u64>()
                        .ok()
                        .filter(|n| matches!(n, 0 | 10 | 25 | 50 | 100))
                        .ok_or_else(|| {
                            tr("Enter a cache limit in GiB; 0 disables automatic cleanup.")
                                .to_string()
                        })?,
                ),
            };
        }
        "proxy" => {
            if value == "default" || value.is_empty() {
                s.proxy = None;
            } else if let Some(why) = sunrise_launcher::proxy::refuse(value) {
                return Err(why);
            } else {
                s.proxy = Some(value.trim().to_string());
            }
        }
        _ => return Err(format!("No setting {key}.")),
    }
    Ok(())
}
