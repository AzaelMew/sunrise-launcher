//! The SDK sends a resolver refusal as text, with no structured blame. The pin summary tells
//! us what the game needs, but only Cargo's dependency report tells us which mod needs otherwise.
use super::{ConflictRequirement, Failed, RegistryConflict};
use semver::{Version, VersionReq};

pub(super) fn classify<'a>(error: String, mods: impl Iterator<Item = &'a str>) -> Failed {
    let conflicts = conflicts(&error, mods);
    if conflicts.is_empty() {
        Failed::Tool {
            retryable: retryable(&error),
            error,
        }
    } else {
        Failed::RegistryConflict { conflicts, error }
    }
}

fn conflicts<'a>(error: &str, mods: impl Iterator<Item = &'a str>) -> Vec<RegistryConflict> {
    let mods = mods.collect::<Vec<_>>();
    let pins: Vec<_> = error
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(": the game locks ")?;
            let (game, moved) =
                rest.split_once(", but resolution without the game's constraint selected ")?;
            Some((
                name,
                game.parse::<Version>().ok()?,
                moved.strip_suffix('.')?.parse::<Version>().ok()?,
            ))
        })
        .collect();
    let mut conflicts = Vec::new();
    for report in error.split("\nerror: ").skip(1) {
        let Some((headline, report)) = report.split_once('\n') else {
            continue;
        };
        let (name, heading_req) = if let Some((name, req)) = headline
            .strip_prefix("failed to select a version for the requirement `")
            .and_then(|tail| tail.strip_suffix('`'))
            .and_then(|value| value.split_once(" = "))
        {
            (name, Some(req.trim_matches('"')))
        } else if let Some(name) = headline
            .strip_prefix("failed to select a version for `")
            .and_then(|tail| tail.strip_suffix("`."))
        {
            (name, None)
        } else {
            continue;
        };
        let (requiring, selected) = report
            .split_once("previously selected package `")
            .unwrap_or((report, ""));
        let requiring_req = heading_req.or_else(|| {
            requiring.lines().find_map(|line| {
                line.trim()
                    .strip_prefix("versions that meet the requirements `")?
                    .split_once('`')
                    .map(|(req, _)| req)
            })
        });
        let feature = requiring.lines().find_map(|line| {
            let (_, rest) = line
                .trim()
                .strip_prefix("package `")?
                .split_once("` depends on `")?;
            let (dependency, rest) = rest.split_once("` with feature `")?;
            let (feature, _) = rest.split_once("` but ")?;
            (dependency == name).then_some(feature)
        });
        // Cargo can report the game pin as the requirement and the mod as the previously
        // selected package. Read both sides, but never blame a mod whose requirement fits.
        let selected_version = selected.lines().next().and_then(|line| {
            let (selected_name, version) = line.strip_suffix('`')?.split_once(" v")?;
            (selected_name == name)
                .then_some(version)?
                .parse::<Version>()
                .ok()
        });
        let selected_req = selected.lines().find_map(|line| {
            let (dependency, rest) = line
                .trim()
                .strip_prefix("... which satisfies dependency `")?
                .split_once(" = \"")?;
            (dependency == name)
                .then_some(rest)?
                .split_once("\"`")
                .map(|(req, _)| req)
        });
        for (_, game, moved) in pins.iter().filter(|(package, _, _)| *package == name) {
            let incompatible = |req: &str| {
                req.parse::<VersionReq>()
                    .is_ok_and(|r| !r.matches(game) && r.matches(moved))
            };
            for (chain, req, features) in [
                (
                    requiring,
                    requiring_req,
                    feature.filter(|_| selected_version.as_ref() == Some(game)),
                ),
                (
                    selected,
                    selected_req.filter(|_| selected_version.as_ref() == Some(moved)),
                    None,
                ),
            ] {
                for line in chain.lines() {
                    let Some((_, path)) = line
                        .trim_end()
                        .strip_suffix(")`")
                        .and_then(|line| line.split_once(" package `"))
                        .and_then(|(_, package)| package.split_once(" ("))
                    else {
                        continue;
                    };
                    // The directory is the manifest ID, not Cargo's crate name (often mod_N).
                    // Match both slash conventions without opening the worker's workspace.
                    let path = path.replace('\\', "/");
                    let Some((_, relative)) = path.rsplit_once("/native-api/") else {
                        continue;
                    };
                    let id = relative.split('/').next().unwrap_or_default();
                    if !mods.contains(&id) {
                        continue;
                    }
                    let requirement = if let Some(feature) = features {
                        ConflictRequirement::Feature(feature.to_owned())
                    } else if let Some(req) = req.filter(|req| incompatible(req)) {
                        ConflictRequirement::Version(req.to_owned())
                    } else {
                        continue;
                    };
                    let conflict = RegistryConflict {
                        mod_id: id.to_owned(),
                        package: name.to_owned(),
                        requirement,
                        game: game.to_string(),
                    };
                    if !conflicts.contains(&conflict) {
                        conflicts.push(conflict);
                    }
                }
            }
        }
    }
    conflicts
}

/// Cargo's reported transport failures, not resolver refusals, cache errors or an exit status
/// alone. Use the error chain, not a package name merely containing these words.
pub(super) fn retryable(error: &str) -> bool {
    error.lines().map(str::trim).any(|line| {
        let line = line.strip_prefix("error: ").unwrap_or(line);
        line.starts_with("failed to download any data for `")
            || (line.starts_with("failed to get successful HTTP response from `")
                && line
                    .rsplit_once(", got ")
                    .and_then(|(_, code)| code.parse::<u16>().ok())
                    .is_some_and(|code| (500..600).contains(&code)))
            || line.starts_with("[6] Could not resolve hostname")
            || line.starts_with("[7] Could not connect to server")
            || line.starts_with("[28] Timeout was reached")
    })
}
