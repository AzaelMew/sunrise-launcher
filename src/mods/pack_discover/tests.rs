use super::*;
use sunrise_launcher::browser::{download, FileRef, Version};

fn project(id: &str, source: SourceId, kind: Kind) -> Project {
    Project {
        source,
        id: id.into(),
        slug: id.into(),
        title: id.into(),
        summary: String::new(),
        author: String::new(),
        downloads: 0,
        page: String::new(),
        icon: None,
        license: String::new(),
        kind,
        game_versions: vec![],
        loaders: vec![],
        blocked: None,
    }
}

/// A setup's folder with worlds `names` (a `level.dat` each).
fn home(names: &[&str]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for n in names {
        let w = dir.path().join("saves").join(n);
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(w.join("level.dat"), b"").unwrap();
    }
    dir
}

#[test]
fn the_query_is_restricted_to_one_pack_kind_that_runs() {
    let q = query(Scope::Data, &"a".repeat(500), "1.21.4");
    assert_eq!(q.kind, Kind::DataPack);
    assert_eq!(q.text.chars().count(), TEXT_MAX);
    assert!(!q.everything);
    assert_eq!(q.game, "1.21.4");
    assert_eq!((q.offset, q.limit), (0, LIMIT));
    assert_eq!(query(Scope::Resource, "", "g").kind, Kind::ResourcePack);
}

#[test]
fn a_scope_keeps_only_modrinth_hits_of_its_own_kind() {
    let hits = || {
        vec![
            project("a", SourceId::Modrinth, Kind::ResourcePack),
            project("b", SourceId::Modrinth, Kind::DataPack),
            project("c", SourceId::Modrinth, Kind::Mod),
            project("d", SourceId::Modrinth, Kind::Modpack),
            project("e", SourceId::Modrinth, Kind::Shader),
            project("f", SourceId::Sunrise, Kind::ResourcePack),
            project("a", SourceId::Modrinth, Kind::ResourcePack),
        ]
    };
    let ids = |v: Vec<Project>| v.into_iter().map(|p| p.id).collect::<Vec<_>>();
    assert_eq!(ids(keep(Scope::Resource, hits())), ["a"]);
    assert_eq!(ids(keep(Scope::Data, hits())), ["b"]);
}

#[test]
fn only_a_plain_world_folder_of_the_setup_is_a_target() {
    let home = home(&["One"]);
    let world = home.path().join("saves/One");
    assert_eq!(check_world(home.path(), &world).as_deref(), Ok("One"));

    // Gone, never a world, or in another setup.
    assert!(check_world(home.path(), &home.path().join("saves/Two")).is_err());
    std::fs::create_dir(home.path().join("saves/NoLevel")).unwrap();
    assert!(check_world(home.path(), &home.path().join("saves/NoLevel")).is_err());
    let other = self::home(&["One"]);
    assert!(check_world(home.path(), &other.path().join("saves/One")).is_err());
    // Not directly in saves/.
    assert!(check_world(home.path(), &world.join("..").join("One")).is_err());
    assert!(check_world(home.path(), home.path()).is_err());
}

#[cfg(unix)]
#[test]
fn a_linked_world_or_datapacks_folder_is_refused() {
    use std::os::unix::fs::symlink;
    let home = home(&["One"]);
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("level.dat"), b"").unwrap();
    symlink(elsewhere.path(), home.path().join("saves/Linked")).unwrap();
    assert!(check_world(home.path(), &home.path().join("saves/Linked")).is_err());

    let world = home.path().join("saves/One");
    symlink(elsewhere.path(), world.join("datapacks")).unwrap();
    assert!(check_world(home.path(), &world).is_err());

    let linked_saves = tempfile::tempdir().unwrap();
    symlink(home.path().join("saves"), linked_saves.path().join("saves")).unwrap();
    assert!(check_world(linked_saves.path(), &linked_saves.path().join("saves/One")).is_err());
}

/// Serves bytes that don't match the checksum the version lists.
struct Tampered(Project);

impl browser::Source for Tampered {
    fn id(&self) -> SourceId {
        SourceId::Modrinth
    }
    fn search(&self, _: &Query) -> Result<Vec<Project>, String> {
        Ok(vec![self.0.clone()])
    }
    fn project(&self, _: &str) -> Result<Project, String> {
        Ok(self.0.clone())
    }
    fn versions(&self, _: &Project) -> Result<Vec<Version>, String> {
        Ok(vec![Version {
            id: "v".into(),
            number: "1".into(),
            published: String::new(),
            game_versions: vec![browser::game()],
            loaders: vec!["datapack".into()],
            files: vec![FileRef {
                url: "https://cdn.modrinth.com/x.zip".into(),
                name: "x.zip".into(),
                size: 3,
                sha512: Some("0".repeat(128)),
                sha256: None,
                sha1: None,
                primary: true,
            }],
            required: vec![],
            blocked: None,
        }])
    }
    fn fetch(
        &self,
        file: &FileRef,
        folder: &Path,
        progress: &dyn Fn(f32),
    ) -> Result<PathBuf, String> {
        download::save(file, &b"bad"[..], folder, progress)
    }
}

#[test]
fn a_data_pack_that_fails_its_checksum_puts_nothing_in_the_world() {
    let home = home(&["One"]);
    let world = home.path().join("saves/One");
    let p = project("dp", SourceId::Modrinth, Kind::DataPack);
    let result = installed::install_datapack(
        &Tampered(p),
        &project("dp", SourceId::Modrinth, Kind::DataPack),
        false,
        &world,
        &browser::game(),
        &|_| {},
    );
    assert!(result.is_err(), "a bad checksum must refuse the install");
    assert!(!world.join("datapacks").exists());
}

#[test]
fn shader_packs_come_from_modrinth_and_sunrises_registry_only() {
    let hits = vec![
        project("m", SourceId::Modrinth, Kind::Shader),
        project("s", SourceId::Sunrise, Kind::Shader),
        project("r", SourceId::Sunrise, Kind::ResourcePack),
        project("x", SourceId::Modrinth, Kind::Mod),
    ];
    let ids: Vec<_> = keep(Scope::Shader, hits.clone())
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids, ["m", "s"]);
    // The registry's shader pack never shows under resource packs, nor Sunrise's under data.
    assert!(keep(Scope::Resource, hits.clone()).is_empty());
    assert!(keep(Scope::Data, hits).is_empty());
    assert_eq!(query(Scope::Shader, "", "g").kind, Kind::Shader);
}
