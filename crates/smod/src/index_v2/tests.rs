use super::*;
use serde_json::{Value, json};
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

fn golden_json() -> Value {
    let bytes = std::fs::read(fixtures().join("two-ports-index.json")).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn parse_value(value: &Value) -> Result<Index, String> {
    Index::parse(&serde_json::to_string(value).unwrap())
}

#[test]
fn golden_index_verifies_and_parses_completely() {
    let key = test_public_key();
    let (bytes, sig) = load_fixture("two-ports-index.json");
    let index = verify_and_parse(&bytes, &sig, &[key.as_str()]).unwrap();

    assert_eq!(index.schema, SCHEMA);
    assert_eq!(index.serial, 41);
    assert_eq!(index.generated, "2026-11-23T00:00:00Z");
    assert_eq!(index.builds.len(), 3);
    assert_eq!(
        index.build("26.3-0.1.0").unwrap().commit,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(
        index.build("dev-2026-11-23-abcdef0").unwrap().commit,
        "abcdef0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
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
    assert_eq!(
        primary.releases[0].workflow,
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1"
    );
    assert_eq!(
        primary.releases[0].fit_verdict("26.3-0.1.0"),
        FitVerdict::Fits
    );
    assert_eq!(
        primary.releases[0].fit_verdict("26.3-0.1.1"),
        FitVerdict::Fits
    );
    assert_eq!(
        primary.releases[0].fit_verdict("26.3-0.2.0"),
        FitVerdict::Unknown
    );
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
    // Fit is never inferred from `sunrise` when `fits` is empty.
    assert_eq!(alt.releases[0].sunrise, "nightly-2026-11-23-abcdef0");
    assert_eq!(
        alt.releases[0].fit_verdict("nightly-2026-11-23-abcdef0"),
        FitVerdict::Unknown
    );
    assert!(!alt.releases[0].fits_build("nightly-2026-11-23-abcdef0"));

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
fn unknown_fields_are_ignored_and_three_way_fit_verdict_distinguishes_all_states() {
    let key = test_public_key();
    let (bytes, sig) = load_fixture("serial-40.json");
    let index = verify_and_parse(&bytes, &sig, &[key.as_str()]).unwrap();
    assert_eq!(index.serial, 40);
    let primary = index.project("Ab3dE9xQ").unwrap();
    let release = &primary.releases[0];
    assert_eq!(
        release.workflow,
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1"
    );
    assert_eq!(release.fit_verdict("26.3-0.1.0"), FitVerdict::Fits);
    assert_eq!(
        release.fit_verdict("dev-2026-11-23-abcdef0"),
        FitVerdict::DoesNotFit
    );
    assert_eq!(release.fit_verdict("26.3-0.9.0"), FitVerdict::Unknown);
    assert!(release.fits_build("26.3-0.1.0"));
    assert!(!release.fits_build("dev-2026-11-23-abcdef0"));
    assert!(!release.fits_build("26.3-0.9.0"));
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
fn non_ascii_generated_timestamp_returns_error_without_panicking() {
    // Byte 18 cuts through the two-byte UTF-8 sequence for 'é' in the seconds field.
    for bad in [
        "2026-11-23T00:00:0éZ",
        "2026-11-23T00:00:éZ",
        "202é-11-23T00:00:00Z",
        "2026-11-23T00:00:00+0é:00",
    ] {
        assert!(
            parse_rfc3339_utc(bad).is_err(),
            "expected error for {bad:?}"
        );
        let mut index = golden_json();
        index["generated"] = json!(bad);
        assert!(parse_value(&index).is_err(), "expected error for {bad:?}");
    }
}

#[test]
fn release_workflow_is_required_and_must_match_project_at_release_tag() {
    // Missing workflow field on a release.
    let mut missing = golden_json();
    missing["projects"][0]["releases"][0]
        .as_object_mut()
        .unwrap()
        .remove("workflow");
    assert!(parse_value(&missing).is_err());

    // Mismatched workflow path or invalid ref forms.
    for bad_ref in [
        "other-org/other-repo/.github/workflows/release.yml@refs/tags/release-v1",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@release-v1",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/heads/main",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/v1",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1a",
        "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1@extra",
    ] {
        let mut value = golden_json();
        value["projects"][0]["releases"][0]["workflow"] = json!(bad_ref);
        assert!(
            parse_value(&value).is_err(),
            "expected rejection for workflow {bad_ref:?}"
        );
    }

    // Higher release-v tag number with matching project workflow path is accepted.
    let mut valid = golden_json();
    valid["projects"][0]["releases"][0]["workflow"] =
        json!("sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v12");
    assert!(parse_value(&valid).is_ok());
}

#[test]
fn a_release_row_names_the_repository_it_was_signed_from_or_falls_back_to_the_project() {
    let golden = golden_json();
    let index = parse_value(&golden).unwrap();
    let project = &index.projects[0];
    assert_eq!(project.releases[0].repository, None);
    assert_eq!(
        project.release_repository(&project.releases[0]),
        project.repository.id
    );

    let mut moved = golden_json();
    moved["projects"][0]["releases"][0]["repository"] = json!(424242);
    let index = parse_value(&moved).unwrap();
    let project = &index.projects[0];
    assert_eq!(project.release_repository(&project.releases[0]), 424242);

    let mut zero = golden_json();
    zero["projects"][0]["releases"][0]["repository"] = json!(0);
    assert!(parse_value(&zero).is_err());
}

#[test]
fn a_release_row_with_jar_unavailable_parses_and_one_without_it_still_parses() {
    let index = parse_value(&golden_json()).unwrap();
    assert!(index.projects[0].releases[0].jar_unavailable.is_empty());
    let written = serde_json::to_value(&index).unwrap();
    assert!(
        written["projects"][0]["releases"][0]
            .get("jar_unavailable")
            .is_none()
    );

    let sha512 = "ab".repeat(64);
    let mut marked = golden_json();
    marked["projects"][0]["releases"][0]["jar_unavailable"] =
        json!([{ "id": "macaws_bridges", "version": "3.0.0", "sha512": sha512 }]);
    let index = parse_value(&marked).unwrap();
    let jars = &index.projects[0].releases[0].jar_unavailable;
    assert_eq!(jars.len(), 1);
    assert_eq!(jars[0].id(), "macaws_bridges");
    assert_eq!(jars[0].version(), "3.0.0");
    assert_eq!(jars[0].sha512(), sha512);
    let written = serde_json::to_value(&index).unwrap();
    assert_eq!(
        written["projects"][0]["releases"][0]["jar_unavailable"],
        json!([{ "id": "macaws_bridges", "version": "3.0.0", "sha512": sha512 }])
    );

    let mut later = golden_json();
    later["projects"][0]["releases"][0]["jar_unavailable"] =
        json!([{ "id": "macaws_bridges", "version": "3.0.0", "sha512": sha512, "since": 1 }]);
    let index = parse_value(&later).unwrap();
    assert_eq!(index.projects[0].releases[0].jar_unavailable, *jars);

    let mut bad = golden_json();
    bad["projects"][0]["releases"][0]["jar_unavailable"] =
        json!([{ "id": "macaws_bridges", "version": "3.0.0", "sha512": "abc" }]);
    assert!(parse_value(&bad).is_err());
}

#[test]
fn a_release_row_with_dependency_unavailable_parses_and_one_without_it_still_parses() {
    let index = parse_value(&golden_json()).unwrap();
    assert!(
        index.projects[0].releases[0]
            .dependency_unavailable
            .is_empty()
    );
    let written = serde_json::to_value(&index).unwrap();
    assert!(
        written["projects"][0]["releases"][0]
            .get("dependency_unavailable")
            .is_none()
    );

    let rows = json!([
        { "build": "26.3-0.1.0", "mod": "macaws_lib" },
        { "build": "26.3-0.1.0", "mod": "other_lib" }
    ]);
    let mut marked = golden_json();
    marked["projects"][0]["releases"][0]["dependency_unavailable"] = rows.clone();
    let index = parse_value(&marked).unwrap();
    let entries = &index.projects[0].releases[0].dependency_unavailable;
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].build, "26.3-0.1.0");
    assert_eq!(entries[0].mod_id, "macaws_lib");
    assert_eq!(entries[1].mod_id, "other_lib");
    let written = serde_json::to_value(&index).unwrap();
    assert_eq!(
        written["projects"][0]["releases"][0]["dependency_unavailable"],
        rows
    );

    let mut bad = golden_json();
    bad["projects"][0]["releases"][0]["dependency_unavailable"] =
        json!([{ "build": "26.3-0.1.0", "mod": "BadId" }]);
    assert!(parse_value(&bad).is_err());
}

#[test]
fn project_and_dependency_mod_ids_reuse_manifest_portable_validator() {
    let too_long = "a".repeat(65);
    for bad_id in ["", "con", "nul", "com1", "lpt9", "BadId", too_long.as_str()] {
        let mut project_bad = golden_json();
        project_bad["projects"][0]["mod"] = json!(bad_id);
        assert!(
            parse_value(&project_bad).is_err(),
            "expected project mod id {bad_id:?} to fail"
        );

        let mut dep_bad = golden_json();
        dep_bad["projects"][0]["releases"][0]["dependencies"][0]["mod"] = json!(bad_id);
        assert!(
            parse_value(&dep_bad).is_err(),
            "expected dependency mod id {bad_id:?} to fail"
        );
    }
}

#[test]
fn release_versions_and_build_ids_are_strictly_validated() {
    for bad_version in ["v1.2.0", "1.2", "01.0.0", "not-semver", ""] {
        let mut value = golden_json();
        value["projects"][0]["releases"][0]["version"] = json!(bad_version);
        assert!(
            parse_value(&value).is_err(),
            "expected version {bad_version:?} to fail"
        );
    }

    for bad_build in [
        "nightly",
        "dev",
        "nightly-2026-11-23",
        "dev-2026-11-23-short",
        "nightly-2026-11-23-ABCDEF0",
        "26.3-0.1.0-rc.1",
        "026.3-0.1.0",
        "26.03-0.1.0",
        "26.3.0.0-0.1.0",
        "26.3-0.1.0+meta",
        "26.3-0.1",
        "26-0.1.0",
        "latest",
    ] {
        let mut bad_sunrise = golden_json();
        bad_sunrise["projects"][0]["releases"][0]["sunrise"] = json!(bad_build);
        assert!(
            parse_value(&bad_sunrise).is_err(),
            "expected sunrise {bad_build:?} to fail"
        );

        let mut bad_fits = golden_json();
        bad_fits["projects"][0]["releases"][0]["fits"] = json!([bad_build]);
        assert!(
            parse_value(&bad_fits).is_err(),
            "expected fits {bad_build:?} to fail"
        );

        let mut bad_dnf = golden_json();
        bad_dnf["projects"][0]["releases"][0]["does_not_fit"] = json!([bad_build]);
        assert!(
            parse_value(&bad_dnf).is_err(),
            "expected does_not_fit {bad_build:?} to fail"
        );
    }

    // Build listed in both fits and does_not_fit is rejected.
    let mut conflict = golden_json();
    conflict["projects"][0]["releases"][0]["does_not_fit"] = json!(["26.3-0.1.0"]);
    assert!(parse_value(&conflict).is_err());

    // Offered build whose commit does not match its channel build short hash is rejected.
    let mut bad_offered = golden_json();
    bad_offered["builds"][2]["commit"] = json!("9999999aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert!(parse_value(&bad_offered).is_err());
}

#[test]
fn screenshots_and_categories_enforce_presence_declaration_and_kind() {
    // Empty screenshot list is rejected.
    let mut no_shots = golden_json();
    no_shots["projects"][0]["screenshots"] = json!([]);
    assert!(parse_value(&no_shots).is_err());

    // Undeclared category is rejected.
    let mut undeclared = golden_json();
    undeclared["projects"][0]["categories"] = json!(["nonexistent"]);
    assert!(parse_value(&undeclared).is_err());

    // Category that does not apply to the project's kind is rejected.
    let mut wrong_kind = golden_json();
    wrong_kind["categories"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "shaders-only",
            "icon": "sparkles",
            "kinds": ["shader"]
        }));
    wrong_kind["projects"][0]["categories"] = json!(["shaders-only"]);
    assert!(parse_value(&wrong_kind).is_err());
}

#[test]
fn page_and_asset_urls_reject_missing_hosts_credentials_and_bad_ports() {
    for bad_url in [
        "https://",
        "https:///path/only",
        "https://?query=only",
        "https://:443/no-host",
        "https://user@example.org/wayfinder",
        "https://user:pass@example.org/wayfinder",
        "https://@example.org/wayfinder",
        "https://example.org:99999/wayfinder",
        "https://example.org:abc/wayfinder",
        "https://example.org:/wayfinder",
        "https://[::1/wayfinder",
        "https://example.org\\evil",
        "ftp://example.org/wayfinder",
        "https://999.999.999.999/wayfinder",
    ] {
        let mut page_bad = golden_json();
        page_bad["projects"][0]["homepage"] = json!(bad_url);
        assert!(
            parse_value(&page_bad).is_err(),
            "expected homepage {bad_url:?} to fail"
        );

        let mut asset_bad = golden_json();
        asset_bad["projects"][0]["releases"][0]["file"]["url"] = json!(bad_url);
        assert!(
            parse_value(&asset_bad).is_err(),
            "expected file.url {bad_url:?} to fail"
        );

        let mut att_bad = golden_json();
        att_bad["projects"][0]["releases"][0]["file"]["attestation"] = json!(bad_url);
        assert!(
            parse_value(&att_bad).is_err(),
            "expected file.attestation {bad_url:?} to fail"
        );
    }

    // Plain http:// is allowed for page links, rejected for asset URLs.
    let mut page_http = golden_json();
    page_http["projects"][0]["homepage"] = json!("http://example.org/wayfinder");
    assert!(parse_value(&page_http).is_ok());

    let mut asset_http = golden_json();
    asset_http["projects"][0]["releases"][0]["file"]["url"] =
        json!("http://github.com/example/wayfinder/releases/download/v1.2.0/wayfinder-1.2.0.smod");
    assert!(parse_value(&asset_http).is_err());

    // Valid IPv6 literal with port is accepted.
    let mut ipv6_page = golden_json();
    ipv6_page["projects"][0]["homepage"] = json!("https://[::1]:8443/wayfinder");
    assert!(parse_value(&ipv6_page).is_ok());
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

#[test]
fn content_addressed_image_bytes_verify_against_filename_sha256() {
    let png_bytes = b"\x89PNG\r\n\x1a\ncontent-addressed-test";
    let png_hash = crate::index::sha256(png_bytes);
    let png_path = format!("pages/Ab3dE9xQ/{png_hash}.png");
    assert_eq!(
        content_addressed_image_sha256("Ab3dE9xQ", &png_path).unwrap(),
        png_hash
    );
    verify_image_bytes("Ab3dE9xQ", &png_path, png_bytes).unwrap();

    let jpg_bytes = b"\xff\xd8\xffcontent-addressed-jpeg";
    let jpg_hash = crate::index::sha256(jpg_bytes);
    let jpg_path = format!("pages/Ab3dE9xQ/{jpg_hash}.jpg");
    verify_image_bytes("Ab3dE9xQ", &jpg_path, jpg_bytes).unwrap();

    // Tampered bytes fail hash verification.
    assert!(verify_image_bytes("Ab3dE9xQ", &png_path, b"different bytes").is_err());

    // Wrong project ID, nested folder, wrong extension, uppercase hex, or non-hash filename fail.
    for bad_path in [
        format!("pages/Zz91kLmN/{png_hash}.png"),
        format!("pages/Ab3dE9xQ/sub/{png_hash}.png"),
        format!("pages/Ab3dE9xQ/{png_hash}.gif"),
        format!("pages/Ab3dE9xQ/{}.png", png_hash.to_ascii_uppercase()),
        "pages/Ab3dE9xQ/icon.png".to_owned(),
    ] {
        assert!(
            verify_image_bytes("Ab3dE9xQ", &bad_path, png_bytes).is_err(),
            "expected rejection for {bad_path:?}"
        );
    }
}

#[test]
fn a_collection_lists_only_its_publishers_listed_projects() {
    let collection = |members: Value| {
        let mut index = golden_json();
        index["collections"] = json!([{
            "id": "Qm4tZ81c",
            "name": "Ways",
            "summary": "Ways to find your way.",
            "publisher": "example",
            "icon": format!("pages/Qm4tZ81c/{}.png", "a".repeat(64)),
            "members": members,
        }]);
        index
    };
    let both = json!([
        { "project": "Ab3dE9xQ", "default": true },
        { "project": "Zz91kLmN", "default": false },
    ]);
    // The owner's rule: Zz91kLmN's repository belongs to another account.
    let err = parse_value(&collection(both.clone())).unwrap_err();
    assert!(err.contains("not published by"), "{err}");

    let mut owned = collection(both);
    owned["projects"][1]["repository"]["name"] = json!("example/port-collection");
    let index = parse_value(&owned).unwrap();
    let parsed = index.collection("Qm4tZ81c").unwrap();
    assert_eq!(parsed.description, "");
    assert_eq!(parsed.members.len(), 2);

    for (members, why) in [
        (
            json!([{ "project": "Ab3dE9xQ", "default": true }]),
            "two members",
        ),
        (
            json!([
                { "project": "Ab3dE9xQ", "default": false },
                { "project": "Zz91kLmN", "default": false },
            ]),
            "picked by default",
        ),
        (
            json!([
                { "project": "Ab3dE9xQ", "default": true },
                { "project": "Nope1234", "default": true },
            ]),
            "not a listed project",
        ),
    ] {
        let mut bad = collection(members);
        bad["projects"][1]["repository"]["name"] = json!("example/port-collection");
        let err = parse_value(&bad).unwrap_err();
        assert!(err.contains(why), "{why}: {err}");
    }
    // A collection's ID is never a project's.
    let mut clash = owned.clone();
    clash["collections"][0]["id"] = json!("Ab3dE9xQ");
    clash["collections"][0]["icon"] = json!(format!("pages/Ab3dE9xQ/{}.png", "a".repeat(64)));
    assert!(
        parse_value(&clash)
            .unwrap_err()
            .contains("invalid collection id")
    );
}
