//! Short cross-process transactions on local launcher metadata, never on game work.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(crate) struct LockedJson {
    gate: fs::File,
    path: PathBuf,
}

impl LockedJson {
    pub(crate) fn open(dir: &Path, name: &str) -> Result<Self, String> {
        let path = dir.join(name);
        let fail = |e| format!("{}: {e}", path.display());
        fs::create_dir_all(dir).map_err(fail)?;
        let gate = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(dir.join(format!(".{name}.lock")))
            .map_err(fail)?;
        gate.lock().map_err(fail)?;
        Ok(Self { gate, path })
    }

    pub(crate) fn read<T: serde::de::DeserializeOwned + Default>(&self) -> Result<T, String> {
        match fs::read(&self.path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(T::default()),
            bytes => serde_json::from_slice(&bytes.map_err(|e| self.error(e))?)
                .map_err(|e| self.error(e)),
        }
    }

    pub(crate) fn write(&self, value: &impl serde::Serialize) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|e| self.error(e))?;
        crate::browser::sunrise::txn::replace_local_record(
            self.path.parent().expect("metadata directory"),
            &self.path,
            &bytes,
        )
        .map_err(|e| self.error(e))
    }

    fn error(&self, error: impl std::fmt::Display) -> String {
        format!("{}: {error}", self.path.display())
    }
}

impl Drop for LockedJson {
    fn drop(&mut self) {
        let _ = self.gate.unlock();
    }
}
