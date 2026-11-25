//! The publisher pins remembered per computer in `publisher-pins.json`: checking, recording and accepting a project's publisher.

use super::*;

/// Per-computer publisher pin file stored in [`backend::data_dir`].
pub const PINS_FILE: &str = "publisher-pins.json";

const PINS_LOCK: &str = ".publisher-pins.lock";
const PIN_SCHEMA: u32 = 1;

/// The publisher remembered for a registry project on this computer: the owner account and
/// workflow path that matter, and the repository last verified, shown when they change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublisherPin {
    /// GitHub owner account ID of the signing repository. `None` only in pins written by
    /// launchers before owner IDs were remembered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<u64>,
    pub repository_id: u64,
    pub repository_name: String,
    pub workflow_path: String,
}

impl PublisherPin {
    /// True when `other` keeps the pinned owner and workflow path. A move to another repository
    /// of the same owner, a repository rename or a newer `release-v<number>` tag on the same
    /// workflow path is not a publisher change. A pin without an owner ID only matches its own
    /// repository.
    pub fn matches(&self, other: &PublisherPin) -> bool {
        self.workflow_path == other.workflow_path
            && match (self.owner_id, other.owner_id) {
                (Some(pinned), Some(incoming)) => pinned == incoming,
                _ => self.repository_id == other.repository_id,
            }
    }
}

/// Details of a verified release whose publisher (owner, or for an old pin repository, and
/// workflow path) differs from the computer's remembered pin for the project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublisherChange {
    pub project_id: String,
    pub pinned: PublisherPin,
    pub incoming: PublisherPin,
}

impl PublisherChange {
    /// True only when both pins know their owner and the owners differ: a pin written before
    /// owner IDs were remembered says nothing of its owner.
    pub fn owner_changed(&self) -> bool {
        matches!(
            (self.pinned.owner_id, self.incoming.owner_id),
            (Some(pinned), Some(incoming)) if pinned != incoming
        )
    }

    pub fn repository_changed(&self) -> bool {
        self.pinned.repository_id != self.incoming.repository_id
    }

    pub fn workflow_changed(&self) -> bool {
        self.pinned.workflow_path != self.incoming.workflow_path
    }
}

/// Outcome of checking a verified release against the computer's remembered publisher pins.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinStatus {
    /// First install of this project on this computer (no prior pin existed).
    FirstInstall,
    /// Matches the owner (or, for an old pin, the repository) and workflow path remembered.
    Matched,
}

#[derive(Debug, Serialize, Deserialize)]
struct PinFile {
    schema: u32,
    projects: BTreeMap<String, PublisherPin>,
}

/// Load all remembered project publisher pins from [`backend::data_dir`].
pub fn load_pins() -> Result<BTreeMap<String, PublisherPin>, Error> {
    load_pins_in(&backend::data_dir())
}

/// Load all remembered project publisher pins from `data_dir`.
pub fn load_pins_in(data_dir: &Path) -> Result<BTreeMap<String, PublisherPin>, Error> {
    Ok(load_file(data_dir)?.projects)
}

/// Check `incoming` against the computer's remembered pin for `project_id` without modifying disk.
pub fn check_pin(project_id: &str, incoming: &PublisherPin) -> Result<PinStatus, Error> {
    check_pin_in(&backend::data_dir(), project_id, incoming)
}

/// Check `incoming` against the remembered pin for `project_id` in `data_dir` without modifying disk.
pub fn check_pin_in(
    data_dir: &Path,
    project_id: &str,
    incoming: &PublisherPin,
) -> Result<PinStatus, Error> {
    let file = load_file(data_dir)?;
    match file.projects.get(project_id) {
        None => Ok(PinStatus::FirstInstall),
        Some(pinned) if pinned.matches(incoming) => Ok(PinStatus::Matched),
        Some(pinned) => Err(Error::PublisherChanged(Box::new(PublisherChange {
            project_id: project_id.to_owned(),
            pinned: pinned.clone(),
            incoming: incoming.clone(),
        }))),
    }
}

/// Check `incoming` against this computer's remembered pin for `project_id`, recording the pin on
/// first install and refreshing it on a match. Returns [`Error::PublisherChanged`] without
/// modifying disk when the publisher differs.
pub fn pin_or_check(project_id: &str, incoming: &PublisherPin) -> Result<PinStatus, Error> {
    pin_or_check_in(&backend::data_dir(), project_id, incoming)
}

/// Check `incoming` against the remembered pin for `project_id` in `data_dir`, recording the pin on
/// first install and refreshing it on a match: a pin without an owner ID gains it, and a same-owner
/// move records the new repository. Returns [`Error::PublisherChanged`] without modifying disk when
/// the publisher differs.
pub fn pin_or_check_in(
    data_dir: &Path,
    project_id: &str,
    incoming: &PublisherPin,
) -> Result<PinStatus, Error> {
    under_lock(data_dir, || {
        let mut file = load_file(data_dir)?;
        match file.projects.get_mut(project_id) {
            None => {
                file.projects
                    .insert(project_id.to_owned(), incoming.clone());
                save_file(data_dir, &file)?;
                Ok(PinStatus::FirstInstall)
            }
            Some(pinned) if pinned.matches(incoming) => {
                // Written again even when unchanged: an earlier save may have made the pin
                // visible and then failed its flush, and success must mean it is durable.
                *pinned = incoming.clone();
                save_file(data_dir, &file)?;
                Ok(PinStatus::Matched)
            }
            Some(pinned) => Err(Error::PublisherChanged(Box::new(PublisherChange {
                project_id: project_id.to_owned(),
                pinned: pinned.clone(),
                incoming: incoming.clone(),
            }))),
        }
    })
}

/// Replace the remembered pin on this computer after the player explicitly accepts `change`.
/// Compares the current pin's identity against `change.pinned` under the pin lock so a stale
/// confirmation cannot overwrite a publisher accepted in the meantime.
pub fn accept_pin(change: &PublisherChange) -> Result<(), Error> {
    accept_pin_in(&backend::data_dir(), change)
}

/// Replace the remembered pin in `data_dir` after the player explicitly accepts `change`.
/// Compares the current pin's identity against `change.pinned` under the pin lock so a stale
/// confirmation cannot overwrite a publisher accepted in the meantime.
pub fn accept_pin_in(data_dir: &Path, change: &PublisherChange) -> Result<(), Error> {
    under_lock(data_dir, || {
        let mut file = load_file(data_dir)?;
        match file.projects.get_mut(&change.project_id) {
            None => {
                file.projects
                    .insert(change.project_id.clone(), change.incoming.clone());
                save_file(data_dir, &file)
            }
            Some(current) if current.matches(&change.pinned) => {
                *current = change.incoming.clone();
                save_file(data_dir, &file)
            }
            Some(current) if current.matches(&change.incoming) => {
                // Written again even when unchanged: an earlier confirmation may have made the
                // pin visible and then failed its flush.
                *current = change.incoming.clone();
                save_file(data_dir, &file)
            }
            Some(current) => Err(Error::PublisherChanged(Box::new(PublisherChange {
                project_id: change.project_id.clone(),
                pinned: current.clone(),
                incoming: change.incoming.clone(),
            }))),
        }
    })
}

fn under_lock<T>(data_dir: &Path, run: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    let err = |e: std::io::Error| Error::PinStore(format!("{PINS_FILE}: {e}"));
    let existing_ancestor = data_dir
        .ancestors()
        .find(|dir| dir.exists())
        .map(Path::to_path_buf);
    fs::create_dir_all(data_dir).map_err(err)?;
    if existing_ancestor.as_deref() != Some(data_dir) {
        // Flush every newly created folder and the existing ancestor holding the top new entry
        // so a first pin survives power loss however many levels were created.
        let mut cursor = Some(data_dir);
        while let Some(dir) = cursor {
            crate::profile_txn::durable::sync_dir(dir).map_err(err)?;
            if Some(dir) == existing_ancestor.as_deref() {
                break;
            }
            cursor = dir.parent();
        }
    }
    let gate = data_dir.join(PINS_LOCK);
    match fs::symlink_metadata(&gate) {
        Ok(meta) if meta.is_file() => (),
        Ok(_) => return Err(Error::PinStore(format!("{PINS_FILE}: invalid lock path"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(err(e)),
    }
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&gate)
        .map_err(err)?;
    lock.lock().map_err(err)?;
    let result = run();
    let _ = lock.unlock();
    result
}

fn load_file(data_dir: &Path) -> Result<PinFile, Error> {
    let path = data_dir.join(PINS_FILE);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => (),
        Ok(_) => return Err(Error::PinStore(format!("{PINS_FILE}: not a regular file"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(PinFile {
                schema: PIN_SCHEMA,
                projects: BTreeMap::new(),
            });
        }
        Err(error) => return Err(Error::PinStore(format!("{PINS_FILE}: {error}"))),
    }
    let bytes = fs::read(&path).map_err(|e| Error::PinStore(format!("{PINS_FILE}: {e}")))?;
    let file: PinFile =
        serde_json::from_slice(&bytes).map_err(|e| Error::PinStore(format!("{PINS_FILE}: {e}")))?;
    if file.schema != PIN_SCHEMA {
        return Err(Error::PinStore(format!(
            "{PINS_FILE}: unsupported schema {}",
            file.schema
        )));
    }
    Ok(file)
}

fn save_file(data_dir: &Path, file: &PinFile) -> Result<(), Error> {
    let mut bytes = serde_json::to_vec_pretty(file)
        .map_err(|e| Error::PinStore(format!("{PINS_FILE}: {e}")))?;
    bytes.push(b'\n');
    // Flushing the parent too keeps the data folder's own entry when the store created it.
    let parent = data_dir.parent().unwrap_or(data_dir);
    crate::profile_txn::durable::replace(parent, &data_dir.join(PINS_FILE), &bytes)
        .map_err(|e| Error::PinStore(format!("{PINS_FILE}: {e}")))
}
