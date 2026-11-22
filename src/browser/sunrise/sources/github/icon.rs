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
    location: Location,
    allow_fetch: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Location {
    Pages(String),
    Repo {
        repo: String,
        reference: Reference,
        path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Reference {
    Branch(String),
    Commit(String),
    SavedRelease(u64),
}

fn icon_path(manifest: &str) -> Option<String> {
    let path = manifest
        .rsplit_once('/')
        .map_or_else(|| "icon.png".into(), |(dir, _)| format!("{dir}/icon.png"));
    safe_path(&path).then_some(path)
}

impl SourceIcon {
    pub fn new(source: &Source, commit: Option<&SourceCommit>, allow_fetch: bool) -> Option<Self> {
        let source = commit.map_or(source, SourceCommit::source);
        let location = if let Some(icon) = source.icon() {
            Location::Pages(icon.to_owned())
        } else {
            let reference = match (commit, source.tracker()) {
                (Some(commit), _) => Reference::Commit(commit.sha().to_owned()),
                (None, Tracker::Branch(branch)) => Reference::Branch(branch.clone()),
                (None, Tracker::Release) => Reference::SavedRelease(source.repo_id()),
            };
            Location::Repo {
                repo: source.repo().to_owned(),
                reference,
                path: icon_path(source.manifest())?,
            }
        };
        Some(Self {
            location,
            allow_fetch,
        })
    }

    /// Run on a worker. A cache miss never resolves a tracker or calls GitHub's API.
    pub fn load(&self, misses: &IconMisses) -> Option<PathBuf> {
        self.load_with(&Github::current(), misses)
    }

    fn load_with(&self, github: &Github, misses: &IconMisses) -> Option<PathBuf> {
        // Optional images must not wait behind, delay, or consume the API backoff of inspections.
        let mut state = State::default();
        match &self.location {
            Location::Pages(path) => {
                let url = Url::parse(sunrise_launcher_addresses::MOD_SOURCES)
                    .expect("constant URL")
                    .join(path)
                    .ok()?;
                self.cached(github, misses, &format!("pages\0{path}"), true, || {
                    github.fetch(
                        &mut state,
                        url.as_str(),
                        Path::new("pages").join(path).as_path(),
                        MAX_ICON as u64,
                    )
                })
            }
            Location::Repo {
                repo,
                reference,
                path,
            } => {
                let (saved, head);
                let (reference, mutable) = match reference {
                    Reference::Branch(branch) => {
                        head = format!("refs/heads/{branch}");
                        (head.as_str(), true)
                    }
                    Reference::Commit(sha) => (sha.as_str(), false),
                    Reference::SavedRelease(repo_id) => {
                        saved = Github::saved_pin(&github.ref_cache_path(
                            repo,
                            *repo_id,
                            &Tracker::Release,
                        ))?;
                        saved.release.as_ref()?;
                        (saved.sha.as_str(), false)
                    }
                };
                self.cached(
                    github,
                    misses,
                    &format!("raw\0{repo}\0{reference}\0{path}"),
                    mutable,
                    || github.raw(&mut state, repo, reference, path, MAX_ICON as u64),
                )
            }
        }
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
        let cached = (|| {
            let file = fs::File::open(&target).ok()?;
            let fresh = !mutable
                || file
                    .metadata()
                    .ok()?
                    .modified()
                    .ok()?
                    .elapsed()
                    .is_ok_and(|age| age < FRESH);
            Icon::parse(bounded(file, MAX_ICON as u64).ok()?).ok()?;
            Some(fresh)
        })();
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
