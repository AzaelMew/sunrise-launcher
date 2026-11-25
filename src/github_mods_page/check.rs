//! Resolving a repository link, following its background check, and picking and submitting what it found.

use super::*;

impl Launcher {
    pub(super) fn github_resolve(&mut self) {
        if !self.github.can_resolve() || self.github.target.is_none() {
            return;
        }
        self.github.clear_picker();
        let link = match github_mods::parse(self.github.text.text()) {
            Ok(link) => link,
            Err(why) => {
                self.github.stage = Stage::Failed {
                    error: FormError::new(tr("Enter a GitHub link."), why),
                    expanded: false,
                    from_choose: false,
                };
                return;
            }
        };
        let (tx, rx) = crate::wake::channel();
        let cancellation = Cancellation::default();
        self.github.stage = Stage::Resolving(rx, cancellation.clone());
        std::thread::spawn(move || {
            let _ = tx.send(resolve_link(&link, &cancellation));
        });
    }

    pub(crate) fn github_tick(&mut self) -> bool {
        if self.ui.sheet != Some(Sheet::GithubAdd) {
            if self.github.target.is_some() {
                self.github.close();
            }
            return false;
        }
        if let Stage::ReadingLocal { rx, from_choose } = &self.github.stage {
            let from_choose = *from_choose;
            let result = match rx.try_recv() {
                Ok(result) => result,
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(FormError::new(
                    tr("Couldn't read the selected source."),
                    String::new(),
                )),
            };
            match result {
                Ok(LocalRead::File(pkg)) => {
                    if let Some(target) = self.github.target.clone() {
                        self.enter_warning(PendingConfirm::Local {
                            target,
                            packages: vec![*pkg],
                            folder: None,
                        });
                    }
                }
                Ok(LocalRead::Folder(path, packages)) => {
                    if packages.len() == 1 {
                        if let Some(target) = self.github.target.clone() {
                            self.enter_warning(PendingConfirm::Local {
                                target,
                                packages,
                                folder: Some(path),
                            });
                        }
                    } else {
                        self.github.stage = Stage::Pick(Box::new(Pick::new(
                            PickInput::LocalFolder(path, packages),
                        )));
                    }
                }
                Err(error) => {
                    self.github.stage = Stage::Failed {
                        error,
                        expanded: false,
                        from_choose,
                    };
                }
            }
            return true;
        }
        if let Stage::Checking(check) = &self.github.stage {
            let result = match check.rx.try_recv() {
                Ok(result) => result,
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(FormError::new(
                    tr("Couldn't check that the mods fit"),
                    tr("GitHub lookup stopped.").into(),
                )),
            };
            if let Stage::Checking(mut check) = std::mem::take(&mut self.github.stage) {
                match result {
                    Ok(sources) => {
                        if let (Some(target), Some(resolved)) = (
                            self.github.target.clone(),
                            check.pick.input.resolved().cloned(),
                        ) {
                            let input = GithubInput {
                                target,
                                link: self.github.text.text().to_owned(),
                                source: resolved,
                                selection: GithubSelection::Source(sources),
                            };
                            self.enter_warning(PendingConfirm::Github(input));
                        }
                    }
                    Err(error) => {
                        check.pick.error = Some(error);
                        self.github.stage = Stage::Pick(check.pick);
                    }
                }
            }
            return true;
        }
        let Stage::Resolving(rx, _) = &self.github.stage else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(input)) => {
                let pick = Pick::new(input);
                if pick.selected.len() == 1 {
                    self.github_submit(pick);
                } else {
                    self.github.stage = Stage::Pick(Box::new(pick));
                }
            }
            Ok(Err(error)) => {
                self.github.stage = Stage::Failed {
                    error,
                    expanded: false,
                    from_choose: false,
                };
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.github.stage = Stage::Failed {
                    error: FormError::transport(tr("GitHub lookup stopped.").into()),
                    expanded: false,
                    from_choose: false,
                };
            }
        }
        true
    }

    pub(super) fn github_pick(&mut self) {
        if !matches!(&self.github.stage, Stage::Pick(pick) if !pick.selected.is_empty() && pick.error.is_none())
        {
            return;
        }
        if let Stage::Pick(pick) = std::mem::take(&mut self.github.stage) {
            self.github_submit(*pick);
        }
    }

    fn github_submit(&mut self, mut pick: Pick) {
        match &pick.input {
            PickInput::Source(_, choices) => {
                let Some(target) = self.github.target.as_ref() else {
                    return;
                };
                let target = match target.source() {
                    Ok(target) => target,
                    Err(error) => {
                        pick.error =
                            Some(FormError::new(tr("Not available for your version"), error));
                        self.github.stage = Stage::Pick(Box::new(pick));
                        return;
                    }
                };
                let selected: Vec<_> = choices
                    .iter()
                    .filter(|package| pick.selected.contains(&package.origin().id))
                    .cloned()
                    .collect();
                let name = match selected.as_slice() {
                    [one] => one.name().to_owned(),
                    _ => tr("One of these mods").to_owned(),
                };
                let cancellation = Cancellation::default();
                let (tx, rx) = crate::wake::channel();
                self.github.stage = Stage::Checking(Box::new(SourceCheck {
                    pick: Box::new(pick),
                    rx,
                    cancellation: cancellation.clone(),
                }));
                std::thread::spawn(move || {
                    let result = sunrise::prepare_pasted_sources(
                        target.profile_target(),
                        target.identity(),
                        target.base(),
                        selected,
                        &cancellation,
                    )
                    .map_err(|error| pasted_source_error(&name, error));
                    let _ = tx.send(result);
                });
            }
            PickInput::Assets(..) => {
                if let Some(target) = self.github.target.clone() {
                    let input = pick.checked(target, self.github.text.text().to_owned());
                    self.enter_warning(PendingConfirm::Github(input));
                }
            }
            PickInput::LocalFolder(..) => {
                if let Some(target) = self.github.target.clone() {
                    let Pick {
                        input: PickInput::LocalFolder(path, mut packages),
                        selected,
                        ..
                    } = pick
                    else {
                        unreachable!()
                    };
                    packages.retain(|p| selected.contains(&p.id));
                    self.enter_warning(PendingConfirm::Local {
                        target,
                        packages,
                        folder: Some(path),
                    });
                }
            }
        }
    }

    pub(super) fn submit_checked_github(&mut self, input: GithubInput) {
        self.close_sheet();
        if let Err(error) = self.queue_github(Arc::new(input)) {
            self.mods.message = Some(Err(error));
        }
    }

    pub(super) fn github_retry(&mut self) {
        let Stage::InstallFailed(failure, _) = &self.github.stage else {
            return;
        };
        let retry = failure.retry.clone();
        self.close_sheet();
        if let Err(error) = self.queue_github(retry) {
            self.mods.message = Some(Err(error));
        }
    }
}
