//! Old pack records remain readable; all routes that would write reject the request first.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[test]
fn legacy_modpack_writers_refuse_in_cli_and_mcp_without_touching_the_pack() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data = root.join("launcher");
    let game = root.join("game");
    let packs = game.join("sunrise-packs");
    let work = packs.join("cozy");
    let save = work.join("home/saves/Quiet World");
    fs::create_dir_all(&save).unwrap();
    for (path, bytes) in [
        (work.join("pack.json"), br#"{"id":"cozy","name":"Cozy","version":"0.9.0","sha256":"0000","mods":["coords_hud 0.1.0"]}"#.as_slice()),
        (save.join("level.dat"), b"existing world"),
        (work.join("home/options.txt"), b"existing options"),
        (work.join("cozy-0.9.0.spack"), b"existing pack"),
    ] {
        fs::write(path, bytes).unwrap();
    }
    let enabled = work
        .join("home")
        .join(sunrise_launcher_smod::mod_list::FILE);
    fs::write(enabled, b"existing choices").unwrap();
    fs::create_dir_all(&data).unwrap();
    let mut index: Value =
        serde_json::from_str(include_str!("../crates/smod/tests/index/index-v1.json")).unwrap();
    let projects = index["projects"].as_array_mut().unwrap();
    let mut fresh = projects.iter().find(|p| p["id"] == "cozy").unwrap().clone();
    fresh["id"] = json!("fresh");
    fresh["name"] = json!("Fresh");
    projects.push(fresh);
    let index_path = root.join("index.json");
    fs::write(&index_path, serde_json::to_vec(&index).unwrap()).unwrap();
    let launcher = |args: &[&str]| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_sunrise-launcher"));
        cmd.args(args)
            .env("SUNRISE_HOME", &game)
            .env("SUNRISE_LAUNCHER_HOME", &data)
            .env("SUNRISE_LAUNCHER_LOCATION_FILE", root.join("location.json"))
            .env("SUNRISE_MOD_INDEX", &index_path)
            .env("SUNRISE_LAUNCHER_NO_SELF_UPDATE", "1")
            .env("SUNRISE_NO_KEYCHAIN", "1")
            .env("SUNRISE_TELEMETRY_URL", "off");
        cmd
    };
    let listed = launcher(&["--json", "modpack", "list"]).output().unwrap();
    assert!(listed.status.success(), "{listed:?}");
    let row: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(row["pack"], "cozy");
    assert_eq!(row["version"], "0.9.0");

    const INSTALL: &str = "Installing and updating Sunrise modpacks is paused.";
    const PLAY: &str = "Playing Sunrise modpacks is paused.";
    const REMOVE: &str = "Removing Sunrise modpacks is paused.";
    let before = bytes_in(&game);
    for (args, expected) in [
        (
            &["mods", "install", "fresh", "--source", "sunrise"][..],
            INSTALL,
        ),
        (&["mods", "install", "cozy", "--source", "sunrise"], INSTALL),
        (&["modpack", "play", "cozy"], PLAY),
        (&["play", "--instance", "pack:cozy"], PLAY),
        (&["modpack", "remove", "cozy"], REMOVE),
        (&["instance", "delete", "pack:cozy"], REMOVE),
    ] {
        let out = launcher(args).output().unwrap();
        assert!(!out.status.success(), "{args:?}: {out:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(expected),
            "{args:?}: {out:?}"
        );
        assert_eq!(bytes_in(&game), before, "{args:?}");
    }

    let mut mcp = launcher(&["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = mcp.stdin.take().unwrap();
    let mut output = BufReader::new(mcp.stdout.take().unwrap());
    let mut rpc = |id, method, params| {
        writeln!(
            input,
            "{}",
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
        )
        .unwrap();
        let mut line = String::new();
        assert_ne!(output.read_line(&mut line).unwrap(), 0);
        let result: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(result["id"], id);
        result["result"].clone()
    };
    rpc(1, "initialize", json!({}));
    for (i, (name, arguments, expected)) in [
        (
            "mods_install",
            json!({"project":"cozy","source":"sunrise"}),
            INSTALL,
        ),
        ("modpack_play", json!({"pack":"cozy"}), PLAY),
        ("play", json!({"instance":"pack:cozy"}), PLAY),
        (
            "modpack_remove",
            json!({"pack":"cozy","confirm":true}),
            REMOVE,
        ),
        (
            "instance_delete",
            json!({"instance":"pack:cozy","confirm":true}),
            REMOVE,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let answer = rpc(
            i + 2,
            "tools/call",
            json!({"name":name,"arguments":arguments}),
        );
        assert_eq!(answer["isError"], true, "{answer}");
        assert!(
            answer["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains(expected),
            "{name}: {answer}"
        );
        assert_eq!(bytes_in(&game), before, "{name}");
    }
    drop(input);
    assert!(mcp.wait().unwrap().success());
}

fn bytes_in(dir: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .flat_map(|path| {
            if path.is_dir() {
                let mut rows = bytes_in(&path);
                rows.insert(path, None);
                rows
            } else {
                let data = fs::read(&path).unwrap();
                BTreeMap::from([(path, Some(data))])
            }
        })
        .collect()
}
