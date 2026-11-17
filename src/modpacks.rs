//! Modpacks: import a Java modpack (a Modrinth `.mrpack`, a CurseForge zip, an instance folder),
//! show what Sunrise runs of it, port that on this computer from the player's own downloads, and
//! play it as a profile of its own (docs/modpacks.md).
//!
//! Each pack lives in `ports/<slug>/` in the game's home: the work folder `sunrise-port-pack`
//! writes (pack.json, inventory, the jars, the carried-over mods, the coverage report) and
//! `home/`, the profile's own saves and options. Nothing is uploaded or shared: mod jars are
//! often All Rights Reserved, so the ports stay on the player's machine.
use crate::backend;
use crate::browser::sunrise::pack as sunrise_pack;
use crate::other_launchers::{self, Found};
pub use crate::porter::{Fetched, How, Log, Pack, Porter, Row, Status, Summary};
use crate::sources::{Folders, Os};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Where imported packs live.
pub fn ports_dir() -> PathBuf {
    backend::game_home().join("ports")
}

/// A pack's folder name: its name in lowercase words joined by dashes.
pub fn slug(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    if words.is_empty() {
        "pack".into()
    } else {
        words.join("-")
    }
}

/// An imported pack, as its work folder has it.
pub struct Imported {
    pub slug: String,
    pub work: PathBuf,
    pub pack: Pack,
    pub rows: Vec<Row>,
    /// The headline's numbers.
    pub summary: Summary,
    pub fetched: Fetched,
    /// The carried-over mods the profile loads.
    pub mods: Vec<PathBuf>,
    /// Mods left off, with why: a ported one an earlier one owns a namespace of, or one whose
    /// jar only CurseForge serves (CurseForge downloads coming soon).
    pub left_off: Vec<String>,
    /// The mods the port step gets and carries over (content, data tier).
    pub portable: usize,
    /// Ordinary resource packs staged for this profile; enabling them is the player's choice.
    pub resourcepacks: usize,
    /// Index-listed resource packs the port step can obtain (separate from Java mods).
    pub indexed_resources: usize,
    /// Data packs from jars that hold nothing else; new worlds in the profile start with them.
    pub datapacks: usize,
    /// Why no installed porter reads this pack (one imported before the launcher kept its status): it is listed, and nothing else.
    pub unread: Option<String>,
}

impl Imported {
    pub fn can_play(&self) -> bool {
        self.unread.is_none()
            && (!self.mods.is_empty() || self.resourcepacks > 0 || self.datapacks > 0)
    }
    pub fn can_port(&self) -> bool {
        self.unread.is_none() && (self.portable > 0 || self.indexed_resources > 0)
    }
    pub fn title(&self) -> &str {
        if self.pack.name.is_empty() {
            &self.slug
        } else {
            &self.pack.name
        }
    }
    pub fn count(&self, status: Status) -> usize {
        self.rows.iter().filter(|r| r.status == status).count()
    }
    /// "Minecraft 1.21.1 · neoforge 21.1.259 · 496 mods".
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.pack.minecraft.is_empty() {
            parts.push(format!("Minecraft {}", self.pack.minecraft));
        }
        if !self.pack.loader.is_empty() {
            parts.push(format!("{} {}", self.pack.loader, self.pack.loader_version));
        }
        parts.push(crate::i18n::trf(
            "{count} files",
            &[(
                "count",
                &self.rows.len().max(self.pack.files.len()).to_string(),
            )],
        ));
        parts.join("  ·  ")
    }
    /// The plain headline above the numbers, in the launcher's language: how much of the pack
    /// comes over and what does not, as `coverage.md` opens; none for a pack with no mod to port.
    pub fn headline(&self) -> Vec<String> {
        use crate::i18n::{tr, trf};
        if let Some(why) = &self.unread {
            return vec![why.clone()];
        }
        let s = self.summary;
        let n = |count: usize| count.to_string();
        let mut lines = Vec::new();
        if s.needing == 0 {
            return lines;
        }
        lines.push(trf(
            "{carried} of {total} mods that need porting come over as content (blocks, items and \
             structures); their own code does not run.",
            &[("carried", &n(s.carried)), ("total", &n(s.needing))],
        ));
        if s.ready + s.blocked + s.later > 0 {
            lines.push(trf(
                "Not yet: {ready} are content mods Port can carry over, {blocked} need mod support \
                 Sunrise does not have yet (machines, menus, behaviour, mobs, dimensions, client \
                 features), {later} are not sorted yet (not identified, or waiting on other mods or \
                 other work).",
                &[
                    ("ready", &n(s.ready)),
                    ("blocked", &n(s.blocked)),
                    ("later", &n(s.later)),
                ],
            ));
        }
        if s.partial > 0 {
            lines.push(
                tr("Partial means part of a mod came over, but something is left out, differs from \
                    the original or has not been checked yet.")
                .to_string(),
            );
        }
        lines
    }
    /// What getting the jars did, in one line, or None before it ran.
    pub fn jars(&self) -> Option<String> {
        if self.fetched.files.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        for (how, what) in [
            (How::Downloaded, "downloaded"),
            (How::Local, "copied from your folder"),
            (How::Present, "already here"),
            (
                How::NeedsKey,
                "only on CurseForge (CurseForge downloads coming soon: choose your installed \
                 instance instead)",
            ),
            (How::NotFound, "not found"),
            (How::Failed, "failed"),
        ] {
            let n = self.fetched.count(how);
            if n > 0 {
                parts.push(format!("{n} {}", crate::i18n::tr(what)));
            }
        }
        Some(crate::i18n::trf(
            "Jars: {summary}.",
            &[("summary", &parts.join(", "))],
        ))
    }
}

/// A row's plain explanation in the launcher's language. Unknown sentences from older or
/// newer reports keep their text; technical details remain untouched.
pub fn row_plain(row: &Row) -> String {
    use crate::i18n::{tr, trf};
    let plain = match row.plain.as_str() {
        "Its blocks and items came over and passed Sunrise's checks; the mod's own code is not run." => {
            tr(
                "Its blocks and items came over and passed Sunrise's checks; the mod's own code is not run.",
            )
        }
        "Part of it came over, but something is left out, differs from the original or has not been checked yet; the technical detail says what." => {
            tr(
                "Part of it came over, but something is left out, differs from the original or has not been checked yet; the technical detail says what.",
            )
        }
        "It does not come over yet." => tr("It does not come over yet."),
        "Nothing to port: Sunrise has it built in or does not need it." => {
            tr("Nothing to port: Sunrise has it built in or does not need it.")
        }
        "Sunrise cannot tell yet what this mod does (it needs its jar or a reviewed entry), so it cannot say whether it will come over." => {
            tr(
                "Sunrise cannot tell yet what this mod does (it needs its jar or a reviewed entry), so it cannot say whether it will come over.",
            )
        }
        "It joins other mods together, so it waits until those are ported." => {
            tr("It joins other mods together, so it waits until those are ported.")
        }
        "Content only (blocks, items, structures). Its jar is here, but it has not been carried over yet." => {
            tr(
                "Content only (blocks, items, structures). Its jar is here, but it has not been carried over yet.",
            )
        }
        "Content only (blocks, items, structures). It can come over once its jar is here." => {
            tr("Content only (blocks, items, structures). It can come over once its jar is here.")
        }
        "Needs what blocks and items do when used, placed, broken or ticked, which Sunrise's mod API cannot run from a mod yet, so it does not come over." => {
            tr(
                "Needs what blocks and items do when used, placed, broken or ticked, which Sunrise's mod API cannot run from a mod yet, so it does not come over.",
            )
        }
        "Needs machine-style features (block entities, menus, item, fluid and energy handling, networking) that Sunrise's mod API does not have yet, so it does not come over." => {
            tr(
                "Needs machine-style features (block entities, menus, item, fluid and energy handling, networking) that Sunrise's mod API does not have yet, so it does not come over.",
            )
        }
        "Needs mobs, custom rendering and models, dimensions or new game hooks that Sunrise's mod API does not have yet, so it does not come over." => {
            tr(
                "Needs mobs, custom rendering and models, dimensions or new game hooks that Sunrise's mod API does not have yet, so it does not come over.",
            )
        }
        "Needs the game's client interfaces (keys, targeting, inventory reading, interface, rendering), so it does not come over yet." => {
            tr(
                "Needs the game's client interfaces (keys, targeting, inventory reading, interface, rendering), so it does not come over yet.",
            )
        }
        "Sunrise has not worked out what this mod needs yet." => {
            tr("Sunrise has not worked out what this mod needs yet.")
        }
        "A library other mods build on. Sunrise's mod API and SDK take its place, so there is nothing to port." => {
            tr(
                "A library other mods build on. Sunrise's mod API and SDK take its place, so there is nothing to port.",
            )
        }
        "It makes the game faster. Sunrise's own engine does that, so there is nothing to port." => {
            tr(
                "It makes the game faster. Sunrise's own engine does that, so there is nothing to port.",
            )
        }
        "Pack tooling or a game feature that Sunrise already has, so there is nothing to port." => {
            tr(
                "Pack tooling or a game feature that Sunrise already has, so there is nothing to port.",
            )
        }
        "Built into Sunrise already, so there is nothing to port." => {
            tr("Built into Sunrise already, so there is nothing to port.")
        }
        "Nothing to port." => tr("Nothing to port."),
        "Made for Minecraft 1.20 or older: Sunrise ports mods made for 1.21 and later." => {
            tr("Made for Minecraft 1.20 or older: Sunrise ports mods made for 1.21 and later.")
        }
        "Shader packs are not converted yet." => tr("Shader packs are not converted yet."),
        "Copied as a resource pack; Sunrise has not checked how it renders. Turn it on in the game's Resource Packs screen." => {
            tr(
                "Copied as a resource pack; Sunrise has not checked how it renders. Turn it on in the game's Resource Packs screen.",
            )
        }
        "Not downloaded yet; Port gets it." => tr("Not downloaded yet; Port gets it."),
        "Sunrise could not use this resource pack yet; the technical detail says why." => {
            tr("Sunrise could not use this resource pack yet; the technical detail says why.")
        }
        other => other,
    };
    let waiting = match row.waiting {
        0 => return plain.to_owned(),
        1 => tr("It also waits for 1 mod it depends on that is not ported yet.").to_owned(),
        n => trf(
            "It also waits for {count} mods it depends on that are not ported yet.",
            &[("count", &n.to_string())],
        ),
    };
    format!("{plain} {waiting}")
}

/// The pack in `work`, from the status the launcher kept, else its current porter's; Err when `work` holds no pack.
pub fn load(work: &Path) -> Result<Imported, String> {
    load_with(work, || Porter::for_version(None)?.status(work))
}

fn load_with(
    work: &Path,
    ask: impl FnOnce() -> Result<crate::porter::WorkStatus, String>,
) -> Result<Imported, String> {
    if let Some(status) = crate::porter::kept(work) {
        return Ok(imported(work, status));
    }
    match ask() {
        Ok(status) => Ok(imported(work, status)),
        Err(why) => {
            let path = work.join("pack.json");
            let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let pack =
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
            let status = crate::porter::WorkStatus {
                pack,
                ..Default::default()
            };
            Ok(Imported {
                unread: Some(why),
                ..imported(work, status)
            })
        }
    }
}

fn imported(work: &Path, status: crate::porter::WorkStatus) -> Imported {
    let mut left_off = status.left_off;
    left_off.extend(
        status
            .fetched
            .files
            .iter()
            .filter(|o| o.how == How::NeedsKey)
            .map(|o| format!("{}: CurseForge downloads coming soon", o.key)),
    );
    let reports = &status.resource_reports;
    for (path, outcome) in reports.staged.files.iter().chain(&reports.profile.files) {
        if matches!(
            outcome.state.as_str(),
            "conflict" | "failed" | "missing" | "unsupported"
        ) {
            left_off.push(format!("{path}: {}: {}", outcome.state, outcome.detail));
        }
    }
    left_off.sort();
    left_off.dedup();
    Imported {
        slug: work
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        work: work.to_path_buf(),
        pack: status.pack,
        rows: status.rows,
        summary: status.summary,
        fetched: status.fetched,
        mods: status.mods,
        left_off,
        portable: status.portable,
        resourcepacks: status.resourcepacks,
        indexed_resources: status.indexed_resources,
        datapacks: status.datapacks,
        unread: None,
    }
}

/// Every imported pack, by name.
pub fn list() -> Vec<Imported> {
    list_in(&ports_dir())
}

pub(crate) fn list_in(ports: &Path) -> Vec<Imported> {
    let mut packs: Vec<Imported> = fs::read_dir(ports)
        .into_iter()
        .flatten()
        .flatten()
        // An import in progress, or a pack being replaced (`import`).
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| load(&e.path()).ok())
        .collect();
    packs.sort_by_key(|p| p.title().to_ascii_lowercase());
    packs
}

/// Imports the pack at `path` into `ports/<slug>/` with `porter` and writes its coverage report
/// from metadata (Modrinth answers by file hash; nothing is downloaded yet). Importing the same
/// pack again keeps the jars and ports it already has.
pub fn import(porter: &Porter, path: &Path, log: Log) -> Result<Imported, String> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let staging = ports_dir().join(format!(
        ".importing-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    let pack = match porter.import(path, &staging, log) {
        Ok(status) => status.pack,
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let name = if pack.name.is_empty() || pack.name == "modlist" {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        pack.name
    };
    let work = ports_dir().join(slug(&name));
    log(format!("Importing {name} into {}", work.display()));
    replace_with(&work, &staging)?;
    porter.inventory(&work, log)?;
    porter.report(&work, log)?;
    porter.profile(&work, log)?;
    load(&work)
}

/// Makes the fresh import `staging` the pack in `work`; what it didn't write of an earlier import there (its jars, ports and the profile's saves) carries over.
fn replace_with(work: &Path, staging: &Path) -> Result<(), String> {
    let at = |e: std::io::Error, path: &Path| format!("{}: {e}", path.display());
    if !work.exists() {
        return fs::rename(staging, work).map_err(|e| at(e, work));
    }
    let name = work.file_name().unwrap_or_default().to_string_lossy();
    let old = work.with_file_name(format!(".replacing-{name}"));
    let _ = fs::remove_dir_all(&old);
    fs::rename(work, &old).map_err(|e| at(e, work))?;
    if let Err(e) = fs::rename(staging, work) {
        let _ = fs::rename(&old, work);
        let _ = fs::remove_dir_all(staging);
        return Err(at(e, work));
    }
    carry_over(&old, work).map_err(|e| at(e, &old))?;
    let _ = fs::remove_dir_all(&old);
    Ok(())
}

/// Moves into `to` each entry of `from` it lacks, going into the folders both have.
fn carry_over(from: &Path, to: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if !target.exists() {
            fs::rename(entry.path(), &target)?;
        } else if entry.file_type()?.is_dir() && target.is_dir() {
            carry_over(&entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Ports what this computer can with `porter`: the portable mods' jars (from `from` when the
/// player has them, else the pack's links or Modrinth), carried over for its version, then the
/// report again.
pub fn port(
    porter: &Porter,
    work: &Path,
    from: Option<&Path>,
    log: Log,
) -> Result<Imported, String> {
    porter.port(work, from, log)?;
    porter.profile(work, log)?;
    load(work)
}

/// What the mod browser does with a modpack: imports it, then ports what this computer can
/// (from the pack's links and Modrinth), so it plays straight away. A port that fails leaves the
/// pack imported; the error says so.
pub fn import_and_port(porter: &Porter, path: &Path, log: Log) -> Result<Imported, String> {
    let pack = import(porter, path, log)?;
    if !pack.can_port() {
        return Ok(pack);
    }
    port(porter, &pack.work, None, log)
        .map_err(|e| format!("{} is imported, but porting it failed: {e}", pack.title()))
}

/// One line on what an import and port gave: the mods carried over and the ones that weren't.
pub fn outcome(pack: &Imported) -> String {
    let not = pack.count(Status::NotYet);
    format!(
        "{} is imported{}: {} of its {} files carried over ({} ported, {} partly), {} don't \
         yet, {} aren't needed{}. {} resource packs available; enable them in Resource Packs. {} data packs on in new worlds. Its list says why.",
        pack.title(),
        if pack.mods.is_empty() && pack.datapacks == 0 {
            ""
        } else {
            " and ported"
        },
        pack.count(Status::Ported) + pack.count(Status::Partial),
        pack.rows.len(),
        pack.count(Status::Ported),
        pack.count(Status::Partial),
        not,
        pack.count(Status::NotNeeded),
        match pack.left_off.len() {
            0 => String::new(),
            n => format!("; {n} are left off (listed under the pack)"),
        },
        pack.resourcepacks,
        pack.datapacks,
    )
}

/// Removes an imported pack: its jars, ports and the profile's saves.
pub fn remove(work: &Path) -> Result<(), String> {
    if sunrise_pack::is_pack(work) {
        return sunrise_pack::remove(work);
    }
    if work.parent() != Some(ports_dir().as_path()) {
        return Err(format!("{} is not an imported pack", work.display()));
    }
    fs::remove_dir_all(work).map_err(|e| format!("{}: {e}", work.display()))
}

/// The profile's own game home: saves, options.txt, logs.
pub fn profile_home(work: &Path) -> PathBuf {
    work.join("home")
}

/// Turns the game's command into this pack's profile: its own home (the player's options.txt
/// copied in the first time, so keys and video settings carry over) and its carried-over mods.
/// A modpack from Sunrise's registry (`sunrise-packs/<id>/`) plays its own profile the same way.
pub fn apply_profile(cmd: &mut Command, work: &Path) -> Result<(), String> {
    if sunrise_pack::is_pack(work) {
        return sunrise_pack::apply(cmd, work);
    }
    // The game's own porter, beside it, installs the profile's resource packs.
    if let Some(porter) = Path::new(cmd.get_program())
        .parent()
        .and_then(|dir| Porter::in_dir(dir, None))
    {
        if let Err(e) = porter.profile(work, &|_| {}) {
            eprintln!("resource packs not installed: {e}");
        }
    }
    let pack = load(work)?;
    if let Some(why) = pack.unread {
        return Err(why);
    }
    if !pack.can_play() {
        return Err(
            "No content mods, data packs or resource packs are available: port the pack first."
                .into(),
        );
    }
    let home = profile_home(work);
    fs::create_dir_all(&home).map_err(|e| format!("{}: {e}", home.display()))?;
    let options = home.join("options.txt");
    let own = backend::game_home().join("options.txt");
    if !options.exists() && own.is_file() {
        let _ = fs::copy(own, &options);
    }
    cmd.current_dir(&home).env("SUNRISE_HOME", &home);
    // Only mods that load together: the porter leaves off the later of two sharing a namespace.
    for m in pack.mods {
        cmd.arg("--dev-mod").arg(m);
    }
    Ok(())
}

/// The modpacks other launchers keep on this computer, for the Modpacks page to offer: each
/// instance with a `mods` folder of its own. A profile of the official launcher that plays in the
/// launcher's own folder is the player's everyday game, not a pack, and is left out.
pub fn found_on_this_computer(os: Os, folders: &Folders) -> Vec<Found> {
    other_launchers::scan(os, folders)
        .into_iter()
        .filter(|f| !f.shared && f.game.join("mods").is_dir())
        .collect()
}

/// The folder "Import an instance folder…" takes for `found`: a Prism or MultiMC instance's own
/// folder, whose `mmc-pack.json` names its versions, else its game folder.
pub fn found_folder(found: &Found) -> PathBuf {
    match found.game.parent() {
        Some(dir) if dir.join("mmc-pack.json").is_file() => dir.to_path_buf(),
        _ => found.game.clone(),
    }
}

/// The slug [`import`] gives `found`: imported once, the page opens it instead.
pub fn found_slug(found: &Found) -> String {
    let folder = found_folder(found);
    slug(&folder.file_name().unwrap_or_default().to_string_lossy())
}

/// One line on `found`, in the order of [`Imported::summary`]: the app it came from, then its
/// Minecraft version and loader when its launcher records them.
pub fn found_summary(found: &Found) -> String {
    let mut parts = vec![other_launchers::app_name(found.launcher).to_string()];
    if let Some(minecraft) = &found.minecraft {
        parts.push(format!("Minecraft {minecraft}"));
    }
    if let Some(l) = &found.loader {
        parts.push(format!("{} {}", l.name, l.version));
    }
    parts.join("  ·  ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_packs_are_other_launchers_instances_with_mods() {
        let root = std::env::temp_dir().join(format!("sunrise-found-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let write = |path: &Path, text: &str| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        let folders = Folders {
            home: root.join("home"),
            data: root.join("data"),
            config: root.join("config"),
        };
        let cf = root.join("home/Documents/curseforge/minecraft/Instances");
        write(
            &cf.join("All the Mods 10/minecraftinstance.json"),
            r#"{"gameVersion": "1.21.1", "baseModLoader": {"name": "neoforge-21.1.133"}}"#,
        );
        write(&cf.join("All the Mods 10/mods/a.jar"), "jar");
        // Resource packs only: not a modpack.
        write(&cf.join("Packs only/resourcepacks/p.zip"), "zip");
        let prism = root.join("data/PrismLauncher/instances/fabric-1.21");
        write(
            &prism.join("mmc-pack.json"),
            r#"{"components": [{"uid": "net.minecraft", "version": "1.21.1"}]}"#,
        );
        write(
            &prism.join("instance.cfg"),
            "name=My Fabric
",
        );
        write(&prism.join("minecraft/mods/b.jar"), "jar");
        // The official launcher's default profile plays in its own folder, mods and all.
        let official = root.join("data/minecraft");
        write(
            &official.join("launcher_profiles.json"),
            r#"{"profiles": {"a": {"type": "latest-release"}}}"#,
        );
        write(&official.join("mods/c.jar"), "jar");

        let found = found_on_this_computer(Os::MacOs, &folders);
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["My Fabric", "All the Mods 10"]);
        // A Prism instance imports from its own folder, which names its version.
        assert_eq!(found_folder(&found[0]), prism);
        assert_eq!(found_slug(&found[0]), "fabric-1-21");
        assert_eq!(found_folder(&found[1]), cf.join("All the Mods 10"));
        assert_eq!(found_slug(&found[1]), "all-the-mods-10");
        assert_eq!(
            found_summary(&found[0]),
            "Prism Launcher  ·  Minecraft 1.21.1"
        );
        assert_eq!(
            found_summary(&found[1]),
            "CurseForge  ·  Minecraft 1.21.1  ·  neoforge 21.1.133"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("All the Mods 10"), "all-the-mods-10");
        assert_eq!(slug("Create: Above & Beyond!"), "create-above-beyond");
        assert_eq!(slug("  "), "pack");
    }

    /// A work folder whose porter last replied `status`.
    fn work_with(name: &str, status: serde_json::Value) -> PathBuf {
        let work = std::env::temp_dir()
            .join(format!("sunrise-launcher-{name}-{}", std::process::id()))
            .join("ports/pack");
        let _ = fs::remove_dir_all(&work);
        fs::create_dir_all(&work).unwrap();
        let mut reply = serde_json::json!({"format": 1, "pack": {"name": "Pack"}});
        reply
            .as_object_mut()
            .unwrap()
            .extend(status.as_object().unwrap().clone());
        fs::write(work.join("launcher-status.json"), reply.to_string()).unwrap();
        work
    }

    #[test]
    fn jars_only_curseforge_serves_are_left_off_as_coming_soon() {
        let work = work_with(
            "cf",
            serde_json::json!({"fetched": {"files": [
                {"key": "allthemodium", "file": "mods/allthemodium.jar", "how": "needs-key"}
            ]}}),
        );
        let pack = load(&work).unwrap();
        assert_eq!(
            pack.left_off,
            ["allthemodium: CurseForge downloads coming soon"]
        );
        assert!(pack
            .jars()
            .unwrap()
            .contains("CurseForge downloads coming soon"));
        fs::remove_dir_all(work.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn a_profile_has_its_own_home_and_the_mods_its_porter_loads() {
        let work = work_with("profile", serde_json::json!({}));
        let mut cmd = Command::new("true");
        assert!(apply_profile(&mut cmd, &work).is_err(), "nothing to play");
        let (a, b) = (work.join("mods/a/carry"), work.join("mods/b/carry"));
        let work = work_with("profile", serde_json::json!({"mods": [a, b]}));
        apply_profile(&mut cmd, &work).unwrap();
        let args: Vec<PathBuf> = cmd.get_args().map(PathBuf::from).collect();
        assert_eq!(args, ["--dev-mod".into(), a, "--dev-mod".into(), b]);
        let home = profile_home(&work);
        assert_eq!(cmd.get_current_dir(), Some(home.as_path()));
        assert!(cmd
            .get_envs()
            .any(|(k, v)| k == "SUNRISE_HOME" && v == Some(home.as_os_str())));
        assert!(home.is_dir());
        fs::remove_dir_all(work.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn resource_and_data_packs_alone_make_a_pack_playable() {
        let work = work_with(
            "packs",
            serde_json::json!({"resourcepacks": 2, "resource_reports": {"profile": {"files": {
                "resourcepacks/look.zip": {"state": "conflict", "detail": "the profile has its own"},
                "resourcepacks/Look": {"state": "available", "detail": ""}
            }}}}),
        );
        let pack = load(&work).unwrap();
        assert!(pack.can_play() && !pack.can_port());
        assert_eq!(
            pack.left_off,
            ["resourcepacks/look.zip: conflict: the profile has its own"]
        );
        assert!(outcome(&pack).contains("2 resource packs available"));
        let mut cmd = Command::new("true");
        apply_profile(&mut cmd, &work).unwrap();
        assert_eq!(cmd.get_args().count(), 0);
        let data = work_with("packs", serde_json::json!({"datapacks": 1}));
        let pack = load(&data).unwrap();
        assert!(pack.can_play());
        assert!(outcome(&pack).contains("1 data packs on in new worlds"));
        fs::remove_dir_all(work.parent().unwrap().parent().unwrap()).unwrap();
    }

    #[test]
    fn a_pack_no_porter_reads_stays_listed_and_says_why() {
        let work = work_with("unread", serde_json::json!({}));
        fs::remove_file(work.join("launcher-status.json")).unwrap();
        let why = "Sunrise 0.9 is too old to import modpacks for this launcher".to_owned();
        assert!(
            load_with(&work, || Err(why.clone())).is_err(),
            "no pack.json: not a pack"
        );
        fs::write(work.join("pack.json"), r#"{"name": "Cozy"}"#).unwrap();
        let pack = load_with(&work, || Err(why.clone())).unwrap();
        assert_eq!(pack.title(), "Cozy");
        assert_eq!(pack.headline(), std::slice::from_ref(&why));
        assert!(!pack.can_play() && !pack.can_port());
        let ports = work.parent().unwrap();
        for hidden in [".importing-1-0", ".replacing-pack"] {
            fs::create_dir_all(ports.join(hidden)).unwrap();
            fs::write(
                ports.join(hidden).join("launcher-status.json"),
                r#"{"format": 1}"#,
            )
            .unwrap();
        }
        fs::write(work.join("launcher-status.json"), r#"{"format": 1}"#).unwrap();
        let listed: Vec<String> = list_in(ports).into_iter().map(|p| p.slug).collect();
        assert_eq!(listed, ["pack"], "import folders in progress are not packs");
        fs::remove_dir_all(ports.parent().unwrap()).unwrap();
    }

    #[test]
    fn importing_again_keeps_the_jars_ports_and_saves_the_new_import_lacks() {
        let root =
            std::env::temp_dir().join(format!("sunrise-launcher-reimport-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (work, staging) = (root.join("pack"), root.join(".importing-1-0"));
        let put = |path: PathBuf, text: &str| {
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        put(work.join("pack.json"), "old");
        put(work.join("jars/a.jar"), "downloaded");
        put(work.join("mods/a/carry/sunrise-mod.toml"), "ported");
        put(work.join("home/saves/World/level.dat"), "save");
        put(staging.join("pack.json"), "new");
        put(staging.join("jars/b.jar"), "from the instance");
        replace_with(&work, &staging).unwrap();
        let read = |path: &str| fs::read_to_string(work.join(path)).unwrap();
        assert_eq!(read("pack.json"), "new");
        assert_eq!(read("jars/a.jar"), "downloaded");
        assert_eq!(read("jars/b.jar"), "from the instance");
        assert_eq!(read("mods/a/carry/sunrise-mod.toml"), "ported");
        assert_eq!(read("home/saves/World/level.dat"), "save");
        let left: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(left, ["pack"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn resource_downloads_enable_port_even_when_no_mods_are_portable() {
        let work = work_with("index", serde_json::json!({"indexed_resources": 1}));
        let pack = load(&work).unwrap();
        assert!(pack.can_port() && !pack.can_play());
        fs::remove_dir_all(work.parent().unwrap().parent().unwrap()).unwrap();
    }
}
