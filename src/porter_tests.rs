use super::*;
use std::cell::RefCell;

/// A porter that answers each command with the reply in `<dir>/<command>.json` (exit 2 when there is none).
#[cfg(unix)]
fn fake(dir: &Path) -> Porter {
    use std::os::unix::fs::PermissionsExt;
    let program = dir.join(PROGRAM);
    let script = format!(
        "#!/bin/sh\nreply=\"{}/$1.json\"\n[ -f \"$reply\" ] || exit 2\necho '{{\"message\": \"working on '\"$1\"'\"}}'\necho 'not json'\ncat \"$reply\"\necho\ngrep -q '\"error\"' \"$reply\" && exit 1\nexit 0\n",
        dir.display()
    );
    fs::write(&program, script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    Porter::in_dir(dir, None).unwrap()
}

#[cfg(unix)]
#[test]
#[ignore = "daily"]
fn a_porter_run_logs_its_lines_keeps_its_status_and_says_what_failed() {
    let dir = std::env::temp_dir().join(format!("sunrise-launcher-porter-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let work = dir.join("work");
    fs::create_dir_all(&work).unwrap();
    let porter = fake(&dir);
    let reply = |command: &str, value: Value| {
        fs::write(dir.join(format!("{command}.json")), value.to_string()).unwrap();
    };
    reply(
        "report",
        serde_json::json!({"format": 1, "pack": {"name": "Cozy"}, "portable": 3,
            "rows": [{"key": "a", "status": "partial", "waiting": 1}], "mods": ["/m/a"],
            "a key from a newer porter": true}),
    );
    let logged = RefCell::new(Vec::new());
    let status = porter
        .report(&work, &|line| logged.borrow_mut().push(line))
        .unwrap();
    assert_eq!(logged.into_inner(), ["working on report"]);
    assert_eq!((status.pack.name.as_str(), status.portable), ("Cozy", 3));
    assert_eq!(status.rows[0].status, Status::Partial);
    let last = kept(&work).expect("the reply is kept");
    assert_eq!(last.mods, [PathBuf::from("/m/a")]);

    // A porter from before `profile` leaves the profile alone; one without --json can't import.
    porter.profile(&work, &|_| {}).unwrap();
    assert!(porter
        .inventory(&work, &|_| {})
        .unwrap_err()
        .contains("too old"));

    reply(
        "inventory",
        serde_json::json!({"format": 1, "error": {"kind": "inventory", "message": "pack.json: missing"}}),
    );
    assert_eq!(
        porter.inventory(&work, &|_| {}).unwrap_err(),
        "pack.json: missing"
    );
    reply("status", serde_json::json!({"format": 2, "pack": {}}));
    assert!(porter
        .status(&work)
        .unwrap_err()
        .contains("update the launcher"));
    assert_eq!(
        kept(&work).unwrap().portable,
        3,
        "a failed run keeps the last status"
    );
    fs::remove_dir_all(&dir).unwrap();
}
