use super::*;
use image::ImageEncoder;
use serde_json::json;

const REPO: &str = "Driftwood/trail-atlas";
const MANIFEST: &str = "mods/atlas/sunrise-mod.toml";

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
fn discover_icons_fetch_only_explicit_pages_icons_and_never_guess_from_github() {
    let root = tempfile::tempdir().unwrap();
    let github = fixture(root.path());
    let misses = IconMisses::default();
    let pages = png(2, 2, 100);
    put(&github, "pages/icons/atlas.png", &pages);
    let start = test_requests();
    let listed = source(json!({"branch": "topic/camps"}), Some("icons/atlas.png"));
    let icon = SourceIcon::new(&listed, None, true).unwrap();
    let path = icon.load_with(&github, &misses).unwrap();
    assert_eq!(&fs::read(path).unwrap(), &pages);
    assert_eq!(
        test_requests() - start,
        1,
        "one direct GET per explicit icon, no API calls"
    );

    let uniconed = source(json!({"branch": "topic/camps"}), None);
    assert!(
        SourceIcon::new(&uniconed, None, true).is_none(),
        "a listing without an explicit icon leaves the placeholder; do not guess from GitHub"
    );
    assert_eq!(test_requests() - start, 1);

    let missing = source(json!({"branch": "topic/camps"}), Some("icons/missing.png"));
    put(&github, "pages/icons/missing.status", b"404\n");
    assert!(
        SourceIcon::new(&missing, None, true)
            .unwrap()
            .load_with(&github, &misses)
            .is_none(),
        "a missing listing icon leaves the letter"
    );
    assert_eq!(test_requests() - start, 2);
}

#[test]
fn icon_refuses_invalid_pngs_and_an_offline_snapshot_only_reads_disk() {
    let root = tempfile::tempdir().unwrap();
    let github = fixture(root.path());
    let misses = IconMisses::default();
    let listed = source(json!({"branch": "main"}), Some("icons/atlas.png"));
    let online = SourceIcon::new(&listed, None, true).unwrap();
    let offline = SourceIcon::new(&listed, None, false).unwrap();
    let fixture_path = "pages/icons/atlas.png";
    let start = test_requests();
    assert!(offline.load_with(&github, &misses).is_none());
    assert_eq!(test_requests(), start);
    for bytes in [png(4, 3, 0), png(513, 513, 0), vec![0; MAX_ICON + 1]] {
        put(&github, fixture_path, &bytes);
        assert!(online.load_with(&github, &misses).is_none());
        assert!(!github.cache.exists(), "no invalid icon is cached");
    }
    put(&github, fixture_path, &png(4, 4, 0));
    let path = online.load_with(&github, &misses).unwrap();
    assert_eq!(offline.load_with(&github, &misses), Some(path));
    assert_eq!(test_requests() - start, 4);
}
