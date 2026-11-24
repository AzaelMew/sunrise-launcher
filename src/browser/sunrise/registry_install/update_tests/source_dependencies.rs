use super::*;
use crate::{
    browser::sunrise::sources,
    github_mods::{self, AddedFrom},
    instances::Channel,
};
use std::cell::Cell;

const ROOT: &str = "corn_delight";
const FARMER: &str = "ported_farmersdelight";
const LIB: &str = "fixture_library";

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
                continue_after_failure: &|_| panic!("Add must not compile"),
                cancellation: None,
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
    let before = managed_play::inputs(fixture.home()).unwrap();
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
        assert_eq!(managed_play::inputs(fixture.home()).unwrap(), before);
        let approve = approve.expect("unlisted dependency should never be offered");
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
        assert_eq!(managed_play::inputs(fixture.home()).unwrap(), before);
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
fn stable_refuses_a_source_mod_whose_required_mod_isnt_installed() {
    Fixture::run(
        concat!(
            module_path!(),
            "::stable_refuses_a_source_mod_whose_required_mod_isnt_installed"
        ),
        |mut fixture| {
            fixture.instance.channel = Channel::Stable;
            let root = source(
                &fixture,
                ROOT,
                1,
                "1.0.0",
                "[dependencies]\nported_farmersdelight='^1'",
            );
            fs::create_dir_all(fixture.home()).unwrap();
            let before = managed_play::inputs(fixture.home()).unwrap();
            let result = add(&fixture, root, |_, _| {
                panic!("a missing Stable dependency must not reach consent")
            });
            assert_eq!(
                result,
                Err(SourceError::Catalogue(
                    "Required mod ported_farmersdelight isn't installed. Install it from Discover first, then try again."
                        .into()
                ))
            );
            assert!(github_mods::origin(fixture.home(), ROOT).is_none());
            assert_eq!(managed_play::inputs(fixture.home()).unwrap(), before);
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
