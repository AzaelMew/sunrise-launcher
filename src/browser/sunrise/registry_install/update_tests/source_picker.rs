//! A checked paste can offer choices before their exact selected set is proved.
use super::*;
use crate::github_mods::{self, AddedFrom, Resolved, SourceTrack};

#[test]
fn pasted_sources_prove_only_the_selected_packages_together() {
    Fixture::run(
        concat!(
            module_path!(),
            "::pasted_sources_prove_only_the_selected_packages_together"
        ),
        |fixture| {
            let fixtures = fixture.root.join("github");
            std::env::set_var("SUNRISE_SOURCE_FIXTURES", &fixtures);
            let repo = fixtures.join("repos/Fixture/Pair");
            fs::create_dir_all(&repo).unwrap();
            let git = |args: &[&str]| {
                let output = Command::new("git")
                    .current_dir(&repo)
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
            for (id, pin, dependencies) in [
                ("main_mod", 'a', "[dependencies]\nhelper_mod='^1'\n"),
                ("helper_mod", 'a', ""),
                ("unfit_mod", 'b', ""),
            ] {
                let dir = repo.join(id);
                fs::create_dir_all(dir.join("src")).unwrap();
                fs::write(dir.join("src/lib.rs"), b"").unwrap();
                fs::write(
                    dir.join("Cargo.toml"),
                    format!("[package]\nname='{id}'\nversion='1.0.0'\n"),
                )
                .unwrap();
                fs::write(dir.join("sunrise-mod.toml"), format!("id='{id}'\nname='{id}'\nversion='1.0.0'\napi='0.2'\nside='both'\ncapabilities=['registries']\n{dependencies}\n[native]\nsunrise='{}'\nmixins='{}'\ngame='{}'\n", pin.to_string().repeat(40), "c".repeat(40), backend::PINNED_GAME)).unwrap();
            }
            git(&["add", "."]);
            git(&["commit", "-qm", "three checked choices"]);
            let sha = git(&["rev-parse", "HEAD"]);
            let api = fixtures.join("api/repos/Fixture");
            let heads = api.join("Pair/git/ref/heads");
            fs::create_dir_all(&heads).unwrap();
            fs::write(
                api.join("Pair.json"),
                json!({"id":42,"full_name":"Fixture/Pair","default_branch":"main","private":false})
                    .to_string(),
            )
            .unwrap();
            fs::write(
                heads.join("main.json"),
                json!({"ref":"refs/heads/main","object":{"type":"commit","sha":&sha}}).to_string(),
            )
            .unwrap();
            let commits = api.join("Pair/git/commits");
            fs::create_dir_all(&commits).unwrap();
            fs::write(
                commits.join(format!("{sha}.json")),
                json!({"sha": &sha}).to_string(),
            )
            .unwrap();
            let link = github_mods::parse(&format!("https://github.com/Fixture/Pair/commit/{sha}"))
                .unwrap();
            let found = Resolved {
                owner: "Fixture".into(),
                repo: "Pair".into(),
                tag: None,
                commit: sha.clone(),
                assets: Vec::new(),
            };
            let cancel = native_mods::Cancellation::default();
            let packages = read_pasted_sources(&link, &found, &cancel).unwrap();
            assert_eq!(
                packages
                    .iter()
                    .map(|row| row.origin().id.as_str())
                    .collect::<Vec<_>>(),
                ["helper_mod", "main_mod", "unfit_mod"]
            );
            assert!(packages.iter().all(|row| {
                let origin = row.origin();
                origin.commit == sha
                    && origin.source.as_ref().unwrap().track == SourceTrack::Pinned
                    && origin.added_from == Some(AddedFrom::Github)
            }));
            let identity = install_target(&fixture.instance).unwrap();
            let check = |ids: &[&str]| {
                prepare_pasted_sources(
                    &fixture.instance,
                    identity,
                    BUILD,
                    packages
                        .iter()
                        .filter(|row| ids.contains(&row.origin().id.as_str()))
                        .cloned()
                        .collect(),
                    &cancel,
                )
            };
            assert!(
                matches!(check(&["main_mod"]), Err(SourceError::Dependency { required: Some(id), .. }) if id == "helper_mod")
            );
            assert!(matches!(
                check(&["helper_mod", "main_mod", "unfit_mod"]),
                Err(SourceError::DoesNotFit(_))
            ));
            let offered = check(&["helper_mod", "main_mod"]).unwrap();
            assert_eq!(
                offered
                    .iter()
                    .map(|row| row.origin().id.as_str())
                    .collect::<Vec<_>>(),
                ["helper_mod", "main_mod"]
            );
            // The paste goes through real deferred publication and leaves a readable receipt.
            backend::Settings::default().allow_native_mods().unwrap();
            apply_sources(
                &fixture.instance,
                identity,
                &offered,
                || FirstBuild::installed(BUILD),
                SourceHooks {
                    install: InstallHooks {
                        progress: &|_| {},
                        native: &|_| panic!("source permission has its own gate"),
                        registry_crates: None,
                        continue_after_failure: &|_| false,
                        resolve: None,
                    },
                    cancellation: &cancel,
                    authorize: &|_| Ok(true),
                    approve_worlds: &|_| false,
                },
            )
            .unwrap();
            for source in &offered {
                let recorded = github_mods::origin(fixture.home(), &source.origin().id).unwrap();
                assert_eq!(&recorded, source.origin());
            }
            assert!(!fixture.home().join(crate::profile_txn::POINTER).exists());
            assert!(!fixture.root.join("compiled").exists());
        },
    );
}
