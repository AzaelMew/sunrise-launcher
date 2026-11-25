//! `update`: the update check for Sunrise and the launcher, and installing what is newer.
use super::*;

pub(super) fn update(check: bool, as_json: bool) -> Result<(), Error> {
    let updates = backend::check_updates();
    // A feed that couldn't be read is never "up to date": its error is printed, and the command fails after the other feed's result.
    let failed = || match (&updates.sunrise_error, &updates.launcher_error) {
        (None, None) => Ok(()),
        (Some(e), None) => Err(format!("Sunrise's update check failed: {e}")),
        (None, Some(e)) => Err(format!("The launcher's update check failed: {e}")),
        (Some(s), Some(l)) => Err(format!(
            "Both update checks failed: Sunrise's ({s}) and the launcher's ({l})"
        )),
    };
    if as_json && check {
        json(serde_json::json!({
            "sunrise": updates.sunrise.as_ref().map(|r| &r.tag),
            "launcher": updates.launcher.as_ref().and_then(|r| backend::launcher_version(&r.tag)),
            "launcher_current": backend::VERSION,
            "sunrise_error": updates.sunrise_error,
            "launcher_error": updates.launcher_error,
        }));
        return failed().map_err(Error::from);
    }
    match &updates.sunrise {
        None if updates.sunrise_error.is_some() => println!("Sunrise: not checked."),
        None => println!("Sunrise: up to date."),
        Some(r) if check => println!("Sunrise: {} is available.", r.tag),
        Some(r) => {
            let tag =
                backend::install_tag(&r.tag, true, progress(format!("Installing {}", r.tag)))?;
            println!("Sunrise: installed {tag}.");
        }
    }
    match &updates.launcher {
        None if updates.launcher_error.is_some() => println!("Launcher: not checked."),
        None => println!("Launcher: {} is up to date.", backend::VERSION),
        Some(r) => {
            let version = backend::launcher_version(&r.tag).unwrap_or_default();
            if check {
                println!(
                    "Launcher: {version} is available (this is {}).",
                    backend::VERSION
                );
            } else if !backend::self_update_allowed() {
                println!("Launcher: {version} is available; this build does not replace itself.");
            } else {
                backend::self_update(r, progress(format!("Updating the launcher to {version}")))?;
                println!("Launcher: updated to {version}. Restart the launcher to use it.");
            }
        }
    }
    failed().map_err(Error::from)
}
