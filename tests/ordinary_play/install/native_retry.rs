//! A proved native no-fit can change the verified closure before any compiler work.
use super::*;

#[test]
fn a_different_native_dependency_needs_a_new_answer_after_no_fit() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry();
    let template = registry.project("coords_hud").unwrap().clone();
    registry.projects.clear();
    for (id, name, code, releases) in [
        (
            "first_light",
            "First Light",
            Some('b'),
            vec![("1.0.0", vec![])],
        ),
        ("new_light", "New Light", Some('a'), vec![("1.0.0", vec![])]),
        (
            "shared_light",
            "Shared Light",
            Some('a'),
            vec![("1.0.0", vec![])],
        ),
        (
            "root_mod",
            "Lanterns",
            None,
            vec![
                ("2.0.0", vec!["first_light", "shared_light"]),
                ("1.0.0", vec!["new_light", "shared_light"]),
            ],
        ),
    ] {
        let kind = if code.is_some() {
            index::Kind::NativeMod
        } else {
            index::Kind::Mod
        };
        let mut versions = Vec::new();
        for (version, dependencies) in releases {
            let mut manifest = format!("id='{id}'\nname='{name}'\nversion='{version}'\napi='0.2'\nside='both'\ncapabilities=['registries']\n");
            if !dependencies.is_empty() {
                manifest.push_str("[dependencies]\n");
                for dep in &dependencies {
                    manifest.push_str(&format!("{dep}='^1'\n"));
                }
            }
            let cargo = format!("[package]\nname='{id}'\nversion='{version}'\n");
            let files: &[(&str, &[u8])] = if let Some(pin) = code {
                manifest.push_str(&format!(
                    "[native]\nsunrise='{}'\nmixins='{}'\ngame='26.3'\n",
                    pin.to_string().repeat(40),
                    "c".repeat(40)
                ));
                &[("Cargo.toml", cargo.as_bytes()), ("src/lib.rs", b"")]
            } else {
                &[]
            };
            let bytes = Package::write_files(&manifest, files).unwrap();
            let filename = format!("{id}-{version}.smod");
            let file = fixture.root.join(&filename);
            fs::write(&file, &bytes).unwrap();
            let facts = index::inspect(&kind, &file).unwrap();
            let mut row = template.versions[0].clone();
            row.version = version.into();
            row.game = "26.3".into();
            row.api = facts.api;
            row.side = facts.side;
            row.capabilities = serde_json::from_value(json!(facts.capabilities)).unwrap();
            row.dependencies = Some(
                dependencies
                    .into_iter()
                    .map(|id| index::Dependency {
                        id: id.into(),
                        version: "^1".into(),
                        optional: false,
                        side: None,
                    })
                    .collect(),
            );
            row.file = index::File {
                name: filename,
                url: format!("file://{}", file.display()),
                size: bytes.len() as u64,
                sha256: index::sha256(&bytes),
                signature: None,
            };
            versions.push(row);
        }
        registry.projects.push(index::Project {
            id: id.into(),
            name: name.into(),
            kind,
            key: None,
            versions,
            ..template.clone()
        });
    }
    fixture.publish_registry(&registry);
    let tool = fixture.root.join("fixture-tool");
    script(
        &tool,
        "#!/bin/sh\ntouch \"$FIXTURE_UNEXPECTED_WORK\"\nexit 7\n",
    );
    let tool_ran = fixture.root.join("tool-ran");
    let mut worker = Command::new(std::env::current_exe().unwrap());
    for (key, value) in fixture.registry_client(&[]).get_envs() {
        worker.env(key, value.unwrap());
    }
    let report = worker
        .args(["--exact", "install::native_retry::retry_worker"])
        .env("FIXTURE_UNEXPECTED_WORK", &tool_ran)
        .env("SUNRISE_LAUNCHER_DEVELOPER_TOOL", &tool)
        .env("SUNRISE_LAUNCHER_DEVELOPER_SDK", &fixture.root)
        .output()
        .unwrap();
    assert!(
        report.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&report.stdout),
        String::from_utf8_lossy(&report.stderr)
    );
    assert!(!tool_ran.exists());
    assert!(!fixture.home.join(index::browser_state::MANIFEST).exists());
    assert!(!fixture.home.join(mod_list::DIR).exists());
    assert!(!fixture.home.join(mod_list::NATIVE_DIR).exists());
    assert!(!fixture.home.join("mod-browser/active-build.json").exists());
}

#[test]
fn switch_proves_native_against_the_exact_target_even_with_an_unlike_catalogue_game() {
    use sunrise_launcher::browser::{installed, Kind, SourceId};
    for listed in [true, false] {
        let fixture = Fixture::new();
        fixture.other_build();
        let mut registry = fixture.registry();
        let template = registry.project("coords_hud").unwrap().clone();
        let kind = index::Kind::NativeMod;
        let bytes = Package::write_files(
        &format!("id='night_light'\nname='Night Light'\nversion='1'\napi='{}'\nside='both'\ncapabilities=['registries']\n[native]\nsunrise='{}'\nmixins='{}'\ngame='26.3'", manifest::API, "a".repeat(40), "c".repeat(40)),
        &[("Cargo.toml", b"[package]\nname='night_light'\nversion='1.0.0'\n"), ("src/lib.rs", b""), (manifest::USES_FILE, b"{}")],
    ).unwrap();
        let file = format!("{}/night_light.smod", mod_list::NATIVE_DIR);
        let path = fixture.home.join(&file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
        let hash = index::sha256(&bytes);
        let facts = index::inspect(&kind, &path).unwrap();
        let mut version = template.versions[0].clone();
        version.version = "1".into();
        version.game = "26.3".into();
        version.api = facts.api;
        version.side = facts.side;
        version.capabilities = serde_json::from_value(json!(facts.capabilities)).unwrap();
        version.dependencies = Some(Vec::new());
        version.file = index::File {
            name: "night_light.smod".into(),
            url: format!("file://{}", path.display()),
            size: bytes.len() as u64,
            sha256: hash.clone(),
            signature: None,
        };
        if !listed {
            version.file.sha256 = "0".repeat(64);
        }
        registry.projects = vec![index::Project {
            id: "night_light".into(),
            name: "Night Light".into(),
            key: None,
            kind,
            versions: vec![version],
            ..template
        }];
        fixture.publish_registry(&registry);
        let row = installed::Entry {
            source: SourceId::Sunrise,
            project: "night_light".into(),
            mod_id: None,
            slug: "night_light".into(),
            title: "Night Light".into(),
            kind: Kind::Mod,
            version: "1".into(),
            number: "1".into(),
            file,
            checksum: format!("SHA-256:{hash}"),
            page: String::new(),
            dependency: false,
            verdict: None,
            install_hold: None,
            repository: None,
        };
        let rows = fixture.home.join(index::browser_state::MANIFEST);
        fs::create_dir_all(rows.parent().unwrap()).unwrap();
        fs::write(rows, json!({"entries": [row]}).to_string()).unwrap();
        if listed {
            fixture.decision_worker("switch-package-catalogue-mismatch");
        } else {
            sunrise_launcher::browser::mod_list::set_enabled(&fixture.home, "night_light", true)
                .unwrap();
            let result = fixture
                .registry_client(&["switch", OTHER])
                .output()
                .unwrap();
            assert!(!result.status.success(), "{result:?}");
            let report = String::from_utf8_lossy(&result.stderr);
            assert!(report.contains("Fit could not be verified"), "{report}");
            assert!(report.contains("run Switch in a terminal"), "{report}");
            assert!(!fixture.home.join("mod-browser/active-build.json").exists());
            fixture.decision_worker("switch-package-no-consent");
        }
        let active = Active::parse(&fixture.pointer()).unwrap();
        assert_eq!(active.base(), OTHER);
        let hold = active.holds().next().unwrap();
        assert_eq!(hold.id(), "night_light");
        assert_eq!(hold.reason().unproved(), !listed);
        assert!(!sunrise_launcher::browser::mod_list::is_enabled(
            &fixture.home,
            "night_light"
        ));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn switch_reports_a_newer_uses_list_and_its_content_dependent_as_unknown() {
    use sunrise_launcher::rust_toolchain;
    use sunrise_launcher_release_verify::{sha256_hex, sums_file, SigningKey, SIGNATURE, SUMS};
    use sunrise_launcher_sdk::{self as sdk, Asset, Kind, Side};

    const TARGET: &str = "v26.4-fixture";
    let fixture = Fixture::new();
    fixture.other_build();
    let versions = fixture.data.join("versions");
    fs::rename(versions.join(OTHER), versions.join(TARGET)).unwrap();
    fs::write(
        fixture.data.join("settings.json"),
        r#"{"native_mods_allowed_at":1,"native_consent_revision":1}"#,
    )
    .unwrap();
    fixture.publish_registry(&fixture.registry());
    let mut archives = Vec::new();
    for (id, title, layer, manifest_extra, files) in [
        (
            "night_light",
            "Night Light",
            mod_list::NATIVE_DIR,
            format!(
                "[native]\nsunrise='{}'\nmixins='{}'\ngame='26.3'",
                "a".repeat(40),
                "c".repeat(40)
            ),
            &[
                (
                    "Cargo.toml",
                    b"[package]\nname='night_light'\nversion='1.0.0'\n".as_slice(),
                ),
                ("src/lib.rs", b"".as_slice()),
                (manifest::USES_FILE, b"{}".as_slice()),
            ][..],
        ),
        (
            "lanterns",
            "Lanterns",
            mod_list::DIR,
            "[dependencies]\nnight_light='^1'".into(),
            &[][..],
        ),
    ] {
        let bytes = Package::write_files(
            &format!("id='{id}'\nname='{title}'\nversion='1.0.0'\napi='{}'\nside='both'\ncapabilities=['registries']\n{manifest_extra}", manifest::API),
            files,
        ).unwrap();
        let path = fixture.home.join(layer).join(format!("{id}.smod"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
        sunrise_launcher::browser::mod_list::set_enabled(&fixture.home, id, true).unwrap();
        archives.push((id, title, path, bytes));
    }
    // The signed test SDK has only the protocol tool. Its shell script records stdin
    // and prints FIXTURE_NATIVE_CHECK_REPLY; it cannot compile a mod or run a game.
    let tool = include_bytes!("native-check-tool.tar.xz");
    let host = rust_toolchain::host().expect("a supported SDK host");
    let asset = Asset::name(Kind::Tool, host.target, Side::Client);
    let commit = "b".repeat(40);
    let sdk_tag = format!("sdk-{commit}");
    let sdk_base = fixture.root.join("sdk");
    let index = json!({
        "schema": sdk::SCHEMA,
        "package_format": sdk::PACKAGE_FORMAT,
        "build": {"commit": commit, "game": "26.4", "date": "2026-11-21"},
        "toolchain": {"rust": rust_toolchain::VERSION},
        "content_api": manifest::API,
        "surface": {"hash": "ab".repeat(16)},
        "mixins": {"rev": "c".repeat(40), "patch": "0".repeat(64)},
        "inputs": serde_json::from_str::<Value>(include_str!("../../../crates/sdk/src/sdk/inputs.json")).unwrap(),
        "assets": {&asset: {"sha256": sha256_hex(tool), "size": tool.len(), "kind": "tool", "codec": "xz", "target": host.target}},
    }).to_string();
    let signer = SigningKey::generate().unwrap().0;
    let sums = sums_file(&[
        (sdk::INDEX.into(), sha256_hex(index.as_bytes())),
        (asset.clone(), sha256_hex(tool)),
    ]);
    for tag in [TARGET, &sdk_tag] {
        let release = sdk_base.join(tag);
        fs::create_dir_all(&release).unwrap();
        fs::write(release.join(sdk::INDEX), &index).unwrap();
        fs::write(release.join(SUMS), &sums).unwrap();
        fs::write(release.join(SIGNATURE), signer.sign(tag, sums.as_bytes())).unwrap();
    }
    fs::write(sdk_base.join(sdk_tag).join(asset), tool).unwrap();
    let request = fixture.root.join("native-check-request");
    let output = fixture.decision_worker_command("switch-package-newer-list")
        .env("SUNRISE_SDK_BASE", sdk_base)
        .env("SUNRISE_SDK_TRUSTED_KEY", signer.public_base64())
        .env("FIXTURE_NATIVE_CHECK_REQUEST", &request)
        .env("FIXTURE_NATIVE_CHECK_REPLY", json!({"contract":2,"route":null,"mods":[{"id":"night_light","verdict":{"kind":"no-list","made_for":"a".repeat(40),"why":"newer-schema"}}]}).to_string())
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let checked: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
    assert_eq!(checked["build"], commit);
    let active = Active::parse(&fixture.pointer()).unwrap();
    assert_eq!(active.base(), TARGET);
    assert_eq!(active.holds().count(), archives.len());
    for (id, _, path, bytes) in archives {
        let held = active.holds().find(|held| held.id() == id).unwrap();
        assert!(held.reason().unproved());
        assert!(!sunrise_launcher::browser::mod_list::is_enabled(
            &fixture.home,
            id
        ));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

const CREATURE_MODS: [(&str, &str); 11] = [
    ("creature", "Creature Library"),
    ("beetles", "Beetles"),
    ("butterflies", "Butterflies"),
    ("crickets", "Crickets"),
    ("dragonflies", "Dragonflies"),
    ("jellyfish", "Jellyfish"),
    ("ladybugs", "Ladybugs"),
    ("phasmids", "Phasmids"),
    ("seagulls", "Seagulls"),
    ("vultures", "Vultures"),
    ("vulture_nests", "Vulture Nests"),
];

fn switch_creature_mods(decision: SwitchDecision) {
    let fixture = Fixture::new();
    let (registry, _) = fixture.content_index();
    fixture.publish_registry(&registry);
    fixture.install_mod("extra_mod");
    fixture.other_build();
    let target = fixture.data.join("versions").join(OTHER);
    fs::write(
        target.join("research-lock.json"),
        r#"{"minecraft":{"version":"26.3"}}"#,
    )
    .unwrap();
    fs::rename(
        target.join(".local/sunrise-client/26.4"),
        target.join(".local/sunrise-client/26.3"),
    )
    .unwrap();
    let mut archives = BTreeMap::new();
    for (i, (id, title)) in CREATURE_MODS.into_iter().enumerate() {
        let native = i < 10;
        let dependency = match id {
            "creature" => "",
            "vulture_nests" => "[dependencies]\nvultures='^1'\n",
            _ => "[dependencies]\ncreature='^1'\n",
        };
        let layer = if native {
            mod_list::NATIVE_DIR
        } else {
            mod_list::DIR
        };
        let mut manifest = format!("id='{id}'\nname='{title}'\nversion='1.0.0'\napi='{}'\nside='both'\ncapabilities=['registries']\n{dependency}", manifest::API);
        let cargo = format!("[package]\nname='{id}'\nversion='1.0.0'\n");
        let files: &[(&str, &[u8])] = if native {
            manifest.push_str(&format!(
                "[native]\nsunrise='{}'\nmixins='{}'\ngame='26.3'\n",
                "a".repeat(40),
                "c".repeat(40)
            ));
            &[("Cargo.toml", cargo.as_bytes()), ("src/lib.rs", b"")]
        } else {
            &[]
        };
        let bytes = Package::write_files(&manifest, files).unwrap();
        let file_name = if id == "beetles" {
            "beetles-renamed.smod".to_owned()
        } else {
            format!("{id}.smod")
        };
        let path = fixture.home.join(layer).join(file_name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
        sunrise_launcher::browser::mod_list::set_enabled(&fixture.home, id, true).unwrap();
        archives.insert(id, (path, bytes));
    }
    let mut origins = Vec::new();
    for id in ["creature", "beetles"] {
        let (_, bytes) = &archives[id];
        let origin = fixture
            .home
            .join(mod_list::NATIVE_DIR)
            .join(format!("{id}.origin.json"));
        let receipt = serde_json::to_vec(&sunrise_launcher::github_mods::Origin {
            id: id.into(),
            version: "1.0.0".into(),
            owner: "fixture".into(),
            repo: id.into(),
            tag: None,
            commit: "c".repeat(40),
            asset: None,
            sha256: index::sha256(bytes),
            added_from: None,
            source: None,
        })
        .unwrap();
        fs::write(&origin, &receipt).unwrap();
        origins.push((origin, receipt));
    }
    let selection = fixture.home.join(mod_list::FILE);
    let before = (fixture.pointer(), fs::read(&selection).unwrap());
    let config = fixture.home.join("config/creature.toml");
    fs::create_dir(config.parent().unwrap()).unwrap();
    fs::write(&config, "fixture=true").unwrap();
    let save = (decision == SwitchDecision::Remove).then(|| {
        let path = write_world(&fixture.home, 4300);
        let bytes = fs::read(&path).unwrap();
        (path, bytes)
    });
    fixture.decision_worker(match decision {
        SwitchDecision::Cancel => "switch-creatures-cancel",
        SwitchDecision::TurnOff => "switch-creatures-off",
        SwitchDecision::Remove => "switch-creatures-remove",
    });
    let active = Active::parse(&fixture.pointer()).unwrap();
    match decision {
        SwitchDecision::Cancel => {
            assert_eq!(fixture.pointer(), before.0);
            assert_eq!(fs::read(selection).unwrap(), before.1);
        }
        SwitchDecision::TurnOff => {
            assert_eq!(active.base(), OTHER);
            assert_eq!(active.holds().count(), archives.len());
            assert!(active
                .holds()
                .all(|hold| !hold.reason().unproved() && hold.matches_archive(&fixture.home)));
        }
        SwitchDecision::Remove => {
            assert_eq!(active.base(), OTHER);
            assert_eq!(active.holds().count(), 0);
            assert_eq!(
                active.pending_changes().unwrap().removed.len(),
                archives.len()
            );
        }
    }
    assert!(sunrise_launcher::browser::mod_list::is_enabled(
        &fixture.home,
        "extra_mod"
    ));
    for (id, (path, bytes)) in archives {
        if decision == SwitchDecision::Remove {
            assert!(!path.exists());
        } else {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        assert_eq!(
            sunrise_launcher::browser::mod_list::is_enabled(&fixture.home, id),
            decision == SwitchDecision::Cancel
        );
    }
    for (origin, receipt) in origins {
        if decision == SwitchDecision::Remove {
            assert!(!origin.exists());
        } else {
            assert_eq!(fs::read(origin).unwrap(), receipt);
        }
    }
    if let Some((path, bytes)) = save {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(fs::read_to_string(config).unwrap(), "fixture=true");
}

#[test]
fn switch_pinned_mods_cancel_keeps_all_inputs() {
    switch_creature_mods(SwitchDecision::Cancel);
}

#[test]
fn switch_pinned_mods_turns_them_and_their_dependents_off() {
    switch_creature_mods(SwitchDecision::TurnOff);
}

#[test]
fn switch_pinned_mods_removes_only_the_listed_archives_and_receipts() {
    switch_creature_mods(SwitchDecision::Remove);
}

#[test]
fn switch_signed_release_prunes_mismatched_pinned_mods_as_unproved() {
    use sunrise_launcher::rust_toolchain;
    use sunrise_launcher_release_verify::{sha256_hex, sums_file, SigningKey, SIGNATURE_V2, SUMS};
    use sunrise_launcher_sdk::{self as sdk, Asset, Kind, Side};

    const TARGET: &str = "dev-2026-11-22-bbbbbbb";
    let fixture = Fixture::new();
    let (registry, _) = fixture.content_index();
    fixture.publish_registry(&registry);
    fixture.install_mod("extra_mod");
    fixture.other_build();
    let versions = fixture.data.join("versions");
    let target = versions.join(TARGET);
    fs::rename(versions.join(OTHER), &target).unwrap();
    fs::remove_file(target.join("developer.json")).unwrap();
    fs::write(
        target.join("research-lock.json"),
        r#"{"minecraft":{"version":"26.3"}}"#,
    )
    .unwrap();
    fs::rename(
        target.join(".local/sunrise-client/26.4"),
        target.join(".local/sunrise-client/26.3"),
    )
    .unwrap();

    for (i, (id, title)) in CREATURE_MODS.into_iter().enumerate() {
        let native = i < 10;
        let dependency = match id {
            "creature" => "",
            "vulture_nests" => "[dependencies]\nvultures='^1'\n",
            _ => "[dependencies]\ncreature='^1'\n",
        };
        let layer = if native {
            mod_list::NATIVE_DIR
        } else {
            mod_list::DIR
        };
        let mut manifest = format!(
            "id='{id}'\nname='{title}'\nversion='1.0.0'\napi='{}'\nside='both'\ncapabilities=['registries']\n{dependency}",
            manifest::API
        );
        let cargo = format!("[package]\nname='{id}'\nversion='1.0.0'\n");
        let files: &[(&str, &[u8])] = if native {
            manifest.push_str(&format!(
                "[native]\nsunrise='{}'\nmixins='{}'\ngame='26.3'\n",
                "a".repeat(40),
                "c".repeat(40)
            ));
            &[("Cargo.toml", cargo.as_bytes()), ("src/lib.rs", b"")]
        } else {
            &[]
        };
        let bytes = Package::write_files(&manifest, files).unwrap();
        let path = fixture.home.join(layer).join(format!("{id}.smod"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
        sunrise_launcher::browser::mod_list::set_enabled(&fixture.home, id, true).unwrap();
    }

    let host = rust_toolchain::host().expect("a supported SDK host");
    let asset = Asset::name(Kind::Tool, host.target, Side::Client);
    let commit = "b".repeat(40);
    let sdk_base = fixture.root.join("sdk");
    let index = json!({
        "schema": sdk::SCHEMA,
        "package_format": sdk::PACKAGE_FORMAT,
        "build": {"commit": commit, "game": "26.3", "date": "2026-11-22"},
        "toolchain": {"rust": rust_toolchain::VERSION},
        "content_api": manifest::API,
        "surface": {"hash": "ab".repeat(16)},
        "mixins": {"rev": "c".repeat(40), "patch": "0".repeat(64)},
        "inputs": serde_json::from_str::<Value>(include_str!("../../../crates/sdk/src/sdk/inputs.json")).unwrap(),
        "assets": {&asset: {"sha256": "0".repeat(64), "size": 0, "kind": "tool", "codec": "xz", "target": host.target}},
    })
    .to_string();
    let signer = SigningKey::generate().unwrap().0;
    let sums = sums_file(&[(sdk::INDEX.into(), sha256_hex(index.as_bytes()))]);
    let release = sdk_base.join(TARGET);
    fs::create_dir_all(&release).unwrap();
    fs::write(release.join(sdk::INDEX), &index).unwrap();
    fs::write(release.join(SUMS), &sums).unwrap();
    fs::write(
        release.join(SIGNATURE_V2),
        signer.sign(TARGET, sums.as_bytes()),
    )
    .unwrap();

    let output = fixture
        .decision_worker_command("switch-creatures-signed-unproved")
        .env("SUNRISE_SDK_BASE", sdk_base)
        .env("SUNRISE_SDK_TRUSTED_KEY", signer.public_base64())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let active = Active::parse(&fixture.pointer()).unwrap();
    assert_eq!(active.base(), TARGET);
    assert_eq!(active.holds().count(), CREATURE_MODS.len());
    assert!(active
        .holds()
        .all(|hold| hold.reason().unproved() && hold.matches_archive(&fixture.home)));
    assert!(sunrise_launcher::browser::mod_list::is_enabled(
        &fixture.home,
        "extra_mod"
    ));
}

pub(super) fn creature_decision_worker(
    mode: &str,
    instance: &instances::Instance,
    identity: sunrise_launcher::profile_txn::Identity,
) {
    use std::cell::Cell;
    let (tag, decision, expect_unproved) = match mode {
        "switch-creatures-cancel" => (OTHER, SwitchDecision::Cancel, false),
        "switch-creatures-off" => (OTHER, SwitchDecision::TurnOff, false),
        "switch-creatures-remove" => (OTHER, SwitchDecision::Remove, false),
        "switch-creatures-signed-unproved" => {
            ("dev-2026-11-22-bbbbbbb", SwitchDecision::TurnOff, true)
        }
        _ => unreachable!(),
    };
    let trials = Cell::new(0);
    let questions = Cell::new(0);
    let cutoff = Cell::new(0);
    let result = switch_build(
        instance,
        identity,
        tag,
        SwitchHooks {
            cancellation: &Cancellation::default(),
            progress: &|step| {
                if step == InstallProgress::Preparing {
                    trials.set(trials.get() + 1);
                }
            },
            registry_crates: None,
            approve: &|q| {
                questions.set(questions.get() + 1);
                let mut names: Vec<_> = CREATURE_MODS.iter().map(|(_, name)| *name).collect();
                names.sort_unstable();
                if expect_unproved {
                    assert_eq!(q.unproved, names);
                    assert!(q.held.is_empty());
                } else {
                    assert_eq!(q.held, names);
                    assert!(q.unproved.is_empty());
                }
                assert_eq!(q.worlds.needed(), decision == SwitchDecision::Remove);
                if decision == SwitchDecision::Remove {
                    assert_eq!(q.worlds.affected[0].world, "Glass Basin");
                    assert!(q.worlds.affected[0]
                        .mods
                        .iter()
                        .any(|name| name == "Vulture Nests"));
                }
                Ok(decision)
            },
            at_cutoff: &|| {
                cutoff.set(cutoff.get() + 1);
                Ok(())
            },
        },
    )
    .unwrap();
    assert_eq!(questions.get(), 1);
    assert_eq!(
        trials.get(),
        1,
        "the ten pinned mods and their dependent aren't branches"
    );
    assert_eq!(cutoff.get(), u32::from(decision != SwitchDecision::Cancel));
    assert_eq!(
        matches!(result, Switched::Stayed),
        decision == SwitchDecision::Cancel
    );
}

#[test]
fn retry_worker() {
    if std::env::var_os("FIXTURE_UNEXPECTED_WORK").is_none() {
        return;
    }
    use sunrise_launcher::{backend::Settings, browser::sunrise::*, instances, managed_play};
    let instance = instances::selection().unwrap();
    let questions = std::cell::RefCell::new(Vec::new());
    let done = install_registry(
        &instance,
        install_target(&instance).unwrap(),
        || vec!["root_mod".into()],
        || managed_play::headless_first(&instance),
        InstallHooks {
            progress: &|_| {},
            native: &|q| {
                questions.borrow_mut().push((q.retry, q.names.clone()));
                if q.retry {
                    return Ok(false);
                }
                Settings::load().allow_native_mods()?;
                Ok(true)
            },
            registry_crates: None,
            continue_after_failure: &|_| panic!("no compile can run"),
            resolve: None,
            cancellation: None,
        },
    );
    assert!(matches!(done, Err(InstallError::Cancelled)), "{done:?}");
    assert_eq!(
        questions.into_inner(),
        [
            (
                false,
                vec!["First Light".to_owned(), "Shared Light".to_owned()]
            ),
            (true, vec!["New Light".to_owned()])
        ]
    );
}
