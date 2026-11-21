//! Player-facing copy, separate from the exact archive details shown by command-line clients.
use super::Change;
use crate::i18n::{tr, trf};

impl Change {
    pub(super) fn gui_review(
        &self,
        world_count: usize,
        unchecked_update: bool,
    ) -> (String, String) {
        let mut lines = Vec::new();
        let (heading, one_mod) = match self {
            Self::Update(updates) => {
                let mut updated = Vec::new();
                for update in updates {
                    if let Some(installed) = &update.installed {
                        updated.push(update.name.as_str());
                        lines.push(trf(
                            "{name} will go from {old} to {new}.",
                            &[
                                ("old", installed),
                                ("new", &update.replacement),
                                ("name", &update.name),
                            ],
                        ));
                    } else {
                        lines.push(trf(
                            "{name} {version} will also be installed.",
                            &[("version", &update.replacement), ("name", &update.name)],
                        ));
                    }
                }
                let heading = match updated.as_slice() {
                    [name] => trf("Update {name}?", &[("name", name)]),
                    _ => trf(
                        "Update {count} mods?",
                        &[("count", &updated.len().to_string())],
                    ),
                };
                (heading, updated.len() == 1)
            }
            Self::GithubUpdate { target, .. } => {
                lines.push(
                    tr("This replaces the installed version with the GitHub release you selected.")
                        .into(),
                );
                (trf("Update {name}?", &[("name", &target.name)]), true)
            }
            Self::Remove { removed, disabled } => {
                let heading = match removed.as_slice() {
                    [archive] => trf("Remove {name}?", &[("name", &archive.target.name)]),
                    _ => {
                        let mut seen = std::collections::BTreeSet::new();
                        let duplicate_names: std::collections::BTreeSet<_> = removed
                            .iter()
                            .map(|archive| archive.target.name.as_str())
                            .filter(|name| !seen.insert(*name))
                            .collect();
                        let labels: Vec<_> = removed
                            .iter()
                            .map(|archive| {
                                let name = archive.target.name.as_str();
                                if duplicate_names.contains(name) {
                                    trf(
                                        "{name} ({file})",
                                        &[("name", name), ("file", &archive.file)],
                                    )
                                } else {
                                    name.to_owned()
                                }
                            })
                            .collect();
                        let names: Vec<_> = labels.iter().map(String::as_str).collect();
                        lines.push(trf("Remove: {mods}.", &[("mods", &joined_names(&names))]));
                        trf(
                            "Remove {count} mods?",
                            &[("count", &removed.len().to_string())],
                        )
                    }
                };
                match (removed.len() == 1, disabled.as_slice()) {
                    (_, []) => (),
                    (true, [one]) => lines.push(trf(
                        "{name} needs it. It'll be turned off, and its files stay installed.",
                        &[("name", &one.name)],
                    )),
                    (false, [one]) => lines.push(trf(
                        "{name} needs one or more of these mods. It'll be turned off, and its files stay installed.",
                        &[("name", &one.name)],
                    )),
                    (single_target, many) => {
                        let names = many.iter().map(|item| item.name.as_str()).collect::<Vec<_>>();
                        let names = joined_names(&names);
                        lines.push(if single_target {
                            trf("{names} need it. They'll be turned off, and their files stay installed.", &[("names", &names)])
                        } else {
                            trf("{names} need one or more of these mods. They'll be turned off, and their files stay installed.", &[("names", &names)])
                        });
                    }
                }
                (heading, removed.len() == 1 && disabled.is_empty())
            }
        };
        match (world_count, one_mod) {
            (0, _) if unchecked_update => lines.push(
                tr("Sunrise can't check whether this update removes blocks or items your worlds use.")
                    .into(),
            ),
            (0, _) => (),
            (1, true) => {
                lines.push(tr("1 saved world may lose blocks and items from this mod.").into())
            }
            (1, false) => {
                lines.push(tr("1 saved world may lose blocks and items from these mods.").into())
            }
            (count, true) => lines.push(trf(
                "{count} saved worlds may lose blocks and items from this mod.",
                &[("count", &count.to_string())],
            )),
            (count, false) => lines.push(trf(
                "{count} saved worlds may lose blocks and items from these mods.",
                &[("count", &count.to_string())],
            )),
        }
        (heading, lines.join("\n\n"))
    }
}

fn joined_names(names: &[&str]) -> String {
    match names {
        [rest @ .., last] if !rest.is_empty() => trf(
            "{names} and {last}",
            &[("names", &rest.join(", ")), ("last", last)],
        ),
        _ => names.join(", "),
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        ChangeImpact, ReviewedArchive, ReviewedMod, ReviewedRelease, ReviewedUpdate,
    };
    use super::*;

    fn item(name: &str, id: &str) -> ReviewedMod {
        ReviewedMod {
            name: name.into(),
            id: id.into(),
        }
    }

    fn remove(targets: &[(&str, &str)], disabled: Vec<ReviewedMod>) -> Change {
        Change::Remove {
            removed: targets
                .iter()
                .map(|&(name, id)| ReviewedArchive {
                    target: item(name, id),
                    file: format!("sunrise-mods/{id}.smod"),
                    digest: "reviewed digest".into(),
                })
                .collect(),
            disabled,
        }
    }

    #[test]
    fn single_removal_only_explains_the_consequences_that_apply() {
        let core = [("Lantern Core", "lantern_core")];
        let bare = remove(&core, vec![]);
        assert_eq!(
            bare.gui_review(0, false),
            (String::from("Remove Lantern Core?"), String::new())
        );
        assert_eq!(
            bare.gui_review(1, false).1,
            "1 saved world may lose blocks and items from this mod."
        );
        let one = remove(&core, vec![item("Fireflies", "fireflies")]);
        assert_eq!(
            one.gui_review(1, false).1,
            concat!(
                "Fireflies needs it. It'll be turned off, and its files stay installed.\n\n",
                "1 saved world may lose blocks and items from these mods."
            )
        );
        let two = remove(
            &core,
            vec![item("Fireflies", "fireflies"), item("Lanterns", "lanterns")],
        );
        assert_eq!(two.gui_review(3, false).1, concat!(
            "Fireflies and Lanterns need it. They'll be turned off, and their files stay installed.\n\n",
            "3 saved worlds may lose blocks and items from these mods."
        ));
    }

    #[test]
    fn bulk_removal_names_the_targets_by_display_name() {
        let change = remove(
            &[
                ("Lantern Core", "lantern_core"),
                ("Night Lights (Deluxe)", "night_lights"),
                ("Torches", "torches"),
            ],
            vec![item("Fireflies", "fireflies"), item("Lanterns", "lanterns")],
        );
        let (heading, detail) = change.gui_review(0, false);
        assert_eq!(heading, "Remove 3 mods?");
        assert_eq!(
            detail,
            concat!(
                "Remove: Lantern Core, Night Lights (Deluxe) and Torches.\n\n",
                "Fireflies and Lanterns need one or more of these mods. ",
                "They'll be turned off, and their files stay installed."
            )
        );
    }

    #[test]
    fn bulk_removal_disambiguates_only_duplicate_display_names() {
        let impact = ChangeImpact {
            change: remove(
                &[
                    ("Lanterns", "lanterns"),
                    ("Lanterns", "lanterns_extra"),
                    ("Torches", "torches"),
                ],
                vec![],
            ),
            evidence: vec![],
            unchecked_update: false,
        };
        assert_eq!(
            impact.gui_review(),
            (
                "Remove 3 mods?".into(),
                concat!(
                    "Remove: Lanterns (sunrise-mods/lanterns.smod), ",
                    "Lanterns (sunrise-mods/lanterns_extra.smod) and Torches."
                )
                .into()
            )
        );
        let detail = impact.detail();
        assert!(detail.contains("sunrise-mods/torches.smod"));
        assert!(detail.contains("reviewed digest"));
    }

    #[test]
    fn registry_update_distinguishes_a_new_dependency_from_an_update() {
        let change = Change::Update(vec![
            ReviewedUpdate {
                name: "Lanterns".into(),
                installed: Some("1.0.0".into()),
                replacement: "1.1.0".into(),
            },
            ReviewedUpdate {
                name: "Lantern Core".into(),
                installed: None,
                replacement: "1.0.0".into(),
            },
        ]);
        let (heading, detail) = change.gui_review(2, false);
        assert_eq!(heading, "Update Lanterns?");
        assert_eq!(
            detail,
            concat!(
                "Lanterns will go from 1.0.0 to 1.1.0.\n\n",
                "Lantern Core 1.0.0 will also be installed.\n\n",
                "2 saved worlds may lose blocks and items from this mod."
            )
        );
    }

    #[test]
    fn github_gui_does_not_show_archive_identity_but_detail_does() {
        let impact = ChangeImpact {
            change: Change::GithubUpdate {
                target: item("Lanterns", "lanterns"),
                file: "sunrise-mods/lanterns.smod".into(),
                releases: [
                    ReviewedRelease {
                        label: "old release".into(),
                        sha256: "old digest".into(),
                    },
                    ReviewedRelease {
                        label: "new release".into(),
                        sha256: "new digest".into(),
                    },
                ],
            },
            evidence: vec![],
            unchecked_update: true,
        };
        let (heading, warning) = impact.gui_review();
        assert_eq!(heading, "Update Lanterns?");
        assert_eq!(
            warning,
            concat!(
                "This replaces the installed version with the GitHub release you selected.\n\n",
                "Sunrise can't check whether this update removes blocks or items your worlds use."
            )
        );
        assert!(!impact.risks_worlds());
        assert_eq!(
            impact.detail(),
            concat!(
            "Update: Lanterns (lanterns).\n\n",
            "Installed archive: old release\nsunrise-mods/lanterns.smod\nSHA-256: old digest\n\n",
            "Replacement archive: new release\nsunrise-mods/lanterns.smod\nSHA-256: new digest\n\n",
            "Sunrise can't check whether this update removes blocks or items your worlds use."
        )
        );
    }
}
