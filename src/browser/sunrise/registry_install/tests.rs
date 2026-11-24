use super::*;
use sunrise_launcher_smod::Package;

#[test]
fn a_served_package_comes_on_with_its_host_and_goes_off_when_the_host_is_removed() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path();
    let write = |dir: &str, id: &str, side: &str, extra: &str, files: &[(&str, &[u8])]| {
        let body =
            format!("id='{id}'\nname='{id}'\nversion='1.0.0'\napi='0.2'\nside='{side}'\n{extra}");
        fs::create_dir_all(home.join(dir)).unwrap();
        fs::write(
            home.join(dir).join(format!("{id}.smod")),
            Package::write_files(&body, files).unwrap(),
        )
        .unwrap();
    };
    let native = |serves: bool| {
        format!(
            "[native]\ngame='26.3'\nsunrise='{}'\nserves_content={serves}\n",
            "a".repeat(40)
        )
    };
    let crate_files: &[(&str, &[u8])] = &[
        (
            "Cargo.toml",
            b"[package]\nname='java_host'\nversion='1.0.0'\n",
        ),
        ("src/lib.rs", b"fn host() {}"),
    ];
    write(
        mod_list::MODS_DIR,
        "runes",
        "both",
        "served_by='java_host'\n",
        &[],
    );
    write(
        crate::native_mods::INSTALLED,
        "java_host",
        "both",
        &native(true),
        crate_files,
    );

    // Off host: the served package can't come on alone, but can in one batch with it.
    let alone = check_activation(home, None, &["runes"]).unwrap_err();
    assert!(alone.contains("java_host"), "{alone}");
    check_activation(home, None, &["runes", "java_host"]).unwrap();

    // A native mod that doesn't opt in to serving content is no host.
    write(
        crate::native_mods::INSTALLED,
        "java_host",
        "both",
        &native(false),
        crate_files,
    );
    assert!(check_activation(home, None, &["runes", "java_host"]).is_err());

    // A host must run everywhere the package it serves must: a client-only host can't serve both.
    write(
        crate::native_mods::INSTALLED,
        "java_host",
        "client",
        &native(true),
        crate_files,
    );
    let uncovered = check_activation(home, None, &["runes", "java_host"]).unwrap_err();
    assert!(uncovered.contains("can't serve"), "{uncovered}");

    // Removing the host takes its enabled served package with it.
    write(
        crate::native_mods::INSTALLED,
        "java_host",
        "both",
        &native(true),
        crate_files,
    );
    let present = inventory(home).unwrap();
    let enabled: BTreeSet<String> = ["runes".into(), "java_host".into()].into();
    let gone = mutations::unavailable_with(&present, &enabled, &["java_host".into()].into());
    assert!(gone.contains("runes"), "{gone:?}");
}
