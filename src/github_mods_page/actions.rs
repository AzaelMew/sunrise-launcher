//! State changes behind the "Add from source" sheet.
use super::*;

/// Close the sheet, canceling the work it started.
pub(super) fn cancel(this: &mut Launcher) {
    this.close_sheet();
}
