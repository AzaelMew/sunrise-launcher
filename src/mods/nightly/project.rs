//! The pinned source detail. GitHub content is data; it never selects the profile or executes code.
use super::*;
use crate::mods_page::install::{stage_label, SourceQuestion};
use sunrise_launcher::browser::sunrise::SourceError;

impl Launcher {
    pub(in crate::mods) fn nightly_project(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // A mod the registry lists shows the registry's description and screenshots; otherwise
        // its README. Pictures go in the same gallery and viewer as a registry mod's.
        let registry_page = self.ui.mods.nightly.open.as_ref().and_then(|open| {
            let index = sunrise::known_index()?;
            let project = index.project(open.source.id())?;
            (!project.description.trim().is_empty())
                .then(|| (project.description.clone(), project.screenshots.clone()))
        });
        // A registry entry without screenshots keeps the README's pictures.
        let pictures = match &registry_page {
            Some((_, screenshots)) if !screenshots.is_empty() => screenshots
                .iter()
                .cloned()
                .map(super::super::viewer::Picture::Url)
                .collect(),
            _ => self
                .ui
                .mods
                .nightly
                .open
                .as_ref()
                .and_then(|open| open.inspection.as_ref()?.data.readme.as_ref())
                .map(|readme| {
                    readme
                        .images()
                        .iter()
                        .cloned()
                        .map(super::super::viewer::Picture::Readme)
                        .collect()
                })
                .unwrap_or_default(),
        };
        let gallery = self.gallery(pictures, cx);
        let state = &self.ui.mods.nightly;
        let open = state.open.as_ref().expect("a Nightly detail is open");
        let source = &open.source;
        let inspection = open.inspection.as_ref();
        let details = inspection.map(|i| &i.data);
        let readme = details.and_then(|d| d.readme.as_ref());
        let commit = details.and_then(|d| d.commit.as_ref());
        let failure = details
            .and_then(|detail| detail.error.clone())
            .map(SourceError::Github);
        let rows = self.discover_installed();
        let presence = rows.map(|rows| rows.source_presence(source));
        let pending = rows.is_some_and(|rows| rows.unbuilt.contains(source.id()));
        let installed = match presence {
            Some(SourcePresence::Installed(origin)) => Some(origin),
            _ => None,
        };
        let missing = presence == Some(SourcePresence::Missing);
        let conflict = presence == Some(SourcePresence::Conflict);
        let failed_job = (!conflict && presence.is_some())
            .then(|| self.source_outcome(source.id())?.as_ref().err())
            .flatten();
        let offline = !self.nightly_catalogue_current();
        let current = self
            .source_activity()
            .filter(|a| a.ids.iter().any(|id| id == source.id()));
        let retry = |id, cx: &mut Context<Self>| {
            let source = source.clone();
            button(id, tr("Try again")).on_click(cx.listener(move |this, _, _, cx| {
                this.inspect_nightly(source.clone(), true);
                cx.notify();
            }))
        };
        let build_label = if let Some(activity) = current.as_ref() {
            if activity.progress == sunrise::InstallProgress::Publishing {
                tr("Finishing…").into()
            } else {
                stage_label(activity.progress)
            }
        } else if matches!(failed_job, Some(SourceError::Tools(_))) {
            tr("Retry download").into()
        } else {
            tr("Add from source…").into()
        };
        let build = primary("nightly-build-source", build_label)
            .w_full()
            .h(px(tokens::ACTION));
        let listed = state
            .catalogue
            .as_ref()
            .is_some_and(|list| list.sources.contains(source));
        let can_build =
            !offline && listed && details.and_then(sources::Details::checked_commit).is_some();
        let busy = self.nightly_busy();
        let build = if busy || current.is_some() {
            dim(build)
        } else if !offline && listed && failed_job.is_some() {
            let source = source.clone();
            build.on_click(cx.listener(move |this, _, _, cx| {
                this.reread_and_add(source.clone());
                cx.notify();
            }))
        } else if can_build {
            build.on_click(cx.listener(|this, _, _, cx| {
                this.build_nightly();
                cx.notify();
            }))
        } else {
            dim(build)
        };
        let installed_actions = installed.map(|origin| {
            let locked = busy || self.mutating() || self.mods_locked().is_some();
            use sunrise_launcher::github_mods::SourceTrack;
            let update = origin
                .source
                .as_ref()
                .filter(|source| !matches!(source.track, SourceTrack::Pinned))
                .map(|_| {
                    let candidate = self.source_update_version(origin);
                    let checking = self.source_updates_checking();
                    let label = if let Some(version) = candidate {
                        trf("Update to {version}", &[("version", version)])
                    } else if checking {
                        tr("Checking source updates…").into()
                    } else {
                        tr("Check for update").into()
                    };
                    let b = primary("nightly-update-source", label).w_full();
                    if locked || checking {
                        dim(b)
                    } else if candidate.is_some() {
                        let id = origin.id.clone();
                        b.on_click(cx.listener(move |this, _, _, cx| {
                            this.submit_source_update(&id);
                            cx.notify();
                        }))
                    } else {
                        b.on_click(cx.listener(|this, _, _, cx| {
                            this.check_source_updates();
                            cx.notify();
                        }))
                    }
                });
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(install_fact(short_sha(&origin.commit), pending))
                .children(update)
                .child(self.remove_mod_button(&origin.id, locked, cx))
                .children(
                    self.source_update_notice()
                        .map(|notice| note(notice.to_owned())),
                )
        });
        let failure = if installed.is_some() {
            self.source_update_issue(source.id())
        } else if missing {
            failure.as_ref()
        } else {
            None
        };
        // An Add or Update this mod was refused: said once by the modal when it happened, and
        // reopened from one line under the button.
        let refused_line = failed_job
            .map(|error| (error, installed.is_some()))
            .or_else(|| installed.and(failure).map(|error| (error, true)))
            .and_then(|(error, update)| {
                let refused = crate::mods::refused::Refused::one(
                    source.name(),
                    Some(source.repo().to_owned()),
                    update,
                    error,
                )?;
                Some(self.refused_line("nightly-refused", refused, cx))
            });
        // Reading the source itself failed: what to do about it, in place.
        let failure_panel = (!installed.is_some()).then_some(failure).flatten().and_then(|error| {
            let words = source_error(error, Some(source))?;
            Some(div().flex().flex_col().items_start().gap(px(7.))
                .child(note(words))
                .children(matches!(error, SourceError::Github(_) | SourceError::Changed(_))
                    .then(|| if busy || matches!(error, SourceError::Github(sources::Failure::RateLimited { retry_at: Some(at) }) if *at > SystemTime::now()) {
                        dim(button("nightly-detail-retry", tr("Try again")))
                    } else { retry("nightly-detail-retry", cx) })))
        });
        let tracking = match source.tracker() {
            sources::Tracker::Branch(branch) => branch.as_str(),
            sources::Tracker::Release => commit
                .and_then(|pin| pin.release_tag())
                .unwrap_or(tr("Release")),
        };
        let pin = commit.map_or_else(
            || tr("Not checked").to_owned(),
            |pin| {
                trf(
                    "{track} @ {commit}",
                    &[("track", tracking), ("commit", short_sha(pin.sha()))],
                )
            },
        );
        let repo_url = format!("https://github.com/{}", source.repo());
        let allow_icon_fetch = !offline
            && details
                .and_then(|details| details.error.as_ref())
                .is_none_or(|error| *error == sources::Failure::NoManifest);
        let badge = made_for_build(
            self.nightly_build_revision().as_deref(),
            details.and_then(|detail| detail.build_pin.as_deref()),
        )
        .then(|| div().flex().child(badge(tr("Made for this build"))));
        let title = div()
            .flex()
            .items_center()
            .min_w_0()
            .gap(px(12.))
            .child(self.nightly_icon_tile(source, commit, allow_icon_fetch, 56., cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(tokens::DISPLAY))
                            .font_weight(FontWeight::BOLD)
                            .child(crate::selectable::selectable(source.name().to_owned())),
                    )
                    .child(muted(source.repo().to_owned()))
                    .children(badge),
            );
        let panel = card()
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(source.name().to_owned()),
            )
            .children(missing.then_some(build))
            .children(refused_line)
            .children(conflict.then(|| note(tr(SOURCE_ID_CONFLICT))))
            .children(installed_actions)
            .child(super::super::project::about_row(tr("Source"), pin))
            .child(super::super::project::about_row(
                tr("Game build"),
                self.discover_target_name(),
            ))
            .children(
                (offline && missing)
                    .then(|| note(tr("Connect to check the source and build this mod."))),
            )
            .children((!offline && !listed).then(|| {
                note(tr(
                    "This source listing changed. Open it again from Discover.",
                ))
            }))
            .children(
                (!busy && missing && offline && !self.ui.mods.nightly.reading)
                    .then(|| retry("nightly-refresh-catalogue", cx)),
            )
            .children(failure_panel)
            .child(
                div().flex().justify_end().child(
                    button("nightly-github", tr("Open on GitHub"))
                        .gap(px(5.))
                        .child("↗")
                        .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&repo_url))),
                ),
            );
        let mut left = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(12.))
            .child(title);
        let (above, beside) = if self.content_width() < 610. {
            (Some(panel), None)
        } else {
            (
                None,
                Some(
                    panel
                        .w(px(if self.content_width() < 860. {
                            228.
                        } else {
                            315.
                        }))
                        .flex_none(),
                ),
            )
        };
        left = left.children(above).children(gallery);
        let saved_readme = readme.is_some()
            && matches!(
                details.and_then(|details| details.error.as_ref()),
                Some(sources::Failure::Offline(_) | sources::Failure::RateLimited { .. })
            );
        if registry_page.is_none() {
            left = left.child(muted(if saved_readme {
                tr("README from the repository · saved copy")
            } else {
                tr("README from the repository")
            }));
        }
        let body = if let Some((description, _)) = &registry_page {
            super::super::project::prose(self.theme, description)
        } else if let Some(readme) = readme {
            // Its pictures are in the gallery above.
            crate::markdown::render_pinned(readme, &self.theme, |_| div().into_any_element())
        } else if inspection.is_none() {
            note(tr("Checking the pinned source…")).into_any_element()
        } else {
            card()
                .p(px(14.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .items_start()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(tr("README unavailable")),
                )
                .child(note(tr("You can still inspect the source on GitHub.")))
                .children(
                    (details.is_some_and(|d| d.error.is_none()) && !busy)
                        .then(|| retry("nightly-readme-retry", cx)),
                )
                .into_any_element()
        };
        left = left.child(body);
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .p(px(24.))
            .gap(px(12.))
            .children(self.strip(cx))
            .child(
                div()
                    .id("nightly-project-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&crate::frame::scrolls("nightly-project"))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .child(super::super::project::back(tr("Discover")).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.choose_view(Page::Discover);
                                    cx.notify();
                                }),
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(18.))
                                    .child(left)
                                    .children(beside),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(in crate::mods) fn nightly_log_button(
        &self,
        name: &str,
        log: std::sync::Arc<str>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let name = name.to_owned();
        button("nightly-build-log", tr("View build log")).on_click(cx.listener(
            move |this, _, _, cx| {
                this.ui.mods.nightly.work.log = Some((name.clone(), log.clone()));
                this.open_sheet(crate::sheets::Sheet::NightlySourceLog);
                cx.notify();
            },
        ))
    }

    pub(crate) fn source_log_body(&self, cx: &mut Context<Self>) -> Option<Div> {
        let (name, log) = self.ui.mods.nightly.work.log.as_ref()?;
        let title = trf("{mod} build log", &[("mod", name)]);
        let log = log.to_string();
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .text_size(px(tokens::HEADING))
                        .font_weight(FontWeight::BOLD)
                        .child(title),
                )
                .child(
                    div()
                        .id("nightly-log-lines")
                        .max_h(px(310.))
                        .overflow_y_scroll()
                        .child(crate::code_box::code_box(
                            "nightly-log",
                            vec![log.clone().into()],
                            None,
                        )),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            button("nightly-log-copy", tr("Copy log")).on_click(cx.listener(
                                move |_, _, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        log.clone(),
                                    ));
                                },
                            )),
                        )
                        .child(
                            button("nightly-log-close", tr("Close")).on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.close_sheet();
                                    cx.notify();
                                },
                            )),
                        ),
                ),
        )
    }

    pub(crate) fn source_already_allowed(&self, question: &SourceQuestion) -> bool {
        self.settings.native_mods_allowed_at.is_some()
            && question.mods.iter().all(|(_, origin)| {
                source_grant(origin)
                    .is_some_and(|grant| self.settings.source_mod_grants.contains(&grant))
            })
    }

    pub(crate) fn source_consent_body(&self, cx: &mut Context<Self>) -> Option<Div> {
        let question = self.source_question()?;
        let base = crate::sheets::build_name(&question.base);
        let mut list = div().flex().flex_col().gap(px(8.));
        for (n, (name, origin)) in question.mods.iter().enumerate() {
            let url = origin.source_url();
            list = list.child(
                card()
                    .p(px(9.))
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                            .child(note(format!(
                                "{}/{} @ {}",
                                origin.owner, origin.repo, origin.commit
                            ))),
                    )
                    .child(
                        button(("nightly-consent-source", n), tr("View source"))
                            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                    ),
            );
        }
        let approve = primary("nightly-consent-build", tr("Allow")).on_click(cx.listener(
            |this, _, _, cx| {
                let Some(question) = this.source_question() else {
                    return;
                };
                let grants: Option<Vec<_>> = question
                    .mods
                    .iter()
                    .map(|(_, origin)| source_grant(origin))
                    .collect();
                let answer = grants
                    .ok_or_else(|| tr("The source could not be verified.").to_owned())
                    .and_then(|grants| this.settings.allow_source_mods(grants))
                    .map(|()| true);
                this.answer_source_question(answer);
                cx.notify();
            },
        ));
        Some(div().flex().flex_col().gap(px(12.))
            .child(div().text_size(px(tokens::HEADING)).font_weight(FontWeight::BOLD)
                .child(tr("Allow unreviewed source code?")))
            .child(note(trf("Game build: {build}", &[("build", &base)])))
            .child(div().id("nightly-consent-mods").max_h(px(185.)).overflow_y_scroll().child(list))
            .child(note(tr("Sunrise hasn't reviewed this code. Building and playing run the author's code on your computer.")))
            .child(div().flex().justify_end().gap(px(8.))
                .child(button("nightly-consent-cancel", tr("Cancel")).on_click(cx.listener(|this, _, _, cx| {
                    this.close_sheet();
                    cx.notify();
                })))
                .child(approve)))
    }
}

fn source_grant(origin: &sunrise_launcher::github_mods::Origin) -> Option<backend::SourceModGrant> {
    Some(backend::SourceModGrant {
        repo_id: origin.source.as_ref()?.repo_id,
        owner: origin.owner.to_ascii_lowercase(),
        id: origin.id.clone(),
    })
}

pub(in crate::mods) fn short_sha(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

pub(in crate::mods) fn source_error(
    error: &SourceError,
    source: Option<&Source>,
) -> Option<String> {
    Some(match error {
        SourceError::Cancelled => return None,
        SourceError::Github(error) => match error {
            sources::Failure::RepositoryUnavailable => {
                tr("Repository unavailable. It may be private, moved or deleted.").into()
            }
            sources::Failure::RefUnavailable => tr("Tracked branch/release unavailable").into(),
            sources::Failure::RateLimited {
                retry_at: Some(time),
            } => trf(
                "GitHub’s request limit was reached. Try again after {time}.",
                &[("time", &source_time(*time))],
            ),
            sources::Failure::RateLimited { retry_at: None } => {
                tr("GitHub’s request limit was reached. Try again later.").into()
            }
            sources::Failure::NoManifest => source.map_or_else(
                || tr("No Sunrise mod at this commit").into(),
                |source| {
                    trf(
                        "No Sunrise mod at this commit. Expected {manifest}.",
                        &[("manifest", source.manifest())],
                    )
                },
            ),
            sources::Failure::Offline(_) => {
                tr("You're offline. Connect to check the source.").into()
            }
            sources::Failure::Other(_) => tr("Couldn't check this source.").into(),
        },
        SourceError::Dependency {
            required: Some(id), ..
        } => trf(
            "Needs {mod}. Install a fitting version first.",
            &[("mod", id)],
        ),
        SourceError::Dependency { required: None, .. } => {
            tr("Can't install these mods together").into()
        }
        SourceError::Unproved(_) => tr("Couldn't add this mod").into(),
        SourceError::Busy => {
            tr("Sunrise is building mods right now. Try again when that finishes.").into()
        }
        SourceError::Changed(_) => {
            tr("The profile or game build changed. Check the source again; nothing was changed.")
                .into()
        }
        SourceError::DoesNotFit(_) => tr("Doesn't fit this build.").into(),
        SourceError::Tools(_) => {
            tr("Couldn't download build tools. Check your connection and try again.").into()
        }
        SourceError::Build { .. } => tr("Couldn't build on this computer.").into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_status_keeps_local_paths_out_of_regular_copy() {
        let diagnostic = "fixture /local/private/.installed C:\\Private\\build.log";
        for error in [
            SourceError::Github(sources::Failure::Other(diagnostic.into())),
            SourceError::Unproved(diagnostic.into()),
            SourceError::Changed(diagnostic.into()),
            SourceError::Dependency {
                required: Some("api_mod".into()),
                detail: diagnostic.into(),
            },
            SourceError::DoesNotFit(diagnostic.into()),
        ] {
            assert_eq!(error.to_string(), diagnostic);
            assert!(!source_error(&error, None).unwrap().contains("Private"));
            assert!(!source_error(&error, None).unwrap().contains("/local/"));
        }
    }
}
