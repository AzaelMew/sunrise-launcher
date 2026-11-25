//! Resolve a mod set's passive Java jars before a launcher-managed launch.
mod download;
mod launch;
use crate::browser::installed::Installed;
use crate::i18n::{tr, trf};
pub use launch::{prepare_for_command, prepare_launch, LaunchError};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, fs, io,
    path::{Path, PathBuf},
};
use sunrise_launcher_smod::manifest::{JarRef, JarRequirement, MissingJar};

/// Download consent for this mod set, never a permission to execute Java code. It decides only
/// jars that no installed Sunrise registry mod lists: those are fetched without asking.
#[non_exhaustive]
pub enum Permission<'a> {
    Refuse,
    Allow,
    Ask(&'a dyn Fn(&[&JarRef]) -> bool),
}

impl Permission<'static> {
    /// Headless managed launches never ask: downloading needs an explicit per-call opt-in.
    pub fn headless(allow_downloads: bool) -> Self {
        if allow_downloads {
            Self::Allow
        } else {
            Self::Refuse
        }
    }
}

/// Preparation failures retain their kind; callers never inspect error text.
#[derive(Debug)]
#[non_exhaustive]
pub enum PrepareError {
    Refused {
        missing: Vec<MissingJar>,
    },
    /// One metadata request failed for this complete missing set.
    Lookup {
        missing: Vec<MissingJar>,
        reason: String,
    },
    Download {
        jar: Box<JarRef>,
        path: PathBuf,
        reason: String,
    },
    HashMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    Io {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for PrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused { missing } => {
                f.write_str(tr("Jar downloads were not allowed. Supply the original files at these exact cache paths, or pass --allow-jar-downloads:\n"))?;
                write_missing_jars(f, missing)
            }
            Self::Lookup { missing, reason } => {
                f.write_str(&trf(
                    "Jar metadata lookup failed: {reason}. Put the original files at these exact cache paths, then retry:\n",
                    &[("reason", reason)],
                ))?;
                write_missing_jars(f, missing)
            }
            Self::Download { jar, path, reason } => f.write_str(&trf(
                "Jar {id} {version}: {reason}. Put the original file with this exact SHA-512 at {path}, then retry.",
                &[("id", jar.id()), ("version", jar.version()), ("reason", reason), ("path", &path.display().to_string())],
            )),
            Self::HashMismatch { path, expected, actual } => f.write_str(&trf(
                "SHA-512 mismatch at {path}: expected {expected}, got {actual}. Replace this cache entry with the exact original file, or remove it and retry with --allow-jar-downloads.",
                &[("path", &path.display().to_string()), ("expected", expected), ("actual", actual)],
            )),
            Self::Io { path, source } => f.write_str(&trf(
                "Cannot read or write jar cache {path}: {error}. Check the file and its permissions before retrying.",
                &[("path", &path.display().to_string()), ("error", &source.to_string())],
            )),
        }
    }
}
fn write_missing_jars(f: &mut fmt::Formatter<'_>, missing: &[MissingJar]) -> fmt::Result {
    for entry in missing {
        writeln!(f, "{entry}")?;
    }
    Ok(())
}

impl std::error::Error for PrepareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// In-game ids of the mods installed from the Sunrise registry, read from the installed rows of
/// the game folder `game_dir`. An unreadable list names none, so every jar then asks as before.
pub fn registry_mods(game_dir: &Path) -> BTreeSet<String> {
    Installed::load(game_dir)
        .entries
        .iter()
        .filter_map(|row| row.in_game_mod_id().map(str::to_owned))
        .collect()
}

/// Cache hits work offline. Missing jars are resolved by exact SHA-512. A missing jar listed by a
/// mod in `registry_mods` is downloaded without asking; the rest are downloaded only after one
/// consent decision for their complete set.
pub fn prepare(
    home: &Path,
    jars: &[JarRequirement],
    registry_mods: &BTreeSet<String>,
    permission: Permission<'_>,
) -> Result<(), PrepareError> {
    prepare_with(
        home,
        jars,
        registry_mods,
        permission,
        download::jar_urls,
        download::download_jar,
    )
}

fn prepare_with(
    home: &Path,
    jars: &[JarRequirement],
    registry_mods: &BTreeSet<String>,
    permission: Permission<'_>,
    mut lookup: impl FnMut(&[&JarRef]) -> Result<BTreeMap<String, String>, String>,
    mut download: impl FnMut(&str, &JarRef, &Path) -> Result<(), String>,
) -> Result<(), PrepareError> {
    let cache = home.join("jars");
    let mut missing = Vec::new();
    for requirement in jars {
        let jar = &requirement.jar;
        let path = jar.cache_path(&cache);
        match fs::File::open(&path) {
            Ok(file) => {
                let hash =
                    download::hash_reader::<sha2::Sha512>(file, io::sink()).map_err(|source| {
                        PrepareError::Io {
                            path: path.clone(),
                            source,
                        }
                    })?;
                let actual = format!("{hash:x}");
                if actual != jar.sha512() {
                    return Err(PrepareError::HashMismatch {
                        path,
                        expected: jar.sha512().into(),
                        actual,
                    });
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => missing.push(requirement),
            Err(source) => return Err(PrepareError::Io { path, source }),
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let entries = |list: &[&JarRequirement]| -> Vec<MissingJar> {
        list.iter()
            .map(|requirement| MissingJar {
                owners: requirement.owners.clone(),
                jar: requirement.jar.clone(),
                path: requirement.jar.cache_path(&cache),
            })
            .collect()
    };
    let missing_entries = || entries(&missing);
    let missing_refs: Vec<_> = missing.iter().map(|requirement| &requirement.jar).collect();
    let asking: Vec<_> = missing
        .iter()
        .copied()
        .filter(|requirement| {
            !requirement
                .owners
                .iter()
                .any(|owner| registry_mods.contains(owner))
        })
        .collect();
    if !asking.is_empty() {
        match permission {
            Permission::Refuse => {
                return Err(PrepareError::Refused {
                    missing: entries(&asking),
                })
            }
            Permission::Allow => {}
            Permission::Ask(ask) => {
                let asking_refs: Vec<_> =
                    asking.iter().map(|requirement| &requirement.jar).collect();
                if !ask(&asking_refs) {
                    return Err(PrepareError::Refused {
                        missing: entries(&asking),
                    });
                }
            }
        }
    }
    let urls = lookup(&missing_refs).map_err(|reason| PrepareError::Lookup {
        missing: missing_entries(),
        reason,
    })?;
    let urls: Vec<_> = missing_refs
        .iter()
        .map(|jar| {
            urls.get(jar.sha512())
                .ok_or_else(|| PrepareError::Download {
                    jar: Box::new((*jar).clone()),
                    path: jar.cache_path(&cache),
                    reason: tr("no exact file on Modrinth").into(),
                })
        })
        .collect::<Result<_, _>>()?;
    fs::create_dir_all(&cache).map_err(|source| PrepareError::Io {
        path: cache.clone(),
        source,
    })?;
    for (jar, url) in missing_refs.into_iter().zip(urls) {
        let path = jar.cache_path(&cache);
        download(url, jar, &path).map_err(|reason| PrepareError::Download {
            jar: Box::new(jar.clone()),
            path,
            reason,
        })?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "jars_tests.rs"]
mod tests;
