//! What the Mods screens show, worked out from the registry index and the installed list:
//! pure data in and out, no files, no network, no window. Everything here only reads.
use std::collections::{BTreeSet, HashMap, HashSet};
use sunrise_launcher::browser::installed::Entry;
use sunrise_launcher::i18n::{tr, trf};
use sunrise_launcher_smod::index::{self, Index};
use sunrise_launcher_smod::manifest::Side;

pub(super) mod collection;

/// What a mod is made of: a native source bundle or a content-only `.smod`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum Make {
    Native,
    Content,
}

impl Make {
    pub(super) const ALL: [Make; 2] = [Make::Native, Make::Content];

    pub(super) fn label(self) -> &'static str {
        match self {
            Make::Native => tr("Native"),
            Make::Content => tr("Content"),
        }
    }
}

/// Whether the version the player would get runs on the build they play.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Fit {
    Fits,
    /// Versions exist, none for this build: the newest build one was made for.
    OtherBuild(String),
    /// A version for this build asks for something this Sunrise doesn't have: a newer Sunrise
    /// has it.
    NeedsNewerSunrise,
    /// Taken down, or made for an earlier mod system.
    Refused(String),
    /// Not known yet (no game to judge by): never a reason to hide it.
    Unknown,
}

impl Fit {
    pub(super) fn fits(&self) -> bool {
        matches!(self, Fit::Fits)
    }

    /// Whether "Fits my game" puts it away: it is known not to run on the build played.
    pub(super) fn hides(&self) -> bool {
        !matches!(self, Fit::Fits | Fit::Unknown)
    }

    pub(super) fn reason(&self) -> Option<String> {
        match self {
            Fit::Fits => None,
            // A game id that is a Nightly's is named by its day, any other stays as it is.
            Fit::OtherBuild(game) => Some(match sunrise_launcher::nightly::date_of(game) {
                Some(date) => trf(
                    "Not available for your version (made for the {day} Nightly)",
                    &[("day", &sunrise_launcher::nightly::day(date))],
                ),
                None => trf(
                    "Not available for your version (made for {game})",
                    &[("game", game)],
                ),
            }),
            Fit::NeedsNewerSunrise => Some(tr("Needs a newer Sunrise").to_string()),
            Fit::Refused(why) => Some(why.clone()),
            Fit::Unknown => Some(tr("Checking…").to_string()),
        }
    }
}

/// One mod in Discover.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Card {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) authors: String,
    pub(super) summary: String,
    pub(super) downloads: u64,
    pub(super) make: Make,
    pub(super) categories: Vec<String>,
    pub(super) icon: Option<String>,
    /// The newest version's day, for sorting.
    pub(super) updated: String,
    pub(super) first: String,
    pub(super) fit: Fit,
    /// The registry has no key for its author: its files carry no signature.
    pub(super) unsigned: bool,
    /// A native mod with no prebuilt binary for this game: it is built on this computer.
    pub(super) builds_locally: bool,
    /// A collection's card: how many mods it lists.
    pub(super) members: Option<usize>,
    /// The collection that lists this mod, by name: it shows in Discover only as that card.
    pub(super) part_of: Option<String>,
    /// A library (`collection::libraries`): found by
    /// searching, never in the browse list.
    pub(super) library: bool,
}

pub(super) fn make_of(p: &index::Project) -> Option<Make> {
    match p.kind {
        index::Kind::NativeMod => Some(Make::Native),
        index::Kind::Mod => Some(Make::Content),
        _ => None,
    }
}

pub(super) fn fit_of(index: &Index, p: &index::Project, game: &str) -> Fit {
    // No game to judge by yet: unknown, not "doesn't fit".
    if game.is_empty() {
        return Fit::Unknown;
    }
    if index.installable(p, game).is_some() {
        return Fit::Fits;
    }
    let for_game: Vec<&index::Version> = p.versions.iter().filter(|v| v.game == game).collect();
    if for_game.is_empty() {
        return Fit::OtherBuild(
            p.versions
                .first()
                .map(|v| v.game.clone())
                .unwrap_or_default(),
        );
    }
    if let Some(r) = for_game
        .iter()
        .find_map(|v| index.revoked(&p.id, &v.file.sha256))
    {
        return Fit::Refused(trf("Taken down: {reason}", &[("reason", &r.reason)]));
    }
    if for_game
        .iter()
        .any(|v| !v.capabilities.unknown().is_empty())
    {
        return Fit::NeedsNewerSunrise;
    }
    Fit::Refused(tr("Made for Sunrise's earlier mod system").to_string())
}

/// Every mod and native mod of the registry, for the build `game`. Packs are left out.
pub(super) fn cards(index: &Index, game: &str) -> Vec<Card> {
    let part_of = collection::part_of(index);
    let libraries = collection::libraries(index);
    let mut cards: Vec<Card> = index
        .projects
        .iter()
        .filter_map(|p| {
            let make = make_of(p)?;
            Some(Card {
                id: p.id.clone(),
                name: p.name.clone(),
                authors: p.authors.join(", "),
                summary: p.summary.clone(),
                downloads: p.downloads,
                make,
                categories: p.categories.clone(),
                icon: p.icon.clone(),
                updated: p
                    .versions
                    .iter()
                    .map(|v| v.published.as_str())
                    .max()
                    .unwrap_or("")
                    .into(),
                first: p
                    .versions
                    .iter()
                    .map(|v| v.published.as_str())
                    .min()
                    .unwrap_or("")
                    .into(),
                fit: fit_of(index, p, game),
                // Only claimed when the registry lists no prebuilt at all: which one fits this
                // computer is decided at install, by build and platform.
                unsigned: p.key.is_none(),
                builds_locally: make == Make::Native
                    && index
                        .installable(p, game)
                        .is_some_and(|v| v.native_binaries.is_empty()),
                members: None,
                part_of: part_of.get(p.id.as_str()).map(|name| (*name).to_owned()),
                library: libraries.contains(&p.id),
            })
        })
        .collect();
    let collections = collection::cards(index, &cards);
    cards.extend(collections);
    cards
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Sort {
    Downloads,
    Updated,
    Newest,
    Name,
}

impl Sort {
    pub(super) const ALL: [Sort; 4] = [Sort::Downloads, Sort::Updated, Sort::Newest, Sort::Name];

    pub(super) fn label(self) -> &'static str {
        match self {
            Sort::Downloads => tr("Most downloaded"),
            Sort::Updated => tr("Recently updated"),
            Sort::Newest => tr("Newest"),
            Sort::Name => tr("Name"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Filters {
    pub(super) text: String,
    pub(super) category: Option<String>,
    pub(super) fits_only: bool,
    pub(super) make: Option<Make>,
    pub(super) sort: Sort,
}

impl Default for Filters {
    fn default() -> Self {
        Self {
            text: String::new(),
            category: None,
            fits_only: true,
            make: None,
            sort: Sort::Downloads,
        }
    }
}

/// Indexes into `cards` that pass `f`, in the order asked for.
pub(super) fn discover(cards: &[Card], f: &Filters) -> Vec<usize> {
    let words: Vec<String> = f.text.split_whitespace().map(str::to_lowercase).collect();
    let mut shown: Vec<usize> = (0..cards.len())
        .filter(|&i| {
            let c = &cards[i];
            let hay = format!("{} {} {} {}", c.id, c.name, c.summary, c.authors).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
                && f.category.as_ref().is_none_or(|k| c.categories.contains(k))
                && f.make.is_none_or(|m| c.make == m)
                && (!f.fits_only || !c.fit.hides())
                // Browsing shows a collection's card for its mods and no libraries; a search
                // finds each of them.
                && (!f.text.is_empty() || (c.part_of.is_none() && !c.library))
        })
        .collect();
    match f.sort {
        Sort::Downloads => shown.sort_by_key(|&i| std::cmp::Reverse(cards[i].downloads)),
        Sort::Updated => shown.sort_by(|&a, &b| {
            cards[b]
                .updated
                .cmp(&cards[a].updated)
                .then(cards[b].downloads.cmp(&cards[a].downloads))
        }),
        Sort::Newest => shown.sort_by(|&a, &b| {
            cards[b]
                .first
                .cmp(&cards[a].first)
                .then(cards[b].downloads.cmp(&cards[a].downloads))
        }),
        Sort::Name => shown.sort_by_key(|&i| cards[i].name.to_lowercase()),
    }
    shown
}

/// The categories the mods carry, most used first.
pub(super) fn categories(cards: &[Card]) -> Vec<String> {
    let mut uses: HashMap<&str, usize> = HashMap::new();
    for c in cards {
        for k in &c.categories {
            *uses.entry(k).or_default() += 1;
        }
    }
    let mut all: Vec<(&str, usize)> = uses.into_iter().collect();
    all.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    all.into_iter().map(|(k, _)| k.to_owned()).collect()
}

/// `6814` → `6.8k downloads`.
pub(super) fn downloads(n: u64) -> String {
    let n = match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    };
    trf("{count} downloads", &[("count", &n)])
}

pub(super) fn size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KiB", bytes as f64 / 1024.),
        _ => format!("{:.1} MiB", bytes as f64 / 1_048_576.),
    }
}

pub(super) fn side_label(side: Option<&Side>) -> &'static str {
    match side {
        None | Some(Side::Client) => tr("Not needed on servers"),
        Some(Side::Both) => tr("Servers need it too"),
        Some(Side::Server) => tr("Server only"),
        Some(_) => tr("Server only"),
    }
}

// ---------------------------------------------------------------- the project page

#[derive(Clone, Debug, PartialEq)]
pub(super) struct VersionLine {
    pub(super) number: String,
    pub(super) published: String,
    pub(super) game: String,
    pub(super) fit: Fit,
    pub(super) size: u64,
}

/// Everything the project page shows, read from the index in one go.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Page {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) authors: Vec<String>,
    pub(super) description: String,
    pub(super) license: String,
    pub(super) source: String,
    pub(super) downloads: u64,
    pub(super) updated: String,
    pub(super) categories: Vec<String>,
    pub(super) screenshots: Vec<String>,
    pub(super) icon: Option<String>,
    pub(super) make: Make,
    pub(super) side: &'static str,
    pub(super) versions: Vec<VersionLine>,
    pub(super) fit: Fit,
    /// The newest version that fits: what Add installs.
    pub(super) will_install: Option<VersionLine>,
    /// What the registry says about who made it and what it may do (`trust_and_capabilities`).
    pub(super) trust: Vec<String>,
    /// Its author signs its files.
    pub(super) signed: bool,
    /// Native code made for another game build than the one played: checked before it runs.
    pub(super) another_build: bool,
    /// Mods the newest version lists as part of itself (a registry modpack's rule; a mod rarely has any).
    pub(super) requires: Vec<(String, String)>,
}

pub(super) fn page(
    index: &Index,
    id: &str,
    game: &str,
    trust: Vec<String>,
    another_build: bool,
) -> Option<Page> {
    let p = index.project(id)?;
    let make = make_of(p)?;
    let lines: Vec<VersionLine> = p
        .versions
        .iter()
        .map(|v| VersionLine {
            number: v.version.clone(),
            published: v.published.clone(),
            game: v.game.clone(),
            fit: match (v.game == game, index.revoked(&p.id, &v.file.sha256)) {
                (_, Some(r)) => Fit::Refused(trf("Taken down: {reason}", &[("reason", &r.reason)])),
                _ if !v.capabilities.unknown().is_empty() => {
                    Fit::Refused(tr("Made for Sunrise's earlier mod system").to_string())
                }
                (true, None) => Fit::Fits,
                (false, None) => Fit::OtherBuild(v.game.clone()),
            },
            size: v.file.size,
        })
        .collect();
    let will_install = index
        .installable(p, game)
        .and_then(|v| lines.iter().find(|l| l.number == v.version).cloned());
    Some(Page {
        id: p.id.clone(),
        name: p.name.clone(),
        authors: p.authors.clone(),
        description: p.description.clone(),
        license: p.license.clone(),
        source: p.source.clone(),
        downloads: p.downloads,
        updated: lines
            .iter()
            .map(|l| l.published.clone())
            .max()
            .unwrap_or_default(),
        categories: p.categories.clone(),
        screenshots: p.screenshots.clone(),
        icon: p.icon.clone(),
        make,
        side: side_label(p.versions.first().and_then(|v| v.side.as_ref())),
        fit: fit_of(index, p, game),
        will_install,
        versions: lines,
        trust,
        signed: p.key.is_some(),
        another_build,
        requires: p
            .versions
            .first()
            .map(|v| {
                v.requires
                    .iter()
                    .map(|r| {
                        let name = index
                            .project(&r.id)
                            .map_or(r.id.clone(), |m| m.name.clone());
                        (name, r.version.clone())
                    })
                    .collect()
            })
            .unwrap_or_default(),
    })
}

// ---------------------------------------------------------------- installed

/// What stands between an installed mod and working, shown on its row.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Need {
    /// Taken down by the registry: it can't be turned on.
    TakenDown(String),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Row {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) authors: String,
    pub(super) make: Option<Make>,
    pub(super) version: String,
    pub(super) on: bool,
    pub(super) need: Option<Need>,
    /// A browser-installed mod whose registry status couldn't be checked this run.
    pub(super) registry_unchecked: bool,
    /// The version an update would install.
    pub(super) update: Option<String>,
    /// Installed because another mod needed it.
    pub(super) dependency: bool,
    /// The player chose this version (`sunrise::chosen`): a batch leaves it alone.
    pub(super) pinned: bool,
    /// Not installed by the registry; no registry update or removal.
    pub(super) local: Option<Local>,
    pub(super) icon: Option<String>,
    pub(super) entry: Option<Entry>,
}

/// How a status reads: which of the theme's colours its glyph takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tone {
    Warning,
    Info,
    Muted,
}

/// A row's status: a glyph, a few words, and the whole sentence for the tooltip.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Status {
    pub(super) icon: &'static str,
    pub(super) label: String,
    pub(super) tone: Tone,
    pub(super) tip: String,
}

/// The one button a row may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowAction {
    Update,
}

/// What a row puts in the list's shared columns. Every row has every column: one with nothing
/// to say leaves its status and action cells empty, and the switch's place is always kept, so
/// nothing in a row moves with what another row says.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Cells {
    /// "Native" or "Content"; empty when the registry doesn't say.
    pub(super) kind: &'static str,
    pub(super) version: String,
    pub(super) status: Option<Status>,
    pub(super) action: Option<RowAction>,
    /// The switch can be flipped (a taken-down mod's is drawn dim).
    pub(super) switch: bool,
}

/// The widths of the list's fixed columns on a roomy window.
pub(super) mod column {
    pub(in super::super) const KIND: f32 = 64.;
    pub(in super::super) const VERSION: f32 = 72.;
    pub(in super::super) const STATUS: f32 = 176.;
    pub(in super::super) const ACTION: f32 = 96.;
    pub(in super::super) const SWITCH: f32 = 44.;
}

/// `row`'s cells. `error` is why its last update failed. The status is the first thing that
/// applies, most pressing first; whatever else applies is in its tooltip.
pub(super) fn cells(row: &Row, error: Option<&str>, native_allowed: bool) -> Cells {
    let mut said: Vec<Status> = Vec::new();
    let mut say = |icon, label: String, tone, tip: String| {
        said.push(Status {
            icon,
            label,
            tone,
            tip,
        })
    };
    if let Some(why) = error {
        say(
            "information-circle",
            tr("Update failed").into(),
            Tone::Warning,
            why.to_owned(),
        );
    }
    if let Some(Need::TakenDown(why)) = &row.need {
        say(
            "cancel-01",
            tr("Taken down").into(),
            Tone::Warning,
            trf("Taken down: {reason}", &[("reason", why)]),
        );
    }
    if let Some(to) = &row.update {
        say(
            "refresh",
            trf("{version} is out", &[("version", to)]),
            Tone::Info,
            match update_refusal(row, native_allowed) {
                Some(why) => trf(
                    "Version {version} is out. {why}",
                    &[("version", to), ("why", &why)],
                ),
                None => trf("Version {version} is out", &[("version", to)]),
            },
        );
    }
    if let Some(local) = &row.local {
        say(
            "folder-01",
            match local.github {
                true => tr("From GitHub").into(),
                false => tr("Local file").into(),
            },
            Tone::Muted,
            match local.github {
                true => tr("Get updates from GitHub, not the registry.").into(),
                false => tr("Replace the local file to update this mod.").into(),
            },
        );
    }
    let tip = said
        .iter()
        .map(|s| s.tip.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Cells {
        kind: row.make.map_or("", Make::label),
        version: row.version.clone(),
        status: said.into_iter().next().map(|first| Status { tip, ..first }),
        action: row.update.as_ref().map(|_| RowAction::Update),
        switch: !matches!(row.need, Some(Need::TakenDown(_)))
            && !row.local.as_ref().is_some_and(|l| l.unreadable),
    }
}

impl Row {
    pub(super) fn native(&self) -> bool {
        self.make == Some(Make::Native)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Group {
    Updates,
    Rest,
}

impl Row {
    pub(super) fn group(&self) -> Group {
        if self.update.is_some() {
            Group::Updates
        } else {
            Group::Rest
        }
    }
}

/// What `rows` needs to know about the computer, read by the caller.
pub(super) struct World<'a> {
    pub(super) index: Option<&'a Index>,
    pub(super) game: &'a str,
    pub(super) enabled: &'a HashSet<String>,
    /// The versions the player picked, by project id.
    pub(super) chosen: &'a std::collections::BTreeMap<String, String>,
    /// Native files the browser didn't install.
    pub(super) local: &'a [Local],
}

/// A `.smod` in the game folder that the registry browser didn't install (a local file or a
/// GitHub install): it can be turned on or off but is not managed by the registry.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Local {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) github: bool,
    /// Native code, which needs permission and a build, rather than content only.
    pub(super) native: bool,
    /// The file can't be read: it has no id of its own (the id is only its file name), so it is
    /// never on, needs nothing and has no switch that could act on a working mod's id.
    pub(super) unreadable: bool,
}

/// The registry mods the browser installed, as rows. Packs, files of other sources and files
/// the browser didn't install are not rows (the caller says how many were left alone).
pub(super) fn rows(entries: &[Entry], w: &World) -> Vec<Row> {
    use sunrise_launcher::browser::{sunrise::mod_id, Kind, SourceId};
    let mut out: Vec<Row> = entries
        .iter()
        .filter(|e| e.source == SourceId::Sunrise && e.kind == Kind::Mod)
        .filter_map(|e| {
            let id = mod_id(e)?.to_owned();
            let p = w.index.and_then(|i| i.project(&e.project));
            let native = sunrise_launcher::native_status::is_native_file(&e.file);
            let make = match (p.and_then(make_of), native) {
                (_, true) => Some(Make::Native),
                (m, false) => m,
            };
            let on = w.enabled.contains(&id);
            let taken_down = w.index.and_then(|i| {
                let hash = e.checksum.strip_prefix("SHA-256:").unwrap_or_default();
                i.revoked(&e.project, hash).map(|r| r.reason.clone())
            });
            let need = taken_down.map(Need::TakenDown);
            let update = w
                .index
                .zip(p)
                .and_then(|(i, p)| newer(i, p, &e.version, w.game));
            Some(Row {
                title: p.map_or_else(|| id.clone(), |p| p.name.clone()),
                id,
                authors: p.map(|p| p.authors.join(", ")).unwrap_or_default(),
                make,
                version: e.number.clone(),
                on,
                need,
                registry_unchecked: w.index.is_none(),
                update,
                dependency: e.dependency,
                pinned: w.chosen.contains_key(&e.project),
                local: None,
                icon: p.and_then(|p| p.icon.clone()),
                entry: Some(e.clone()),
            })
        })
        .collect();
    out.extend(w.local.iter().map(|l| {
        let on = !l.unreadable && w.enabled.contains(&l.id);
        Row {
            id: l.id.clone(),
            title: l.name.clone(),
            authors: String::new(),
            make: l.native.then_some(Make::Native),
            version: l.version.clone(),
            on,
            need: None,
            registry_unchecked: false,
            update: None,
            dependency: false,
            pinned: false,
            local: Some(l.clone()),
            icon: None,
            entry: None,
        }
    }));
    sort(&mut out);
    out
}

/// Rows in the order the list shows them: by group, then by name.
pub(super) fn sort(rows: &mut [Row]) {
    rows.sort_by(|a, b| {
        (a.group(), a.title.to_lowercase()).cmp(&(b.group(), b.title.to_lowercase()))
    });
}

impl PartialOrd for Group {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Group {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

/// The version an update would install: the newest for `game` that wasn't taken down, if it is
/// newer than `installed` (the index lists newest first).
pub(super) fn newer(
    index: &Index,
    p: &index::Project,
    installed: &str,
    game: &str,
) -> Option<String> {
    let target = index.installable(p, game)?;
    let at = |v: &str| p.versions.iter().position(|w| w.version == v);
    (at(&target.version)? < at(installed)?).then(|| target.version.clone())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Show {
    All,
    On,
    Off,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct RowFilter {
    pub(super) text: String,
    pub(super) show: Show,
    pub(super) makes: BTreeSet<Make>,
}

impl Default for RowFilter {
    fn default() -> Self {
        Self {
            text: String::new(),
            show: Show::All,
            makes: BTreeSet::new(),
        }
    }
}

/// Indexes into `rows` that pass the filter; the order of `rows` (grouped, by name) is kept.
pub(super) fn visible(rows: &[Row], f: &RowFilter) -> Vec<usize> {
    let words: Vec<String> = f.text.split_whitespace().map(str::to_lowercase).collect();
    (0..rows.len())
        .filter(|&i| {
            let r = &rows[i];
            let hay = format!("{} {} {}", r.id, r.title, r.authors).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
                && match f.show {
                    Show::All => true,
                    Show::On => r.on,
                    Show::Off => !r.on,
                }
                && (f.makes.is_empty() || r.make.is_some_and(|m| f.makes.contains(&m)))
        })
        .collect()
}

// ---------------------------------------------------------------- batches

/// What "Update N mods" would do, shown before anything is downloaded.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct UpdatePlan {
    pub(super) included: Vec<Included>,
    pub(super) skipped: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Included {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) native: bool,
}

/// Why a batch leaves out `r`'s update, if it does.
pub(super) fn update_refusal(r: &Row, native_allowed: bool) -> Option<String> {
    if r.pinned {
        Some(tr("you chose its version: update it on its own").to_string())
    } else if r.native() && !native_allowed {
        Some(tr("native code: allow it first, then update this mod on its own").to_string())
    } else {
        None
    }
}

/// The whole pack's compatible updates, not only the visible rows. Native code is only updated
/// with the player's permission, and a version the player chose stays. A mod taken down is
/// updated: the new file is checked like any install and replaces the one that was taken down.
pub(super) fn plan_updates(rows: &[Row], native_allowed: bool) -> UpdatePlan {
    let mut plan = UpdatePlan::default();
    for r in rows.iter().filter(|r| r.local.is_none()) {
        let Some(to) = &r.update else { continue };
        // A mod that is off is updated like any other and stays off: downloading a newer checked
        // file never switches a mod on, and a native mod that is off builds when it is turned on.
        if let Some(why) = update_refusal(r, native_allowed) {
            plan.skipped.push((r.title.clone(), why));
        } else {
            plan.included.push(Included {
                id: r.id.clone(),
                title: r.title.clone(),
                from: r.version.clone(),
                to: to.clone(),
                native: r.native(),
            });
        }
    }
    plan
}

/// The count on the "Update N mods" button: what [`plan_updates`] would really include, and no
/// button (`None`) when that is nothing.
pub(super) fn update_all_button(rows: &[Row], native_allowed: bool) -> Option<usize> {
    Some(plan_updates(rows, native_allowed).included.len()).filter(|n| *n > 0)
}

/// Which of the chosen mods can be turned on now, and which can't (and why).
pub(super) fn turn_on_plan<'a>(
    chosen: &[&'a Row],
    native_allowed: bool,
) -> (Vec<&'a Row>, Vec<(&'a Row, String)>) {
    let (mut ok, mut no) = (Vec::new(), Vec::new());
    for r in chosen {
        match &r.need {
            Some(Need::TakenDown(why)) => {
                no.push((*r, trf("taken down: {reason}", &[("reason", why)])))
            }
            _ if r.native() && !native_allowed => no.push((
                *r,
                tr("native code needs your permission first").to_string(),
            )),
            _ => ok.push(*r),
        }
    }
    (ok, no)
}

/// Installed mods that are not being removed but whose newest registry version lists a removed
/// mod among what it needs: "(name, [needed names])".
pub(super) fn dependents(
    rows: &[Row],
    removing: &HashSet<String>,
    index: Option<&Index>,
) -> Vec<(String, Vec<String>)> {
    let Some(index) = index else {
        return Vec::new();
    };
    rows.iter()
        .filter(|r| !removing.contains(&r.id))
        .filter_map(|r| {
            let e = r.entry.as_ref()?;
            let p = index.project(&e.project)?;
            let v = p.versions.iter().find(|v| v.version == e.version)?;
            let needed: Vec<String> = v
                .requires
                .iter()
                .filter(|q| removing.contains(&q.id))
                .map(|q| {
                    index
                        .project(&q.id)
                        .map_or(q.id.clone(), |m| m.name.clone())
                })
                .collect();
            (!needed.is_empty()).then(|| (r.title.clone(), needed))
        })
        .collect()
}

#[cfg(test)]
mod tests;
