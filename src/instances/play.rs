//! A run of the game: its play time, and the instance's folder and options for the launch.
use super::*;

/// Adds a finished run of the game to the instance's play time (by id: a renamed instance keeps
/// it).
/// Complete the accepted run, not the profile (if any) currently using its name.
/// The callback must run synchronously so the profile cannot be replaced during the hook.
pub fn finish_play(
    instance: &Instance,
    played: Duration,
    hook: impl FnOnce(),
) -> Result<(), String> {
    finish_play_in(
        &backend::game_home(),
        &backend::data_dir(),
        instance,
        played,
        hook,
    )
}

pub(super) fn finish_play_in(
    game: &Path,
    data: &Path,
    instance: &Instance,
    played: Duration,
    hook: impl FnOnce(),
) -> Result<(), String> {
    let expected = instance.incarnation.ok_or_else(lifecycle::changed)?;
    let owner = lifecycle::owner_file(game, &instance.home)?;
    let profile =
        crate::profile_txn::Admission::claim_after_exit(&instance.home, owner.as_deref())?;
    expected.check(instance, profile.identity())?;
    let saved = record_play_in(data, &instance.id, played, SystemTime::now());
    hook();
    saved
}

pub(super) fn record_play_in(
    data: &Path,
    id: &str,
    played: Duration,
    ended: SystemTime,
) -> Result<(), String> {
    Store::mutate(data, |store| {
        let meta = store.instances.entry(id.to_string()).or_default();
        meta.played_secs = meta.played_secs.saturating_add(played.as_secs());
        meta.last_played = ended.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs());
        Ok(())
    })
}

/// "12 min", "3 h 05 min", "140 h": the play time as players read it.
pub fn play_time(played: Duration) -> String {
    let minutes = played.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => crate::i18n::tr("under a minute").into(),
        (0, m) => format!("{m} min"),
        (h, m) if h < 100 => format!("{h} h {m:02} min"),
        (h, _) => format!("{h} h"),
    }
}

/// Turns the game's command into this instance: its own game folder, and a pack's mods. The
/// player's answers that hold everywhere follow it in: data collection and No AI as the
/// launcher has them, and options.txt and the offline name when it has none yet.
pub fn apply(cmd: &mut Command, instance: &Instance) -> Result<(), String> {
    match instance.kind {
        Kind::Default => return Ok(()),
        Kind::Own => {
            fs::create_dir_all(&instance.home)
                .map_err(|e| format!("{}: {e}", instance.home.display()))?;
            cmd.current_dir(&instance.home)
                .env("SUNRISE_HOME", &instance.home);
        }
        Kind::Pack => return Err(registry_pack::play_unavailable()),
    }
    follow_player(&backend::game_home(), &instance.home);
    Ok(())
}

pub(super) fn follow_player(game_home: &Path, home: &Path) {
    for f in PLAYER_FILES {
        if !home.join(f).exists() && game_home.join(f).is_file() {
            let _ = fs::copy(game_home.join(f), home.join(f));
        }
    }
    let consent = sunrise_launcher_telemetry::consent::Consent::path(game_home);
    if consent.is_file() {
        let to = sunrise_launcher_telemetry::consent::Consent::path(home);
        let _ = fs::create_dir_all(to.parent().unwrap());
        let _ = fs::copy(consent, to);
    }
    let no_ai = crate::no_ai::is_on(game_home);
    if crate::no_ai::is_on(home) != no_ai {
        let _ = crate::no_ai::set(home, no_ai);
    }
}
