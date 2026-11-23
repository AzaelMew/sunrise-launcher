use super::*;
use crate::{
    browser::sunrise::sources,
    github_mods::{self, AddedFrom},
    instances::Channel,
    managed_play::{Active, Held, HeldReason, HeldRoot, HeldSource},
};
use std::cell::Cell;

const ROOT: &str = "corn_delight";
const FARMER: &str = "ported_farmersdelight";
const LIB: &str = "fixture_library";
const PEER: &str = "fixture_peer";

fn source(fixture: &Fixture, id: &str, repo_id: u64, version: &str, requirements: &str) -> Value {
    let root = fixture.root.join("github");
    let repo = format!("Fixture/{id}");
    let checkout = root.join("repos").join(&repo);
    fs::create_dir_all(checkout.join("src")).unwrap();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(&checkout)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgSign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q", "--initial-branch=main", "--template="]);
    let manifest = format!("id='{id}'\nname='{id}'\nversion='{version}'\napi='0.2'\nside='both'\ncapabilities=['registries']\n{requirements}\n[native]\nsunrise='{}'\nmixins='{}'\ngame='{}'\n", "a".repeat(40), "b".repeat(40), backend::PINNED_GAME);
    fs::write(checkout.join("sunrise-mod.toml"), &manifest).unwrap();
    fs::write(
        checkout.join("Cargo.toml"),
        format!("[package]\nname='{id}'\nversion='{version}'\n"),
    )
    .unwrap();
    fs::write(checkout.join("src/lib.rs"), b"pub fn inert() {}").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "inert source fixture"]);
    let sha = git(&["rev-parse", "HEAD"]);
    let api = root.join("api/repos").join(&repo);
    for (path, value) in [
        (
            api.with_extension("json"),
            json!({"id": repo_id, "full_name": repo, "private": false}),
        ),
        (
            api.join("git/ref/heads/main.json"),
            json!({"ref": "refs/heads/main", "object": {"type": "commit", "sha": sha}}),
        ),
        (
            api.join(format!("git/commits/{sha}.json")),
            json!({"sha": sha}),
        ),
    ] {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }
    let raw = root.join("raw").join(&repo).join(&sha);
    fs::create_dir_all(&raw).unwrap();
    fs::write(raw.join("sunrise-mod.toml"), &manifest).unwrap();
    json!({"id": id, "name": id, "summary": "Inert test mod", "repo_id": repo_id, "repo": repo, "manifest": "sunrise-mod.toml", "track": {"branch": "main"}})
}

fn add(
    fixture: &Fixture,
    listing: Value,
    authorize: impl Fn(&[PreparedSource], &[SourceDependency]) -> Result<bool, String>,
) -> Result<(), SourceError> {
    let cancel = native_mods::Cancellation::default();
    add_with(fixture, BUILD, listing, &cancel, |_| {}, authorize)
}

fn add_with(
    fixture: &Fixture,
    base: &str,
    listing: Value,
    cancel: &native_mods::Cancellation,
    progress: impl Fn(InstallProgress),
    authorize: impl Fn(&[PreparedSource], &[SourceDependency]) -> Result<bool, String>,
) -> Result<(), SourceError> {
    std::env::set_var("SUNRISE_SOURCE_FIXTURES", fixture.root.join("github"));
    let listing = serde_json::from_value(listing).unwrap();
    let pin = sources::resolve(&listing, true).unwrap();
    let packed = pack_sources(&[pin], AddedFrom::Discover, cancel)
        .pop()
        .unwrap()
        .unwrap();
    add_sources(
        &fixture.instance,
        install_target(&fixture.instance).unwrap(),
        base,
        vec![packed],
        SourceHooks {
            install: InstallHooks {
                progress: &progress,
                native: &|_| panic!("one source and dependency approval"),
                registry_crates: None,
                continue_after_failure: &|_| panic!("Add must not compile"),
                resolve: None,
            },
            cancellation: cancel,
            authorize: &authorize,
            approve_worlds: &|_| panic!("no replacements"),
        },
    )
}

fn listings(fixture: &Fixture, projects: &[Value]) {
    let file = fixture.root.join("sources.json");
    fs::write(&file, json!({"format": 1, "sources": projects}).to_string()).unwrap();
    std::env::set_var("SUNRISE_MOD_SOURCES", file);
}

fn source_install(fixture: Fixture, approve: Option<bool>) {
    let root = source(
        &fixture,
        ROOT,
        1,
        "1.0.0",
        "[dependencies]\nported_farmersdelight='=3.6.30-sunrise.1'",
    );
    let farmer = source(
        &fixture,
        FARMER,
        2,
        "3.6.30-sunrise.1",
        "[dependencies]\nfixture_library='^1'",
    );
    let lib = source(&fixture, LIB, 3, "1.1.0", "");
    if approve.is_none() {
        listings(&fixture, &[root.clone(), farmer]);
    } else {
        listings(&fixture, &[root.clone(), farmer, lib]);
    }
    let count = Cell::new(0);
    install_target(&fixture.instance).unwrap();
    let before = managed_play::inputs(fixture.home(), None).unwrap();
    let result = add(&fixture, root, |roots, dependencies| {
        count.set(count.get() + 1);
        assert_eq!(
            roots
                .iter()
                .map(|mod_| mod_.origin().id.as_str())
                .collect::<Vec<_>>(),
            [ROOT]
        );
        assert_eq!(
            dependencies
                .iter()
                .map(|dep| (
                    dep.origin.as_ref().unwrap().id.as_str(),
                    dep.version.as_str()
                ))
                .collect::<Vec<_>>(),
            [(LIB, "1.1.0"), (FARMER, "3.6.30-sunrise.1")]
        );
        assert_eq!(managed_play::inputs(fixture.home(), None).unwrap(), before);
        let approve = approve.expect("unlisted dependency should never be offered");
        if approve {
            backend::Settings::default().allow_native_mods()?;
        }
        Ok(approve)
    });
    assert_eq!(count.get(), usize::from(approve.is_some()));
    match approve {
        Some(true) => result.unwrap(),
        Some(false) => assert_eq!(result, Err(SourceError::Cancelled)),
        None => assert_eq!(
            result,
            Err(SourceError::Catalogue(
                "Required mod fixture_library isn't listed in this profile's catalogue.".into()
            ))
        ),
    }
    for id in [ROOT, FARMER, LIB] {
        assert_eq!(
            github_mods::origin(fixture.home(), id).is_some(),
            approve == Some(true)
        );
        assert_eq!(
            fixture
                .home()
                .join(native_mods::INSTALLED)
                .join(format!("{id}.smod"))
                .exists(),
            approve == Some(true)
        );
    }
    if approve != Some(true) {
        assert_eq!(managed_play::inputs(fixture.home(), None).unwrap(), before);
    }
    assert!(!fixture.home().join(crate::profile_txn::POINTER).exists());
}

#[test]
fn confirms_and_publishes_the_listed_transitive_source_set() {
    Fixture::run(
        concat!(
            module_path!(),
            "::confirms_and_publishes_the_listed_transitive_source_set"
        ),
        |fixture| source_install(fixture, Some(true)),
    );
}

#[test]
fn declines_the_entire_source_set_without_publication() {
    Fixture::run(
        concat!(
            module_path!(),
            "::declines_the_entire_source_set_without_publication"
        ),
        |fixture| source_install(fixture, Some(false)),
    );
}

#[test]
fn refuses_an_unlisted_required_mod_without_asking() {
    Fixture::run(
        concat!(
            module_path!(),
            "::refuses_an_unlisted_required_mod_without_asking"
        ),
        |fixture| source_install(fixture, None),
    );
}

#[test]
fn stable_uses_the_registry_to_choose_and_publish_the_dependency_versions() {
    Fixture::run(
        concat!(
            module_path!(),
            "::stable_uses_the_registry_to_choose_and_publish_the_dependency_versions"
        ),
        |mut fixture| {
            fixture.instance.channel = Channel::Stable;
            fixture.project(FARMER, "Farmer Delight", index::Kind::NativeMod);
            fixture.project(LIB, "Shared Library", index::Kind::NativeMod);
            let mut release = |id: &str, version: &str, requirements: &str, revision: char| {
                let native = format!(
                    "{requirements}\n[native]\nsunrise='{}'\nmixins='{}'\ngame='{}'\n",
                    revision.to_string().repeat(40),
                    "b".repeat(40),
                    backend::PINNED_GAME
                );
                let cargo = format!("[package]\nname='{id}'\nversion='{version}'\n");
                fixture.release(
                    id,
                    version,
                    &native,
                    &[
                        ("Cargo.toml", cargo.as_bytes()),
                        ("src/lib.rs", b"pub fn inert() {}"),
                    ],
                );
            };
            release(LIB, "1.1.0", "", 'a');
            release(FARMER, "1.0.0", "[dependencies]\nfixture_library='^1'", 'a');
            release(FARMER, "1.1.0", "[dependencies]\nfixture_library='^2'", 'a');
            // The newest fits the declared ranges, but not the exact developer revision.
            release(FARMER, "1.2.0", "[dependencies]\nfixture_library='^1'", 'b');
            let root = source(
                &fixture,
                ROOT,
                1,
                "1.0.0",
                "[dependencies]\nported_farmersdelight='^1'",
            );
            let count = Cell::new(0);
            add(&fixture, root, |_, dependencies| {
                count.set(count.get() + 1);
                assert!(dependencies.iter().all(|dep| dep.origin.is_none()));
                assert_eq!(
                    dependencies
                        .iter()
                        .map(|dep| (dep.name.as_str(), dep.version.as_str()))
                        .collect::<Vec<_>>(),
                    [("Shared Library", "1.1.0"), ("Farmer Delight", "1.0.0")]
                );
                assert!(!fixture.home().join(installed::MANIFEST).exists());
                backend::Settings::default().allow_native_mods()?;
                Ok(true)
            })
            .unwrap();
            assert_eq!(count.get(), 1);
            assert!(github_mods::origin(fixture.home(), ROOT).is_some());
            let doc = Document::read(fixture.home()).unwrap();
            assert_eq!(doc.row(FARMER).unwrap().unwrap()["version"], "1.0.0");
            assert_eq!(doc.row(LIB).unwrap().unwrap()["version"], "1.1.0");
        },
    );
}

fn native_release(
    fixture: &mut Fixture,
    id: &str,
    version: &str,
    tables: &str,
    revision: char,
    extra: &[(&str, &[u8])],
) {
    let native = format!(
        "{tables}\n[native]\nsunrise='{}'\nmixins='{}'\ngame='{}'\n",
        revision.to_string().repeat(40),
        "b".repeat(40),
        backend::PINNED_GAME
    );
    let cargo = format!("[package]\nname='{id}'\nversion='{version}'\n");
    let mut files: Vec<(&str, &[u8])> = vec![
        ("Cargo.toml", cargo.as_bytes()),
        ("src/lib.rs", b"pub fn inert() {}"),
    ];
    files.extend_from_slice(extra);
    fixture.release(id, version, &native, &files);
}

#[test]
fn stable_rejects_index_missing_revoked_section() {
    Fixture::run(
        concat!(
            module_path!(),
            "::stable_rejects_index_missing_revoked_section"
        ),
        |mut fixture| {
            fixture.instance.channel = Channel::Stable;
            fixture.project(FARMER, "Farmer Delight", index::Kind::NativeMod);
            native_release(&mut fixture, FARMER, "1.0.0", "", 'a', &[]);
            let index_path = fixture.root.join("index.json");
            let mut raw: Value = serde_json::from_slice(&fs::read(&index_path).unwrap()).unwrap();
            raw.as_object_mut().unwrap().remove("revoked");
            fs::write(&index_path, serde_json::to_vec(&raw).unwrap()).unwrap();

            let root = source(
                &fixture,
                ROOT,
                1,
                "1.0.0",
                "[dependencies]\nported_farmersdelight='^1'",
            );
            let result = add(&fixture, root, |_, _| {
                panic!("incomplete index must never reach consent")
            });
            assert!(
                matches!(&result, Err(SourceError::Unproved(why)) if why.contains("revoked")),
                "expected omitted revoked refusal, got {result:?}"
            );
            assert!(github_mods::origin(fixture.home(), ROOT).is_none());
            assert!(!fixture.home().join(installed::MANIFEST).exists());
        },
    );
}

#[test]
fn stable_treats_disabled_and_held_transitive_mods_as_unavailable_during_solving() {
    Fixture::run(
        concat!(
            module_path!(),
            "::stable_treats_disabled_and_held_transitive_mods_as_unavailable_during_solving"
        ),
        |mut fixture| {
            fixture.instance.channel = Channel::Stable;
            fixture.project(FARMER, "Farmer Delight", index::Kind::NativeMod);
            fixture.project(LIB, "Shared Library", index::Kind::Mod);
            fixture.release(LIB, "1.0.0", "", &[]);
            native_release(&mut fixture, FARMER, "1.0.0", "", 'a', &[]);
            native_release(
                &mut fixture,
                FARMER,
                "1.1.0",
                "[dependencies]\nfixture_library='^1'",
                'a',
                &[],
            );
            // Install LIB on disk and switch it off; solving must pick FARMER 1.0.0 without
            // touching or enabling LIB.
            let off_dir = fixture.home().join(mod_list::MODS_DIR);
            fs::create_dir_all(&off_dir).unwrap();
            let off_manifest = format!(
                "id='{LIB}'\nname='Shared Library'\nversion='1.0.0'\napi='{}'\nside='both'\ncapabilities=['registries']\n",
                sunrise_launcher_smod::manifest::API
            );
            let off_bytes = Package::write_files(&off_manifest, &[]).unwrap();
            fs::write(off_dir.join(format!("{LIB}.smod")), &off_bytes).unwrap();
            mod_list::set_enabled(fixture.home(), LIB, false).unwrap();

            let root = source(
                &fixture,
                ROOT,
                1,
                "1.0.0",
                "[dependencies]\nported_farmersdelight='^1'",
            );
            add(&fixture, root, |_, dependencies| {
                assert_eq!(
                    dependencies
                        .iter()
                        .map(|dep| (dep.name.as_str(), dep.version.as_str()))
                        .collect::<Vec<_>>(),
                    [("Farmer Delight", "1.0.0")]
                );
                backend::Settings::default().allow_native_mods()?;
                Ok(true)
            })
            .unwrap();
            assert!(!mod_list::is_enabled(fixture.home(), LIB));
            assert_eq!(
                fs::read(off_dir.join(format!("{LIB}.smod"))).unwrap(),
                off_bytes
            );
            let doc = Document::read(fixture.home()).unwrap();
            assert_eq!(doc.row(FARMER).unwrap().unwrap()["version"], "1.0.0");
            assert!(doc.row(LIB).unwrap().is_none());
        },
    );
}

#[test]
fn stable_treats_held_transitive_mod_as_unavailable_and_chooses_older_library() {
    Fixture::run(
        concat!(
            module_path!(),
            "::stable_treats_held_transitive_mod_as_unavailable_and_chooses_older_library"
        ),
        |mut fixture| {
            fixture.instance.channel = Channel::Stable;
            fixture.project(FARMER, "Farmer Delight", index::Kind::NativeMod);
            fixture.project(LIB, "Shared Library", index::Kind::NativeMod);
            native_release(&mut fixture, LIB, "1.0.0", "", 'a', &[]);
            native_release(&mut fixture, FARMER, "1.0.0", "", 'a', &[]);
            native_release(
                &mut fixture,
                FARMER,
                "1.1.0",
                "[dependencies]\nfixture_library='^1'",
                'a',
                &[],
            );
            // Commit an active generation that holds LIB out, without placing LIB on disk.
            install_target(&fixture.instance).unwrap();
            let mut profile = Profile::open(fixture.home(), None).unwrap();
            let attempt = profile
                .begin_first(|| FirstBuild::installed(BUILD), |_| {})
                .unwrap();
            let cancellation = native_mods::Cancellation::default();
            let image = attempt
                .exact_image(
                    fixture.instance.native_profile,
                    &cancellation,
                    |_| {},
                    None,
                    None,
                )
                .unwrap();
            let active = attempt.commit(&image, &cancellation).unwrap();
            assert_eq!(active.executable(), BUILD);
            drop(profile);
            let pointer = fixture.home().join(crate::profile_txn::POINTER);
            let mut image: Value = serde_json::from_slice(&fs::read(&pointer).unwrap()).unwrap();
            let hold = Held::new(
                LIB.into(),
                "Shared Library".into(),
                [7_u8; 32],
                (HeldRoot::Native, format!("{LIB}.smod")),
                HeldSource::Registry {
                    project: LIB.into(),
                    version: "1.0.0".into(),
                    repository: None,
                },
                true,
                HeldReason::NoFit {
                    detail: None,
                    build: BUILD.into(),
                },
            )
            .unwrap();
            let receipt_id = [8_u8; 16];
            image["receipts"] = json!([{"id": receipt_id, "waiting_for_update": [hold], "removed": [], "acknowledged": true}]);
            fs::write(&pointer, serde_json::to_vec(&image).unwrap()).unwrap();

            let root = source(
                &fixture,
                ROOT,
                1,
                "1.0.0",
                "[dependencies]\nported_farmersdelight='^1'",
            );
            add(&fixture, root, |_, dependencies| {
                assert_eq!(
                    dependencies
                        .iter()
                        .map(|dep| (dep.name.as_str(), dep.version.as_str()))
                        .collect::<Vec<_>>(),
                    [("Farmer Delight", "1.0.0")]
                );
                backend::Settings::default().allow_native_mods()?;
                Ok(true)
            })
            .unwrap();
            let after = Active::read_pointer(fixture.home()).unwrap().unwrap();
            assert_eq!(after.holds().map(Held::id).collect::<Vec<_>>(), vec![LIB]);
            let doc = Document::read(fixture.home()).unwrap();
            assert_eq!(doc.row(FARMER).unwrap().unwrap()["version"], "1.0.0");
            assert!(doc.row(LIB).unwrap().is_none());
        },
    );
}

#[test]
fn stable_applies_client_graph_rules_for_side_and_load_before_cycles() {
    Fixture::run(
        concat!(
            module_path!(),
            "::stable_applies_client_graph_rules_for_side_and_load_before_cycles"
        ),
        |mut fixture| {
            fixture.instance.channel = Channel::Stable;
            fixture.project(FARMER, "Farmer Delight", index::Kind::NativeMod);
            native_release(&mut fixture, FARMER, "1.0.0", "", 'a', &[]);
            // Server-only incompatibility is not a client conflict, so 1.1.0 is valid.
            native_release(
                &mut fixture,
                FARMER,
                "1.1.0",
                "[incompatible]\nfixture_peer={version='*',side='server'}",
                'a',
                &[],
            );
            // 1.2.0 creates a client load_before cycle with the enabled peer.
            native_release(
                &mut fixture,
                FARMER,
                "1.2.0",
                "load_before=['fixture_peer']",
                'a',
                &[],
            );

            // Install and enable a content peer that orders itself before FARMER and has a
            // server-only incompatibility with it.
            let mods_dir = fixture.home().join(mod_list::MODS_DIR);
            fs::create_dir_all(&mods_dir).unwrap();
            let peer_manifest = format!(
                "id='{PEER}'\nname='Fixture Peer'\nversion='1.0.0'\napi='{}'\nside='both'\ncapabilities=['registries']\nload_before=['{FARMER}']\n[incompatible]\n{FARMER}={{version='*',side='server'}}\n",
                sunrise_launcher_smod::manifest::API
            );
            fs::write(
                mods_dir.join(format!("{PEER}.smod")),
                Package::write_files(&peer_manifest, &[]).unwrap(),
            )
            .unwrap();
            mod_list::set_enabled(fixture.home(), PEER, true).unwrap();

            let root = source(
                &fixture,
                ROOT,
                1,
                "1.0.0",
                "[dependencies]\nported_farmersdelight='^1'",
            );
            add(&fixture, root, |_, dependencies| {
                assert_eq!(
                    dependencies
                        .iter()
                        .map(|dep| (dep.name.as_str(), dep.version.as_str()))
                        .collect::<Vec<_>>(),
                    [("Farmer Delight", "1.1.0")]
                );
                backend::Settings::default().allow_native_mods()?;
                Ok(true)
            })
            .unwrap();
            let doc = Document::read(fixture.home()).unwrap();
            assert_eq!(doc.row(FARMER).unwrap().unwrap()["version"], "1.1.0");
        },
    );
}

#[test]
fn stable_stops_on_unreadable_input_or_unavailable_sdk_instead_of_treating_as_no_fit() {
    Fixture::run(
        concat!(
            module_path!(),
            "::stable_stops_on_unreadable_input_or_unavailable_sdk_instead_of_treating_as_no_fit"
        ),
        |mut fixture| {
            fixture.instance.channel = Channel::Stable;
            fixture.project(FARMER, "Farmer Delight", index::Kind::NativeMod);
            native_release(&mut fixture, FARMER, "1.0.0", "", 'a', &[]);
            native_release(&mut fixture, FARMER, "1.1.0", "", 'a', &[]);
            let root = source(
                &fixture,
                ROOT,
                1,
                "1.0.0",
                "[dependencies]\nported_farmersdelight='^1'",
            );

            // 1. Corrupt dependency-overrides.toml is an unreadable input, not a graph conflict.
            let mods_dir = fixture.home().join(mod_list::MODS_DIR);
            fs::create_dir_all(&mods_dir).unwrap();
            let overrides = mods_dir.join(sunrise_launcher_smod::dependencies::OVERRIDES);
            fs::write(&overrides, "not valid toml = [").unwrap();
            let unreadable = add(&fixture, root.clone(), |_, _| {
                panic!("unreadable overrides must not reach consent")
            });
            assert!(
                matches!(unreadable, Err(SourceError::Unproved(_))),
                "expected Unproved on unreadable overrides, got {unreadable:?}"
            );
            fs::remove_file(&overrides).unwrap();

            // 2. A release base with no reachable SDK reports Unchecked -> Unproved immediately
            // rather than marking 1.1.0 unfit and retrying 1.0.0.
            let release_base = format!("{}-0.1.0", backend::PINNED_GAME);
            let base_dir = backend::version_dir(&release_base);
            let resources = backend::resources(&release_base);
            fs::create_dir_all(&resources).unwrap();
            fs::write(resources.join("sunrise-resources.json"), b"{}").unwrap();
            fs::write(base_dir.join(".installed"), []).unwrap();
            fs::write(base_dir.join(backend::CLIENT), b"fixture client").unwrap();
            fs::write(
                base_dir.join("sunrise-commit.txt"),
                format!("{}\n", "a".repeat(40)),
            )
            .unwrap();
            std::env::set_var("SUNRISE_SDK_DIR", fixture.root.join("missing-sdk"));
            let downloads = Cell::new(0usize);
            let cancel = native_mods::Cancellation::default();
            let result = add_with(
                &fixture,
                &release_base,
                root,
                &cancel,
                |step| {
                    if matches!(step, InstallProgress::Downloading(fraction) if fraction >= 1.0) {
                        downloads.set(downloads.get() + 1);
                    }
                },
                |_, _| panic!("unavailable SDK must not reach consent"),
            );
            assert!(
                matches!(result, Err(SourceError::Unproved(_))),
                "expected Unproved when SDK is unavailable, got {result:?}"
            );
            assert_eq!(
                downloads.get(),
                1,
                "unavailable SDK must stop on the first trial without backtracking to older versions"
            );
        },
    );
}

#[test]
fn stable_cancels_mid_stream_while_downloading_registry_dependency() {
    Fixture::run(
        concat!(
            module_path!(),
            "::stable_cancels_mid_stream_while_downloading_registry_dependency"
        ),
        |mut fixture| {
            fixture.instance.channel = Channel::Stable;
            fixture.project(FARMER, "Farmer Delight", index::Kind::NativeMod);
            // High-entropy payload so even the compressed .smod exceeds multiple 64 KiB read chunks.
            let mut state = 0x9e37_79b9_7f4a_7c15_u64;
            let mut payload = Vec::with_capacity(200_000);
            for _ in 0..25_000 {
                state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
                let mut z = state;
                z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                payload.extend_from_slice(&(z ^ (z >> 31)).to_le_bytes());
            }
            native_release(
                &mut fixture,
                FARMER,
                "1.0.0",
                "",
                'a',
                &[("assets/ported_farmersdelight/blob.bin", &payload)],
            );
            let root = source(
                &fixture,
                ROOT,
                1,
                "1.0.0",
                "[dependencies]\nported_farmersdelight='^1'",
            );
            let cancel = native_mods::Cancellation::default();
            let chunks = Cell::new(0usize);
            let result = add_with(
                &fixture,
                BUILD,
                root,
                &cancel,
                |step| {
                    if let InstallProgress::Downloading(fraction) = step {
                        chunks.set(chunks.get() + 1);
                        if fraction < 1.0 {
                            cancel.stop();
                        }
                    }
                },
                |_, _| panic!("cancelled download must not reach consent"),
            );
            assert_eq!(result, Err(SourceError::Cancelled));
            assert_eq!(
                chunks.get(),
                1,
                "download must stop on the very next read after cancellation"
            );
            assert!(github_mods::origin(fixture.home(), ROOT).is_none());
            assert!(!fixture.home().join(installed::MANIFEST).exists());
        },
    );
}

#[test]
fn nightly_names_failing_listed_dependency_on_github_resolve_and_pack_errors() {
    Fixture::run(
        concat!(
            module_path!(),
            "::nightly_names_failing_listed_dependency_on_github_resolve_and_pack_errors"
        ),
        |fixture| {
            let root = source(
                &fixture,
                ROOT,
                1,
                "1.0.0",
                "[dependencies]\nported_farmersdelight='^3'",
            );
            // 1. Dependency repo resolves, but has no sunrise-mod.toml at the pinned commit.
            let mut farmer = source(&fixture, FARMER, 2, "3.6.30-sunrise.1", "");
            farmer["name"] = json!("Farmer's Delight");
            farmer["manifest"] = json!("missing/sunrise-mod.toml");
            // 2. A distinct listed dependency whose repository is unavailable (private/removed).
            let root_for_lib = source(
                &fixture,
                "corn_unavailable",
                3,
                "1.0.0",
                "[dependencies]\nfixture_library='^1'",
            );
            let missing_repo = json!({
                "id": LIB,
                "name": "Missing Library",
                "summary": "Inert test mod",
                "repo_id": 4,
                "repo": "Fixture/missing_library",
                "manifest": "sunrise-mod.toml",
                "track": {"branch": "main"}
            });
            listings(
                &fixture,
                &[root.clone(), farmer, root_for_lib.clone(), missing_repo],
            );
            let missing = add(&fixture, root, |_, _| {
                panic!("broken dependency must not reach consent")
            });
            assert_eq!(
                missing,
                Err(SourceError::DependencyGithub {
                    name: "Farmer's Delight".into(),
                    failure: sources::Failure::NoManifest,
                })
            );
            let displayed = missing.unwrap_err().to_string();
            assert!(
                displayed.contains("Farmer's Delight") && !displayed.contains(ROOT),
                "unexpected message: {displayed}"
            );

            let unavailable = add(&fixture, root_for_lib, |_, _| {
                panic!("unavailable dependency repo must not reach consent")
            });
            assert_eq!(
                unavailable,
                Err(SourceError::DependencyGithub {
                    name: "Missing Library".into(),
                    failure: sources::Failure::RepositoryUnavailable,
                })
            );
            let displayed = unavailable.unwrap_err().to_string();
            assert!(
                displayed.contains("Missing Library") && !displayed.contains("corn_unavailable"),
                "unexpected message: {displayed}"
            );
        },
    );
}
