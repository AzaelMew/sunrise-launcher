use super::*;

fn project(
    id: &str,
    mod_id: &str,
    name: &str,
    repo: (u64, &str),
    version: &str,
    fit: (&[&str], &[&str]),
) -> index_v2::Project {
    let (repo_id, repo_name) = repo;
    let (fits, does_not_fit) = fit;
    let slug = repo_name.rsplit('/').next().unwrap_or(mod_id);
    index_v2::Project {
        id: id.into(),
        slug: slug.into(),
        mod_id: mod_id.into(),
        kind: index_v2::Kind::Mod,
        name: name.into(),
        summary: format!("{name} from {repo_name}"),
        description: String::new(),
        authors: vec!["Author".into()],
        owners: vec!["owner".into()],
        license: "MIT".into(),
        homepage: None,
        issues: None,
        source: None,
        icon: format!("pages/{id}/{}.png", "a".repeat(64)),
        screenshots: vec![],
        categories: vec!["utility".into()],
        repository: index_v2::Repository {
            id: repo_id,
            name: repo_name.into(),
            path: ".".into(),
        },
        workflow: "sunrise-org/sunrise-mods/.github/workflows/release.yml".into(),
        releases: vec![index_v2::Release {
            version: version.into(),
            published: "2026-11-22".into(),
            sunrise: "26.3-0.1.0".into(),
            workflow: "sunrise-org/sunrise-mods/.github/workflows/release.yml@refs/tags/release-v1"
                .into(),
            repository: None,
            fits: fits.iter().map(|s| (*s).to_owned()).collect(),
            does_not_fit: does_not_fit.iter().map(|s| (*s).to_owned()).collect(),
            jar_unavailable: Vec::new(),
            dependency_unavailable: Vec::new(),
            file: index_v2::File {
                url: format!("https://example.invalid/{slug}-{version}.smod"),
                size: 128,
                sha256: "b".repeat(64),
                attestation: format!("https://example.invalid/{slug}-{version}.sigstore.json"),
            },
            dependencies: vec![],
        }],
    }
}

fn index_with(projects: Vec<index_v2::Project>) -> index_v2::Index {
    index_v2::Index {
        schema: index_v2::SCHEMA,
        serial: 1,
        generated: "2026-11-22T00:00:00Z".into(),
        builds: vec![],
        categories: vec![index_v2::Category {
            id: "utility".into(),
            icon: "wrench".into(),
            kinds: vec![index_v2::Kind::Mod],
        }],
        projects,
        revoked: vec![],
        collections: Vec::new(),
    }
}

#[test]
fn group_ports_groups_competing_ports_by_mod_id_with_distinct_publishers() {
    let index = index_with(vec![
        project(
            "jade0001",
            "jade",
            "Jade",
            (10, "joeypc6/sunrise-jade"),
            "26.3.8",
            (&["26.3-0.1.0"], &[]),
        ),
        project(
            "jade0002",
            "jade",
            "Jade",
            (20, "zekrenlight/jade-sunrise"),
            "1.4.2",
            (&["26.3-0.1.0"], &[]),
        ),
        project(
            "solo0001",
            "frontier",
            "Frontier",
            (30, "Zekrenlight/frontier"),
            "1.2.0",
            (&["26.3-0.1.0"], &[]),
        ),
    ]);

    let groups = group_ports(&index, Some("26.3-0.1.0"), &BTreeMap::new(), "", None, true);
    assert_eq!(groups.len(), 1);
    let jade = &groups[0];
    assert_eq!(jade.mod_id, "jade");
    assert_eq!(jade.display_name, "Jade");
    assert_eq!(jade.ports.len(), 2);
    assert_eq!(jade.ports[0].publisher_repo, "joeypc6/sunrise-jade");
    assert_eq!(jade.ports[1].publisher_repo, "zekrenlight/jade-sunrise");
    // Each competing port has its own Install action; never one shared install.
    assert_eq!(jade.ports[0].action, PortAction::Install);
    assert_eq!(jade.ports[1].action, PortAction::Install);
    assert_ne!(jade.ports[0].project_id, jade.ports[1].project_id);

    // Single-project mods stay out of competing-port groups.
    let grouped = grouped_project_ids(&groups);
    assert!(grouped.contains("jade0001"));
    assert!(grouped.contains("jade0002"));
    assert!(!grouped.contains("solo0001"));
}

#[test]
fn group_ports_marks_installed_port_and_offers_switch_on_alternatives() {
    let index = index_with(vec![
        project(
            "jade0001",
            "jade",
            "Jade",
            (10, "joeypc6/sunrise-jade"),
            "26.3.8",
            (&["26.3-0.1.0"], &[]),
        ),
        project(
            "jade0002",
            "jade",
            "Jade",
            (20, "zekrenlight/jade-sunrise"),
            "1.4.2",
            (&["26.3-0.1.0"], &[]),
        ),
    ]);
    let mut installed = BTreeMap::new();
    installed.insert(
        "jade".into(),
        InstalledPort {
            project_id: "jade0001".into(),
            version: "26.3.8".into(),
            repo: "joeypc6/sunrise-jade".into(),
        },
    );

    let groups = group_ports(&index, Some("26.3-0.1.0"), &installed, "", None, true);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].ports[0].action, PortAction::Installed);
    assert_eq!(groups[0].ports[1].action, PortAction::SwitchToPort);
    let req = groups[0].ports[1].switch_request.as_ref().unwrap();
    assert_eq!(req.from_repo, "joeypc6/sunrise-jade");
    assert_eq!(req.to_project_id, "jade0002");
    assert_eq!(req.to_repo, "zekrenlight/jade-sunrise");
}

#[test]
fn group_ports_marks_unfitting_port_unavailable() {
    let index = index_with(vec![
        project(
            "jade0001",
            "jade",
            "Jade",
            (10, "joeypc6/sunrise-jade"),
            "26.3.8",
            (&["26.3-0.1.0"], &[]),
        ),
        project(
            "jade0002",
            "jade",
            "Jade",
            (20, "zekrenlight/jade-sunrise"),
            "1.4.2",
            (&[], &["26.3-0.1.0"]),
        ),
    ]);

    let groups = group_ports(&index, Some("26.3-0.1.0"), &BTreeMap::new(), "", None, true);
    assert_eq!(groups[0].ports[0].action, PortAction::Install);
    assert_eq!(groups[0].ports[1].fit, FitVerdict::DoesNotFit);
    assert_eq!(groups[0].ports[1].action, PortAction::Unavailable);
    assert!(groups[0].ports[1].switch_request.is_none());

    // Without Show all, the port that doesn't fit leaves, and one fitting port is no group.
    assert!(group_ports(
        &index,
        Some("26.3-0.1.0"),
        &BTreeMap::new(),
        "",
        None,
        false
    )
    .is_empty());
}

#[test]
fn other_ports_for_returns_alternatives_with_switch_requests() {
    let index = index_with(vec![
        project(
            "jade0001",
            "jade",
            "Jade",
            (10, "joeypc6/sunrise-jade"),
            "26.3.8",
            (&["26.3-0.1.0"], &[]),
        ),
        project(
            "jade0002",
            "jade",
            "Jade",
            (20, "zekrenlight/jade-sunrise"),
            "1.4.2",
            (&["26.3-0.1.0"], &[]),
        ),
    ]);

    let other = other_ports_for(&index, "jade0001", "jade", "26.3.8", Some("26.3-0.1.0")).unwrap();
    assert_eq!(other.mod_id, "jade");
    assert_eq!(other.mod_name, "Jade");
    assert_eq!(other.alternatives.len(), 1);
    assert_eq!(
        other.alternatives[0].publisher_repo,
        "zekrenlight/jade-sunrise"
    );
    let req = other.alternatives[0].switch_request.as_ref().unwrap();
    assert_eq!(req.from_repo, "joeypc6/sunrise-jade");
    assert_eq!(req.from_version, "26.3.8");
    assert_eq!(req.to_project_id, "jade0002");
    assert_eq!(req.to_repo, "zekrenlight/jade-sunrise");
    assert_eq!(req.to_version, "1.4.2");

    // No alternatives for a solo project.
    let solo = index_with(vec![project(
        "solo0001",
        "frontier",
        "Frontier",
        (30, "Zekrenlight/frontier"),
        "1.2.0",
        (&["26.3-0.1.0"], &[]),
    )]);
    assert!(other_ports_for(&solo, "solo0001", "frontier", "1.2.0", Some("26.3-0.1.0")).is_none());
}
