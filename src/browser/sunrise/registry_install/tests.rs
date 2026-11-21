use super::*;
use installed::InstallHold;
use sunrise_launcher_smod::Package;

fn archive(home: &Path, id: &str, name: &str, version: &str, depends: bool) -> installed::Entry {
    let path = format!("{}/{id}.smod", mod_list::MODS_DIR);
    let dependencies = if depends {
        "[dependencies]\nlight_core = '^1'\n"
    } else {
        ""
    };
    let bytes = Package::write_files(&format!("id='{id}'\nname='{name}'\nversion='{version}'\napi='0.2'\nside='both'\ncapabilities=['registries']\n{dependencies}"), &[]).unwrap();
    fs::create_dir_all(home.join(mod_list::MODS_DIR)).unwrap();
    fs::write(home.join(&path), &bytes).unwrap();
    installed::Entry {
        source: SourceId::Sunrise,
        project: id.into(),
        slug: id.into(),
        title: name.into(),
        kind: Kind::Mod,
        version: version.into(),
        number: version.into(),
        file: path,
        checksum: format!("SHA-256:{}", index::sha256(&bytes)),
        page: String::new(),
        dependency: false,
        verdict: None,
        install_hold: None,
    }
}

#[test]
fn resolve_attributes_incompatibility_to_the_games_short_version() {
    let old = tempfile::tempdir().unwrap();
    let candidate = tempfile::tempdir().unwrap();
    let blocker = archive(old.path(), "lanterns", "Lanterns", "1.0.0", false);
    let manifest = "id='lanterns'\nname='Lanterns'\nversion='1.0.0'\napi='0.2'\ncapabilities=['registries']\n[incompatible]\nlight_core='^2'\n";
    fs::write(
        old.path().join(blocker.file),
        Package::write_files(manifest, &[]).unwrap(),
    )
    .unwrap();
    archive(candidate.path(), "light_core", "Light Core", "2", false);
    mod_list::set_enabled(candidate.path(), "light_core", true).unwrap();
    let catalogue = index::Index::parse(include_str!(
        "../../../../crates/smod/tests/index/index-v1.json"
    ))
    .unwrap();
    let before = inventory(old.path()).unwrap();
    let question =
        conflicts::question(&catalogue, &[], &before, candidate.path(), &["lanterns"]).unwrap();
    assert_eq!(
        question.blockers,
        [("Lanterns".into(), "Doesn't work with Light Core 2.".into())]
    );
}

#[test]
fn continue_proposal_discloses_off_dependents_and_preserves_the_working_predecessor() {
    let old = tempfile::tempdir().unwrap();
    let candidate = tempfile::tempdir().unwrap();
    let original = archive(old.path(), "engraver", "Engraver", "1.0.0", false);
    let old_bytes = fs::read(old.path().join(&original.file)).unwrap();
    let row = serde_json::to_value(&original).unwrap();
    let mut before_doc = Document::read(old.path()).unwrap();
    before_doc.set("engraver", row.clone()).unwrap();
    before_doc.write(old.path()).unwrap();
    mod_list::set_enabled(old.path(), "engraver", true).unwrap();
    let selection = fs::read(old.path().join(sunrise_launcher_smod::mod_list::FILE)).unwrap();
    let rows = fs::read(old.path().join(installed::MANIFEST)).unwrap();

    let mut entries: BTreeMap<_, _> = [
        archive(candidate.path(), "light_core", "Light Core", "1.0.0", false),
        archive(candidate.path(), "lanterns", "Lanterns", "1.0.0", true),
        archive(candidate.path(), "engraver", "Engraver", "2.0.0", true),
    ]
    .into_iter()
    .map(|entry| (entry.project.clone(), entry))
    .collect();
    let mut doc = Document::read(candidate.path()).unwrap();
    for entry in entries.values() {
        doc.replace(entry).unwrap();
    }
    doc.write(candidate.path()).unwrap();
    mod_list::set_all_enabled(
        candidate.path(),
        &["light_core", "lanterns", "engraver"],
        true,
    )
    .unwrap();
    let changes = restore_or_disable(
        old.path(),
        candidate.path(),
        &inventory(old.path()).unwrap(),
        &mut entries,
        &mut doc,
        BTreeMap::from([("light_core".into(), InstallHold::Compile)]),
    )
    .unwrap();
    let changes: BTreeMap<_, _> = changes
        .into_iter()
        .map(|change| (change.project.clone(), change))
        .collect();
    assert_eq!(changes.len(), 3);
    assert_eq!(changes["light_core"].reason, InstallHold::Compile);
    let needs = InstallHold::Needs {
        name: "Light Core".into(),
    };
    assert_eq!(changes["lanterns"].reason, needs);
    assert_eq!(changes["engraver"].reason, needs);
    assert_eq!(changes["engraver"].predecessor.as_deref(), Some("1.0.0"));
    assert!(
        changes["light_core"].predecessor.is_none() && changes["lanterns"].predecessor.is_none()
    );
    assert_eq!(
        fs::read(candidate.path().join(&original.file)).unwrap(),
        old_bytes
    );
    assert_eq!(
        fs::read(old.path().join(&original.file)).unwrap(),
        old_bytes
    );
    assert_eq!(
        fs::read(old.path().join(installed::MANIFEST)).unwrap(),
        rows
    );
    assert_eq!(
        fs::read(old.path().join(sunrise_launcher_smod::mod_list::FILE)).unwrap(),
        selection
    );
    assert!(mod_list::is_enabled(candidate.path(), "engraver"));
    let reduced = Document::read(candidate.path()).unwrap();
    assert_eq!(reduced.row("engraver").unwrap(), Some(&row));
    for (id, reason) in [("light_core", InstallHold::Compile), ("lanterns", needs)] {
        assert!(!mod_list::is_enabled(candidate.path(), id));
        let entry: installed::Entry =
            serde_json::from_value(reduced.row(id).unwrap().unwrap().clone()).unwrap();
        assert_eq!(entry.install_hold, Some(reason));
    }
}
