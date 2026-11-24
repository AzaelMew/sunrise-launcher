use super::*;

const BUILD: &str = "dev-2026-11-23-abcdef0";
const LISTED: &str = "26.3-0.1.0";

/// Two projects: a native one whose 1.2.0 fits the stable builds only, and a content mod.
fn index() -> Index {
    serde_json::from_str(include_str!(
        "../../crates/smod/tests/index_v2/two-ports-index.json"
    ))
    .unwrap()
}

fn native(index: &mut Index) -> &mut sunrise_launcher_smod::index_v2::Project {
    index
        .projects
        .iter_mut()
        .find(|p| p.id == "Ab3dE9xQ")
        .unwrap()
}

fn release(of: &Release, version: &str, sha: char) -> Release {
    let mut release = of.clone();
    release.version = version.into();
    release.fits.clear();
    release.file.sha256 = sha.to_string().repeat(64);
    release
}

#[test]
fn a_gap_is_the_newest_live_native_release_the_index_says_nothing_of() {
    let mut index = index();
    let project = native(&mut index);
    let newest = project.releases[0].clone();
    // An older release, and a newer one that was taken down.
    project.releases.push(release(&newest, "1.1.0", 'b'));
    project.releases.push(release(&newest, "1.3.0", 'c'));

    let found: Vec<&str> = gaps(&index, BUILD)
        .iter()
        .map(|r| r.version.as_str())
        .collect();
    assert_eq!(
        found,
        ["1.2.0"],
        "the content mod and the taken-down release are no gaps"
    );
    assert!(gaps(&index, LISTED).is_empty(), "1.2.0 fits {LISTED}");
}

#[test]
fn a_registry_verdict_on_any_release_leaves_no_gap() {
    let mut index = index();
    let project = native(&mut index);
    let newest = project.releases[0].clone();
    let mut older = release(&newest, "1.1.0", 'b');
    older.fits.push(BUILD.into());
    project.releases.push(older);
    assert!(gaps(&index, BUILD).is_empty(), "an older release fits");

    let mut index = self::index();
    native(&mut index).releases[0]
        .does_not_fit
        .push(BUILD.into());
    assert!(
        gaps(&index, BUILD).is_empty(),
        "the newest is known not to fit"
    );
}

#[test]
fn a_local_answer_fills_only_what_the_index_says_nothing_of() {
    let index = index();
    let sha = native(&mut index.clone()).releases[0].file.sha256.clone();
    let answered = |said: Local| move |_: &str| Found::from([(sha.to_ascii_lowercase(), said)]);

    let filled = fill(&index, answered.clone()(Local::DoesNotFit));
    let got = &filled.project("Ab3dE9xQ").unwrap().releases[0];
    assert_eq!(got.fit_verdict(BUILD), FitVerdict::DoesNotFit);
    assert_eq!(
        got.fit_verdict(LISTED),
        FitVerdict::Fits,
        "the index's own verdict wins"
    );

    let filled = fill(&index, answered.clone()(Local::Fits));
    let got = &filled.project("Ab3dE9xQ").unwrap().releases[0];
    assert_eq!(got.fit_verdict(BUILD), FitVerdict::Fits);

    let filled = fill(&index, answered(Local::Unknown));
    let got = &filled.project("Ab3dE9xQ").unwrap().releases[0];
    assert_eq!(got.fit_verdict(BUILD), FitVerdict::Unknown);
}

#[test]
fn only_a_newer_uses_schema_leaves_the_answer_open() {
    let made_for = || "26.3-0.1.0".to_owned();
    let no_list = |why| fit::Verdict::NoList {
        made_for: made_for(),
        why: Some(why),
    };
    assert_eq!(local(&fit::Verdict::MadeFor), Local::Fits);
    assert_eq!(
        local(&fit::Verdict::Fits {
            made_for: made_for()
        }),
        Local::Fits
    );
    assert_eq!(
        local(&no_list(fit::NoListReason::NewerSchema)),
        Local::Unknown
    );
    assert_eq!(
        local(&no_list(fit::NoListReason::OlderTool)),
        Local::DoesNotFit
    );
    let exact = fit::Verdict::Exact {
        made_for: made_for(),
        why: "internals".into(),
    };
    assert_eq!(local(&exact), Local::DoesNotFit);
}
