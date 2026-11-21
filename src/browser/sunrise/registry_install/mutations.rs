//! Explicit same-base registry update planning.
use super::*;
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
