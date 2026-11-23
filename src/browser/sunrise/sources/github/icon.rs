//! Optional package icons, fetched independently of source inspection and builds.
use super::*;
use std::sync::Arc;
use sunrise_launcher_smod::icon::{Icon, MAX_ICON};

/// Remember 404s by their resolved cache path, even when a release card and its detail
/// arrive at the same commit through different lookups.
#[derive(Clone, Default)]
pub struct IconMisses(Arc<Mutex<HashSet<PathBuf>>>);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceIcon {
    path: String,
    allow_fetch: bool,
}

impl SourceIcon {
    /// A file of the source list's own Pages site, such as a collection's icon.
    pub fn pages(path: &str, allow_fetch: bool) -> Self {
        Self {
            path: path.to_owned(),
            allow_fetch,
        }
    }

    pub fn new(source: &Source, commit: Option<&SourceCommit>, allow_fetch: bool) -> Option<Self> {
        let source = commit.map_or(source, SourceCommit::source);
        source.icon().map(|path| Self::pages(path, allow_fetch))
    }

    /// Run on a worker. A cache miss never resolves a tracker or calls GitHub's API.
    pub fn load(&self, misses: &IconMisses) -> Option<PathBuf> {
        self.load_with(&Github::current(), misses)
    }

    fn load_with(&self, github: &Github, misses: &IconMisses) -> Option<PathBuf> {
        // Optional images must not wait behind, delay, or consume the API backoff of inspections.
        let mut state = State::default();
        let url = Url::parse(sunrise_launcher_addresses::MOD_SOURCES)
            .expect("constant URL")
            .join(&self.path)
            .ok()?;
        self.cached(github, misses, &format!("pages\0{}", self.path), true, || {
            github.fetch(
                &mut state,
                url.as_str(),
                Path::new("pages").join(&self.path).as_path(),
                MAX_ICON as u64,
            )
        })
    }

    fn cached(
        &self,
        github: &Github,
        misses: &IconMisses,
        key: &str,
        mutable: bool,
        fetch: impl FnOnce() -> Result<Option<Vec<u8>>, Failure>,
    ) -> Option<PathBuf> {
        let target = github
            .cache
            .join(format!("icon-{}.png", digest(key.as_bytes())));
        let cached = cached_bytes(&target, MAX_ICON as u64)
            .and_then(|(bytes, fresh)| Icon::parse(bytes).ok().map(|_| !mutable || fresh));
        if !self.allow_fetch || cached == Some(true) {
            return cached.map(|_| target);
        }
        if misses
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&target)
        {
            return None;
        }
        match fetch() {
            Ok(Some(bytes)) => {
                let icon = Icon::parse(bytes).ok()?;
                github.save(&target, icon.bytes()).ok()?;
                Some(target)
            }
            Ok(None) => {
                misses
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(target);
                None
            }
            Err(_) => cached.map(|_| target),
        }
    }
}

#[cfg(test)]
mod tests;
