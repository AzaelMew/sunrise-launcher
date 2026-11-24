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

/// The versions of the releases a check of `build` would read.
fn gap_versions<'a>(index: &'a Index, build: &str) -> Vec<&'a str> {
    gaps(index, build)
        .into_iter()
        .map(|r| r.version.as_str())
        .collect()
}

fn release(of: &Release, version: &str, sha: char) -> Release {
    let mut release = of.clone();
    release.version = version.into();
    release.fits.clear();
    release.file.sha256 = sha.to_string().repeat(64);
    release
}

#[test]
fn a_gap_is_the_newest_live_mod_release_the_index_says_nothing_of() {
    let mut index = index();
    let project = native(&mut index);
    let newest = project.releases[0].clone();
    // An older release, and a newer one that was taken down.
    project.releases.push(release(&newest, "1.1.0", 'b'));
    project.releases.push(release(&newest, "1.3.0", 'c'));

    assert_eq!(
        gap_versions(&index, BUILD),
        ["1.2.0", "2.0.0-beta.1"],
        "the native mod's newest live release and the content mod's are gaps"
    );
    assert_eq!(
        gap_versions(&index, LISTED),
        ["2.0.0-beta.1"],
        "1.2.0 fits {LISTED}"
    );
}

#[test]
fn a_registry_verdict_on_any_release_leaves_no_gap() {
    let mut index = index();
    let project = native(&mut index);
    let newest = project.releases[0].clone();
    let mut older = release(&newest, "1.1.0", 'b');
    older.fits.push(BUILD.into());
    project.releases.push(older);
    assert_eq!(
        gap_versions(&index, BUILD),
        ["2.0.0-beta.1"],
        "an older release fits"
    );

    let mut index = self::index();
    native(&mut index).releases[0]
        .does_not_fit
        .push(BUILD.into());
    assert_eq!(
        gap_versions(&index, BUILD),
        ["2.0.0-beta.1"],
        "the newest is known not to fit"
    );
}

#[test]
fn a_local_answer_fills_only_what_the_index_says_nothing_of() {
    let index = index();
    let sha = native(&mut index.clone()).releases[0].file.sha256.clone();
    let answered = |said: Local| move |_: &str| Found::from([(sha.to_ascii_lowercase(), said)]);

    let filled = fill(&index, Vec::new(), answered.clone()(Local::DoesNotFit));
    let got = &filled.project("Ab3dE9xQ").unwrap().releases[0];
    assert_eq!(got.fit_verdict(BUILD), FitVerdict::DoesNotFit);
    assert_eq!(
        got.fit_verdict(LISTED),
        FitVerdict::Fits,
        "the index's own verdict wins"
    );

    let filled = fill(&index, Vec::new(), answered.clone()(Local::Fits));
    let got = &filled.project("Ab3dE9xQ").unwrap().releases[0];
    assert_eq!(got.fit_verdict(BUILD), FitVerdict::Fits);

    let filled = fill(&index, Vec::new(), answered(Local::Unknown));
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

/// The local answers for build id `commit`: the native release does not fit, the content mod
/// fits.
fn answers(index: &Index, commit: &'static str) -> impl Fn(&str) -> Found {
    let native = index.project("Ab3dE9xQ").unwrap().releases[0]
        .file
        .sha256
        .clone();
    let content = index.project("Zz91kLmN").unwrap().releases[0]
        .file
        .sha256
        .clone();
    move |asked: &str| {
        if asked != commit {
            return Found::new();
        }
        Found::from([
            (native.to_ascii_lowercase(), Local::DoesNotFit),
            (content.to_ascii_lowercase(), Local::Fits),
        ])
    }
}

#[test]
fn an_unlisted_build_is_checked_here_until_the_registry_lists_it() {
    let listed = index();
    let commit = "abcdef0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut unlisted = listed.clone();
    unlisted.builds.retain(|b| b.id != BUILD);

    // Before the registry lists the build, every live mod's newest release is checked here, and
    // the answers kept under the build id its release names fill the index.
    assert_eq!(gap_versions(&unlisted, BUILD), ["1.2.0", "2.0.0-beta.1"]);
    let filled = fill(
        &unlisted,
        vec![(BUILD.into(), commit.into())],
        answers(&unlisted, commit),
    );
    let verdict =
        |index: &Index, id: &str| index.project(id).unwrap().releases[0].fit_verdict(BUILD);
    assert_eq!(verdict(&filled, "Ab3dE9xQ"), FitVerdict::DoesNotFit);
    assert_eq!(verdict(&filled, "Zz91kLmN"), FitVerdict::Fits);
    let nothing_kept = fill(&unlisted, Vec::new(), answers(&unlisted, commit));
    assert_eq!(
        verdict(&nothing_kept, "Ab3dE9xQ"),
        FitVerdict::Unknown,
        "an unlisted build whose id was never read has no answers"
    );

    // Then the registry lists it, judging every release: its verdicts replace this computer's,
    // and there is nothing left to check.
    let mut judged = listed;
    judged.projects[0].releases[0].fits.push(BUILD.into());
    judged.projects[1].releases[0]
        .does_not_fit
        .push(BUILD.into());
    assert!(gaps(&judged, BUILD).is_empty());
    let filled = fill(
        &judged,
        vec![(BUILD.into(), commit.into())],
        answers(&judged, commit),
    );
    assert_eq!(verdict(&filled, "Ab3dE9xQ"), FitVerdict::Fits);
    assert_eq!(verdict(&filled, "Zz91kLmN"), FitVerdict::DoesNotFit);
}

#[test]
fn a_build_the_registry_already_judged_is_not_checked_here() {
    let mut index = index();
    let commit = "abcdef0aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    index.projects[0].releases[0].fits.push(BUILD.into());
    index.projects[1].releases[0]
        .does_not_fit
        .push(BUILD.into());
    assert!(gaps(&index, BUILD).is_empty(), "no check runs");

    // Answers this computer kept earlier say the opposite: the registry's verdicts stand.
    let filled = fill(&index, Vec::new(), answers(&index, commit));
    let verdict = |id: &str| filled.project(id).unwrap().releases[0].fit_verdict(BUILD);
    assert_eq!(verdict("Ab3dE9xQ"), FitVerdict::Fits);
    assert_eq!(verdict("Zz91kLmN"), FitVerdict::DoesNotFit);
}
