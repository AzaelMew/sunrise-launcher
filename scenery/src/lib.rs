//! The launcher's scenery: the pixel sunset scene (a GPUI view of its own), the glass plates that
//! frost it, and how it moves. Kept apart from the launcher so its drawing compiles once and the
//! launcher's own changes rebuild less.
pub mod art;
pub mod clock;
pub mod glass;
pub mod scene;
