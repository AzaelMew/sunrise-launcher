//! Collections in Mods: what Discover folds into a collection's card, which projects are
//! libraries, and what a collection's page lists and installs. Pure data in and out; the install
//! plan is the registry planner's own (`index::resolve`).
use super::{Card, Fit, Make};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use sunrise_launcher::i18n::{tr, trf};
use sunrise_launcher_smod::index::resolve::{self, InstallPlan, InstalledMod, RequestResult};
use sunrise_launcher_smod::index::{Collection, Index};
use sunrise_launcher_smod::manifest::Side;

/// The first collection that lists each member, by name: what a member's card says it is part of.
pub(in crate::mods) fn part_of(index: &Index) -> HashMap<&str, &str> {
    let mut of = HashMap::new();
    for c in &index.collections {
        for m in &c.members {
            of.entry(m.id.as_str()).or_insert(c.name.as_str());
        }
    }
    of
}

/// The registry marks no library, so the rule is: a mod that another listed mod's newest version
/// requires, and that no collection lists as a member. A member a collection's own mods need
/// (the creature guide, say) stays a member of its collection, on that collection's page.
pub(in crate::mods) fn libraries(index: &Index) -> HashSet<String> {
    let listed: HashSet<&str> = index
        .collections
        .iter()
        .flat_map(|c| c.members.iter().map(|m| m.id.as_str()))
        .collect();
    index
        .projects
        .iter()
        .filter_map(|p| p.versions.first().and_then(|v| v.dependencies.as_ref()))
        .flatten()
        .filter(|d| !d.optional)
        .map(|d| d.id.as_str())
        .filter(|id| !listed.contains(id))
        .filter(|id| index.project(id).and_then(super::make_of).is_some())
        .map(str::to_owned)
        .collect()
}

/// The collections' own cards for Discover: their members' downloads and newest day added up,
/// fitting when any member fits the build.
pub(in crate::mods) fn cards(index: &Index, mods: &[Card]) -> Vec<Card> {
    index
        .collections
        .iter()
        .filter_map(|c| {
            let members: Vec<&Card> = c
                .members
                .iter()
                .filter_map(|m| mods.iter().find(|card| card.id == m.id))
                .collect();
            let first = members.first()?;
            let mut categories: Vec<String> = Vec::new();
            for k in members.iter().flat_map(|m| &m.categories) {
                if !categories.contains(k) {
                    categories.push(k.clone());
                }
            }
            Some(Card {
                id: c.id.clone(),
                name: c.name.clone(),
                authors: c.owners.join(", "),
                summary: c.summary.clone(),
                downloads: members.iter().map(|m| m.downloads).sum(),
                make: if members.iter().any(|m| m.make == Make::Native) {
                    Make::Native
                } else {
                    Make::Content
                },
                categories,
                icon: Some(c.icon.clone()),
                updated: members
                    .iter()
                    .map(|m| m.updated.clone())
                    .max()
                    .unwrap_or_default(),
                first: members
                    .iter()
                    .map(|m| m.first.clone())
                    .min()
                    .unwrap_or_default(),
                fit: members
                    .iter()
                    .find(|m| m.fit.fits())
                    .map_or_else(|| first.fit.clone(), |_| Fit::Fits),
                builds_locally: false,
                members: Some(members.len()),
                part_of: None,
                library: false,
            })
        })
        .collect()
}

/// How much of a set is ticked, as the header's box draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::mods) enum Ticks {
    None,
    Some,
    All,
}

pub(in crate::mods) fn ticks(ticked: usize, total: usize) -> Ticks {
    match ticked {
        0 => Ticks::None,
        n if n >= total => Ticks::All,
        _ => Ticks::Some,
    }
}

/// One line of a collection's list: a member, or a library one of the members needs.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::mods) struct Line {
    pub(in crate::mods) id: String,
    pub(in crate::mods) name: String,
    pub(in crate::mods) summary: String,
    pub(in crate::mods) icon: Option<String>,
    pub(in crate::mods) version: String,
    pub(in crate::mods) library: bool,
    pub(in crate::mods) ticked: bool,
    /// The ticked mods that need this one: it can't be unticked while there are any.
    pub(in crate::mods) needed_by: Vec<String>,
    pub(in crate::mods) installed: bool,
    /// Why this build can't install it.
    pub(in crate::mods) refused: Option<String>,
}

impl Line {
    pub(in crate::mods) fn locked(&self) -> bool {
        !self.needed_by.is_empty()
    }
}

/// What pressing Install does, counted from the plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::mods) struct Summary {
    pub(in crate::mods) mods: usize,
    pub(in crate::mods) libraries: usize,
    pub(in crate::mods) updates: usize,
    pub(in crate::mods) bytes: u64,
}

impl Summary {
    pub(in crate::mods) fn nothing(&self) -> bool {
        self.mods + self.libraries + self.updates == 0
    }

    /// "Installs 8 mods and 1 library", with "Updates 1" after it when an installed mod must move.
    pub(in crate::mods) fn line(&self) -> String {
        let count = |n: usize, one: &'static str, many: &'static str| {
            (n > 0).then(|| match n {
                1 => tr(one).to_string(),
                n => trf(many, &[("count", &n.to_string())]),
            })
        };
        let things: Vec<String> = [
            count(self.mods, "1 mod", "{count} mods"),
            count(self.libraries, "1 library", "{count} libraries"),
        ]
        .into_iter()
        .flatten()
        .collect();
        let installs = match things.as_slice() {
            [] => None,
            [one] => Some(trf("Installs {things}", &[("things", one)])),
            [a, b] => Some(trf(
                "Installs {things}",
                &[("things", &trf("{a} and {b}", &[("a", a), ("b", b)]))],
            )),
            _ => unreachable!("two kinds of thing"),
        };
        let updates = count(
            self.updates,
            "Updates 1 installed mod",
            "Updates {count} installed mods",
        );
        match (installs, updates) {
            (Some(i), Some(u)) => format!("{i}. {u}"),
            (Some(i), None) => i,
            (None, Some(u)) => u,
            (None, None) => tr("Everything you picked is installed.").to_string(),
        }
    }
}

/// A collection's page for the ticks made so far.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::mods) struct View {
    /// The members in the collection's order, then the libraries they need.
    pub(in crate::mods) lines: Vec<Line>,
    pub(in crate::mods) members: usize,
    pub(in crate::mods) ticked: usize,
    pub(in crate::mods) header: Ticks,
    pub(in crate::mods) summary: Summary,
}

/// The members a collection's page starts with ticked: its defaults, minus any this build
/// refuses.
pub(in crate::mods) fn initial(
    index: &Index,
    c: &Collection,
    held: &[InstalledMod],
    game: &str,
) -> BTreeSet<String> {
    let refused = refusals(index, c, held, game);
    c.default_members()
        .filter(|id| !refused.contains_key(*id))
        .map(str::to_owned)
        .collect()
}

fn plan(
    index: &Index,
    c: &Collection,
    chosen: &[&str],
    held: &[InstalledMod],
    game: &str,
) -> InstallPlan {
    resolve::install(
        index,
        &c.requests(chosen),
        held,
        game,
        Side::Both,
        |_, _| true,
    )
}

/// The members the planner refuses when asked for every one, with its reason.
fn refusals(
    index: &Index,
    c: &Collection,
    held: &[InstalledMod],
    game: &str,
) -> BTreeMap<String, String> {
    let all: Vec<&str> = c.members.iter().map(|m| m.id.as_str()).collect();
    plan(index, c, &all, held, game)
        .requested
        .into_iter()
        .filter_map(|(id, r)| match r {
            RequestResult::Refused { reason } => Some((id, reason)),
            _ => None,
        })
        .collect()
}

pub(in crate::mods) fn view(
    index: &Index,
    c: &Collection,
    ticked: &BTreeSet<String>,
    held: &[InstalledMod],
    game: &str,
) -> View {
    let refused = refusals(index, c, held, game);
    let all: Vec<&str> = c
        .members
        .iter()
        .map(|m| m.id.as_str())
        .filter(|id| !refused.contains_key(*id))
        .collect();
    let chosen: Vec<&str> = all
        .iter()
        .copied()
        .filter(|id| ticked.contains(*id))
        .collect();
    let now = plan(index, c, &chosen, held, game);
    // Libraries come from asking for everything, so their lines stay when nothing needs them.
    let everything = plan(index, c, &all, held, game);
    let named = |id: &str| {
        index
            .project(id)
            .map_or_else(|| id.to_owned(), |p| p.name.clone())
    };
    let needed_by = |id: &str| -> Vec<String> {
        // In the collection's order, not the planner's.
        let by = now.required_by.get(id);
        c.members
            .iter()
            .filter(|m| by.is_some_and(|by| by.contains(&m.id)))
            .map(|m| named(&m.id))
            .collect()
    };
    let have = |id: &str| held.iter().any(|h| h.id == id);
    let line = |id: &str, library: bool| {
        let p = index.project(id);
        Line {
            id: id.to_owned(),
            name: named(id),
            summary: p.map(|p| p.summary.clone()).unwrap_or_default(),
            icon: p.and_then(|p| p.icon.clone()),
            version: p
                .and_then(|p| index.installable(p, game).or(p.versions.first()))
                .map(|v| v.version.clone())
                .unwrap_or_default(),
            library,
            ticked: if library {
                !needed_by(id).is_empty()
            } else {
                chosen.contains(&id)
            },
            needed_by: needed_by(id),
            installed: have(id),
            refused: refused.get(id).cloned(),
        }
    };
    let mut lines: Vec<Line> = c.members.iter().map(|m| line(&m.id, false)).collect();
    let member = |id: &str| c.members.iter().any(|m| m.id == id);
    lines.extend(
        everything
            .required_by
            .keys()
            .filter(|id| !member(id))
            .map(|id| line(id, true)),
    );
    let libs = libraries(index);
    let mut summary = Summary {
        updates: now.update.len(),
        ..Summary::default()
    };
    for m in &now.install {
        match libs.contains(&m.id) {
            true => summary.libraries += 1,
            false => summary.mods += 1,
        }
        summary.bytes += index
            .project(&m.id)
            .and_then(|p| p.versions.iter().find(|v| v.version == m.version))
            .map_or(0, |v| v.file.size);
    }
    View {
        members: c.members.len() - refused.len(),
        ticked: chosen.len(),
        header: ticks(chosen.len(), c.members.len() - refused.len()),
        lines,
        summary,
    }
}

impl View {
    /// A row's box: ticks or unticks a member. A library or a mod others need follows the picks.
    pub(in crate::mods) fn flip(&self, ticked: &mut BTreeSet<String>, id: &str) {
        let Some(line) = self.lines.iter().find(|l| l.id == id) else {
            return;
        };
        if line.library || line.locked() || line.refused.is_some() {
            return;
        }
        if !ticked.remove(id) {
            ticked.insert(id.to_owned());
        }
    }

    /// The header's box: every member that can be ticked, or, when all are, none. What a pick
    /// needed is then needed by nothing and unlocks on its own.
    pub(in crate::mods) fn flip_all(&self, ticked: &mut BTreeSet<String>) {
        ticked.clear();
        if self.header != Ticks::All {
            ticked.extend(
                self.lines
                    .iter()
                    .filter(|l| !l.library && l.refused.is_none())
                    .map(|l| l.id.clone()),
            );
        }
    }
}
