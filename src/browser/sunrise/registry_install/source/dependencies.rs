//! Hydrate listed source pins for a source mod's required dependencies.
use super::*;
use crate::{browser::sunrise::sources, instances::Channel};

#[derive(Clone)]
pub struct SourceDependency {
    pub name: String,
    pub version: String,
    pub origin: Option<Origin>,
}

#[derive(Default)]
pub(in crate::browser::sunrise::registry_install) struct SourceAdd {
    pub(in crate::browser::sunrise::registry_install) dependencies: Vec<SourceDependency>,
}

pub(in crate::browser::sunrise::registry_install) fn complete_source_add(
    instance: &Instance,
    home: &Path,
    base: &str,
    roots: &[Entry<'_>],
    active: Option<&managed_play::Active>,
    cancellation: &native_mods::Cancellation,
) -> Result<SourceAdd, SourceError> {
    let before = inventory(home)?;
    let enabled = mutations::enabled(home)?;
    let held_ids: BTreeSet<_> = active
        .iter()
        .flat_map(|active| active.holds())
        .map(|held| held.id())
        .collect();
    let mut unavailable = BTreeMap::new();
    for (id, existing) in &before {
        if !enabled.contains(id) {
            unavailable.insert(
                id.clone(),
                trf(
                    "Required mod {mod} is installed but off. Enable it first.",
                    &[("mod", &existing.package.manifest.name)],
                ),
            );
        }
    }
    for &id in &held_ids {
        unavailable.insert(id.to_owned(), held_change_refusal(id).to_string());
    }
    let mut manifests = before
        .iter()
        .filter(|(id, _)| enabled.contains(*id) && !held_ids.contains(id.as_str()))
        .map(|(id, seen)| (id.clone(), seen.package.manifest.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut pending = roots
        .iter()
        .map(|root| root.origin.id.clone())
        .collect::<Vec<_>>();
    let mut visited = BTreeSet::new();
    let mut listings = None;
    let mut packed = BTreeMap::new();
    while let Some(id) = pending.pop() {
        cancellation.check()?;
        if !visited.insert(id.clone()) {
            continue;
        }
        if !manifests.contains_key(&id) {
            if let Some(reason) = unavailable.get(&id) {
                return Err(SourceError::Catalogue(reason.clone()));
            }
            if instance.channel == Channel::Stable {
                return Err(SourceError::Catalogue(trf(
                    "Required mod {mod} isn't installed. Install it from Discover first, then try again.",
                    &[("mod", &id)],
                )));
            }
            let catalogue = listings.get_or_insert_with(|| sources::catalogue(false));
            if catalogue.error.is_some() || catalogue.saved_at.is_none() {
                return Err(SourceError::Catalogue(
                    tr("Connect to check the source catalogue for required mods.").into(),
                ));
            }
            let listing = catalogue
                .sources
                .iter()
                .find(|source| source.id() == id)
                .ok_or_else(|| {
                    SourceError::Catalogue(trf(
                        "Required mod {mod} isn't listed in this profile's catalogue.",
                        &[("mod", &id)],
                    ))
                })?;
            let dep_github = |failure| {
                if cancellation.is_stopped() {
                    SourceError::Cancelled
                } else {
                    SourceError::DependencyGithub {
                        name: listing.name().to_owned(),
                        failure,
                    }
                }
            };
            let pin = sources::resolve(listing, false).map_err(dep_github)?;
            cancellation.check()?;
            let (package, mut origin) =
                github_mods::pack_pin(&pin, cancellation).map_err(dep_github)?;
            origin.added_from = Some(AddedFrom::Discover);
            let bundle = index::native::Bundle::read(&package.file)?;
            manifests.insert(id.clone(), bundle.manifest);
            packed.insert(id.clone(), PastedSource { package, origin });
        }
        pending.extend(
            manifests[&id]
                .dependencies
                .iter()
                .filter(|(_, rule)| {
                    let details = rule.details();
                    !details.optional && details.side != Side::Server
                })
                .map(|(id, _)| id.clone()),
        );
    }
    // A source listing contributes one exact pin, so there are no alternate versions to try;
    // the set check below proves every manifest requirement.
    if !packed.is_empty() {
        let entries = packed
            .values()
            .map(|pasted| Entry {
                package: &pasted.package,
                origin: &pasted.origin,
                prior: github::Prior::Fresh,
            })
            .collect::<Vec<_>>();
        github::stage_entries(home, &entries, &BTreeMap::new(), active, base)?;
    }
    check_source_set(home, &[], base, cancellation)?;
    Ok(SourceAdd {
        dependencies: packed
            .into_values()
            .map(|pasted| SourceDependency {
                name: pasted.package.name,
                version: pasted.package.version,
                origin: Some(pasted.origin),
            })
            .collect(),
    })
}
