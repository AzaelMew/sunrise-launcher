//! Serialize settings writes with the per-source grants of Add from source, so an ordinary
//! Settings save can never drop or replay a grant another write made.
use super::{data_dir, write_private, Settings, SourceModGrant};
use crate::i18n::tr;
use serde_json::{Map, Value};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

const SETTINGS: &str = "settings.json";
const SOURCE_GRANTS: &str = "source_mod_grants";

// Owned file lock, released when the caller drops it.
struct SettingsFence {
    _gate: fs::File,
    path: PathBuf,
}

impl SettingsFence {
    fn open(data: &Path) -> Result<Self, String> {
        let path = data.join(SETTINGS);
        let result = (|| {
            fs::create_dir_all(data)?;
            let gate = fs::File::options()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(data.join(".settings.json.lock"))?;
            gate.lock()?;
            Ok::<_, io::Error>(gate)
        })();
        let gate = result.map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(Self { _gate: gate, path })
    }

    fn read<T: Default + serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(T::default()),
            Err(e) => return Err(format!("{}: {e}", self.path.display())),
        };
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", self.path.display()))
    }

    fn write(&self, fields: &Map<String, Value>) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(fields).map_err(|e| e.to_string())?;
        let dir = self.path.parent().expect("settings directory");
        let temp = tempfile::Builder::new()
            .prefix(".settings-")
            .tempfile_in(dir)
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        write_private(temp.path(), &bytes)
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        temp.persist(&self.path)
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        #[cfg(unix)]
        fs::File::open(dir)
            .and_then(|parent| parent.sync_all())
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        Ok(())
    }
}

impl Settings {
    /// An ordinary Settings save keeps the source grants on disk as they are.
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&data_dir())
    }

    pub(crate) fn save_to(&self, data: &Path) -> Result<(), String> {
        let file = SettingsFence::open(data)?;
        let live: Map<String, Value> = file.read()?;
        let Value::Object(mut now) = serde_json::to_value(self).map_err(|e| e.to_string())? else {
            unreachable!("Settings serializes as an object");
        };
        if let Some(value) = live.get(SOURCE_GRANTS) {
            now.insert(SOURCE_GRANTS.into(), value.clone());
        } else {
            now.remove(SOURCE_GRANTS);
        }
        file.write(&now)
    }

    /// Persist the exact source grants in one fenced settings write.
    pub fn allow_source_mods(&mut self, approved: Vec<SourceModGrant>) -> Result<(), String> {
        self.allow_source_mods_in(&data_dir(), approved)
    }

    fn allow_source_mods_in(
        &mut self,
        data: &Path,
        approved: Vec<SourceModGrant>,
    ) -> Result<(), String> {
        if approved.is_empty() {
            return Err(tr("The source could not be verified.").into());
        }
        let file = SettingsFence::open(data)?;
        let mut live: Map<String, Value> = file.read()?;
        let mut grants: Vec<SourceModGrant> = live
            .remove(SOURCE_GRANTS)
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        for grant in approved {
            if !grants.contains(&grant) {
                grants.push(grant);
            }
        }
        live.insert(
            SOURCE_GRANTS.into(),
            serde_json::to_value(&grants).map_err(|error| error.to_string())?,
        );
        file.write(&live)?;
        self.source_mod_grants = grants;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_grants_use_repo_id_owner_and_mod_and_survive_stale_settings_saves() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let stale = Settings::default();
        let mut editor = Settings::default();
        let grant = |repo_id, owner: &str, id: &str| SourceModGrant {
            repo_id,
            owner: owner.into(),
            id: id.into(),
        };
        let first = grant(1, "river", "trail");
        editor
            .allow_source_mods_in(data, vec![first.clone()])
            .unwrap();
        for different in [
            grant(2, "river", "trail"),
            grant(1, "grove", "trail"),
            grant(1, "river", "camp"),
        ] {
            assert!(!editor.source_mod_grants.contains(&different));
        }
        let second = grant(1, "river", "camp");
        editor
            .allow_source_mods_in(data, vec![first.clone(), second.clone()])
            .unwrap();
        stale.save_to(data).unwrap();
        let loaded = Settings::load_from(data);
        assert_eq!(loaded.source_mod_grants, [first, second]);
    }
}
