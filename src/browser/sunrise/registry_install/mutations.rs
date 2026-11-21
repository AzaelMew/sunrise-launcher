//! Explicit same-base mutations. Approval is for the exact aggregate impact before publication.
use super::*;
use crate::worlds;
use sunrise_launcher_smod::{dependencies::parse_version, mod_list::ModList};

pub(super) fn update_floors(
    catalogue: &index::Index,
    before: &Inventory,
    game: &str,
    requested: Option<&[String]>,
) -> Result<BTreeMap<String, semver::Version>, InstallError> {
    let ids = match requested {
        Some(ids) => ids.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        None => before
            .iter()
            .filter(|(_, entry)| entry.owned)
            .map(|(id, _)| id.as_str())
            .collect(),
    };
    let mut above = BTreeMap::new();
    for id in ids {
        let old = before.get(id).filter(|p| p.owned).ok_or_else(|| {
            InstallError::Refused(trf(
                "{mod} wasn't installed from the Sunrise registry.",
                &[("mod", id)],
            ))
        })?;
        let installed =
            parse_version(&old.package.manifest.version).map_err(|_| unproved_mods())?;
        if catalogue.project(id).is_some_and(|p| {
            p.versions.iter().any(|v| {
                fits_game(v, game)
                    && catalogue.revoked(id, &v.file.sha256).is_none()
                    && parse_version(&v.version)
                        .is_ok_and(|new| new.cmp_precedence(&installed).is_gt())
            })
        }) {
            above.insert(id.to_owned(), installed);
        }
    }
    Ok(above)
}

pub(super) fn enabled(home: &Path) -> Result<BTreeSet<String>, String> {
    let path = home.join(sunrise_launcher_smod::mod_list::FILE);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(ModList::parse(&text).enabled),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(error) => Err(at(&path, error)),
    }
}

pub(super) fn loss_ids(
    candidate: &Path,
    before: &Inventory,
    entries: &BTreeMap<String, installed::Entry>,
) -> Vec<String> {
    entries
        .iter()
        .filter_map(|(id, next)| {
            let old = before.get(id)?;
            let native = old.package.manifest.native.is_some()
                || crate::native_status::is_native_file(&next.file);
            (native
                || !super::super::content_loss::dropped(
                    &old.package.path,
                    &candidate.join(&next.file),
                )
                .is_ok_and(|ids| ids.is_empty()))
            .then(|| id.clone())
        })
        .collect()
}

#[derive(Debug)]
pub struct ChangeImpact {
    change: Change,
    affected: Vec<WorldUse>,
}

#[derive(Debug)]
pub(super) enum Change {
    Update(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WorldUse {
    path: std::path::PathBuf,
    name: String,
    mods: Vec<String>,
    known: bool,
}

impl ChangeImpact {
    pub(super) fn read(home: &Path, change: Change, ids: &[String]) -> Result<Self, String> {
        Ok(Self {
            change,
            affected: Self::world_use(home, ids)?,
        })
    }

    fn world_use(home: &Path, ids: &[String]) -> Result<Vec<WorldUse>, String> {
        let mut affected = if ids.is_empty() {
            Vec::new()
        } else {
            worlds::at_risk(home, ids)?
                .into_iter()
                .map(|(world, mods, known)| WorldUse {
                    path: world.path,
                    name: world.name,
                    mods,
                    known,
                })
                .collect::<Vec<_>>()
        };
        affected.sort();
        Ok(affected)
    }

    pub fn risks_worlds(&self) -> bool {
        !self.affected.is_empty()
    }

    pub fn detail(&self) -> String {
        let mut lines = Vec::new();
        let action = match &self.change {
            Change::Update(mods) => {
                lines.push(trf("Update: {mods}.", &[("mods", &mods.join(", "))]));
                tr("update")
            }
        };
        match self.affected.len() {
            0 => (),
            1 => lines.push(trf(
                "This {action} may remove modded content from 1 saved world.",
                &[("action", action)],
            )),
            n => lines.push(trf(
                "This {action} may remove modded content from {count} saved worlds.",
                &[("action", action), ("count", &n.to_string())],
            )),
        }
        if self.affected.iter().any(|world| !world.known) {
            lines.push(tr("Sunrise couldn't check which mods some of these worlds use.").into());
        }
        lines.join("\n\n")
    }

    pub(super) fn recheck(&self, home: &Path, ids: &[String]) -> Result<(), String> {
        if Self::world_use(home, ids)? != self.affected {
            return Err(tr(
                "The affected worlds changed after review. Nothing was changed; try again.",
            )
            .into());
        }
        Ok(())
    }
}
