//! Pointer-operated scrollbars for the launcher's vertical scroll areas.

use gpui::ScrollHandle;

#[derive(Clone, Default)]
pub(crate) struct Scrollbar {
    pub handle: ScrollHandle,
}
