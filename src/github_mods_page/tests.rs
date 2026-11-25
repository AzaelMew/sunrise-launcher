use super::*;
use sunrise_launcher_smod::{index, Package};

#[test]
fn a_refused_github_source_says_what_it_means_and_keeps_the_checkers_words_for_details() {
    let fit = pasted_source_error(
        "Jade",
        SourceError::DoesNotFit("jade pins build ed3ac19 and carries no list".into()),
    );
    assert!(fit
        .line
        .starts_with("Jade isn't ready for your game version yet."));
    assert_eq!(fit.detail, "jade pins build ed3ac19 and carries no list");
    assert!(!fit.retry);

    let raw = pasted_source_error(
        "Jade",
        SourceError::Unproved("Permission denied: /home/someone/profile".into()),
    );
    assert_eq!(raw.line, "Sunrise couldn't add Jade.");
    assert!(!raw.line.contains("/home/"));

    let offline = pasted_source_error("Jade", SourceError::Github(Failure::Offline("dns".into())));
    assert!(offline.retry);
}

#[test]
fn unreviewed_warning_defaults_to_cancel_and_describes_provenance() {
    assert_eq!(WarningAction::DEFAULT, WarningAction::Cancel);
    assert_ne!(WarningAction::DEFAULT, WarningAction::Confirm);

    let dir = tempfile::tempdir().unwrap();
    let smod = dir.path().join("custom-tweaks-0.1.0.smod");
    let bytes = Package::write_files(
        "id = 'custom_tweaks'\nname = 'Custom Tweaks'\nversion = '0.1.0'\napi = '0.2'\nside = 'both'\n",
        &[],
    )
    .unwrap();
    std::fs::write(&smod, &bytes).unwrap();
    let pkg = github_mods::read_local_smod(&smod).unwrap();

    let target = GithubTarget::fixture(dir.path());
    let unsigned = PendingConfirm::Local {
        target: target.clone(),
        packages: vec![pkg],
        folder: None,
    };
    assert_eq!(unsigned.badge(), ManualBadge::Local);
    assert!(unsigned.unsigned_local_file());
    let prov = unsigned.provenance();
    assert_eq!(prov.headline, tr("Unsigned local file, not reviewed"));
    assert!(prov.detail.contains(&smod.display().to_string()));

    // Verified sidecar attestation shows repository and workflow without calling the mod reviewed.
    let mut verified_pkg = github_mods::read_local_smod(&smod).unwrap();
    verified_pkg.attestation = github_mods::LocalAttestation::Verified {
        repository: "example/custom-tweaks".into(),
        workflow: "example/custom-tweaks/.github/workflows/release.yml@refs/tags/v0.1.0".into(),
    };
    let verified = PendingConfirm::Local {
        target: target.clone(),
        packages: vec![verified_pkg],
        folder: None,
    };
    assert!(!verified.unsigned_local_file());
    let verified_prov = verified.provenance();
    assert_eq!(
        verified_prov.headline,
        tr("Signed local file, not reviewed")
    );
    assert!(verified_prov.detail.contains("example/custom-tweaks"));
    assert!(verified_prov
        .detail
        .contains(".github/workflows/release.yml@refs/tags/v0.1.0"));
    assert!(verified_prov.detail.contains("has not checked"));

    // Failed attestation verification is distinguished from a missing bundle.
    let mut failed_pkg = github_mods::read_local_smod(&smod).unwrap();
    failed_pkg.attestation = github_mods::LocalAttestation::Failed("digest mismatch".into());
    let failed = PendingConfirm::Local {
        target: target.clone(),
        packages: vec![failed_pkg],
        folder: None,
    };
    assert!(failed.unsigned_local_file());
    let failed_prov = failed.provenance();
    assert_eq!(
        failed_prov.headline,
        tr("Attestation check failed, not reviewed")
    );
    assert!(failed_prov.detail.contains("digest mismatch"));

    // GitHub release assets describe the release tag and asset filename rather than a commit SHA.
    let release_input = checked_subset(target.clone());
    let release_confirm = PendingConfirm::Github(GithubInput {
        target: release_input.target.clone(),
        link: release_input.link.clone(),
        source: release_input.source.clone(),
        selection: match &release_input.selection {
            GithubSelection::Add(packages) => GithubSelection::Add(packages.clone()),
            GithubSelection::Source(_) => unreachable!(),
        },
    });
    let release_prov = release_confirm.provenance();
    assert!(
        release_prov.detail.contains("Release v1 (two.smod)"),
        "{}",
        release_prov.detail
    );
    assert!(
        !release_prov.detail.contains("aaaaaaa"),
        "{}",
        release_prov.detail
    );

    let repo_dir = dir.path().join("camp-lanterns");
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::write(
        repo_dir.join("sunrise-mod.toml"),
        "id = 'camp_lanterns'\nname = 'Camp Lanterns'\nversion = '0.4.0'\napi = '0.2'\nside = 'both'\n",
    )
    .unwrap();
    let folder_pkgs = github_mods::read_local_folder(&repo_dir).unwrap();
    let folder = PendingConfirm::Local {
        target,
        packages: folder_pkgs,
        folder: Some(repo_dir),
    };
    assert_eq!(folder.badge(), ManualBadge::FromSource);
    assert!(!folder.unsigned_local_file());
    assert_eq!(
        folder.provenance().headline,
        tr("Local repository folder, not reviewed")
    );
}

#[test]
fn picker_handles_cancellation_errors_and_stale_callbacks() {
    let mut form = Github::new();
    form.stage = Stage::Choose(SourceChoice::LocalFile);

    // User cancellation leaves the stage quietly on Choose.
    let token = form.start_picker(PickerKind::LocalFile);
    assert_eq!(
        form.finish_picker(token, PickerKind::LocalFile, Ok(None)),
        None
    );
    assert!(matches!(form.stage, Stage::Choose(_)));

    // Picker errors surface as a failure instead of quiet cancellation.
    let token = form.start_picker(PickerKind::LocalFile);
    assert_eq!(
        form.finish_picker(
            token,
            PickerKind::LocalFile,
            Err("portal unavailable".into()),
        ),
        None
    );
    assert!(matches!(
        &form.stage,
        Stage::Failed {
            error,
            from_choose: true,
            ..
        } if error.line == tr("Couldn't open the file picker.")
            && error.detail == "portal unavailable"
    ));

    // Stale callbacks after closing, switching stage, or opening a newer picker are ignored.
    form.stage = Stage::Choose(SourceChoice::LocalFile);
    let stale = form.start_picker(PickerKind::LocalFile);
    form.close();
    assert_eq!(
        form.finish_picker(
            stale,
            PickerKind::LocalFile,
            Ok(Some(PathBuf::from("a.smod")))
        ),
        None
    );

    form.stage = Stage::Repository;
    let first = form.start_picker(PickerKind::LocalFolder);
    let second = form.start_picker(PickerKind::LocalFolder);
    assert_eq!(
        form.finish_picker(
            first,
            PickerKind::LocalFolder,
            Ok(Some(PathBuf::from("old")))
        ),
        None
    );
    assert_eq!(
        form.finish_picker(
            second,
            PickerKind::LocalFolder,
            Ok(Some(PathBuf::from("fresh"))),
        ),
        Some(PathBuf::from("fresh"))
    );

    // Callback arriving after stage switched away from Repository is ignored.
    form.stage = Stage::Repository;
    let moved = form.start_picker(PickerKind::LocalFolder);
    form.stage = Stage::Choose(SourceChoice::Repository);
    assert_eq!(
        form.finish_picker(moved, PickerKind::LocalFolder, Err("late error".into())),
        None
    );
    assert!(matches!(form.stage, Stage::Choose(_)));
}

pub(crate) fn checked_subset(target: GithubTarget) -> Arc<GithubInput> {
    let release = "https://github.com/fixture/add/releases/download/v1";
    let archive = |id: &str| {
        let manifest = format!("id='{id}'\nname='{id}'\nversion='1.0.0'\napi='0.2'\nside='both'\ncapabilities=['registries']\n[native]\nsunrise='{}'\nmixins='{}'\ngame='26.3'\n", "b".repeat(40), "c".repeat(40));
        let cargo = format!("[package]\nname='{id}'\nversion='1.0.0'\n");
        Package::write_files(
            &manifest,
            &[("Cargo.toml", cargo.as_bytes()), ("src/lib.rs", b"")],
        )
        .unwrap()
    };
    let packages = [archive("one"), archive("two")];
    let found = Resolved {
        owner: "fixture".into(),
        repo: "add".into(),
        tag: Some("v1".into()),
        commit: "a".repeat(40),
        assets: ["one", "two"]
            .into_iter()
            .zip(&packages)
            .map(|(id, bytes)| github_mods::Asset {
                name: format!("{id}.smod"),
                url: format!("{release}/{id}.smod"),
                sha256: Some(index::sha256(bytes)),
            })
            .collect(),
    };
    let checked = github_mods::fetch_with(&found, |url| {
        let i = found
            .assets
            .iter()
            .position(|asset| asset.url == url)
            .unwrap();
        Ok(packages[i].clone())
    })
    .unwrap();
    let mut pick = Pick::new(PickInput::Assets(found, checked));
    assert_eq!(pick.selected.len(), 2);
    pick.toggle("one");
    Arc::new(pick.checked(target, "https://github.com/fixture/add".into()))
}
