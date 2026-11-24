//! Fit worked out on this computer for registry releases the signed index gives no verdict for
//! on a build: the build's own `native-check` reads each release's `.smod`, all in one run, and
//! the answers are kept beside that build's SDK (`sdk/<commit>/fits.json`, by the `.smod`'s
//! SHA-256). These only fill gaps: a verdict in the index always wins over one kept here.
use super::planning::check_projects;
use super::{bundle_in, fit, sdk_cache, Bundle, Cancellation};
use crate::browser::sunrise::fetch_release_bytes;
use crate::i18n::{tr, trf};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use sunrise_launcher_smod::index::Kind;
use sunrise_launcher_smod::index_v2::{FitVerdict, Index, Release};

/// What this computer's check said of one release on one build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Local {
    Fits,
    DoesNotFit,
    /// The build's tool can't read the release's uses list (it is in a newer schema): only a
    /// newer build's tool can tell.
    Unknown,
}

/// The kept answers for one build, by `.smod` SHA-256 in lowercase.
pub type Found = BTreeMap<String, Local>;

/// The file keeping build `commit`'s answers.
const FILE: &str = "fits.json";

/// What this computer has found for build `commit` so far; nothing when it never checked one.
pub fn kept(commit: &str) -> Found {
    if !super::is_revision(commit) {
        return Found::new();
    }
    fs::read(sdk_cache::build_folder(commit).join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// The releases a check of `build` would read: for each live native project with no release
/// the index lists as fitting `build`, its newest live release when the index says nothing of it.
pub fn gaps<'a>(index: &'a Index, build: &str) -> Vec<&'a Release> {
    index
        .projects
        .iter()
        .filter(|p| p.kind == Kind::NativeMod && index.revoked(&p.id, None).is_none())
        .filter_map(|p| {
            let live: Vec<&Release> = p
                .releases
                .iter()
                .filter(|r| index.revoked(&p.id, Some(&r.file.sha256)).is_none())
                .collect();
            if live
                .iter()
                .any(|r| r.fit_verdict(build) == FitVerdict::Fits)
            {
                return None;
            }
            let newest = live.into_iter().max_by(|a, b| newest_last(a, b))?;
            (newest.fit_verdict(build) == FitVerdict::Unknown).then_some(newest)
        })
        .collect()
}

/// Release order by version, then publication date, the way Discover orders them.
fn newest_last(a: &Release, b: &Release) -> std::cmp::Ordering {
    let version = |r: &Release| semver::Version::parse(&r.version).ok();
    version(a)
        .cmp(&version(b))
        .then_with(|| a.published.cmp(&b.published))
        .then_with(|| a.version.cmp(&b.version))
}

/// `index` with this computer's answers written into each release's `fits` or `does_not_fit`
/// for every build the index lists, only where the index says nothing of that build.
pub fn filled(index: &Index) -> Index {
    fill(index, kept)
}

/// [`filled`], with build commit's answers read by `found`.
fn fill(index: &Index, found: impl Fn(&str) -> Found) -> Index {
    let mut out = index.clone();
    for build in &index.builds {
        let found = found(&build.commit);
        if found.is_empty() {
            continue;
        }
        for release in out.projects.iter_mut().flat_map(|p| p.releases.iter_mut()) {
            if release.fit_verdict(&build.id) != FitVerdict::Unknown {
                continue;
            }
            match found.get(&release.file.sha256.to_ascii_lowercase()) {
                Some(Local::Fits) => release.fits.push(build.id.clone()),
                Some(Local::DoesNotFit) => release.does_not_fit.push(build.id.clone()),
                Some(Local::Unknown) | None => {}
            }
        }
    }
    out
}

/// What the tool's verdict means for a card.
fn local(verdict: &fit::Verdict) -> Local {
    match verdict {
        fit::Verdict::MadeFor | fit::Verdict::Fits { .. } => Local::Fits,
        fit::Verdict::NoList {
            why: Some(fit::NoListReason::NewerSchema),
            ..
        } => Local::Unknown,
        _ => Local::DoesNotFit,
    }
}

/// How many `.smod` files are downloaded at once.
const PARALLEL: usize = 4;

/// Check every release in [`gaps`] of `index` on `build` that this computer hasn't answered yet,
/// keeping each answer as it comes. Fails, with what the player reads, when the build's SDK or a
/// release can't be read; the answers found before that stay kept.
pub fn check(index: &Index, build: &str) -> Result<(), String> {
    let commit = &index
        .build(build)
        .ok_or_else(|| {
            trf(
                "{build} isn't in the Sunrise registry yet.",
                &[("build", build)],
            )
        })?
        .commit;
    let known = kept(commit);
    let wanted: Vec<&Release> = gaps(index, build)
        .into_iter()
        .filter(|r| !known.contains_key(&r.file.sha256.to_ascii_lowercase()))
        .collect();
    if wanted.is_empty() {
        return Ok(());
    }
    let scratch = tempfile::Builder::new()
        .prefix("sunrise-fit-")
        .tempdir()
        .map_err(|e| e.to_string())?;
    // Each release unpacked into a folder of its own: one run reads them all, and a release
    // that stops the run is then checked alone.
    let mut unpacked: Vec<(String, Bundle)> = Vec::new();
    for (n, chunk) in wanted.chunks(PARALLEL).enumerate() {
        let fetched: Vec<Result<Vec<u8>, String>> = std::thread::scope(|scope| {
            let jobs: Vec<_> = chunk
                .iter()
                .map(|release| scope.spawn(move || download(release)))
                .collect();
            jobs.into_iter()
                .map(|job| {
                    job.join()
                        .unwrap_or_else(|_| Err(tr("The download stopped.").into()))
                })
                .collect()
        });
        for (m, (release, bytes)) in chunk.iter().zip(fetched).enumerate() {
            let dir = scratch.path().join(format!("{}", n * PARALLEL + m));
            let bundle = unpack(&bytes?, &dir)?;
            unpacked.push((release.file.sha256.to_ascii_lowercase(), bundle));
        }
    }

    let cancellation = Cancellation::default();
    let _lease = super::build_lease(&cancellation, &|| {})?.map_err(super::cache_error)?;
    let base = sdk_cache::Base::from_env();
    let keys = sunrise_launcher_release_verify::sdk_keys_for(crate::backend::GAME_REPO);
    let sdk = sdk_cache::index(&base, commit, &keys).map_err(|r| r.to_string())?;
    // The tool and the build's surface first: a run that fails after this is about the mods.
    sdk_cache::tool(&sdk, &base, &|_| {}).map_err(|r| r.to_string())?;
    sdk_cache::surface_file(&sdk, &base).map_err(|r| r.to_string())?;

    let mut found = kept(commit);
    let mut answer = |sha256: &str, said: Local| {
        found.insert(sha256.to_owned(), said);
    };
    // One run for every mod with an id no other here has; a shared id is checked alone.
    let mut ids = BTreeSet::new();
    let (batch, alone): (Vec<_>, Vec<_>) = unpacked
        .iter()
        .partition(|(_, bundle)| ids.insert(bundle.manifest.id.clone()));
    let together = scratch.path().join("all");
    for (_, bundle) in &batch {
        super::copy_bundle(bundle, &together.join(&bundle.manifest.id))?;
    }
    let bundles = super::scan_bundles(&together)?;
    let alone = match check_projects(&together, &bundles, &sdk, &base, &cancellation) {
        Ok(checks) => {
            for (sha256, bundle) in &batch {
                answer(sha256, said_of(&checks, &bundle.manifest.id));
            }
            alone
        }
        Err(_) => unpacked.iter().collect(),
    };
    let mut failed = None;
    for (sha256, bundle) in alone {
        let only = std::slice::from_ref(bundle);
        match check_projects(&bundle.dir, only, &sdk, &base, &cancellation) {
            Ok(checks) => answer(sha256, said_of(&checks, &bundle.manifest.id)),
            // The tool can't read this mod: no build of it can either.
            Err(sdk_cache::Refusal::Refused { .. }) => answer(sha256, Local::DoesNotFit),
            Err(refusal) => {
                failed = Some(refusal.to_string());
                break;
            }
        }
    }
    let bytes = serde_json::to_vec_pretty(&found).map_err(|e| e.to_string())?;
    sdk_cache::write_whole(&sdk_cache::build_folder(commit).join(FILE), &bytes)
        .map_err(|r| r.to_string())?;
    failed.map_or(Ok(()), Err)
}

/// The answer `checks` gives for mod `id`: a refusal means no build of it works here.
fn said_of(checks: &super::Checks, id: &str) -> Local {
    checks
        .verdicts
        .iter()
        .find(|(mod_id, _)| mod_id == id)
        .map_or(Local::DoesNotFit, |(_, verdict)| local(verdict))
}

/// The release's `.smod`, checked against the signed index's size and SHA-256.
fn download(release: &Release) -> Result<Vec<u8>, String> {
    let bytes = fetch_release_bytes(&release.file.url, release.file.size)?;
    if bytes.len() as u64 != release.file.size
        || !sunrise_launcher_smod::index::sha256(&bytes).eq_ignore_ascii_case(&release.file.sha256)
    {
        return Err(trf(
            "{file} doesn't match the registry's checksum.",
            &[("file", &release.file.url)],
        ));
    }
    Ok(bytes)
}

/// `bytes` read as a native package and its files written under `dir`.
fn unpack(bytes: &[u8], dir: &Path) -> Result<Bundle, String> {
    let file = dir.with_extension("smod");
    fs::write(&file, bytes).map_err(|e| e.to_string())?;
    let package = sunrise_launcher_smod::index::native::Bundle::read(&file)?;
    for (name, bytes) in &package.files {
        let to = dir.join(name);
        fs::create_dir_all(to.parent().unwrap_or(dir)).map_err(|e| e.to_string())?;
        fs::write(&to, bytes).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    bundle_in(dir)?.ok_or_else(|| {
        trf(
            "{mod} holds no native mod.",
            &[("mod", &package.manifest.id)],
        )
    })
}

#[cfg(test)]
#[path = "local_fit_tests.rs"]
mod tests;
