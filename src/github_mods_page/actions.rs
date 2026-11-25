//! State changes behind the "Add from source" sheet.
use super::*;

/// Close the sheet, canceling the work it started.
pub(super) fn cancel(this: &mut Launcher) {
    this.close_sheet();
}

/// Start the repository step from the first source choice.
pub(super) fn start_repository(this: &mut Launcher) {
    this.github.clear_picker();
    this.github.stage = Stage::Repository;
    this.github.repo_needs_focus = true;
}

/// Go back from the source step to the repository step.
pub(super) fn back_to_repository(this: &mut Launcher) {
    this.github.clear_picker();
    this.github.stage = Stage::Choose(SourceChoice::Repository);
}

/// Read a `.smod` file chosen on this computer.
pub(super) fn read_local(this: &mut Launcher, cx: &mut Context<Launcher>) {
    this.pick_local_smod(cx);
}

/// Read a mod folder chosen on this computer.
pub(super) fn pick_folder(this: &mut Launcher, cx: &mut Context<Launcher>) {
    this.pick_local_folder(cx);
}

/// Resolve the link or folder typed in the source step.
pub(super) fn resolve(this: &mut Launcher) {
    this.github_resolve();
}

/// Continue with the mods selected in the list.
pub(super) fn pick(this: &mut Launcher) {
    this.github_pick();
}

/// Retry an install that failed its fit check.
pub(super) fn retry(this: &mut Launcher) {
    this.github_retry();
}
