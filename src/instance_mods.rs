//! Home's Mods tab (docs/design-system.md, "Night"; plan R0, screens 5 and 6): every mod of the
//! selected instance, the browser's and those put in its folder by hand, with its version, where
//! it comes from, an update when the browser found one, and a switch. A Java instance turns a mod
//! off by renaming its jar (`.disabled`, as `java_instance` does); a Sunrise instance's mods are
//! the registry's, switched as the game's Options > Mods does. Adding mods is the Mods page's.
//!
//! Each row shows the mod's own icon: Modrinth's for what the browser installed from it (the URLs
//! asked for in one request and kept beside the image cache, the images through the Mods page's
//! cache at the row's size), else the package's icon or the one its jar names, else a generic glyph.
use super::*;
use crate::plate::alpha;
use crate::shell::{c};
use build_mode::ModKind;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use sunrise_launcher::browser::{
    images, installed, java, modrinth, sunrise, Kind as PackKind};
use theme::night;

/// A turned-off jar's suffix, which the loaders don't read.
const OFF: &str = ".disabled";

/// A mod as the tab lists it.
#[derive(Clone)]
pub(super) struct ModRow {
    /// Its file in the instance, as it is when on (`mods/lumen-0.6.2.jar`).
    pub(super) file: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) on: bool,
    /// What the browser knows of it; None: a file put in by hand.
    pub(super) entry: Option<installed::Entry>,
    /// Its validated package icon or jar icon, kept between draws.
    pub(super) icon: Option<Arc<gpui::Image>>,
    pub(super) error: Option<String>,
    pub(super) native: Option<native_status::Installed>,
    pub(super) internals: Vec<String>,
    /// What the package is and its mod id, for a Sunrise mod; None for a Java jar.
    pub(super) kind: Option<(ModKind, String)>,
}

/// Modrinth projects' icon URLs by project id, kept on disk beside the image cache so the icons
/// show offline too; each id is asked for once a session.
#[derive(Default)]
pub(super) struct ProjectIcons {
    urls: HashMap<String, String>,
    loaded: bool,
    asked: HashSet<String>,
}

#[derive(Default)]
pub(super) struct InstanceMods {
    home: Option<PathBuf>,
    pub(super) fresh: bool,
    reading: bool,
    pub(super) list: Vec<ModRow>,
    /// The browser was installing at the last draw: its end changes the list.
    was_busy: bool,
    /// Only the rows near the view are built (a modpack has hundreds), measured again when the
    /// shown list changes (its length and the filter).
    pub(super) cull: cull::Cull,
    pub(super) shown: (usize, String),
    fingerprint: String,
}

impl InstanceMods {
    fn invalidate(&mut self) {
        self.fresh = false;
    }

    /// Marks a read pending and returns the fingerprint its result must match.
    fn begin_read(&mut self) -> String {
        self.reading = true;
        self.fingerprint.clone()
    }

}

/// The mods of the instance at `home`: a Java one's jars in `mods/` (on or off), a Sunrise one's
/// registry mods. Each jar names itself (`fabric.mod.json`, `quilt.mod.json`...), else its file.
fn read(home: &Path, java_instance: bool) -> Vec<ModRow> {
    let entries: Vec<installed::Entry> = installed::Installed::load(home)
        .entries
        .into_iter()
        .filter(|e| e.kind == PackKind::Mod)
        .collect();
    let mut rows: Vec<ModRow> = if java_instance {
        let Ok(dir) = std::fs::read_dir(home.join("mods")) else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|f| {
                let name = f.file_name().to_string_lossy().into_owned();
                let on = name.ends_with(".jar");
                let jar = name.strip_suffix(OFF).unwrap_or(&name);
                (jar.ends_with(".jar") && f.path().is_file()).then(|| {
                    let file = format!("mods/{jar}");
                    let entry = entries.iter().find(|e| e.file == file).cloned();
                    let (name, version) = match &entry {
                        Some(e) => (e.title.clone(), e.number.clone()),
                        None => sunrise_launcher::browser::java::jar_title(&f.path())
                            .unwrap_or_else(|| {
                                (jar.trim_end_matches(".jar").into(), String::new())
                            }),
                    };
                    let icon = images::jar_icon(&f.path(), images::ROW)
                        .map(|png| Arc::new(gpui::Image::from_bytes(gpui::ImageFormat::Png, png)));
                    ModRow {
                        file,
                        name,
                        version,
                        on,
                        entry,
                        icon,
                        error: None,
                        native: None,
                        internals: Vec::new(),
                        kind: None,
                    }
                })
            })
            .collect()
    } else {
        entries
            .into_iter()
            .map(|e| {
                let package = sunrise_launcher_smod::manifest::Package::read(&home.join(&e.file));
                let icon = mod_icons::load(package.as_ref().map(|p| p.icon.as_ref()));
                ModRow {
                    file: e.file.clone(),
                    name: e.title.clone(),
                    version: e.number.clone(),
                    on: sunrise::mod_id(&e).is_some_and(|id| java::is_enabled(home, id)),
                    entry: Some(e),
                    icon: icon.image,
                    error: icon.error,
                    native: None,
                    internals: package
                        .as_ref()
                        .map(|p| native_status::declared_internals(&p.manifest))
                        .unwrap_or_default(),
                    kind: package
                        .as_ref()
                        .ok()
                        .map(|p| (ModKind::of(&p.manifest), p.manifest.id.clone())),
                }
            })
            .collect()
    };
    if !java_instance {
        rows.extend(native_status::untracked(home).into_iter().map(|native| {
            let icon = mod_icons::load(native.icon.as_ref().map(|i| i.as_deref()));
            ModRow {
                file: native.file.clone(),
                name: native.id.clone(),
                version: native.version.clone(),
                on: java::is_enabled(home, &native.id),
                entry: None,
                icon: icon.image,
                error: icon.error,
                kind: Some((ModKind::Native, native.id.clone())),
                internals: native.internals.clone(),
                native: Some(native),
            }
        }));
    }
    rows.sort_by_key(|r| r.name.to_lowercase());
    rows
}

impl Launcher {
    pub(super) fn instance_mods_changed(&mut self) {
        self.instance_mods.invalidate();
        self.instance_mods.cull.reset();
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHome(PathBuf);

    impl TestHome {
        fn new() -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            use std::time::{SystemTime, UNIX_EPOCH};

            static NEXT: AtomicU64 = AtomicU64::new(0);
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is before Unix epoch")
                .as_nanos();
            loop {
                let home = std::env::temp_dir().join(format!(
                    "instance-mods-refresh-{}-{timestamp}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&home) {
                    Ok(()) => return Self(home),
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => panic!("create test home {}: {error}", home.display()),
                }
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn package_rows_cache_content_and_code_icons_and_preserve_refusals() {
        use std::io::{Cursor, Write};

        let scratch = TestHome::new();
        let home = &scratch.0;
        std::fs::create_dir_all(home.join("mods")).unwrap();
        std::fs::create_dir_all(home.join(native_mods::INSTALLED)).unwrap();
        let mut png = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(8, 8, image::Rgba([35, 180, 145, 255]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let png = png.into_inner();
        let pin = "a".repeat(40);
        let mut entries = Vec::new();
        for (id, code, tracked, icon) in [
            ("content", false, true, Some(png.as_slice())),
            ("tracked_code", true, true, Some(png.as_slice())),
            ("local_code", true, false, Some(png.as_slice())),
            ("no_icon", false, true, None),
            ("broken", true, false, Some(b"not a PNG".as_slice())),
        ] {
            let folder = if code { native_mods::INSTALLED } else { "mods" };
            let file = format!("{folder}/{id}.smod");
            let mut manifest = format!(
                "id = '{id}'\nname = '{id}'\nversion = '1'\napi = '0.2'\ncapabilities = ['registries']\n"
            );
            if code {
                manifest +=
                    &format!("[native]\ngame = '26.3'\nsunrise = '{pin}'\nmixins = '{pin}'\n");
                if id == "tracked_code" || id == "local_code" {
                    manifest = manifest.replace("[native]", &format!(
                        "internals = [{{path = 'sunrise_client::game::Game', hash = '{}'}}]\n[native]",
                        "b".repeat(64)
                    ));
                }
            }
            let mut zip = zip::ZipWriter::new(std::fs::File::create(home.join(&file)).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("sunrise-mod.toml", options).unwrap();
            zip.write_all(manifest.as_bytes()).unwrap();
            if code {
                zip.start_file("Cargo.toml", options).unwrap();
                zip.write_all(b"[package]\nname = 'other'\nversion = '9'\n")
                    .unwrap();
                zip.start_file("src/lib.rs", options).unwrap();
                zip.write_all(b"pub fn hooks() {}\n").unwrap();
            }
            if let Some(icon) = icon {
                zip.start_file("icon.png", options).unwrap();
                zip.write_all(icon).unwrap();
            }
            zip.finish().unwrap();
            if tracked {
                entries.push(serde_json::from_value(serde_json::json!({"source":"sunrise","project":id,"slug":id,"title":id,"kind":"mod","version":"1","number":"1","file":file,"checksum":"","page":""})).unwrap());
            }
        }
        installed::Installed { entries }.save(home).unwrap();
        let rows = read(home, false);
        assert_eq!(rows.len(), 5, "tracked native rows must not be duplicated");
        for id in ["content", "tracked_code", "local_code"] {
            let row = rows.iter().find(|r| r.name == id).unwrap();
            assert!(row.icon.is_some(), "{id} must show its package icon");
            assert!(row.error.is_none(), "{id}: {:?}", row.error);
            let expected: &[&str] = if id == "content" {
                &[]
            } else {
                &["sunrise_client::game::Game"]
            };
            assert_eq!(row.internals, expected);
        }
        let no_icon = rows.iter().find(|r| r.name == "no_icon").unwrap();
        assert!(no_icon.icon.is_none() && no_icon.error.is_none());
        let broken = rows.iter().find(|r| r.name == "broken").unwrap();
        assert!(broken.icon.is_none());
        assert!(broken.error.as_ref().unwrap().contains("icon.png"));
        assert!(
            broken.native.is_some(),
            "invalid files must remain removable"
        );
    }

    #[test]
    fn sunrise_mods_include_local_native_files_without_duplicating_registry_rows() {
        let home =
            std::env::temp_dir().join(format!("instance-native-mods-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(native_mods::INSTALLED)).unwrap();
        for id in ["local", "registry"] {
            std::fs::write(
                home.join(native_mods::INSTALLED).join(format!("{id}.smod")),
                b"unreadable",
            )
            .unwrap();
        }
        let entry: installed::Entry = serde_json::from_value(serde_json::json!({"source":"sunrise","project":"registry","slug":"registry","title":"Registry mod","kind":"mod","version":"1","number":"1","file":"sunrise-native-mods/registry.smod","checksum":"","page":""})).unwrap();
        installed::Installed {
            entries: vec![entry],
        }
        .save(&home)
        .unwrap();
        java::set_enabled(&home, "local", false).unwrap();
        java::set_enabled(&home, "registry", true).unwrap();
        let rows = read(&home, false);
        let seen: Vec<_> = rows
            .iter()
            .map(|r| (r.name.as_str(), r.on, r.native.is_some()))
            .collect();
        assert_eq!(
            seen,
            [("local", false, true), ("Registry mod", true, false)]
        );
        assert!(
            read(&home, true).is_empty(),
            "native mods stay out of Java instances"
        );
        std::fs::remove_dir_all(home).unwrap();
    }

}
