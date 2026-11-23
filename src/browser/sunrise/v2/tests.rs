use super::*;
use std::io::{BufRead, BufReader};
use std::net::TcpListener;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/smod/tests/index_v2")
}

fn load_fixture(name: &str) -> (Vec<u8>, String) {
    let bytes = fs::read(fixtures().join(name)).unwrap();
    let sig = fs::read_to_string(fixtures().join(format!("{name}.minisig"))).unwrap();
    (bytes, sig)
}

fn now_2026_11_24() -> i64 {
    index_v2::parse_rfc3339_utc("2026-11-24T00:00:00Z").unwrap()
}

#[test]
fn test_key_constant_matches_fixture_and_good_signature_caches_golden_index() {
    let fixture_key = fs::read_to_string(fixtures().join("test.pub")).unwrap();
    assert_eq!(REGISTRY_KEYS, [fixture_key.trim()].as_slice());

    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join(CACHE_DIR);
    let mut store = Store::new(cache.clone(), REGISTRY_KEYS);

    let (bytes, sig) = load_fixture("two-ports-index.json");
    let catalog = store
        .load_from_fetch(Ok((bytes, sig)), now_2026_11_24())
        .unwrap();
    assert!(!catalog.from_cache);
    assert_eq!(catalog.stale_warning, None);
    assert_eq!(catalog.index.serial, 41);
    assert_eq!(catalog.index.projects.len(), 2);
    assert_eq!(catalog.index.revoked.len(), 2);
    assert_eq!(store.highest_serial().unwrap(), Some(41));

    // Offline load falls back to the verified v2 cache namespace.
    let mut reopened = Store::new(cache, REGISTRY_KEYS);
    let cached = reopened
        .load_from_fetch(Err("offline".into()), now_2026_11_24())
        .unwrap();
    assert!(cached.from_cache);
    assert_eq!(cached.index.serial, 41);
    assert_eq!(cached.index.projects.len(), 2);
}

#[test]
fn without_a_configured_key_the_reader_reports_registry_key_not_configured_and_trusts_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join(CACHE_DIR);

    // Seed a genuinely signed cache copy first.
    let (bytes, sig) = load_fixture("two-ports-index.json");
    let mut seeded = Store::new(cache.clone(), REGISTRY_KEYS);
    seeded
        .load_from_fetch(Ok((bytes.clone(), sig.clone())), now_2026_11_24())
        .unwrap();

    // A store with no configured keys trusts neither the live response nor the disk cache.
    let mut unconfigured = Store::new(cache, &[]);
    let live_err = unconfigured
        .load_from_fetch(Ok((bytes, sig)), now_2026_11_24())
        .unwrap_err();
    assert_eq!(live_err, LoadError::KeyNotConfigured);
    assert!(
        live_err.to_string().contains("registry key not configured"),
        "unexpected error: {live_err}"
    );

    let cache_err = unconfigured.load_cached(now_2026_11_24()).unwrap_err();
    assert_eq!(cache_err, LoadError::KeyNotConfigured);
    assert!(
        cache_err
            .to_string()
            .contains("registry key not configured"),
        "unexpected error: {cache_err}"
    );
}

#[test]
fn bad_signature_and_tampered_bytes_are_refused_without_falling_back_to_cache() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join(CACHE_DIR);
    let mut store = Store::new(cache.clone(), REGISTRY_KEYS);

    let (bytes, sig) = load_fixture("two-ports-index.json");
    store
        .load_from_fetch(Ok((bytes.clone(), sig.clone())), now_2026_11_24())
        .unwrap();

    // Wrong key signature is refused directly (does not silently serve the cached copy).
    let other_sig = fs::read_to_string(fixtures().join("two-ports-index.other.minisig")).unwrap();
    let err = store
        .load_from_fetch(Ok((bytes.clone(), other_sig)), now_2026_11_24())
        .unwrap_err();
    assert!(matches!(err, LoadError::BadSignature(_)), "{err:?}");

    // Tampered index bytes are refused directly.
    let mut tampered = bytes;
    tampered[0] ^= 0x01;
    let err = store
        .load_from_fetch(Ok((tampered, sig)), now_2026_11_24())
        .unwrap_err();
    assert!(matches!(err, LoadError::BadSignature(_)), "{err:?}");

    // Tampered disk cache is never trusted offline either.
    fs::write(cache.join(CACHE_INDEX), b"{}").unwrap();
    let err = store.load_cached(now_2026_11_24()).unwrap_err();
    assert!(matches!(err, LoadError::BadSignature(_)), "{err:?}");
}

#[test]
fn lower_serial_is_refused_for_both_fetched_and_cached_copies() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join(CACHE_DIR);
    let mut store = Store::new(cache.clone(), REGISTRY_KEYS);

    let (bytes_41, sig_41) = load_fixture("two-ports-index.json");
    let (bytes_40, sig_40) = load_fixture("serial-40.json");
    let (bytes_42, sig_42) = load_fixture("serial-42.json");

    // Accept serial 41.
    store
        .load_from_fetch(Ok((bytes_41.clone(), sig_41.clone())), now_2026_11_24())
        .unwrap();

    // Refuse lower serial 40 from network.
    let err = store
        .load_from_fetch(Ok((bytes_40.clone(), sig_40.clone())), now_2026_11_24())
        .unwrap_err();
    assert_eq!(
        err,
        LoadError::Rollback {
            got: 40,
            highest: 41
        }
    );

    // Equal serial 41 is still accepted.
    let same = store
        .load_from_fetch(Ok((bytes_41, sig_41)), now_2026_11_24())
        .unwrap();
    assert_eq!(same.index.serial, 41);

    // Higher serial 42 advances the persisted high-water mark.
    let newer = store
        .load_from_fetch(Ok((bytes_42, sig_42)), now_2026_11_24())
        .unwrap();
    assert_eq!(newer.index.serial, 42);
    assert_eq!(
        newer.index.project("Ab3dE9xQ").unwrap().releases[0]
            .workflow
            .as_deref(),
        Some("sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v2")
    );

    // Replace cached files with genuinely signed older serial 40; offline load MUST refuse it.
    fs::write(cache.join(CACHE_INDEX), &bytes_40).unwrap();
    fs::write(cache.join(CACHE_SIG), &sig_40).unwrap();
    let mut reopened = Store::new(cache, REGISTRY_KEYS);
    let cache_err = reopened
        .load_from_fetch(Err("offline".into()), now_2026_11_24())
        .unwrap_err();
    assert_eq!(
        cache_err,
        LoadError::Rollback {
            got: 40,
            highest: 42
        }
    );
}

#[test]
fn stale_generated_timestamp_warns_without_failing() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join(CACHE_DIR);
    let mut store = Store::new(cache, REGISTRY_KEYS);

    let (bytes, sig) = load_fixture("stale.json");
    let catalog = store
        .load_from_fetch(Ok((bytes, sig)), now_2026_11_24())
        .unwrap();
    assert_eq!(catalog.index.serial, 41);
    let warning = catalog
        .stale_warning
        .expect("stale generated timestamp must produce a warning");
    assert!(warning.contains("2025-01-01T00:00:00Z"), "{warning}");
}

#[test]
fn malformed_signed_indexes_are_refused_with_no_salvage() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path().join(CACHE_DIR);
    let mut store = Store::new(cache.clone(), REGISTRY_KEYS);

    for fixture in [
        "malformed-json.json",
        "malformed-schema.json",
        "malformed-missing-revoked.json",
        "malformed-empty-releases.json",
        "malformed-bad-generated.json",
    ] {
        let (bytes, sig) = load_fixture(fixture);
        let err = store
            .load_from_fetch(Ok((bytes, sig)), now_2026_11_24())
            .unwrap_err();
        assert!(
            matches!(err, LoadError::Malformed(_)),
            "{fixture}: expected Malformed, got {err:?}"
        );
    }
    assert!(!cache.join(CACHE_INDEX).exists());
    assert_eq!(store.highest_serial().unwrap(), None);
}

#[test]
fn v2_reader_never_reuses_the_v1_cache_file() {
    let temp = tempfile::tempdir().unwrap();
    let v1_cache = temp.path().join(super::super::CACHE);
    fs::create_dir_all(v1_cache.parent().unwrap()).unwrap();
    let (bytes, _) = load_fixture("two-ports-index.json");
    fs::write(&v1_cache, &bytes).unwrap();

    let mut store = Store::new(temp.path().join(CACHE_DIR), REGISTRY_KEYS);
    let err = store
        .load_from_fetch(Err("network down".into()), now_2026_11_24())
        .unwrap_err();
    assert_eq!(err, LoadError::Fetch("network down".into()));
}

#[test]
fn fetch_and_load_verifies_local_files_and_loopback_http() {
    assert_eq!(
        signature_location("https://sunrise-org.github.io/sunrise-mods/v2/index.json"),
        "https://sunrise-org.github.io/sunrise-mods/v2/index.json.minisig"
    );
    assert_eq!(
        signature_location("https://example.org/v2/index.json?rev=2"),
        "https://example.org/v2/index.json.minisig?rev=2"
    );

    // Local file path override still verifies the `.minisig` beside it.
    let temp = tempfile::tempdir().unwrap();
    let local_index = fixtures().join("serial-40.json");
    let mut local_store = Store::new(temp.path().join("local-cache"), REGISTRY_KEYS);
    let local_catalog = local_store
        .fetch_and_load(local_index.to_str().unwrap(), now_2026_11_24())
        .unwrap();
    assert_eq!(local_catalog.index.serial, 40);
    assert_eq!(
        local_catalog.index.project("Ab3dE9xQ").unwrap().releases[0]
            .workflow
            .as_deref(),
        Some("sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1")
    );

    // Loopback HTTP server serves both `index.json` and `index.json.minisig`.
    let (index_bytes, sig_text) = load_fixture("two-ports-index.json");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
            }
            let path = request_line.split_whitespace().nth(1).unwrap_or("");
            let (status, body): (&str, &[u8]) = match path {
                "/v2/index.json" => ("200 OK", &index_bytes),
                "/v2/index.json.minisig" => ("200 OK", sig_text.as_bytes()),
                _ => ("404 Not Found", b"missing"),
            };
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
            stream.flush().unwrap();
        }
    });

    let url = format!("http://{addr}/v2/index.json");
    let mut http_store = Store::new(temp.path().join("http-cache"), REGISTRY_KEYS);
    let http_catalog = http_store.fetch_and_load(&url, now_2026_11_24()).unwrap();
    server.join().unwrap();
    assert!(!http_catalog.from_cache);
    assert_eq!(http_catalog.index.serial, 41);
    assert_eq!(http_catalog.index.projects.len(), 2);
}
