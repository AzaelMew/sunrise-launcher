//! The game's `sunrise-release-sign` fixture test, run on a copy of its fixtures: both verifiers accept the same bytes.
use crate as release;
use crate::{Channel, PublicKey, SIGNATURE, SIGNATURE_V2, SUMS};
use std::path::{Path, PathBuf};

const LINUX: &str = "sunrise-linux-x86_64.tar.gz";
const SDK_BUILD: &str = "0123456789abcdef0123456789abcdef01234567";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/signed")
}

fn read(path: &str) -> Vec<u8> {
    std::fs::read(fixtures().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn text(path: &str) -> String {
    String::from_utf8(read(path)).unwrap()
}

fn keys() -> Vec<PublicKey> {
    release::parse_keys(&text("test.pub")).unwrap()
}

fn digest_of(dir: &str, asset: &str) -> String {
    release::sha256_hex(&read(&format!("{dir}/{asset}")))
}

#[test]
fn a_release_signed_by_the_script_installs_and_nothing_else_passes_for_it() {
    let (sums, signature) = (
        read(&format!("release/{SUMS}")),
        text(&format!("release/{SIGNATURE}")),
    );
    let digest =
        |tag: &str| release::signed_digest(tag, LINUX, Some(&sums), Some(&signature), &keys());
    let signed = digest("v2026.10.19").unwrap();
    assert_eq!(signed, digest_of("release", LINUX));
    assert!(release::check(LINUX, &signed, &digest_of("release", LINUX)).is_ok());
    assert!(
        release::check(
            LINUX,
            &signed,
            &digest_of("release", "sunrise-windows-x64.zip")
        )
        .is_err()
    );
    for tag in ["v2026.10.20", "nightly", "dev", "sdk-v2026.10.19"] {
        assert!(digest(tag).is_err(), "{tag}");
    }
    assert!(
        release::signed_digest("v2026.10.19", LINUX, Some(&sums), Some(&signature), &[]).is_err()
    );
    let mut changed = sums.clone();
    changed[0] ^= 1;
    assert!(
        release::signed_digest(
            "v2026.10.19",
            LINUX,
            Some(&changed),
            Some(&signature),
            &keys()
        )
        .is_err()
    );
}

#[test]
fn a_build_installs_by_its_own_version_and_never_by_the_channel_or_the_other_channel() {
    let stable = "v2026.10.19";
    let stable_sums = read(&format!("release/{SUMS}"));
    let stable_signature = text(&format!("release/{SIGNATURE}"));
    let stable_keys = keys();
    for (channel, version, other, public_key) in [
        (
            Channel::Nightly,
            "nightly-2026-10-28-0c02067",
            Channel::Dev,
            "test.pub",
        ),
        (
            Channel::Dev,
            "dev-2026-11-13-0c02067",
            Channel::Nightly,
            "dev/ephemeral.pub",
        ),
    ] {
        let keys = release::parse_keys(&text(public_key)).unwrap();
        let dir = channel.tag();
        let sums = read(&format!("{dir}/{SUMS}"));
        let v2 = text(&format!("{dir}/{SIGNATURE_V2}"));
        let channel_signature = text(&format!("{dir}/{SIGNATURE}"));
        let digest = |channel, version: &str, signature: &str| {
            release::build_digest(channel, version, LINUX, Some(&sums), Some(signature), &keys)
        };
        assert_eq!(
            digest(channel, version, &v2),
            Ok(digest_of(dir, LINUX)),
            "{version}"
        );
        assert!(
            digest(channel, version, &channel_signature).is_err(),
            "{version}"
        );
        let other_version = version.replacen(dir, other.tag(), 1);
        assert!(
            digest(other, &other_version, &v2).is_err(),
            "{other_version}"
        );
        assert!(digest(channel, &version.replace("0c02067", "0c02068"), &v2).is_err());
        assert!(release::verify(dir, &sums, &channel_signature, &keys).is_ok());
        assert!(release::verify(other.tag(), &sums, &channel_signature, &keys).is_err());
        assert!(release::verify(dir, &stable_sums, &stable_signature, &stable_keys).is_err());
        assert!(
            release::build_digest(
                channel,
                version,
                LINUX,
                Some(&stable_sums),
                Some(&stable_signature),
                &stable_keys,
            )
            .is_err()
        );
        for signature in [&channel_signature, &v2] {
            assert!(
                release::signed_digest(stable, LINUX, Some(&sums), Some(signature), &keys).is_err()
            );
        }
        assert!(
            release::signed_digest(dir, LINUX, Some(&sums), Some(&channel_signature), &keys)
                .is_err()
        );
    }
}

#[test]
fn an_sdk_installs_only_as_the_sdk_of_its_build() {
    let (sums, signature) = (
        read(&format!("sdk/{SUMS}")),
        text(&format!("sdk/{SIGNATURE}")),
    );
    let digest = release::sdk_digest(
        SDK_BUILD,
        "sdk.json",
        Some(&sums),
        Some(&signature),
        &keys(),
    );
    assert_eq!(digest, Ok(digest_of("sdk", "sdk.json")));
    let other = "f".repeat(40);
    assert!(
        release::sdk_digest(&other, "sdk.json", Some(&sums), Some(&signature), &keys()).is_err()
    );
    let tag = format!("{}{SDK_BUILD}", release::SDK_PREFIX);
    assert!(
        release::signed_digest(&tag, "sdk.json", Some(&sums), Some(&signature), &keys()).is_err()
    );
}
