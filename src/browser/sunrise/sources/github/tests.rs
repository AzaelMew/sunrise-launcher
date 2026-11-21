use super::*;

const REPO: &str = "Driftwood/trail-atlas";
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MANIFEST: &str = "mods/trail_atlas/sunrise-mod.toml";

struct Sandbox {
    _temp: tempfile::TempDir,
    github: Github,
}

impl Sandbox {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let cache = temp.path().join("cache");
        let github = Github {
            cache,
            fixture: Some(temp.path().join("fixture")),
        };
        let sandbox = Self {
            _temp: temp,
            github,
        };
        sandbox.put(
            &format!("api/repos/{REPO}.json"),
            br#"{"id":42,"full_name":"Driftwood/trail-atlas","private":false}"#,
        );
        sandbox.put(
            &format!("api/repos/{REPO}/git/ref/heads/topic/camps.json"),
            format!(
                r#"{{"ref":"refs/heads/topic/camps","object":{{"sha":"{SHA}","type":"commit"}}}}"#
            ),
        );
        sandbox.put(&format!("raw/{REPO}/{SHA}/{MANIFEST}"), format!("id='trail_atlas'\nname='Trail Atlas'\nversion='0.1.0'\napi='0.2'\ncapabilities=['registries']\n[native]\ngame='26.3'\nsunrise='{SHA}'\nmixins='{SHA}'\n"));
        sandbox
    }

    fn put(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.github.fixture.as_ref().unwrap().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn source(repo_id: u64) -> Source {
        Source::for_origin(
            "trail_atlas",
            REPO,
            repo_id,
            MANIFEST,
            Tracker::Branch("topic/camps".into()),
        )
        .unwrap()
    }
}

#[test]
fn a_branch_must_match_public_identity_and_exact_heads_ref() {
    let s = Sandbox::new();
    let bad = resolve_checked(&s.github, &Sandbox::source(43), None);
    assert_eq!(bad.unwrap_err(), Failure::RepositoryUnavailable);
    s.put(
        &format!("api/repos/{REPO}/git/ref/heads/topic/camps.json"),
        format!(r#"{{"ref":"refs/tags/topic/camps","object":{{"sha":"{SHA}","type":"commit"}}}}"#),
    );
    let bad = resolve_checked(&s.github, &Sandbox::source(42), None);
    assert_eq!(bad.unwrap_err(), Failure::RefUnavailable);
    s.put(
        &format!("api/repos/{REPO}.json"),
        br#"{"id":42,"full_name":"Driftwood/old-name","private":false}"#,
    );
    let bad = resolve_checked(&s.github, &Sandbox::source(42), Some(Instant::now()));
    assert_eq!(bad.unwrap_err(), Failure::RepositoryUnavailable);
}

#[test]
fn a_release_resolves_its_tag_object_to_source_not_to_its_assets() {
    const TAG_OBJECT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let s = Sandbox::new();
    s.put(&format!("api/repos/{REPO}/releases/latest.json"), br#"{"tag_name":"v0.1.0","published_at":"2026-11-21T04:00:00Z","draft":false,"prerelease":false,"assets":[{"name":"trail_atlas.smod"}]}"#);
    s.put(
        &format!("api/repos/{REPO}/git/ref/tags/v0.1.0.json"),
        format!(r#"{{"ref":"refs/tags/v0.1.0","object":{{"sha":"{TAG_OBJECT}","type":"tag"}}}}"#),
    );
    s.put(
        &format!("api/repos/{REPO}/git/tags/{TAG_OBJECT}.json"),
        format!(r#"{{"object":{{"sha":"{SHA}","type":"commit"}}}}"#),
    );
    let listing = Source::for_origin("trail_atlas", REPO, 42, MANIFEST, Tracker::Release).unwrap();
    let commit = resolve_checked(&s.github, &listing, None).unwrap();
    assert_eq!(commit.sha(), SHA);
    assert_eq!(commit.release_tag(), Some("v0.1.0"));
    assert_eq!(commit.published_at(), Some("2026-11-21T04:00:00Z"));
    assert_eq!(commit.source().manifest(), MANIFEST);
}

#[test]
fn a_missing_or_mismatched_manifest_disables_build_without_hiding_the_pinned_readme() {
    let s = Sandbox::new();
    let path = format!("raw/{REPO}/{SHA}/{MANIFEST}");
    s.put(&path, b"id='mismatch'\n");
    s.put(
        &format!("api/repos/{REPO}/readme/{SHA}.json"),
        br#"{"path":"docs/README.md","size":110,"type":"file"}"#,
    );
    s.put(
        &format!("raw/{REPO}/{SHA}/docs/README.md"),
        "# Pinned\n\n![Atlas](media/atlas.png)",
    );
    let start = test_requests();
    let details = inspect(&s.github, &Sandbox::source(42), None, true);
    assert_eq!(details.error, Some(Failure::NoManifest));
    assert_eq!(details.commit.unwrap().sha(), SHA);
    assert_eq!(details.readme.unwrap().images().len(), 1);
    assert_eq!(
        test_requests() - start,
        5,
        "only repo, ref, manifest and README metadata/content; no image"
    );
}

#[test]
fn repository_and_ref_collapse_across_mods_and_cached_readme_works_offline() {
    let s = Sandbox::new();
    s.put(
        &format!("api/repos/{REPO}/readme/{SHA}.json"),
        br#"{"path":"README.md","size":24,"type":"file"}"#,
    );
    s.put(
        &format!("raw/{REPO}/{SHA}/README.md"),
        "# Trail Atlas\n\nOld notes.",
    );
    let start = test_requests();
    let epoch = Some(Instant::now());
    let details = inspect(&s.github, &Sandbox::source(42), epoch, true);
    assert!(details.error.is_none(), "{:?}", details.error);
    let other = Source::for_origin(
        "other",
        REPO,
        42,
        "other/sunrise-mod.toml",
        Tracker::Branch("topic/camps".into()),
    )
    .unwrap();
    let bad = resolve_checked(&s.github, &other, epoch).unwrap_err();
    assert_eq!(bad, Failure::NoManifest);
    assert_eq!(
        test_requests() - start,
        6,
        "the second mod only checks its manifest"
    );
    s.put(&format!("api/repos/{REPO}.status"), "offline\n");
    let stale = inspect(&s.github, &Sandbox::source(42), Some(Instant::now()), true);
    assert!(matches!(stale.error, Some(Failure::Offline(_))));
    assert_eq!(
        stale.readme.unwrap().markdown(),
        "# Trail Atlas\n\nOld notes."
    );
    assert_eq!(
        test_requests() - start,
        7,
        "only one attempt while recovering the saved README"
    );
}

#[test]
fn a_rate_limit_names_reset_time_and_pauses_the_next_repository_lookup() {
    let s = Sandbox::new();
    s.put(
        &format!("api/repos/{REPO}.status"),
        "429\nRetry-After: 500\n",
    );
    let start = test_requests();
    let before = SystemTime::now();
    let failure = resolve_checked(&s.github, &Sandbox::source(42), None).unwrap_err();
    let Failure::RateLimited {
        retry_at: Some(retry),
    } = failure
    else {
        panic!("{failure:?}");
    };
    assert!(retry >= before + Duration::from_secs(500));
    let next = Sandbox::source(48);
    assert_eq!(
        resolve_checked(&s.github, &next, Some(Instant::now())).unwrap_err(),
        failure
    );
    assert_eq!(test_requests() - start, 1);
}
