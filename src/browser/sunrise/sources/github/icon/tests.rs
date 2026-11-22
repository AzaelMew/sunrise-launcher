use super::*;
use image::ImageEncoder;
use serde_json::json;

const REPO: &str = "Driftwood/trail-atlas";
const MANIFEST: &str = "mods/atlas/sunrise-mod.toml";
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn source(track: serde_json::Value, icon: Option<&str>) -> Source {
    serde_json::from_value(json!({
        "id": "atlas", "name": "Trail Atlas", "summary": "Maps and markers",
        "repo": REPO, "repo_id": 42, "manifest": MANIFEST, "track": track, "icon": icon
    }))
    .unwrap()
}

fn fixture(root: &Path) -> Github {
    Github {
        cache: root.join("cache"),
        fixture: Some(root.join("fixture")),
    }
}

fn put(github: &Github, path: &str, bytes: &[u8]) {
    let file = github.fixture.as_ref().unwrap().join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, bytes).unwrap();
}

fn png(width: u32, height: u32, intensity: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            &vec![intensity; (width * height) as usize],
            width,
            height,
            image::ExtendedColorType::L8,
        )
        .unwrap();
    bytes
}

#[test]
fn icon_is_beside_the_manifest_and_refuses_traversal() {
    assert_eq!(icon_path("sunrise-mod.toml").as_deref(), Some("icon.png"));
    assert_eq!(icon_path(MANIFEST).as_deref(), Some("mods/atlas/icon.png"));
    for unsafe_manifest in [
        "/sunrise-mod.toml",
        "mods/../sunrise-mod.toml",
        "mods/%2F/sunrise-mod.toml",
    ] {
        assert_eq!(icon_path(unsafe_manifest), None);
    }
}

#[test]
fn discover_icons_use_one_pages_or_raw_get_and_no_github_api() {
    let root = tempfile::tempdir().unwrap();
    let github = fixture(root.path());
    let misses = IconMisses::default();
    let pages = png(2, 2, 100);
    let raw = png(2, 2, 200);
    put(&github, "pages/icons/atlas.png", &pages);
    put(
        &github,
        &format!("raw/{REPO}/refs/heads/topic/camps/mods/atlas/icon.png"),
        &raw,
    );
    let start = test_requests();
    for (listed_icon, expected) in [(Some("icons/atlas.png"), &pages), (None, &raw)] {
        let source = source(json!({"branch": "topic/camps"}), listed_icon);
        let icon = SourceIcon::new(&source, None, true).unwrap();
        let path = icon.load_with(&github, &misses).unwrap();
        assert_eq!(&fs::read(path).unwrap(), expected);
    }
    assert_eq!(
        test_requests() - start,
        2,
        "one direct GET per icon, no API calls"
    );

    let listed = source(json!({"branch": "topic/camps"}), Some("icons/missing.png"));
    put(&github, "pages/icons/missing.status", b"404\n");
    assert!(
        SourceIcon::new(&listed, None, true)
            .unwrap()
            .load_with(&github, &misses)
            .is_none(),
        "a missing listing icon leaves the letter; do not fall through to raw"
    );
    assert_eq!(test_requests() - start, 3);
}

#[test]
fn icon_refuses_invalid_pngs_and_an_offline_snapshot_only_reads_disk() {
    let root = tempfile::tempdir().unwrap();
    let github = fixture(root.path());
    let misses = IconMisses::default();
    let source = source(json!({"branch": "main"}), None);
    let online = SourceIcon::new(&source, None, true).unwrap();
    let offline = SourceIcon::new(&source, None, false).unwrap();
    let fixture_path = format!("raw/{REPO}/refs/heads/main/mods/atlas/icon.png");
    let start = test_requests();
    assert!(offline.load_with(&github, &misses).is_none());
    assert_eq!(test_requests(), start);
    for bytes in [png(4, 3, 0), png(513, 513, 0), vec![0; MAX_ICON + 1]] {
        put(&github, &fixture_path, &bytes);
        assert!(online.load_with(&github, &misses).is_none());
        assert!(!github.cache.exists(), "no invalid icon is cached");
    }
    put(&github, &fixture_path, &png(4, 4, 0));
    let path = online.load_with(&github, &misses).unwrap();
    assert_eq!(offline.load_with(&github, &misses), Some(path));
    assert_eq!(test_requests() - start, 4);
}

#[test]
fn trackers_use_a_saved_release_or_the_exact_inspected_commit() {
    let root = tempfile::tempdir().unwrap();
    let github = fixture(root.path());
    let misses = IconMisses::default();
    let release = source(json!({"release": true}), None);
    let release_icon = SourceIcon::new(&release, None, true).unwrap();
    let start = test_requests();
    assert!(release_icon.load_with(&github, &misses).is_none());
    assert_eq!(
        test_requests(),
        start,
        "never resolve a release for an icon"
    );
    let pin = Pin {
        sha: SHA.into(),
        release: Some(Release {
            tag: "v1".into(),
            published_at: "2026-11-21T04:00:00Z".into(),
        }),
    };
    github
        .save(
            &github.ref_cache_path(REPO, 42, &Tracker::Release),
            &serde_json::to_vec(&pin).unwrap(),
        )
        .unwrap();
    let inspected = SourceCommit {
        source: release.clone(),
        pin,
        github_url: String::new(),
    };
    let icon = SourceIcon::new(&release, Some(&inspected), true).unwrap();
    assert!(release_icon.load_with(&github, &misses).is_none());
    assert!(icon.load_with(&github, &misses).is_none());
    assert_eq!(
        test_requests() - start,
        1,
        "both lookups remember the same 404"
    );

    let misses = IconMisses::default();
    let bytes = png(2, 2, 123);
    put(
        &github,
        &format!("raw/{REPO}/{SHA}/mods/atlas/icon.png"),
        &bytes,
    );
    let path = release_icon.load_with(&github, &misses).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(icon.load_with(&github, &misses), Some(path));
    assert_eq!(
        test_requests() - start,
        2,
        "reuse the pinned icon, not the branch"
    );
}
