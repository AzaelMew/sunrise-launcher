//! The list the sheet draws: installed builds first, then every downloadable one by channel and
//! month, and which row the arrow keys land on.
use super::*;

/// The line the Up or Down key lands on from line `from` (None: no row has the keyboard):
/// the nearest build in that direction that can be chosen, or none at the end of the list.
pub(super) fn next_row(
    lines: &[Line],
    from: Option<usize>,
    down: bool,
    usable: impl Fn(&Build) -> bool,
) -> Option<usize> {
    let ok = |n: &usize| matches!(&lines[*n], Line::Build(b) if usable(b));
    match (from, down) {
        (None, true) => (0..lines.len()).find(ok),
        (None, false) => (0..lines.len()).rev().find(ok),
        (Some(n), true) => (n + 1..lines.len()).find(ok),
        (Some(n), false) => (0..n.min(lines.len())).rev().find(ok),
    }
}

/// A line of the list.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Line {
    Head(String),
    Build(Build),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Build {
    pub(super) tag: String,
    /// "Nov 17", or a release's own name.
    pub(super) name: String,
    /// "Nightly · 0c02067", or a release's day.
    pub(super) about: String,
    pub(super) installed: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum VersionAction {
    Download,
    Prepare,
    Choose,
}

pub(super) fn confirm_label(tag: &str, current: Option<&str>) -> String {
    if Some(tag) == current {
        tr("Keep this build").into()
    } else {
        trf("Switch to {build}", &[("build", &build_name(tag))])
    }
}

pub(super) fn version_action(
    installed: bool,
    prepared: impl FnOnce() -> bool,
    can_switch: bool,
) -> Option<VersionAction> {
    if !installed {
        Some(VersionAction::Download)
    } else if !prepared() {
        Some(VersionAction::Prepare)
    } else {
        can_switch.then_some(VersionAction::Choose)
    }
}

/// "November 2026" for `2026-11`.
fn month(year_month: &str) -> String {
    let (year, month) = year_month.split_once('-').unwrap_or((year_month, ""));
    let name = match month {
        "01" => tr("January"),
        "02" => tr("February"),
        "03" => tr("March"),
        "04" => tr("April"),
        "05" => tr("May"),
        "06" => tr("June"),
        "07" => tr("July"),
        "08" => tr("August"),
        "09" => tr("September"),
        "10" => tr("October"),
        "11" => tr("November"),
        "12" => tr("December"),
        _ => return year_month.to_owned(),
    };
    format!("{name} {year}")
}

fn build(tag: &str, day: &str, installed: bool) -> Build {
    Build {
        tag: tag.to_owned(),
        name: build_name(tag),
        about: sunrise_launcher::nightly::date_of(tag)
            .or_else(|| backend::dev_date(tag))
            .and_then(|_| tag.splitn(5, '-').nth(4))
            .map_or_else(|| day.to_owned(), sunrise_launcher::nightly::short_commit),
        installed,
    }
}

/// The list for `query`: the installed builds (`current` first), then the builds of `releases`
/// that can be downloaded and aren't installed, under a heading per channel and month.
/// `releases` is newest first, as it is listed. Every word of `query` has to be in a build's
/// version, day or name.
pub(super) fn lines(
    releases: &[Release],
    installed: &[String],
    current: Option<&str>,
    channel: Channel,
    query: &str,
) -> Vec<Line> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let day_of = |tag: &str| {
        sunrise_launcher::nightly::date_of(tag)
            .or_else(|| backend::dev_date(tag))
            .map(str::to_owned)
            .or_else(|| {
                releases
                    .iter()
                    .find(|r| r.tag == tag)
                    .map(|r| r.date.clone())
            })
            .unwrap_or_default()
    };
    let wanted = |b: &Build, day: &str| {
        let hay = format!("{} {} {} {day}", b.tag, b.name, b.about).to_lowercase();
        words.iter().all(|w| hay.contains(w.as_str()))
    };
    // Launcher releases are separate. Dev is opt-in, but keep a profile's exact current build
    // visible even if it has another saved channel (for example, a copy of a Dev profile).
    let game = |tag: &str| {
        !native_mods::is_native(tag)
            && (channel == Channel::Dev || Some(tag) == current || backend::dev_date(tag).is_none())
    };
    let mut out = Vec::new();
    let mut have: Vec<&String> = installed.iter().filter(|t| game(t)).collect();
    have.sort_by(|a, b| {
        (Some(b.as_str()) == current)
            .cmp(&(Some(a.as_str()) == current))
            .then_with(|| sunrise_launcher::backend::compare_game_versions(b, a))
    });
    let have: Vec<Line> = have
        .into_iter()
        .map(|tag| (build(tag, &day_of(tag), true), day_of(tag)))
        .filter(|(b, day)| wanted(b, day))
        .map(|(b, _)| Line::Build(b))
        .collect();
    if !have.is_empty() {
        out.push(Line::Head(tr("Installed").to_string()));
        out.extend(have);
    }
    for (listed, title) in [
        (Channel::Nightly, tr("Nightly")),
        (Channel::Stable, tr("Stable")),
        (Channel::Dev, tr("Dev")),
    ] {
        if listed == Channel::Dev && channel != Channel::Dev {
            continue;
        }
        let mut heading = String::new();
        for r in releases.iter().filter(|r| {
            r.asset.is_some()
                && Channel::of_release(r) == Some(listed)
                && !installed.contains(&r.tag)
        }) {
            let day = day_of(&r.tag);
            let b = build(&r.tag, &day, false);
            if !wanted(&b, &day) {
                continue;
            }
            let under = trf(
                "{channel} · {month}",
                &[
                    ("channel", title),
                    ("month", &month(day.get(..7).unwrap_or(&day))),
                ],
            );
            if under != heading {
                out.push(Line::Head(under.clone()));
                heading = under;
            }
            out.push(Line::Build(b));
        }
    }
    out
}
