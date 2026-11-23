//! "Add from source" dialog (repository folder/link or local `.smod` file) and its prominent
//! unreviewed-code warning. Reading and checking happen in the background; nothing is installed
//! until the player explicitly confirms the warning.
use super::*;
use crate::blocks;
use crate::frame::{button, input, primary, Page, Reach as _};
use crate::mods::ManualBadge;
use crate::mods_page::install::GithubTarget;
use crate::paint::palette;
use crate::sheets::{self, Sheet};
use gpui::rgb;
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};
use sunrise_launcher::{
    browser::sunrise::{self, sources::Failure, PastedSource, PreparedSource, SourceError},
    github_mods::{self, FetchError, LocalPackage, Packed, ResolveError, Resolved},
    native_mods::Cancellation,
};

pub(crate) struct GithubInput {
    pub(crate) target: GithubTarget,
    pub(crate) link: String,
    pub(crate) source: Resolved,
    pub(crate) selection: GithubSelection,
}

pub(crate) enum GithubSelection {
    Add(Vec<Packed>),
    Source(Vec<PreparedSource>),
}

pub(crate) struct LocalInput {
    pub(crate) target: GithubTarget,
    pub(crate) label: String,
    pub(crate) names: Vec<String>,
    pub(crate) packages: Vec<LocalPackage>,
}

pub(crate) struct Feedback {
    pub(crate) profile: instances::Instance,
    pub(crate) link: String,
    pub(crate) result: Result<String, (String, String)>,
    pub(crate) retry: Option<Arc<GithubInput>>,
    pub(crate) local: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SourceChoice {
    #[default]
    Repository,
    LocalFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WarningAction {
    Cancel,
    Confirm,
}

impl WarningAction {
    /// Cancel is always the default on the unreviewed-code warning.
    const DEFAULT: Self = Self::Cancel;
}

struct WarningProvenance {
    icon: &'static str,
    headline: String,
    detail: String,
}

enum PendingConfirm {
    Github(GithubInput),
    Local {
        target: GithubTarget,
        packages: Vec<LocalPackage>,
        folder: Option<PathBuf>,
    },
}

impl PendingConfirm {
    fn badge(&self) -> ManualBadge {
        match self {
            Self::Github(_)
            | Self::Local {
                folder: Some(_), ..
            } => ManualBadge::FromSource,
            Self::Local { folder: None, .. } => ManualBadge::Local,
        }
    }

    fn count(&self) -> usize {
        match self {
            Self::Github(input) => match &input.selection {
                GithubSelection::Add(packages) => packages.len(),
                GithubSelection::Source(packages) => packages.len(),
            },
            Self::Local { packages, .. } => packages.len(),
        }
    }

    fn any_native(&self) -> bool {
        match self {
            Self::Github(_) => true,
            Self::Local { packages, .. } => packages.iter().any(|p| p.native),
        }
    }

    fn unsigned_local_file(&self) -> bool {
        matches!(
            self,
            Self::Local {
                folder: None,
                packages,
                ..
            } if !matches!(
                packages.first().map(|p| &p.attestation),
                Some(github_mods::LocalAttestation::Verified { .. })
            )
        )
    }

    fn provenance(&self) -> WarningProvenance {
        match self {
            Self::Local {
                folder: None,
                packages,
                ..
            } => {
                let pkg = &packages[0];
                let path = pkg.source_path.display().to_string();
                match &pkg.attestation {
                    github_mods::LocalAttestation::Verified {
                        repository,
                        workflow,
                    } => WarningProvenance {
                        icon: "github",
                        headline: tr("Signed local file, not reviewed").into(),
                        detail: trf(
                            "File: {file} · Built by {repo} ({workflow}) · Sunrise has not checked this code.",
                            &[("file", &path), ("repo", repository), ("workflow", workflow)],
                        ),
                    },
                    github_mods::LocalAttestation::Failed(reason) => WarningProvenance {
                        icon: "warning",
                        headline: tr("Attestation check failed, not reviewed").into(),
                        detail: trf(
                            "File: {file} · The bundled attestation could not be verified ({reason}).",
                            &[("file", &path), ("reason", reason)],
                        ),
                    },
                    github_mods::LocalAttestation::Unsigned => WarningProvenance {
                        icon: "warning",
                        headline: tr("Unsigned local file, not reviewed").into(),
                        detail: trf(
                            "File: {file} · Sunrise cannot verify who built this file or where it came from.",
                            &[("file", &path)],
                        ),
                    },
                }
            }
            Self::Local {
                folder: Some(folder),
                packages,
                ..
            } => {
                let mods = packages
                    .iter()
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let folder_name = folder.display().to_string();
                WarningProvenance {
                    icon: "folder",
                    headline: tr("Local repository folder, not reviewed").into(),
                    detail: trf(
                        "Folder: {folder} · Mods: {mods} · Sunrise has not checked this code.",
                        &[("folder", &folder_name), ("mods", &mods)],
                    ),
                }
            }
            Self::Github(input) => {
                let repo = input.source.name();
                let detail = match &input.selection {
                    GithubSelection::Add(packages) => {
                        let mods = packages
                            .iter()
                            .map(|p| p.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let assets: Vec<(&str, &str)> = packages
                            .iter()
                            .filter_map(|p| p.release_asset(&input.source))
                            .collect();
                        if !assets.is_empty() && assets.len() == packages.len() {
                            let tag = assets[0].0;
                            let files = assets
                                .iter()
                                .map(|(_, asset)| *asset)
                                .collect::<Vec<_>>()
                                .join(", ");
                            trf(
                                "Mods: {mods} · Release {tag} ({files}) · Sunrise has not checked this code.",
                                &[("mods", &mods), ("tag", tag), ("files", &files)],
                            )
                        } else if let Some(tag) = input.source.tag.as_deref() {
                            trf(
                                "Mods: {mods} · Release {tag} · Sunrise has not checked this code.",
                                &[("mods", &mods), ("tag", tag)],
                            )
                        } else {
                            let sha: String = input.source.commit.chars().take(7).collect();
                            trf(
                                "Mods: {mods} · Commit {sha} · Sunrise has not checked this code.",
                                &[("mods", &mods), ("sha", &sha)],
                            )
                        }
                    }
                    GithubSelection::Source(packages) => {
                        let mods = packages
                            .iter()
                            .map(PreparedSource::name)
                            .collect::<Vec<_>>()
                            .join(", ");
                        let sha: String = input.source.commit.chars().take(7).collect();
                        trf(
                            "Mods: {mods} · Commit {sha} · Sunrise has not checked this code.",
                            &[("mods", &mods), ("sha", &sha)],
                        )
                    }
                };
                WarningProvenance {
                    icon: "github",
                    headline: trf("From github.com/{repo}, not reviewed", &[("repo", &repo)]),
                    detail,
                }
            }
        }
    }
}

struct FormError {
    line: String,
    detail: String,
    retry: bool,
}

impl FormError {
    fn new(line: &str, detail: String) -> Self {
        Self {
            line: line.into(),
            detail,
            retry: false,
        }
    }

    fn transport(detail: String) -> Self {
        Self {
            line: tr("Could not reach GitHub. Try again.").into(),
            detail,
            retry: true,
        }
    }
}

enum LocalRead {
    File(Box<LocalPackage>),
    Folder(PathBuf, Vec<LocalPackage>),
}

enum PickInput {
    Assets(Resolved, Vec<Packed>),
    Source(Resolved, Vec<PastedSource>),
    LocalFolder(PathBuf, Vec<LocalPackage>),
}

impl PickInput {
    fn rows(&self) -> Vec<(&str, &str, &str)> {
        match self {
            Self::Assets(_, mods) => mods
                .iter()
                .map(|p| (p.id.as_str(), p.name.as_str(), p.version.as_str()))
                .collect(),
            Self::Source(_, mods) => mods
                .iter()
                .map(|p| {
                    (
                        p.origin().id.as_str(),
                        p.name(),
                        p.origin().version.as_str(),
                    )
                })
                .collect(),
            Self::LocalFolder(_, mods) => mods
                .iter()
                .map(|p| (p.id.as_str(), p.name.as_str(), p.version.as_str()))
                .collect(),
        }
    }

    fn subtitle(&self) -> String {
        match self {
            Self::Assets(resolved, _) | Self::Source(resolved, _) => resolved.label(),
            Self::LocalFolder(path, _) => path.display().to_string(),
        }
    }

    fn resolved(&self) -> Option<&Resolved> {
        match self {
            Self::Assets(resolved, _) | Self::Source(resolved, _) => Some(resolved),
            Self::LocalFolder(_, _) => None,
        }
    }
}

struct Pick {
    input: PickInput,
    selected: BTreeSet<String>,
    error: Option<FormError>,
    /// The error's own words show (Show details).
    details: bool,
}

impl Pick {
    fn new(input: PickInput) -> Self {
        let selected = input
            .rows()
            .into_iter()
            .map(|(id, _, _)| id.into())
            .collect();
        Self {
            input,
            selected,
            error: None,
            details: false,
        }
    }

    fn checked(self, target: GithubTarget, link: String) -> GithubInput {
        let Self {
            input, selected, ..
        } = self;
        let (source, selection) = match input {
            PickInput::Assets(source, mut mods) => {
                mods.retain(|package| selected.contains(&package.id));
                (source, GithubSelection::Add(mods))
            }
            PickInput::Source(..) => unreachable!("sources need a selected-set fit proof"),
            PickInput::LocalFolder(..) => unreachable!("local folders install locally"),
        };
        GithubInput {
            target,
            link,
            source,
            selection,
        }
    }

    fn toggle(&mut self, id: &str) {
        self.error = None;
        if !self.selected.remove(id) {
            self.selected.insert(id.to_owned());
        }
    }
}

enum Stage {
    Choose(SourceChoice),
    Repository,
    Resolving(Receiver<Result<PickInput, FormError>>, Cancellation),
    ReadingLocal {
        rx: Receiver<Result<LocalRead, FormError>>,
        from_choose: bool,
    },
    Checking(Box<SourceCheck>),
    Pick(Box<Pick>),
    Warn(Box<PendingConfirm>),
    Failed {
        error: FormError,
        expanded: bool,
        from_choose: bool,
    },
    InstallFailed(Box<CheckedFailure>, bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerKind {
    LocalFile,
    LocalFolder,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingPicker {
    token: u64,
    kind: PickerKind,
}

impl Default for Stage {
    fn default() -> Self {
        Self::Choose(SourceChoice::Repository)
    }
}

struct SourceCheck {
    pick: Box<Pick>,
    rx: Receiver<Result<Vec<PreparedSource>, FormError>>,
    cancellation: Cancellation,
}

struct CheckedFailure {
    error: FormError,
    retry: Arc<GithubInput>,
}

#[derive(Default)]
pub(super) struct Github {
    text: line_edit::LineEdit,
    stage: Stage,
    focus: Option<FocusHandle>,
    cancel_focus: Option<FocusHandle>,
    warn_needs_focus: bool,
    repo_needs_focus: bool,
    target: Option<GithubTarget>,
    pending_failure: Option<CheckedFailure>,
    next_picker: u64,
    pending_picker: Option<PendingPicker>,
}

impl Github {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn close(&mut self) {
        // Dropping a pending lookup's receiver releases its packages even if it finishes later.
        match &self.stage {
            Stage::Resolving(_, cancellation) => cancellation.stop(),
            Stage::Checking(check) => check.cancellation.stop(),
            _ => (),
        }
        self.stage = Stage::Choose(SourceChoice::Repository);
        self.target = None;
        self.warn_needs_focus = false;
        self.repo_needs_focus = false;
        self.pending_picker = None;
        self.text.set(String::new());
    }

    fn start_picker(&mut self, kind: PickerKind) -> u64 {
        self.next_picker = self.next_picker.wrapping_add(1);
        let token = self.next_picker;
        self.pending_picker = Some(PendingPicker { token, kind });
        token
    }

    fn clear_picker(&mut self) {
        self.pending_picker = None;
    }

    fn finish_picker(
        &mut self,
        token: u64,
        kind: PickerKind,
        outcome: Result<Option<PathBuf>, String>,
    ) -> Option<PathBuf> {
        let stage_matches = match kind {
            PickerKind::LocalFile => matches!(self.stage, Stage::Choose(_)),
            PickerKind::LocalFolder => matches!(self.stage, Stage::Repository),
        };
        if self.pending_picker != Some(PendingPicker { token, kind }) || !stage_matches {
            return None;
        }
        self.pending_picker = None;
        match outcome {
            Ok(path) => path,
            Err(why) => {
                self.stage = Stage::Failed {
                    error: FormError::new(tr("Couldn't open the file picker."), why),
                    expanded: false,
                    from_choose: kind == PickerKind::LocalFile,
                };
                None
            }
        }
    }

    pub(super) fn busy(&self) -> bool {
        matches!(
            self.stage,
            Stage::Resolving(..) | Stage::ReadingLocal { .. } | Stage::Checking(_)
        )
    }

    fn can_resolve(&self) -> bool {
        !self.text.text().trim().is_empty()
            && match &self.stage {
                Stage::Repository => true,
                Stage::Failed {
                    error,
                    from_choose: false,
                    ..
                } => error.retry,
                _ => false,
            }
    }
}

fn resolve_link(
    link: &github_mods::Link,
    cancellation: &Cancellation,
) -> Result<PickInput, FormError> {
    let found = github_mods::resolve(link, &github_mods::api).map_err(|error| match error {
        error @ ResolveError::NoRelease(_) => {
            FormError::new(tr("This release wasn’t found."), error.into())
        }
        ResolveError::Lookup(why) => {
            FormError::new(tr("This GitHub link could not be found."), why)
        }
        ResolveError::Transfer(why) => FormError::transport(why),
    })?;
    if found.assets.is_empty() {
        let sources = sunrise::read_pasted_sources(link, &found, cancellation)
            .map_err(|error| pasted_source_error(&found.repo, error))?;
        return Ok(PickInput::Source(found, sources));
    }
    let mods = github_mods::fetch_checked(&found).map_err(|error| match error {
        FetchError::NoMods { release, detail } => FormError::new(
            if release {
                tr("No Sunrise mods were found in this release.")
            } else {
                tr("No Sunrise mods were found at this link.")
            },
            detail,
        ),
        FetchError::Package(why) => {
            FormError::new(tr("The GitHub mod package was not valid."), why)
        }
        FetchError::Transfer(why) => FormError::transport(why),
    })?;
    Ok(PickInput::Assets(found, mods))
}

/// A source the form couldn't take, said as every refusal is (`mods::refused`): what it
/// means for the mod people know as `name`, with the checker's words behind Show details.
fn pasted_source_error(name: &str, error: SourceError) -> FormError {
    eprintln!("GitHub source preflight: {error}");
    if let SourceError::Github(Failure::Offline(_) | Failure::RateLimited { .. })
    | SourceError::DependencyGithub {
        failure: Failure::Offline(_) | Failure::RateLimited { .. },
        ..
    } = error
    {
        return FormError::transport(String::new());
    }
    match crate::mods::refused::Refusal::of(name, &error) {
        Some(refusal) => FormError::new(&refusal.said, refusal.details),
        None => FormError::new(tr("Install cancelled."), String::new()),
    }
}

impl Launcher {
    pub(crate) fn add_from_source_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("add-from-source")
            .tab_index(0)
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .rounded(crate::tokens::r(crate::tokens::R_CONTROL))
            .border_1()
            .border_color(rgb(palette().line))
            .px(px(12.))
            .h(px(tokens::CONTROL))
            .text_color(rgb(palette().text))
            .text_size(px(tokens::LABEL))
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .hover(|s| s.bg(rgb(palette().hover)))
            .focus_visible(|s| s.border_color(rgb(palette().focus)))
            .seen()
            .child(blocks::icon("plus", 14., rgb(palette().text).into()))
            .child(tr("Add from source"))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.open_add_from_source();
                cx.notify();
            }))
    }

    pub(crate) fn open_add_from_source(&mut self) {
        self.github.close();
        self.open_sheet(Sheet::GithubAdd);
        match self.github_target() {
            Ok(target) => self.github.target = Some(target),
            Err(error) => {
                self.github.stage = Stage::Failed {
                    error: FormError::new(tr("Can't start the install"), error),
                    expanded: false,
                    from_choose: true,
                };
                return;
            }
        }
        if self.github.pending_failure.as_ref().is_some_and(|f| {
            self.github
                .target
                .as_ref()
                .is_some_and(|current| f.retry.target.same_destination(current))
        }) {
            if let Some(failure) = self.github.pending_failure.take() {
                self.github.stage = Stage::InstallFailed(Box::new(failure), false);
                self.mods.message = None;
            }
        }
    }

    pub(crate) fn show_github_result(&mut self, feedback: Feedback) {
        let Feedback {
            profile,
            link,
            result,
            retry,
            local,
        } = feedback;
        let same_profile = self.selected_instance().is_some_and(|current| {
            current.home == profile.home
                && current.id == profile.id
                && current.incarnation == profile.incarnation
        });
        match result {
            Ok(message) if same_profile => self.mods.message = Some(Ok(message)),
            Ok(_) => (),
            Err((line, detail)) => {
                let failure = retry.map(|retry| CheckedFailure {
                    error: FormError::new(&line, detail.clone()),
                    retry,
                });
                if !same_profile {
                    if let Some(failure) = failure {
                        self.github.pending_failure = Some(failure);
                    }
                    return;
                }
                if self.ui.page == Page::Installed
                    && self.ui.sheet.is_none()
                    && !self.blocking_dialog()
                {
                    self.github.close();
                    self.open_sheet(Sheet::GithubAdd);
                    self.github.target = self.github_target().ok();
                    self.github.stage = match failure {
                        Some(failure)
                            if self.github.target.as_ref().is_some_and(|current| {
                                failure.retry.target.same_destination(current)
                            }) =>
                        {
                            Stage::InstallFailed(Box::new(failure), false)
                        }
                        _ => {
                            if !local {
                                self.github.text.set(link);
                            }
                            Stage::Failed {
                                error: FormError::new(&line, detail),
                                expanded: false,
                                from_choose: local,
                            }
                        }
                    };
                    self.mods.message = None;
                } else {
                    if let Some(failure) = failure {
                        self.github.pending_failure = Some(failure);
                    }
                    // The notice says what it means; the checker's words stay with the form,
                    // behind Show details, when the player opens it again.
                    self.mods.message = Some(Err(line));
                }
            }
        }
    }

    fn enter_warning(&mut self, pending: PendingConfirm) {
        self.github.stage = Stage::Warn(Box::new(pending));
        self.github.warn_needs_focus = true;
    }

    fn handle_warning_action(&mut self, action: WarningAction) {
        match action {
            WarningAction::Cancel => self.close_sheet(),
            WarningAction::Confirm => self.confirm_unreviewed(),
        }
    }

    fn confirm_unreviewed(&mut self) {
        let Stage::Warn(pending) = std::mem::take(&mut self.github.stage) else {
            return;
        };
        match *pending {
            PendingConfirm::Github(input) => self.submit_checked_github(input),
            PendingConfirm::Local {
                target,
                packages,
                folder,
            } => {
                let label = folder
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .or_else(|| {
                        packages
                            .first()
                            .map(|p| p.source_path.display().to_string())
                    })
                    .unwrap_or_default();
                let names = packages.iter().map(|p| p.name.clone()).collect();
                let input = Arc::new(LocalInput {
                    target,
                    label,
                    names,
                    packages,
                });
                if let Err(error) = self.queue_local(input) {
                    self.github.stage = Stage::Failed {
                        error: FormError::new(&error, String::new()),
                        expanded: false,
                        from_choose: folder.is_none(),
                    };
                } else {
                    self.close_sheet();
                }
            }
        }
    }

    fn pick_local_smod(&mut self, cx: &mut Context<Self>) {
        if self.github.target.is_none() || self.github.busy() {
            return;
        }
        let token = self.github.start_picker(PickerKind::LocalFile);
        let pick = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("Choose a .smod file").into()),
        });
        cx.spawn(async move |this, cx| {
            let outcome: Result<Option<PathBuf>, String> = match pick.await {
                Ok(Ok(paths)) => Ok(paths.and_then(|mut p| p.pop())),
                Ok(Err(e)) => Err(e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                if this.ui.sheet != Some(Sheet::GithubAdd) || this.github.target.is_none() {
                    this.github.clear_picker();
                    return;
                }
                if let Some(path) = this
                    .github
                    .finish_picker(token, PickerKind::LocalFile, outcome)
                {
                    this.start_read_local_smod(path);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_read_local_smod(&mut self, path: PathBuf) {
        if self.ui.sheet != Some(Sheet::GithubAdd) || self.github.target.is_none() {
            return;
        }
        self.github.clear_picker();
        let (tx, rx) = crate::wake::channel();
        self.github.stage = Stage::ReadingLocal {
            rx,
            from_choose: true,
        };
        std::thread::spawn(move || {
            let result = github_mods::read_local_smod(&path)
                .map(|pkg| LocalRead::File(Box::new(pkg)))
                .map_err(|why| FormError::new(tr("Couldn't open that .smod file."), why));
            let _ = tx.send(result);
        });
    }

    fn pick_local_folder(&mut self, cx: &mut Context<Self>) {
        if self.github.target.is_none() || self.github.busy() {
            return;
        }
        let token = self.github.start_picker(PickerKind::LocalFolder);
        let pick = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(tr("Choose a mod folder").into()),
        });
        cx.spawn(async move |this, cx| {
            let outcome: Result<Option<PathBuf>, String> = match pick.await {
                Ok(Ok(paths)) => Ok(paths.and_then(|mut p| p.pop())),
                Ok(Err(e)) => Err(e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            let _ = this.update(cx, |this, cx| {
                if this.ui.sheet != Some(Sheet::GithubAdd) || this.github.target.is_none() {
                    this.github.clear_picker();
                    return;
                }
                if let Some(path) =
                    this.github
                        .finish_picker(token, PickerKind::LocalFolder, outcome)
                {
                    this.start_read_local_folder(path);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_read_local_folder(&mut self, path: PathBuf) {
        if self.ui.sheet != Some(Sheet::GithubAdd) || self.github.target.is_none() {
            return;
        }
        self.github.clear_picker();
        let (tx, rx) = crate::wake::channel();
        self.github.stage = Stage::ReadingLocal {
            rx,
            from_choose: false,
        };
        std::thread::spawn(move || {
            let result = github_mods::read_local_folder(&path)
                .map(|packages| LocalRead::Folder(path, packages))
                .map_err(|why| FormError::new(tr("Couldn't read that mod folder."), why));
            let _ = tx.send(result);
        });
    }

    fn github_resolve(&mut self) {
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

    pub(super) fn github_tick(&mut self) -> bool {
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

    fn github_pick(&mut self) {
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
                        target.instance(),
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

    fn submit_checked_github(&mut self, input: GithubInput) {
        self.close_sheet();
        if let Err(error) = self.queue_github(Arc::new(input)) {
            self.mods.message = Some(Err(error));
        }
    }

    fn github_retry(&mut self) {
        let Stage::InstallFailed(failure, _) = &self.github.stage else {
            return;
        };
        let retry = failure.retry.clone();
        self.close_sheet();
        if let Err(error) = self.queue_github(retry) {
            self.mods.message = Some(Err(error));
        }
    }

    pub(crate) fn github_add_sheet(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let focus = self
            .github
            .focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        if self.github.repo_needs_focus {
            self.github.repo_needs_focus = false;
            window.focus(&focus, cx);
        }
        if matches!(self.github.stage, Stage::Warn(_)) {
            return self.unreviewed_warning_sheet(window, cx);
        }
        let muted = |text: &str| {
            div()
                .text_size(px(tokens::LABEL))
                .text_color(rgb(palette().muted))
                .child(text.to_owned())
        };
        let narrow = f32::from(window.viewport_size().width) < 920.;
        let profile_name = self
            .github
            .target
            .as_ref()
            .map(|t| t.profile_name().to_owned())
            .or_else(|| self.selected_instance().map(|i| i.name.clone()))
            .unwrap_or_else(|| tr("this profile").into());

        // Step 1: choose Repository (local folder or link) or Local .smod file.
        if let Stage::Choose(choice) = self.github.stage {
            let option_card = |id: &'static str,
                               kind: SourceChoice,
                               icon: &'static str,
                               heading: &'static str,
                               desc: &'static str,
                               cx: &mut Context<Self>| {
                let selected = choice == kind;
                let accent = Hsla::from(rgb(palette().accent_ink));
                let rest_border = Hsla::from(rgb(palette().line));
                let hover_border = Hsla::from(rgb(palette().muted));
                div()
                    .id(id)
                    .reach()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .p(px(16.))
                    .rounded(crate::tokens::r(tokens::R_CARD))
                    .border_1()
                    .border_color(if selected {
                        accent.opacity(0.85)
                    } else {
                        rest_border
                    })
                    .bg(if selected {
                        accent.opacity(0.14)
                    } else {
                        Hsla::from(rgb(palette().card_color)).opacity(0.6)
                    })
                    .cursor_pointer()
                    .hover(move |s| s.border_color(if selected { accent } else { hover_border }))
                    .child(
                        div()
                            .size(px(36.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(crate::tokens::r(tokens::R_CONTROL))
                            .border_1()
                            .border_color(rgb(palette().line))
                            .bg(rgb(palette().field))
                            .child(blocks::icon(icon, 18., Hsla::from(rgb(palette().text)))),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(palette().text))
                                    .child(tr(heading)),
                            )
                            .child(
                                div()
                                    .text_size(px(tokens::SMALL))
                                    .text_color(rgb(palette().muted))
                                    .child(tr(desc)),
                            ),
                    )
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.github.stage = Stage::Choose(kind);
                        cx.notify();
                    }))
            };
            let choices = div()
                .flex()
                .when(narrow, |d| d.flex_col())
                .gap(px(12.))
                .child(option_card(
                    "add-source-opt-repo",
                    SourceChoice::Repository,
                    "folder",
                    "Repository (local folder or link)",
                    "Build a mod from a GitHub link or a folder on this computer. Marked From source.",
                    cx,
                ))
                .child(option_card(
                    "add-source-opt-local",
                    SourceChoice::LocalFile,
                    "package",
                    "Local .smod file",
                    "Install a .smod package file already saved on this computer. Marked Local.",
                    cx,
                ));
            let warn_color = Hsla::from(rgb(palette().warning));
            let notice = div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(10.))
                .rounded(crate::tokens::r(tokens::R_CONTROL))
                .border_1()
                .border_color(warn_color.opacity(0.35))
                .bg(warn_color.opacity(0.08))
                .child(blocks::icon("warning", 15., warn_color))
                .child(
                    div()
                        .text_size(px(tokens::SMALL))
                        .text_color(rgb(palette().muted))
                        .child(tr(
                            "Neither option is reviewed by Sunrise, and neither updates automatically.",
                        )),
                );
            let body = div()
                .flex()
                .flex_col()
                .child(sheets::title(tr("Add from source").into()))
                .child(muted(&trf(
                    "Choose where to install an unreviewed mod for {profile}.",
                    &[("profile", &profile_name)],
                )))
                .child(choices)
                .child(notice)
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .gap(px(8.))
                        .child(
                            button("add-source-cancel", tr("Cancel")).on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    this.close_sheet();
                                    cx.notify();
                                },
                            )),
                        )
                        .child(primary("add-source-continue", tr("Continue")).on_click(
                            cx.listener(move |this, _: &ClickEvent, _, cx| {
                                match choice {
                                    SourceChoice::Repository => {
                                        this.github.clear_picker();
                                        this.github.stage = Stage::Repository;
                                        this.github.repo_needs_focus = true;
                                    }
                                    SourceChoice::LocalFile => this.pick_local_smod(cx),
                                }
                                cx.notify();
                            }),
                        )),
                );
            return sheets::frame("github-add-sheet", 560., body, cx);
        }

        // Reading local packages before showing the unreviewed warning.
        if matches!(self.github.stage, Stage::ReadingLocal { .. }) {
            let body = div()
                .flex()
                .flex_col()
                .child(sheets::title(tr("Add from source").into()))
                .child(muted(tr("Reading mod files…")))
                .child(widgets::progress_bar(None, &self.theme).flex_none())
                .child(div().flex().justify_end().child(
                    button("github-add-cancel", tr("Cancel")).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.close_sheet();
                            cx.notify();
                        },
                    )),
                ));
            return sheets::frame("github-add-sheet", 500., body, cx);
        }

        // Failure directly from Step 1 (e.g. bad local .smod or profile target error).
        if let Stage::Failed {
            error,
            expanded,
            from_choose: true,
        } = &self.github.stage
        {
            let can_back = self.github.target.is_some();
            let expanded = *expanded;
            let line = error.line.clone();
            let detail = error.detail.clone();
            let mut body = div()
                .flex()
                .flex_col()
                .child(sheets::title(tr("Add from source").into()))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(tokens::LABEL))
                                .text_color(rgb(palette().danger))
                                .child(line),
                        )
                        .children((!detail.is_empty()).then(|| {
                            button(
                                "github-add-details",
                                crate::mods::refused::details_label(expanded),
                            )
                            .on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    if let Stage::Failed { expanded, .. } = &mut this.github.stage {
                                        *expanded ^= true;
                                    }
                                    cx.notify();
                                },
                            ))
                        })),
                );
            if expanded && !detail.is_empty() {
                body = body.child(crate::mods::refused::details_box(
                    "github-add-error-detail",
                    detail,
                ));
            }
            let body = body.child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .children(can_back.then(|| {
                        button("add-source-back", tr("Back")).on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.github.stage = Stage::Choose(SourceChoice::LocalFile);
                                cx.notify();
                            },
                        ))
                    }))
                    .child(
                        primary("github-add-cancel", tr("Cancel")).on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.close_sheet();
                                cx.notify();
                            },
                        )),
                    ),
            );
            return sheets::frame("github-add-sheet", 500., body, cx);
        }

        let body = div().flex().flex_col();
        let picker = match &self.github.stage {
            Stage::Pick(pick) => Some((pick.as_ref(), false)),
            Stage::Checking(check) => Some((check.pick.as_ref(), true)),
            _ => None,
        };
        let (body, back_action, action) = if let Some((pick, checking)) = picker {
            let rows = div()
                .id("github-add-list")
                .max_h(px(184.))
                .overflow_y_scroll()
                .rounded(crate::tokens::r(tokens::R_CARD))
                .border_1()
                .border_color(rgb(palette().divider))
                .children(pick.input.rows().into_iter().enumerate().map(
                    |(i, (id, name, version))| {
                        let id = id.to_owned();
                        div()
                            .id(("github-add-choice", i))
                            .reach()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .px(px(12.))
                            .h(px(48.))
                            .cursor_pointer()
                            .when(i > 0, |r| {
                                r.border_t_1().border_color(rgb(palette().divider))
                            })
                            .hover(|s| s.bg(rgb(palette().hover)))
                            .child(
                                crate::mods::checkbox(
                                    ("github-add-check", i),
                                    pick.selected.contains(&id),
                                )
                                .tab_stop(false),
                            )
                            .child(crate::mod_icons::tile(
                                None::<gpui::ImageSource>,
                                &id,
                                name,
                                26.,
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name.to_owned()),
                            )
                            .child(muted(version).flex_none())
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if let Stage::Pick(pick) = &mut this.github.stage {
                                    pick.toggle(&id);
                                }
                                cx.notify();
                            }))
                    },
                ));
            let next = primary(
                "github-add-install",
                if checking {
                    tr("Checking fit…")
                } else {
                    tr("Continue")
                },
            );
            let next = if checking || pick.error.is_some() || pick.selected.is_empty() {
                widgets::disabled(next)
            } else {
                next.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.github_pick();
                    cx.notify();
                }))
            };
            (
                body.child(sheets::title(tr("Choose mods to install").into()))
                    .child(muted(&pick.input.subtitle()).truncate())
                    .child(muted(tr(github_mods::WARNING)))
                    .child(rows)
                    .children(pick.error.as_ref().map(|error| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(tokens::LABEL))
                                    .text_color(rgb(palette().danger))
                                    .child(error.line.clone()),
                            )
                            .children((!error.detail.is_empty()).then(|| {
                                div().flex().child(
                                    crate::frame::button(
                                        "github-pick-details",
                                        crate::mods::refused::details_label(pick.details),
                                    )
                                    .on_click(cx.listener(
                                        |this, _: &ClickEvent, _, cx| {
                                            if let Stage::Pick(pick) = &mut this.github.stage {
                                                pick.details ^= true;
                                            }
                                            cx.notify();
                                        },
                                    )),
                                )
                            }))
                            .children((pick.details && !error.detail.is_empty()).then(|| {
                                crate::mods::refused::details_box(
                                    "github-pick-error-detail",
                                    error.detail.clone(),
                                )
                            }))
                    })),
                None,
                next,
            )
        } else {
            let resolving = self.github.busy();
            let checked_failure = match &self.github.stage {
                Stage::InstallFailed(failure, _) => Some(failure.as_ref()),
                _ => None,
            };
            let mut body = if let Some(failure) = checked_failure {
                let selected = match &failure.retry.selection {
                    GithubSelection::Add(packages) => {
                        packages.iter().map(|p| p.name.as_str()).collect::<Vec<_>>()
                    }
                    GithubSelection::Source(packages) => {
                        packages.iter().map(PreparedSource::name).collect()
                    }
                };
                body.child(sheets::title(tr("Add from source").into()))
                    .child(muted(&failure.retry.source.label()).truncate())
                    .child(muted(&selected.join(", ")).truncate())
            } else {
                let focused = !resolving && focus.is_focused(window);
                let mut field_theme = crate::mods::field_theme(self.theme);
                field_theme.glass = true;
                let field = input(focused)
                    .id("github-add-field")
                    .h(px(tokens::ACTION))
                    .px(px(12.))
                    .text_size(px(tokens::BODY))
                    .child(line_edit::content_with(
                        &self.github.text,
                        &field_theme,
                        focused,
                        false,
                        false,
                        "https://github.com/owner/repo".into(),
                    ));
                let field = if resolving {
                    field.opacity(0.55)
                } else {
                    let field = field
                        .track_focus(&focus)
                        .seen()
                        .cursor_text()
                        .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                            window.focus(&focus, cx);
                            cx.notify();
                        }))
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                            match line_edit::handle(&mut this.github.text, e, cx) {
                                line_edit::Key::Pass | line_edit::Key::Escape => return,
                                line_edit::Key::Enter => this.github_resolve(),
                                line_edit::Key::Edited
                                | line_edit::Key::Paste
                                | line_edit::Key::Cut(_) => {
                                    this.github.stage = Stage::Repository;
                                }
                                _ => {}
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }));
                    line_edit::mouse(field, cx, |this| &mut this.github.text)
                };
                let choose_folder = {
                    let btn = button("add-source-pick-folder", tr("Choose local folder…"));
                    if resolving {
                        widgets::disabled(btn)
                    } else {
                        btn.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.pick_local_folder(cx);
                            cx.notify();
                        }))
                    }
                };
                body.child(sheets::title(tr("Add from source").into()))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(muted(tr(
                                "Paste a GitHub link to a repository, release, tag, branch or commit, or choose a mod folder on this computer.",
                            )))
                            .child(field)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(choose_folder)
                                    .child(muted(tr("Marked From source · no auto-updates"))),
                            ),
                    )
            };
            if resolving {
                body = body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(muted(tr("Checking GitHub…")))
                        .child(widgets::progress_bar(None, &self.theme).flex_none()),
                );
            }
            let error = match &self.github.stage {
                Stage::Failed {
                    error, expanded, ..
                } => Some((error, expanded)),
                Stage::InstallFailed(failure, expanded) => Some((&failure.error, expanded)),
                _ => None,
            };
            if let Some((error, expanded)) = error {
                body = body.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(tokens::LABEL))
                                .text_color(rgb(palette().danger))
                                .child(error.line.clone()),
                        )
                        .children((!error.detail.is_empty()).then(|| {
                            button(
                                "github-add-details",
                                crate::mods::refused::details_label(*expanded),
                            )
                            .on_click(cx.listener(
                                |this, _: &ClickEvent, _, cx| {
                                    match &mut this.github.stage {
                                        Stage::Failed { expanded, .. }
                                        | Stage::InstallFailed(_, expanded) => *expanded ^= true,
                                        _ => (),
                                    }
                                    cx.notify();
                                },
                            ))
                        })),
                );
                if *expanded {
                    body = body.child(crate::mods::refused::details_box(
                        "github-add-error-detail",
                        error.detail.clone(),
                    ));
                }
            }
            let back = (checked_failure.is_none() && !resolving).then(|| {
                button("add-source-back", tr("Back")).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.github.clear_picker();
                        this.github.stage = Stage::Choose(SourceChoice::Repository);
                        cx.notify();
                    },
                ))
            });
            let next = primary(
                "github-add-continue",
                if checked_failure.is_some()
                    || matches!(
                        &self.github.stage,
                        Stage::Failed { error, .. } if error.retry
                    )
                {
                    tr("Try again")
                } else {
                    tr("Continue")
                },
            );
            let next = if checked_failure.is_some() {
                next.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.github_retry();
                    cx.notify();
                }))
            } else if self.github.target.is_some() && self.github.can_resolve() {
                next.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.github_resolve();
                    cx.notify();
                }))
            } else {
                widgets::disabled(next)
            };
            (body, back, next)
        };
        let body = body.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .children(back_action)
                .child(div().flex_1())
                .child(
                    button("github-add-cancel", tr("Cancel")).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.close_sheet();
                            cx.notify();
                        },
                    )),
                )
                .child(action),
        );
        sheets::frame("github-add-sheet", 520., body, cx)
    }

    fn unreviewed_warning_sheet(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let cancel_focus = self
            .github
            .cancel_focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        if self.github.warn_needs_focus {
            self.github.warn_needs_focus = false;
            window.focus(&cancel_focus, cx);
        }
        let Stage::Warn(pending) = &self.github.stage else {
            unreachable!("only called for Stage::Warn");
        };
        let badge = pending.badge();
        let multiple = pending.count() > 1;
        let native = pending.any_native();
        let prov = pending.provenance();
        let warn = Hsla::from(rgb(if pending.unsigned_local_file() {
            palette().danger
        } else {
            palette().warning
        }));

        let heading = if multiple {
            tr("These mods have not been reviewed")
        } else {
            tr("This mod has not been reviewed")
        };
        let safety_lead = if multiple {
            tr("Nobody has checked these mods for safety.")
        } else {
            tr("Nobody has checked this mod for safety.")
        };
        let safety_body = if native {
            tr("Native mod code is compiled and runs directly on this computer with your permissions — it can read, change, or delete your files.")
        } else {
            tr("Unreviewed mod files have not been checked by Sunrise and can change how your game behaves.")
        };
        let trust_line = trf(
            "Only continue if you wrote this mod yourself or trust the person who gave it to you. It will be labelled {badge} and will not receive automatic updates.",
            &[("badge", badge.label())],
        );
        let confirm_label = if multiple {
            tr("Install unreviewed mods")
        } else {
            tr("Install unreviewed mod")
        };

        let header = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .px(px(24.))
            .py(px(18.))
            .bg(warn.opacity(0.22))
            .border_b_1()
            .border_color(warn.opacity(0.45))
            .child(
                div()
                    .size(px(44.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(crate::tokens::r(tokens::R_CARD))
                    .bg(warn.opacity(0.2))
                    .border_1()
                    .border_color(warn.opacity(0.55))
                    .child(blocks::icon("warning", 22., warn)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(tokens::CAPTION))
                            .font_weight(FontWeight::BOLD)
                            .text_color(warn)
                            .child(tr("UNREVIEWED CODE WARNING")),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::TITLE))
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(palette().text))
                            .child(heading),
                    ),
            );

        let provenance_card = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(14.))
            .rounded(crate::tokens::r(tokens::R_CARD))
            .bg(warn.opacity(0.10))
            .border_1()
            .border_color(warn.opacity(0.45))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(blocks::icon(prov.icon, 16., warn))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette().text))
                            .child(prov.headline),
                    ),
            )
            .child(
                div()
                    .text_size(px(tokens::SMALL))
                    .text_color(rgb(palette().muted))
                    .child(prov.detail),
            );

        let confirm = button("add-source-confirm", confirm_label)
            .border_color(warn.opacity(0.65))
            .bg(warn.opacity(0.12))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.handle_warning_action(WarningAction::Confirm);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if matches!(e.keystroke.key.as_str(), "enter" | "space") {
                    this.handle_warning_action(WarningAction::Confirm);
                    cx.stop_propagation();
                    cx.notify();
                }
            }));

        let cancel = primary("add-source-cancel", tr("Cancel"))
            .track_focus(&cancel_focus)
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.handle_warning_action(WarningAction::Cancel);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if matches!(e.keystroke.key.as_str(), "enter" | "space") {
                    this.handle_warning_action(WarningAction::Cancel);
                    cx.stop_propagation();
                    cx.notify();
                }
            }));

        let content = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .p(px(24.))
            .child(provenance_card)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette().text))
                            .child(safety_lead),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::LABEL))
                            .text_color(rgb(palette().muted))
                            .child(safety_body),
                    ),
            )
            .child(
                div()
                    .text_size(px(tokens::LABEL))
                    .text_color(rgb(palette().muted))
                    .child(trust_line),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(10.))
                    .child(confirm)
                    .child(cancel),
            );

        let body = div()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(content)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.key == "enter" {
                    this.handle_warning_action(WarningAction::DEFAULT);
                    cx.stop_propagation();
                    cx.notify();
                }
            }));

        sheets::warning_frame("github-add-sheet", 560., warn, body, cx)
    }
}

#[cfg(test)]
pub(crate) mod tests {
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

        let offline =
            pasted_source_error("Jade", SourceError::Github(Failure::Offline("dns".into())));
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
}
