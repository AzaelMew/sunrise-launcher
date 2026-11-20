//! Sunrise modpacks from the registry (`.spack`, docs/modding/src/reference/registry-index.md), each played as its own
//! profile: `sunrise-packs/<id>/` in the game's home holds the pack's file, `pack.json` (what
//! was installed) and `home/`, the profile's game folder: its saves, its options, and the pack's
//! mods in `home/sunrise-mods/`, each downloaded and checked against the index like any
//! registry mod, turned on or off as the pack says. The pack's overrides (settings, resource
//! packs, a mod's saved settings) are copied in once, when it is installed.
use super::{fetch, index, install_version, local, newest, txn, Index};
use crate::{
    backend,
    browser::{installed, mod_list, FileRef, SourceId},
    i18n::tr,
    native_mods,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{fs, io, sync::Arc};
use sunrise_launcher_smod::{
    index as idx,
    locks::{self, LockMode},
    modpack::Modpack,
};

const LEGACY_RECORD: &str = "pack.json";
const RECORD: &str = "mod-browser/pack.json";
const ARCHIVE: &str = "mod-browser/pack.spack";

/// Where registry modpacks live.
pub fn packs_dir() -> PathBuf {
    crate::backend::game_home().join("sunrise-packs")
}

/// What `pack.json` keeps of an installed pack.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
    pub sha256: String,
    /// The pack's mods: `id version`.
    pub mods: Vec<String>,
    #[serde(skip)]
    pub work: PathBuf,
}

/// The installed registry packs, by name.
pub fn list() -> Vec<Installed> {
    list_in(&packs_dir())
}

pub(crate) fn list_in(dir: &Path) -> Vec<Installed> {
    let mut packs: Vec<Installed> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| read(&e.path()))
        .collect();
    packs.sort_by_key(|p| p.name.to_lowercase());
    packs
}

fn read(work: &Path) -> Option<Installed> {
    read_checked(work).ok().flatten()
}

fn read_checked(work: &Path) -> Result<Option<Installed>, String> {
    for file in [home(work).join(RECORD), work.join(LEGACY_RECORD)] {
        match fs::symlink_metadata(&file) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(at(&file, e)),
            Ok(meta) if meta.is_file() && !sunrise_launcher_smod::runtime_leases::linked(&meta) => {
            }
            Ok(_) => return Err(at(&file, io::ErrorKind::InvalidData.into())),
        }
        let mut pack: Installed =
            serde_json::from_slice(&fs::read(&file).map_err(|e| at(&file, e))?)
                .map_err(|e| format!("{}: {e}", file.display()))?;
        if work.file_name().and_then(std::ffi::OsStr::to_str) != Some(pack.id.as_str()) {
            return Err(at(&file, io::ErrorKind::InvalidData.into()));
        }
        pack.work = work.to_owned();
        return Ok(Some(pack));
    }
    Ok(None)
}

/// Whether `work` is a registry pack's folder.
pub fn is_pack(work: &Path) -> bool {
    // Imported Java packs also have pack.json, with a different record format.
    read(work).is_some()
}

/// The profile's game folder.
pub fn home(work: &Path) -> PathBuf {
    work.join("home")
}

/// Install the pack as one complete set. Once a pack exists, only its admitted base can
/// choose a candidate version; the launcher's changing selection is not a migration.
pub fn install(id: &str, progress: &dyn Fn(&str, f32)) -> Result<Installed, String> {
    install_in(
        &*index()?,
        local(),
        id,
        &packs_dir(),
        || backend::resolve_version(None),
        progress,
    )
}

fn install_in(
    index: &Index,
    local: bool,
    id: &str,
    dir: &Path,
    initial_base: impl FnOnce() -> Result<String, String>,
    progress: &dyn Fn(&str, f32),
) -> Result<Installed, String> {
    let p = index
        .project(id)
        .ok_or_else(|| format!("{id} isn't in Sunrise's registry."))?;
    if p.kind != idx::Kind::Modpack {
        return Err(format!("{} is a {}, not a modpack.", p.name, p.kind.name()));
    }
    let work = dir.join(&p.id);
    let _identity = pack_gate(dir, &p.id)?;
    clean_private_trash(dir, &p.id)?;
    let steps = |step: native_mods::Step| progress(&step.label(), 0.99);
    match fs::symlink_metadata(&work) {
        Ok(meta) if meta.is_dir() && !sunrise_launcher_smod::runtime_leases::linked(&meta) => {
            let owner = Arc::new(txn::Lease::admit(&home(&work))?);
            let prior = read_checked(&work)?.ok_or_else(pack_unavailable)?;
            let active = owner
                .active_build()?
                .ok_or_else(|| tr("Play this modpack once before updating it.").to_string())?;
            let game = backend::game_of_build(active.base()).ok_or_else(pack_unavailable)?;
            // No candidate for this build: refuse before creating a staged Install.
            let version = newest(index, p, &game)?;
            let plan = txn::StagedInstall::begin(owner, active)?;
            let mut installed = stage_pack(
                index,
                local,
                p,
                version,
                plan.stage(),
                Some(&prior),
                progress,
            )?;
            plan.commit(&[RECORD, ARCHIVE], native_mods::Profile::Optimized, steps)?;
            installed.work = work;
            Ok(installed)
        }
        Ok(_) => Err(pack_unavailable()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let target = txn::Target::legacy(&initial_base()?)?;
            let game = backend::game_of_build(target.base()).ok_or_else(pack_unavailable)?;
            let version = newest(index, p, &game)?;
            let stage = tempfile::Builder::new()
                .prefix(".pack-stage-")
                .tempdir_in(dir)
                .map_err(|e| at(dir, e))?;
            let staged_home = home(stage.path());
            fs::create_dir(&staged_home).map_err(|e| at(&staged_home, e))?;
            let mut installed = stage_pack(index, local, p, version, &staged_home, None, progress)?;
            let owner = Arc::new(txn::Lease::admit(&staged_home)?);
            let active = txn::seal_private_profile(
                owner,
                target.base(),
                native_mods::Profile::Optimized,
                steps,
            )?;
            txn::expose_private_profile(stage.path(), &work, &active)?;
            let _ = stage.keep();
            installed.work = work;
            Ok(installed)
        }
        Err(e) => Err(at(&work, e)),
    }
}

fn stage_pack(
    index: &Index,
    local: bool,
    project: &idx::Project,
    version: &idx::Version,
    home: &Path,
    before: Option<&Installed>,
    progress: &dyn Fn(&str, f32),
) -> Result<Installed, String> {
    let line = format!("{} {}", project.name, version.version);
    let file = FileRef {
        url: version.file.url.clone(),
        name: version.file.name.clone(),
        size: version.file.size,
        sha512: None,
        sha256: Some(version.file.sha256.clone()),
        sha1: None,
        primary: true,
    };
    let downloads = tempfile::tempdir_in(home).map_err(|e| at(home, e))?;
    let path = fetch(&file, downloads.path(), local, &|f| progress(&line, f))?;
    index::check_file(index, project, version, &path)?;
    let pack = Modpack::read(&path)?;
    let mut mods = Vec::new();
    for m in &pack.manifest.mods {
        let found = index
            .project(&m.id)
            .and_then(|p| Some((p, p.versions.iter().find(|v| v.version == m.version)?)));
        let Some((p, v)) =
            found.filter(|(_, v)| v.file.sha256 == m.sha256 && super::fits_game(v, &version.game))
        else {
            return Err(format!(
                "{} needs {} {}, which isn't in the registry for this build.",
                project.name, m.id, m.version
            ));
        };
        let on = before
            .filter(|before| {
                before
                    .mods
                    .iter()
                    .any(|s| s.split(' ').next() == Some(m.id.as_str()))
            })
            .map_or(m.on, |_| mod_list::is_enabled(home, &m.id));
        let entry = install_version(index, local, p, v, home, progress)?;
        let placed = entry.path(home).ok_or_else(pack_unavailable)?;
        if idx::sha256_file(&placed)? != m.sha256 {
            return Err(pack_unavailable());
        }
        mod_list::set_enabled(home, &m.id, on)?;
        mods.push(format!("{} {}", m.id, m.version));
    }
    if let Some(before) = before {
        let mut installed = installed::Installed::load(home);
        for old in &before.mods {
            let (id, version) = old.split_once(' ').ok_or_else(pack_unavailable)?;
            if pack.manifest.mods.iter().any(|m| m.id == id) {
                continue;
            }
            if let Some(position) = installed.entries.iter().position(|row| {
                row.source == SourceId::Sunrise && row.project == id && row.version == version
            }) {
                let entry = installed.entries.remove(position);
                let file = entry.path(home).ok_or_else(pack_unavailable)?;
                let hash = entry
                    .checksum
                    .strip_prefix("SHA-256:")
                    .ok_or_else(pack_unavailable)?;
                if idx::sha256_file(&file)? != hash {
                    return Err(pack_unavailable());
                }
                fs::remove_file(&file).map_err(|e| at(&file, e))?;
                mod_list::set_enabled(home, id, false)?;
            }
        }
        installed.save(home)?;
    } else {
        Modpack::extract_overrides(&path, home)?;
    }
    let record = Installed {
        id: project.id.clone(),
        name: project.name.clone(),
        version: version.version.clone(),
        sha256: version.file.sha256.clone(),
        mods,
        work: PathBuf::new(),
    };
    let manifest = home.join(RECORD);
    fs::create_dir_all(manifest.parent().expect("inside home")).map_err(|e| at(&manifest, e))?;
    let archive = home.join(ARCHIVE);
    fs::rename(&path, &archive).map_err(|e| at(&archive, e))?;
    let text = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    fs::write(&manifest, text).map_err(|e| at(&manifest, e))?;
    Ok(record)
}

/// The same external owner is inherited by every launcher/game operation on this pack.
/// A private candidate has no live pack ID and uses the ordinary private profile owner.
pub(super) fn owner_file(home: &Path) -> Result<Option<PathBuf>, String> {
    let normalized = home.canonicalize();
    let home = normalized.as_deref().unwrap_or(home);
    let Some(work) = home
        .parent()
        .filter(|_| home.file_name() == Some("home".as_ref()))
    else {
        return Ok(None);
    };
    let Some(id) = work
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|id| idx::valid_id(id))
    else {
        return Ok(None);
    };
    let Some(directory) = work.parent() else {
        return Ok(None);
    };
    let canonical = directory.canonicalize();
    let directory = canonical.as_deref().unwrap_or(directory);
    if directory.file_name() != Some("sunrise-packs".as_ref()) {
        return Ok(None);
    }
    gate_file(directory, &format!("{id}.owner")).map(Some)
}

fn pack_gate(directory: &Path, id: &str) -> Result<locks::Locked, String> {
    let file = gate_file(directory, id)?;
    let opened = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&file)
        .map_err(|e| at(&file, e))?;
    locks::locked(opened, LockMode::Exclusive).map_err(|e| e.to_string())
}

fn gate_file(directory: &Path, id: &str) -> Result<PathBuf, String> {
    if crate::browser::safe_name(id) != Some(id) {
        return Err(pack_unavailable());
    }
    fs::create_dir_all(directory).map_err(|e| at(directory, e))?;
    let meta = fs::symlink_metadata(directory).map_err(|e| at(directory, e))?;
    if !meta.is_dir() || sunrise_launcher_smod::runtime_leases::linked(&meta) {
        return Err(pack_unavailable());
    }
    let gate = directory.join(".launcher-gates");
    match fs::create_dir(&gate) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(at(&gate, e)),
    }
    let meta = fs::symlink_metadata(&gate).map_err(|e| at(&gate, e))?;
    if !meta.is_dir() || sunrise_launcher_smod::runtime_leases::linked(&meta) {
        return Err(pack_unavailable());
    }
    let file = gate.join(id);
    if fs::symlink_metadata(&file)
        .is_ok_and(|meta| !meta.is_file() || sunrise_launcher_smod::runtime_leases::linked(&meta))
    {
        return Err(pack_unavailable());
    }
    Ok(file)
}

fn pack_unavailable() -> String {
    tr("The modpack or its exact game build could not be verified. Nothing was installed.").into()
}

fn at(path: &Path, error: io::Error) -> String {
    format!("{}: {error}", path.display())
}

/// Turns the game's command into the pack's profile: its own game folder (the player's
/// options.txt copied in when the pack brings none).
pub fn apply(cmd: &mut Command, work: &Path) -> Result<(), String> {
    if !is_pack(work) {
        return Err(format!("{} is not an installed modpack.", work.display()));
    }
    let home = home(work);
    fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    let options = home.join("options.txt");
    let own = crate::backend::game_home().join("options.txt");
    if !options.exists() && own.is_file() {
        let _ = fs::copy(own, &options);
    }
    cmd.current_dir(&home).env("SUNRISE_HOME", &home);
    Ok(())
}

/// Detach the whole pack under its lifecycle and profile gates. Cleanup can never delete a
/// later install at the live name, even if this process stops after the atomic rename.
pub fn remove(work: &Path) -> Result<(), String> {
    remove_in(&packs_dir(), work, |_| {})
}

fn remove_in(directory: &Path, work: &Path, detached: impl FnOnce(&Path)) -> Result<(), String> {
    let id = work
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or_else(pack_unavailable)?;
    if work.parent() != Some(directory) {
        return Err(pack_unavailable());
    }
    let _identity = pack_gate(directory, id)?;
    let meta = fs::symlink_metadata(work).map_err(|e| at(work, e))?;
    if !meta.is_dir() || sunrise_launcher_smod::runtime_leases::linked(&meta) {
        return Err(pack_unavailable());
    }
    let owner = txn::Lease::admit(&home(work))?;
    read_checked(work)?.ok_or_else(pack_unavailable)?;
    let trash = trash_for(directory, id)?;
    let attempt = tempfile::Builder::new()
        .prefix("retired-")
        .tempdir_in(&trash)
        .map_err(|e| at(&trash, e))?;
    let payload = attempt.path().join("profile");
    let retired = attempt.keep();
    txn::detach_private_profile(work, &payload)?;
    drop(owner);
    detached(&payload);
    // A cleanup failure leaves only private trash to retry; the pack is already gone.
    if let Err(error) = fs::remove_dir_all(&retired) {
        eprintln!("{}", at(&retired, error));
    }
    Ok(())
}

fn trash_for(directory: &Path, id: &str) -> Result<PathBuf, String> {
    let root = directory.join(".launcher-trash");
    let trash = root.join(id);
    for path in [&root, &trash] {
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(at(path, e)),
        }
        let meta = fs::symlink_metadata(path).map_err(|e| at(path, e))?;
        if !meta.is_dir() || sunrise_launcher_smod::runtime_leases::linked(&meta) {
            return Err(pack_unavailable());
        }
    }
    Ok(trash)
}

fn clean_private_trash(directory: &Path, id: &str) -> Result<(), String> {
    let trash = directory.join(".launcher-trash").join(id);
    if !trash.exists() {
        return Ok(());
    }
    let trash = trash_for(directory, id)?;
    for entry in fs::read_dir(&trash).map_err(|e| at(&trash, e))? {
        let entry = entry.map_err(|e| at(&trash, e))?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|n| n.starts_with("retired-"))
        {
            continue;
        }
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).map_err(|e| at(&path, e))?;
        if meta.is_dir() && !sunrise_launcher_smod::runtime_leases::linked(&meta) {
            if let Err(e) = fs::remove_dir_all(&path) {
                eprintln!("{}", at(&path, e));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A local registry: the mod and a pack of it, as `file://` files.
    fn registry(dir: &Path, on: bool, extra: &str) -> Index {
        registry_at(dir, "1.0.0", on, extra)
    }

    /// [`registry`] with the pack at `pack_version`.
    fn registry_at(dir: &Path, pack_version: &str, on: bool, extra: &str) -> Index {
        let smod_src = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("crates/smod/tests/index/coords_hud-0.1.0.smod");
        let smod = dir.join("coords_hud-0.1.0.smod");
        fs::copy(smod_src, &smod).unwrap();
        let smod_sha = idx::sha256_file(&smod).unwrap();
        let spack = dir.join(format!("cozy-{pack_version}.spack"));
        let mut zip = zip::ZipWriter::new(fs::File::create(&spack).unwrap());
        let o = zip::write::SimpleFileOptions::default();
        zip.start_file("sunrise-modpack.toml", o).unwrap();
        write!(
            zip,
            "format = 1\nid = \"cozy\"\nname = \"Cozy\"\nversion = \"{pack_version}\"\ngame = \"26.3\"\n\
             [[mods]]\nid = \"coords_hud\"\nversion = \"0.1.0\"\nsha256 = \"{smod_sha}\"\non = {on}\n"
        )
        .unwrap();
        zip.start_file("overrides/options.txt", o).unwrap();
        zip.write_all(b"renderDistance:8\n").unwrap();
        zip.finish().unwrap();
        let size = |p: &Path| fs::metadata(p).unwrap().len();
        // A `file:///` URL: forward slashes (a Windows path's backslashes would be escapes in
        // the JSON) and a leading slash before a Windows drive.
        let url = |p: &Path| {
            let path = p.display().to_string().replace('\\', "/");
            if path.starts_with('/') {
                path
            } else {
                format!("/{path}")
            }
        };
        let text = format!(
            r#"{{"schema": 1, "projects": [
              {{"id": "coords_hud", "kind": "mod", "name": "Coordinates HUD", "summary": "", "license": "MIT",
                "source": "https://x.test", "versions": [{{"version": "0.1.0", "api": "0.2", "side": "client",
                "capabilities": ["registries"], "game": "26.3", "file": {{"name": "coords_hud-0.1.0.smod",
                "url": "file://{}", "size": {}, "sha256": "{smod_sha}"}}}}]}},
              {{"id": "cozy", "kind": "modpack", "name": "Cozy", "summary": "", "license": "MIT",
                "source": "https://x.test", "versions": [{{"version": "{pack_version}", "api": "1", "game": "26.3",
                "requires": [{{"id": "coords_hud", "version": "0.1.0", "sha256": "{smod_sha}"}}],
                "file": {{"name": "cozy-{pack_version}.spack", "url": "file://{}", "size": {}, "sha256": "{}"}}}}]}}
            ]{extra}}}"#,
            url(&smod),
            size(&smod),
            url(&spack),
            size(&spack),
            idx::sha256_file(&spack).unwrap()
        );
        Index::parse_local(&text).unwrap()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sunrise-launcher-spack-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn install_fixture(index: &Index, id: &str, packs: &Path) -> Result<Installed, String> {
        let tag = format!(
            "fixture-{}",
            packs
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
        );
        let dir = backend::version_dir(&tag);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(backend::CLIENT), b"test game, not executed").unwrap();
        fs::write(dir.join(".installed"), b"").unwrap();
        fs::write(
            dir.join("sunrise-release.json"),
            br#"{"version_id":"26.3"}"#,
        )
        .unwrap();
        install_in(index, true, id, packs, || Ok(tag), &|_, _| {})
    }

    #[test]
    fn a_pack_installs_as_a_profile_with_its_mods() {
        let dir = scratch("install");
        let index = registry(&dir, false, "");
        let packs = dir.join("sunrise-packs");
        let pack = install_fixture(&index, "cozy", &packs).unwrap();
        assert_eq!(pack.mods, ["coords_hud 0.1.0"]);
        let home = home(&packs.join("cozy"));
        assert!(home.join("sunrise-mods/coords_hud.smod").is_file());
        assert!(
            !mod_list::is_enabled(&home, "coords_hud"),
            "the pack turns it off"
        );
        assert_eq!(
            fs::read_to_string(home.join("options.txt")).unwrap(),
            "renderDistance:8\n"
        );
        assert_eq!(
            list_in(&packs),
            [Installed {
                work: packs.join("cozy"),
                ..pack
            }]
        );
        // Settings the player changed stay on an update.
        fs::write(home.join("options.txt"), "renderDistance:12\n").unwrap();
        install_fixture(&index, "cozy", &packs).unwrap();
        assert_eq!(
            fs::read_to_string(home.join("options.txt")).unwrap(),
            "renderDistance:12\n"
        );
        // Not a modpack, and not there.
        assert!(install_fixture(&index, "coords_hud", &packs)
            .unwrap_err()
            .contains("not a modpack"));
        assert!(install_fixture(&index, "nope", &packs).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A pack update keeps the mods the player turned on or off in the Mods page, as a Java
    /// mod update does; a mod the new pack version adds takes the pack's own `on`.
    #[test]
    fn a_pack_update_keeps_the_players_mod_choices() {
        let dir = scratch("keep-choice");
        let packs = dir.join("sunrise-packs");
        let first = registry_at(&dir, "1.0.0", true, "");
        install_fixture(&first, "cozy", &packs).unwrap();
        let home = home(&packs.join("cozy"));
        assert!(
            mod_list::is_enabled(&home, "coords_hud"),
            "the pack turns it on"
        );

        // The player turns it off on the Mods page; the next pack version leaves it off.
        mod_list::set_enabled(&home, "coords_hud", false).unwrap();
        let second = registry_at(&dir, "1.1.0", true, "");
        let pack = install_fixture(&second, "cozy", &packs).unwrap();
        assert_eq!(pack.version, "1.1.0", "it was an update");
        assert!(
            !mod_list::is_enabled(&home, "coords_hud"),
            "the update kept it off"
        );

        // Turned back on, the next one leaves it on.
        mod_list::set_enabled(&home, "coords_hud", true).unwrap();
        let third = registry_at(&dir, "1.2.0", true, "");
        install_fixture(&third, "cozy", &packs).unwrap();
        assert!(mod_list::is_enabled(&home, "coords_hud"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_pack_with_a_taken_down_mod_installs_nothing() {
        let dir = scratch("revoked");
        let index = registry(
            &dir,
            true,
            r#", "revoked": [{"id": "coords_hud", "reason": "Test."}]"#,
        );
        let packs = dir.join("sunrise-packs");
        let e = install_fixture(&index, "cozy", &packs).unwrap_err();
        assert!(e.contains("taken down: Test."), "{e}");
        assert!(!packs.join("cozy").exists(), "nothing left behind");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_existing_pack_refuses_another_game_without_changing_the_set_or_sidecar() {
        let dir = scratch("same-base");
        let packs = dir.join("sunrise-packs");
        let first = registry_at(&dir, "1.0.0", true, "");
        let prior = install_fixture(&first, "cozy", &packs).unwrap();
        let home = home(&prior.work);
        let pointer_path = home.join("mod-browser/active-build.json");
        let before = fs::read(&pointer_path).unwrap();
        let bytes = fs::read(home.join(ARCHIVE)).unwrap();
        let mut other = registry_at(&dir, "1.1.0", false, "");
        other
            .projects
            .iter_mut()
            .find(|p| p.id == "cozy")
            .unwrap()
            .versions[0]
            .game = "other".into();
        assert!(install_fixture(&other, "cozy", &packs).is_err());
        assert_eq!(fs::read(pointer_path).unwrap(), before);
        assert_eq!(fs::read(home.join(ARCHIVE)).unwrap(), bytes);
        assert_eq!(read(&prior.work).unwrap().version, "1.0.0");
        assert!(!home.join("mod-browser/install-txn.json").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detach_worker() {
        let Some(work) = std::env::var_os("SUNRISE_TEST_INSTALL_OWNER") else {
            return;
        };
        let work = PathBuf::from(work);
        remove_in(work.parent().unwrap(), &work, |_| txn::wait_to_be_killed()).unwrap();
    }

    #[test]
    fn a_detached_pack_is_invisible_even_if_the_owner_is_killed_before_cleanup() {
        let dir = scratch("detach");
        let packs = dir.join("sunrise-packs");
        let registry = registry(&dir, false, "");
        let old = install_fixture(&registry, "cozy", &packs).unwrap();
        let pack_home = home(&old.work);
        let playing = txn::Lease::admit(&pack_home).unwrap();
        assert!(remove_in(&packs, &old.work, |_| panic!("pack is in use")).is_err());
        assert!(old.work.is_dir());
        drop(playing);
        let payload = txn::crash_process(
            "browser::sunrise::pack::tests::detach_worker",
            &old.work,
            None,
            || {
                assert!(!old.work.exists());
                assert!(list_in(&packs).is_empty());
                assert!(pack_gate(&packs, "cozy").is_err());
                let payload = fs::read_dir(packs.join(".launcher-trash/cozy"))
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path()
                    .join("profile");
                assert!(payload.join("home/mod-browser/active-build.json").is_file());
                payload
            },
        );
        assert!(payload.is_dir());
        assert!(txn::Lease::admit(&pack_home).is_err());
        assert!(
            !old.work.exists(),
            "a stale Play must not recreate the pack"
        );
        let next = install_fixture(&registry, "cozy", &packs).unwrap();
        assert!(!payload.exists());
        assert_eq!(next.work, old.work);
        assert!(next
            .work
            .join("home/mod-browser/active-build.json")
            .is_file());
        fs::remove_dir_all(&dir).unwrap();
    }
}
