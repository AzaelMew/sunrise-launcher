use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEV: &str = "dev-2026-11-13-0c02067";
const OTHER: &str = "dev-2026-11-14-1111111";
const API: &str = "https://api.github.com/repos/sunrise-org/sunrise/releases";

fn cached(cache: &Path, url: &str, body: Value) {
    let hash = sunrise_launcher_release_verify::sha256_hex(url.as_bytes());
    fs::write(
        cache.join(format!("{}.json", &hash[..24])),
        json!({
            "url": url, "etag": "test", "body": body, "next": false,
            "saved": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn cli_install_looks_up_the_exact_dev_tag_even_when_the_release_feed_has_no_dev() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("launcher");
    let cache = data.join("cache/github");
    fs::create_dir_all(&cache).unwrap();
    cached(&cache, &format!("{API}?per_page=100"), json!([]));
    let prerelease = json!({
        "tag_name": DEV,
        "name": "Dev 2026-11-13 · 0c02067",
        "published_at": "2026-11-13T12:00:00Z",
        "prerelease": true,
        "assets": [],
    });
    cached(&cache, &format!("{API}/tags/{DEV}"), prerelease.clone());
    cached(&cache, &format!("{API}/tags/{OTHER}"), prerelease);

    let install = |tag| {
        let out = Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"))
            .current_dir(root.path())
            .env("SUNRISE_LAUNCHER_HOME", &data)
            .env("SUNRISE_HOME", root.path().join("game"))
            .env("SUNRISE_NO_KEYCHAIN", "1")
            .env("SUNRISE_NO_HEARTBEAT", "1")
            .env("SUNRISE_TELEMETRY_URL", "off")
            .env("LANG", "en_US.UTF-8")
            .args(["install", tag])
            .output()
            .unwrap();
        assert!(!out.status.success());
        String::from_utf8(out.stderr).unwrap()
    };
    // This error is after resolving the exact release row; the fixture has no runnable package.
    assert!(install(DEV).contains(&format!("{DEV} has no build for this platform.")));
    // The endpoint is not enough if it returns metadata for a different tag.
    assert!(install(OTHER).contains(&format!("No Sunrise Dev build is tagged {OTHER}.")));
    assert!(!data.join("versions").join(DEV).exists());
    assert!(!data.join("versions").join(OTHER).exists());
}
