use super::*;
use serde_json::json;

const FILE: &str = "sunrise-mods/ruby.smod";
const OWNER: &str = "Other owner";

fn assert_missing(path: &Path, case: &str) {
    assert!(
        path.symlink_metadata()
            .is_err_and(|e| e.kind() == ErrorKind::NotFound),
        "{case}: expected {} to be absent",
        path.display()
    );
}

struct Recovery {
    root: tempfile::TempDir,
    home: std::path::PathBuf,
    old: Value,
    new: Value,
    journal: Vec<u8>,
}

impl Recovery {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        fs::create_dir(&home).unwrap();
        let row = |version, bytes| {
            json!({
                "source": "sunrise", "project": "ruby", "title": "Ruby", "kind": "mod",
                "file": FILE, "version": version,
                "checksum": format!("SHA-256:{}", super::super::sha256(bytes)),
            })
        };
        let (old, new) = (row("1", b"old"), row("2", b"new"));
        let journal = serde_json::to_vec(&json!({
            "id": "ruby", "file": FILE, "old": old, "new": new,
        }))
        .unwrap();
        fs::write(home.join(PENDING), &journal).unwrap();
        Self {
            root,
            home,
            old,
            new,
            journal,
        }
    }

    fn archive(&self, bytes: &[u8]) {
        let target = self.home.join(FILE);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }

    fn manifest(&self, rows: &[&Value]) -> Vec<u8> {
        let bytes = serde_json::to_vec(&json!({"entries": rows, "opaque": [3, 1]})).unwrap();
        fs::write(self.home.join(MANIFEST), &bytes).unwrap();
        bytes
    }

    fn assert_pending(&self, case: &str) {
        let journal = fs::read(self.home.join(PENDING)).expect(case);
        assert_eq!(journal, self.journal, "{case}");
    }

    fn assert_archive(&self, case: &str, expected: Option<&[u8]>) {
        let path = self.home.join(FILE);
        if let Some(bytes) = expected {
            assert!(path.symlink_metadata().unwrap().is_file(), "{case}");
            assert_eq!(fs::read(path).unwrap(), bytes, "{case}");
        } else {
            assert_missing(&path, case);
        }
    }

    fn check_claim(
        &self,
        case: &str,
        current: &Value,
        file: &str,
        archive: Option<&[u8]>,
        outcome: Outcome,
    ) {
        // Same project id on a different source still participates in the check.
        let claim = json!({"source": "future-source", "project": "ruby", "title": OWNER, "file": file, "extra": 25});
        let before = self.manifest(&[current, &claim]);
        let result = reconcile(&self.home);
        self.assert_archive(case, archive);
        let manifest = fs::read(self.home.join(MANIFEST)).unwrap();
        match outcome {
            Outcome::Separate => {
                result.unwrap_or_else(|error| panic!("{case}: {file}: {error}"));
                assert_missing(&self.home.join(PENDING), case);
                if archive == Some(b"new") && current == &self.old {
                    assert_eq!(
                        serde_json::from_slice::<Value>(&manifest).unwrap(),
                        json!({"entries": [claim, self.new], "opaque": [3, 1]}),
                        "{case}"
                    );
                } else {
                    assert_eq!(manifest, before, "{case}");
                }
            }
            _ => {
                let error = result.expect_err(case);
                let RecoveryProblem::Claim { problem, .. } = &error else {
                    panic!("{case}: {error}");
                };
                let wording = match (&outcome, problem) {
                    (
                        Outcome::Conflict | Outcome::Possible | Outcome::TrailingAlias,
                        ClaimProblem::Conflict {
                            file,
                            row,
                            possible,
                        },
                    ) => {
                        assert_eq!(row, OWNER, "{case}");
                        assert_eq!(file, FILE, "{case}");
                        match outcome {
                            Outcome::Conflict => assert!(!possible, "{case}"),
                            // Win32 may resolve a padded name to the regular leaf.
                            Outcome::TrailingAlias if cfg!(windows) => (),
                            _ => assert!(*possible, "{case}"),
                        }
                        if *possible {
                            format!("{FILE} may conflict with {OWNER}.")
                        } else {
                            format!("{FILE} belongs to {OWNER}: remove it first.")
                        }
                    }
                    (
                        Outcome::Unknown,
                        ClaimProblem::Lookup {
                            row, file: claimed, ..
                        },
                    ) => {
                        assert_eq!(row, OWNER, "{case}");
                        assert_eq!(claimed, file, "{case}");
                        format!("Couldn't check the file {file} claimed by {OWNER}:")
                    }
                    _ => panic!("{case}: {error}"),
                };
                assert!(error.to_string().contains(&wording), "{case}: {error}");
                self.assert_pending(case);
                assert_eq!(manifest, before, "{case}");
            }
        }
    }
}

enum Outcome {
    Separate,
    Conflict,
    Possible,
    TrailingAlias,
    Unknown,
}

#[test]
fn claims_match_the_game_scenario_table() {
    use Outcome::*;
    enum Layout {
        NeitherDir,
        LowercaseDir,
        RubyPresent,
        Resourcepacks,
    }
    use Layout::*;

    #[rustfmt::skip]
    let cases = [
        ("existing folded leaf",           "sunrise-mods/RUBY.smod",                 RubyPresent,   Conflict),
        ("different lookup depths",        "SUNRISE-MODS/RUBY.smod",                 LowercaseDir,  Conflict),
        ("both paths missing",             "SUNRISE-MODS/RUBY.smod",                 NeitherDir,    Conflict),
        ("parent normalization",          "sunrise-mods/../sunrise-mods/ruby.smod",  RubyPresent,   Conflict),
        ("empty and dot in another claim", "./sunrise-mods//ruby.smod",              RubyPresent,   Conflict),
        ("trailing dot",                   "sunrise-mods/ruby.smod.",                RubyPresent,   TrailingAlias),
        ("trailing space",                 "sunrise-mods/ruby.smod ",                RubyPresent,   TrailingAlias),
        ("uncertain non-ASCII leaf",       "sunrise-mods/rubý.smod",                 LowercaseDir,  Possible),
        ("uncertain short name",           "sunrise-mods/RUBY~1.SMO",                LowercaseDir,  Possible),
        ("known difference first",        "resourcepacks/déjà.zip",                 NeitherDir,    Separate),
        ("distinct parent identities",    "resourcepacks/missing.zip. ",            Resourcepacks, Separate),
        ("different leaf in same parent", "./sunrise-mods/../sunrise-mods/gem.smod", RubyPresent,  Separate),
        ("claim needs Ruby as a dir",      "sunrise-mods/ruby.smod/other.smod",      LowercaseDir,  Conflict),
    ];
    for (case, claim, layout, outcome) in cases {
        let fixture = Recovery::new();
        let archive: Option<&[u8]> = match layout {
            NeitherDir => None,
            LowercaseDir => {
                fs::create_dir(fixture.home.join("sunrise-mods")).unwrap();
                None
            }
            RubyPresent => {
                fixture.archive(b"new");
                Some(b"new")
            }
            Resourcepacks => {
                fixture.archive(b"new");
                fs::create_dir(fixture.home.join("resourcepacks")).unwrap();
                Some(b"new")
            }
        };
        fixture.check_claim(case, &fixture.old, claim, archive, outcome);
    }
}

#[test]
fn ownership_is_checked_even_without_new_bytes_or_with_an_already_new_row() {
    for (case, bytes, already_new) in [
        ("missing target", None, false),
        ("old target bytes", Some(&b"old"[..]), false),
        ("unrelated target bytes", Some(&b"bad"[..]), false),
        ("already-new row and new bytes", Some(&b"new"[..]), true),
        ("already-new row and missing archive", None, true),
        ("already-new row and old bytes", Some(&b"old"[..]), true),
    ] {
        let fixture = Recovery::new();
        if let Some(bytes) = bytes {
            fixture.archive(bytes);
        }
        let current = if already_new {
            &fixture.new
        } else {
            &fixture.old
        };
        fixture.check_claim(case, current, FILE, bytes, Outcome::Conflict);
        fixture.check_claim(case, current, "gem.smod", bytes, Outcome::Separate);
    }
}

#[test]
fn unusable_manifest_keeps_intent_even_when_target_bytes_do_not_match() {
    for bytes in [None, Some(&b"old"[..])] {
        let fixture = Recovery::new();
        if let Some(bytes) = bytes {
            fixture.archive(bytes);
        }
        let manifest = fixture.home.join(MANIFEST);
        for invalid in [b"{oops".as_slice(), br#"{"entries":{}}"#] {
            fs::write(&manifest, invalid).unwrap();
            let error = reconcile(&fixture.home).unwrap_err().to_string();
            assert!(error.contains(MANIFEST), "{error}");
            assert_eq!(fs::read(&manifest).unwrap(), invalid);
            fixture.assert_pending("malformed manifest");
            fixture.assert_archive("malformed manifest", bytes);
        }
        // Use a directory, not permissions which a privileged test runner can bypass.
        fs::remove_file(&manifest).unwrap();
        fs::create_dir(&manifest).unwrap();
        fs::write(manifest.join("marker"), b"unchanged").unwrap();
        assert!(fs::read(&manifest).is_err());
        let error = reconcile(&fixture.home).unwrap_err().to_string();
        assert!(error.contains(MANIFEST), "{error}");
        assert!(manifest.symlink_metadata().unwrap().is_dir());
        assert_eq!(fs::read(manifest.join("marker")).unwrap(), b"unchanged");
        assert_eq!(fs::read_dir(&manifest).unwrap().count(), 1);
        fixture.assert_pending("unreadable manifest");
        fixture.assert_archive("unreadable manifest", bytes);
    }
}

#[test]
fn destination_refuses_nonportable_spellings_on_every_os() {
    let fixture = Recovery::new();
    let absolute = fixture.home.join(FILE);
    for file in [
        "",
        "sunrise-mods/ruby.smod.",
        "sunrise-mods/ruby.smod ",
        "sunrise-mods/cOn.smod",
        "sunrise-mods/CONOUT$.smod",
        "sunrise-mods/aux .smod",
        "CON/sunrise-mods/ruby.smod",
        "sunrise-mods/CoM¹.smod",
        "sunrise-mods/LPT9.smod",
        "sunrise-mods/LPT³.smod",
        "sunrise-mods//ruby.smod",
        "sunrise-mods/ruby.smod/",
        "sunrise-mods/.",
        "sunrise-mods/..",
        "../sunrise-mods/ruby.smod",
        "sunrise-mods/../../ruby.smod",
        "/sunrise-mods/ruby.smod",
        absolute.to_str().unwrap(),
        "C:/sunrise-mods/ruby.smod",
        "C:ruby.smod",
        "sunrise-mods/rub?y.smod",
        "sunrise-mods/rub\u{001f}y.smod",
        #[cfg(unix)]
        r"sunrise-mods\ruby.smod",
        #[cfg(windows)]
        r"sunrise-mods\\ruby.smod",
    ] {
        assert!(
            super::claims::check_destination(&fixture.home, file, &[]).is_err(),
            "accepted {file:?}"
        );
    }
    for file in [
        FILE,
        "./sunrise-mods/ruby.smod",
        "sunrise-mods/../sunrise-mods/ruby.smod",
        "sunrise-mods/COM0.smod",
        #[cfg(windows)]
        r"sunrise-mods\ruby.smod",
    ] {
        super::claims::check_destination(&fixture.home, file, &[])
            .unwrap_or_else(|error| panic!("refused {file:?}: {error}"));
    }
    fs::create_dir_all(fixture.home.join(FILE)).unwrap();
    assert!(super::claims::check_destination(&fixture.home, FILE, &[]).is_err());
}

// CI can run on a Windows account without Developer Mode / SeCreateSymbolicLinkPrivilege,
// or a filesystem without link support. Skip only these explicit setup limitations.
#[cfg(any(unix, windows))]
fn link_available(result: io::Result<()>, case: &str) -> bool {
    match result {
        Ok(()) => true,
        Err(e)
            if e.kind() == ErrorKind::Unsupported
                || cfg!(windows) && e.raw_os_error() == Some(1314) =>
        {
            eprintln!("skipping {case}: cannot set up link: {e}");
            false
        }
        Err(e) => panic!("{case}: cannot set up link: {e}"),
    }
}

#[cfg(any(unix, windows))]
fn symlink(target: &Path, path: &Path, directory: bool) -> bool {
    #[cfg(unix)]
    let result = {
        let _ = directory;
        std::os::unix::fs::symlink(target, path)
    };
    #[cfg(windows)]
    let result = if directory {
        std::os::windows::fs::symlink_dir(target, path)
    } else {
        std::os::windows::fs::symlink_file(target, path)
    };
    link_available(result, &path.display().to_string())
}

#[test]
#[cfg(any(unix, windows))]
fn a_directory_alias_conflicts_even_when_ruby_is_absent() {
    let fixture = Recovery::new();
    let mods = fixture.home.join("sunrise-mods");
    fs::create_dir(&mods).unwrap();
    if !symlink(&mods, &fixture.home.join("alias"), true) {
        return;
    }
    fixture.check_claim(
        "same native parent",
        &fixture.old,
        "alias/ruby.smod",
        None,
        Outcome::Conflict,
    );
}

#[test]
#[cfg(any(unix, windows))]
fn a_hard_link_conflicts_by_existing_file_identity() {
    let fixture = Recovery::new();
    fixture.archive(b"new");
    let gem = fixture.home.join("gem.smod");
    if !link_available(
        fs::hard_link(fixture.home.join(FILE), &gem),
        "hard-link claim",
    ) {
        return;
    }
    fixture.check_claim(
        "same file despite different names",
        &fixture.old,
        "gem.smod",
        Some(b"new"),
        Outcome::Conflict,
    );
    assert_eq!(fs::read(gem).unwrap(), b"new");
}

#[test]
#[cfg(any(unix, windows))]
fn a_claim_leaf_link_is_followed_but_a_dangling_link_refuses() {
    let fixture = Recovery::new();
    fixture.archive(b"new");
    let target = fixture.home.join(FILE);
    let alias = fixture.home.join("gem.smod");
    if !symlink(&target, &alias, false) {
        return;
    }
    fixture.check_claim(
        "claim leaf resolves to Ruby",
        &fixture.old,
        "gem.smod",
        Some(b"new"),
        Outcome::Conflict,
    );
    fs::remove_file(target).unwrap();
    fixture.check_claim(
        "claim leaf's destination is unknown",
        &fixture.old,
        "gem.smod",
        None,
        Outcome::Unknown,
    );
    assert!(alias.symlink_metadata().unwrap().is_symlink());
}

#[test]
#[cfg(any(unix, windows))]
fn destination_allows_an_external_directory_link_but_not_a_leaf_link() {
    let fixture = Recovery::new();
    let external = fixture.root.path().join("external");
    fs::create_dir(&external).unwrap();
    if !symlink(&external, &fixture.home.join("sunrise-mods"), true) {
        return;
    }
    let physical = external.join("ruby.smod");
    fs::write(&physical, b"new").unwrap();
    fixture.manifest(&[&fixture.old]);
    reconcile(&fixture.home).unwrap();
    assert_eq!(fs::read(&physical).unwrap(), b"new");
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(fixture.home.join(MANIFEST)).unwrap()).unwrap(),
        json!({"entries": [fixture.new], "opaque": [3, 1]})
    );
    assert_missing(&fixture.home.join(PENDING), "external directory link");

    fs::remove_file(&physical).unwrap();
    let link_target = external.join("other.smod");
    fs::write(&link_target, b"old").unwrap();
    if !symlink(&link_target, &physical, false) {
        return;
    }
    let old_link = fs::read_link(&physical).unwrap();
    let before = fixture.manifest(&[&fixture.old]);
    fs::write(fixture.home.join(PENDING), &fixture.journal).unwrap();
    assert!(super::claims::check_destination(&fixture.home, FILE, &[]).is_err());
    assert!(reconcile(&fixture.home).is_err());
    fixture.assert_pending("own leaf link");
    assert_eq!(fs::read(fixture.home.join(MANIFEST)).unwrap(), before);
    assert!(physical.symlink_metadata().unwrap().is_symlink());
    assert_eq!(fs::read_link(physical).unwrap(), old_link);
    assert_eq!(fs::read(link_target).unwrap(), b"old");
}

#[test]
#[cfg(any(unix, windows))]
fn parent_after_a_resolved_directory_link_uses_only_its_native_location() {
    let fixture = Recovery::new();
    fixture.archive(b"new");
    let external = fixture.root.path().join("external");
    fs::create_dir_all(external.join("child")).unwrap();
    fs::create_dir(external.join("sunrise-mods")).unwrap();
    let disjoint = external.join(FILE);
    fs::write(&disjoint, b"other").unwrap();
    if !symlink(&external.join("child"), &fixture.home.join("alias"), true) {
        return;
    }
    fixture.check_claim(
        "lexical pop is a false conflict",
        &fixture.old,
        "alias/../sunrise-mods/ruby.smod",
        Some(b"new"),
        Outcome::Separate,
    );
    assert_eq!(fs::read(disjoint).unwrap(), b"other");

    let child = fixture.home.join("sunrise-mods/child");
    fs::create_dir(&child).unwrap();
    fs::create_dir(fixture.home.join("other")).unwrap();
    if !symlink(&child, &fixture.home.join("other/alias"), true) {
        return;
    }
    fs::write(fixture.home.join(PENDING), &fixture.journal).unwrap();
    fixture.check_claim(
        "lexical pop hides a native conflict",
        &fixture.old,
        "other/alias/../ruby.smod",
        Some(b"new"),
        Outcome::Conflict,
    );
}
