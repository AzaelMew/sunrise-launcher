use super::*;
use crate::native_mods::sdk_cache::{
    tests::{index_of, publish, COMMIT},
    BASE_VAR,
};
use sunrise_launcher_release_verify::{sha256_hex, SigningKey};
use sunrise_launcher_sdk::{Asset, Kind, Side};

const DEV: &str = "dev-2026-11-13-0123456";
const FACTS: &[u8] = br#"{"contract":1,"build":"dev-2026-11-13-0123456","commit":"0123456789abcdef0123456789abcdef01234567"}"#;
#[cfg(windows)]
const SLICES: [&[u8]; 2] = [
    include_bytes!("sdk-fixture/windows-runtime.tar.xz"),
    include_bytes!("sdk-fixture/windows-programs.tar.xz"),
];
#[cfg(target_os = "macos")]
const SLICES: [&[u8]; 2] = [
    include_bytes!("sdk-fixture/macos-runtime.tar.xz"),
    include_bytes!("sdk-fixture/unix-programs.tar.xz"),
];
#[cfg(not(any(windows, target_os = "macos")))]
const SLICES: [&[u8]; 2] = [
    include_bytes!("sdk-fixture/linux-runtime.tar.xz"),
    include_bytes!("sdk-fixture/unix-programs.tar.xz"),
];

fn offer(tag: &str, assets: &[&str]) -> Release {
    game_releases(&[serde_json::json!({
        "tag_name": tag,
        "name": "Dev 2026-11-13 · 0123456",
        "prerelease": tag == DEV,
        "assets": assets.iter().map(|name| serde_json::json!({"name": name, "size": 1})).collect::<Vec<_>>(),
    })])
    .pop()
    .unwrap()
}

#[test]
fn first_play_proves_and_installs_the_two_sdk_slices() {
    isolated_case(
        "sdk_install::first_play_proves_and_installs_the_two_sdk_slices",
        || {
            let requested = selected_game_architecture(&Settings::load());
            let Some(triple) = sdk_target(requested) else {
                return;
            };
            let sdk_tag = format!("sdk-{COMMIT}");
            let mirror = PathBuf::from(std::env::var_os(BASE_VAR).unwrap());
            let base = Base::from_env();
            let (key, _) = SigningKey::generate().unwrap();
            let keys = [key.public()];
            let release = offer(DEV, &[INDEX]);
            assert!(crate::managed_play::FirstBuild::download(&release).is_ok());
            let download = GameDownload::select(&release).unwrap();
            let names =
                [Kind::Runtime, Kind::Programs].map(|kind| Asset::name(kind, triple, Side::Client));
            let slices = [
                (names[0].as_str(), SLICES[0]),
                (names[1].as_str(), SLICES[1]),
            ];
            let game = version_dir(DEV);
            let install = |files: &[(&str, &[u8])], tamper: bool| {
                // A playable runtime does not need a matching launcher Rust toolchain.
                let index = index_of(files, "1.0.0").to_json();
                publish(
                    &mirror,
                    DEV,
                    DEV,
                    &key,
                    index.as_bytes(),
                    &[(RELEASE_FILE, FACTS)],
                );
                publish(&mirror, &sdk_tag, &sdk_tag, &key, index.as_bytes(), files);
                if tamper {
                    let mut damaged = SLICES[0].to_vec();
                    let offset = damaged.len() / 2;
                    damaged[offset] ^= 1;
                    fs::write(mirror.join(&sdk_tag).join(&names[0]), damaged).unwrap();
                }
                let proof = download
                    .prove_with(DEV, &keys, |name| {
                        base.release_file(DEV, DEV, name).map_err(|e| e.to_string())
                    })
                    .unwrap();
                let proof = GameProof::saved(serde_json::to_value(proof).unwrap()).unwrap();
                let outcome = download.publish_with(
                    DEV,
                    &proof,
                    InstallIntent::Exact,
                    |_| {},
                    |_, _| unreachable!("SDK reads from the mirror"),
                    (&base, &keys),
                );
                (proof, outcome)
            };
            let (_, missing) = install(&slices[..1], false);
            assert_eq!(
                missing.unwrap_err(),
                format!("{DEV} has no build for the selected game architecture.")
            );
            assert!(!game.exists());
            let (_, tampered) = install(&slices, true);
            let error = tampered.unwrap_err();
            assert!(
                error.contains(&names[0]) && error.contains("checksum"),
                "{error}"
            );
            assert!(!game.exists());
            let (proof, installed) = install(&slices, false);
            assert!(matches!(
                installed.unwrap(),
                InstallOutcome::Installed(None)
            ));
            assert!(download.is_installed(DEV, &proof));
            assert!(crate::managed_play::FirstBuild::installed(DEV).is_ok());
            let (library, std) = match std::env::consts::OS {
                "windows" => ("sunrise_fixture.dll", "std-fixture.dll"),
                "macos" => ("libsunrise_fixture.dylib", "libstd-fixture.dylib"),
                _ => ("libsunrise_fixture.so", "libstd-fixture.so"),
            };
            for file in [
                CLIENT,
                crate::preparer::BINARY,
                library,
                std,
                "exporters/manifest.json",
                "research-lock.json",
                COMPLETE,
            ] {
                assert!(game.join(file).is_file(), "missing {file}");
            }
            if cfg!(windows) {
                assert!(game.join("dxcompiler.dll").is_file());
                assert!(game.join("libgcc_s_seh-1.dll").is_file());
            }
            assert_eq!(fs::read(game.join(RELEASE_FILE)).unwrap(), FACTS);
            assert!(!game.join(Artifact::ARCHIVE_RECEIPT).exists());
            let receipt: serde_json::Value =
                serde_json::from_slice(&fs::read(game.join(Artifact::SDK_RECEIPT)).unwrap())
                    .unwrap();
            assert_eq!(receipt["sdk"]["commit"], COMMIT);
            for (name, bytes) in slices {
                assert_eq!(receipt["sdk"]["slices"][name], sha256_hex(bytes));
            }
            let native = game.with_file_name("native-copy");
            fs::create_dir(&native).unwrap();
            crate::native_mods::copy_base(&game, &native, crate::native_mods::Target::Client, true)
                .unwrap();
            assert!(native.join(crate::preparer::BINARY).is_file());
            assert!(!native.join(Artifact::SDK_RECEIPT).exists());
            fs::write(game.join(library), b"swapped library").unwrap();
            assert!(!download.is_installed(DEV, &proof));
            assert!(crate::managed_play::FirstBuild::installed(DEV).is_err());
        },
    );
}

#[test]
fn player_archive_still_installs_when_the_index_is_listed() {
    isolated_case(
        "sdk_install::player_archive_still_installs_when_the_index_is_listed",
        || {
            let Some(asset) = ASSET else { return };
            let release = offer(TAG, &[INDEX, asset]);
            let download = GameDownload::select(&release).unwrap();
            assert_eq!(download.file, asset);
            let bytes = if asset.ends_with(".zip") {
                let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
                zip.start_file(CLIENT, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(b"static client").unwrap();
                zip.finish().unwrap().into_inner()
            } else {
                let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
                    Vec::new(),
                    flate2::Compression::fast(),
                ));
                let mut header = tar::Header::new_gnu();
                header.set_size(13);
                header.set_mode(0o755);
                tar.append_data(&mut header, CLIENT, b"static client".as_slice())
                    .unwrap();
                tar.into_inner().unwrap().finish().unwrap()
            };
            let mirror = PathBuf::from(std::env::var_os(BASE_VAR).unwrap());
            let (key, _) = SigningKey::generate().unwrap();
            let keys = [key.public()];
            let base = Base::from_env();
            publish(
                &mirror,
                TAG,
                TAG,
                &key,
                b"unused SDK index",
                &[(asset, &bytes)],
            );
            let proof = download
                .prove_with(TAG, &keys, |name| {
                    base.release_file(TAG, TAG, name).map_err(|e| e.to_string())
                })
                .unwrap();
            let (url, server) = serve_archive(asset, bytes);
            download
                .publish_with(
                    TAG,
                    &proof,
                    InstallIntent::Exact,
                    |_| {},
                    |_, _| url.clone(),
                    (&base, &keys),
                )
                .unwrap();
            server.join().unwrap();
            let game = version_dir(TAG);
            assert_eq!(fs::read(game.join(CLIENT)).unwrap(), b"static client");
            assert!(game.join(Artifact::ARCHIVE_RECEIPT).is_file());
            assert!(!game.join(Artifact::SDK_RECEIPT).exists());
        },
    );
}
