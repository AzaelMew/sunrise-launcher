use super::*;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/index_v2")
}

fn test_public_key() -> String {
    std::fs::read_to_string(fixtures().join("test.pub"))
        .unwrap()
        .trim()
        .to_owned()
}

fn other_public_key() -> String {
    std::fs::read_to_string(fixtures().join("other.pub"))
        .unwrap()
        .trim()
        .to_owned()
}

fn load_fixture(name: &str) -> (Vec<u8>, String) {
    let bytes = std::fs::read(fixtures().join(name)).unwrap();
    let sig = std::fs::read_to_string(fixtures().join(format!("{name}.minisig"))).unwrap();
    (bytes, sig)
}

#[test]
fn golden_index_verifies_and_parses_completely() {
    let key = test_public_key();
    let (bytes, sig) = load_fixture("two-ports-index.json");
    let index = verify_and_parse(&bytes, &sig, &[key.as_str()]).unwrap();

    assert_eq!(index.schema, SCHEMA);
    assert_eq!(index.serial, 41);
    assert_eq!(index.generated, "2026-11-23T00:00:00Z");
    assert_eq!(index.categories.len(), 2);
    assert_eq!(index.projects.len(), 2);
    assert_eq!(index.revoked.len(), 2);

    let providers: Vec<&str> = index
        .projects_for_mod("wayfinder")
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(providers, ["Ab3dE9xQ", "Zz91kLmN"]);

    let primary = index.project("Ab3dE9xQ").unwrap();
    assert_eq!(primary.slug, "wayfinder");
    assert_eq!(primary.kind, Kind::NativeMod);
    assert_eq!(primary.repository.id, 123_456_789);
    assert_eq!(primary.repository.path, ".");
    assert_eq!(
        primary.workflow,
        "sunrise-org/sunrise-mods/.github/workflows/release.yml"
    );
    assert_eq!(primary.releases[0].workflow, None);
    assert!(primary.releases[0].fits_build("26.3-0.1.0"));
    assert!(!primary.releases[0].fits_build("26.3-0.2.0"));

    let alt = index.project_by_slug("wayfinder-alternate").unwrap();
    assert_eq!(alt.id, "Zz91kLmN");
    assert_eq!(alt.kind, Kind::Mod);
    assert_eq!(alt.homepage, None);
    assert_eq!(
        alt.screenshots[0].path,
        "pages/Zz91kLmN/gallery/map%20%231%20%25.png"
    );
    assert_eq!(
        alt.releases[0].dependencies[0].project.as_deref(),
        Some("Ab3dE9xQ")
    );

    assert!(index.revoked("Retired0", None).is_some());
    assert!(index.revoked("Ab3dE9xQ", None).is_none());
    assert!(
        index
            .revoked(
                "Ab3dE9xQ",
                Some("cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc")
            )
            .is_some()
    );
    assert!(
        index
            .revoked(
                "Ab3dE9xQ",
                Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            )
            .is_none()
    );
}

#[test]
fn unknown_fields_are_ignored_and_release_workflow_is_parsed() {
    let key = test_public_key();
    let (bytes, sig) = load_fixture("serial-40.json");
    let index = verify_and_parse(&bytes, &sig, &[key.as_str()]).unwrap();
    assert_eq!(index.serial, 40);
    let primary = index.project("Ab3dE9xQ").unwrap();
    assert_eq!(
        primary.releases[0].workflow.as_deref(),
        Some("sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1")
    );
}

#[test]
fn empty_key_list_reports_registry_key_not_configured() {
    let (bytes, sig) = load_fixture("two-ports-index.json");
    let err = verify_and_parse(&bytes, &sig, &[]).unwrap_err();
    assert_eq!(err, VerifyError::KeyNotConfigured);
    assert_eq!(err.to_string(), "registry key not configured");
}

#[test]
fn bad_signature_wrong_key_and_tampered_bytes_are_refused() {
    let key = test_public_key();
    let other = other_public_key();
    let (bytes, sig) = load_fixture("two-ports-index.json");

    // Signed by another key.
    let other_sig =
        std::fs::read_to_string(fixtures().join("two-ports-index.other.minisig")).unwrap();
    assert!(matches!(
        verify_and_parse(&bytes, &other_sig, &[key.as_str()]),
        Err(VerifyError::BadSignature(_))
    ));
    // Trusted key list can hold multiple keys (rotation); succeeds when matching key is present.
    assert!(verify_and_parse(&bytes, &other_sig, &[key.as_str(), other.as_str()]).is_ok());

    // Tampered single byte in the signed JSON.
    let mut tampered = bytes.clone();
    tampered[0] ^= 0x01;
    assert!(matches!(
        verify_and_parse(&tampered, &sig, &[key.as_str()]),
        Err(VerifyError::BadSignature(_))
    ));

    // Malformed minisign text.
    assert!(matches!(
        verify_and_parse(&bytes, "not a signature", &[key.as_str()]),
        Err(VerifyError::BadSignature(_))
    ));
}

#[test]
fn malformed_signed_payloads_are_refused_without_salvage() {
    let key = test_public_key();
    for fixture in [
        "malformed-json.json",
        "malformed-schema.json",
        "malformed-missing-revoked.json",
        "malformed-empty-releases.json",
        "malformed-bad-generated.json",
    ] {
        let (bytes, sig) = load_fixture(fixture);
        let err = verify_and_parse(&bytes, &sig, &[key.as_str()]).unwrap_err();
        assert!(
            matches!(err, VerifyError::Malformed(_)),
            "{fixture}: expected Malformed, got {err:?}"
        );
    }
}

#[test]
fn stale_timestamp_detection_is_informational_only() {
    let key = test_public_key();
    let (bytes, sig) = load_fixture("stale.json");
    let index = verify_and_parse(&bytes, &sig, &[key.as_str()]).unwrap();
    let now_2026 = parse_rfc3339_utc("2026-11-23T00:00:00Z").unwrap();
    assert!(index.is_stale_at(now_2026));

    let fresh_at = parse_rfc3339_utc("2025-01-05T00:00:00Z").unwrap();
    assert!(!index.is_stale_at(fresh_at));
}

#[test]
fn image_url_resolves_relative_paths_and_rejects_escapes() {
    let base = "https://sunrise-org.github.io/sunrise-mods/v2/index.json";
    assert_eq!(
        image_url(base, "pages/Ab3dE9xQ/icon.png").as_deref(),
        Some("https://sunrise-org.github.io/sunrise-mods/v2/pages/Ab3dE9xQ/icon.png")
    );
    assert_eq!(
        image_url(base, "pages/Zz91kLmN/gallery/map%20%231%20%25.png").as_deref(),
        Some(
            "https://sunrise-org.github.io/sunrise-mods/v2/pages/Zz91kLmN/gallery/map%20%231%20%25.png"
        )
    );
    assert_eq!(image_url(base, "pages/Ab3dE9xQ/../secret.png"), None);
    assert_eq!(image_url(base, "https://evil.example/icon.png"), None);
    assert_eq!(image_url(base, "/etc/passwd"), None);
}
