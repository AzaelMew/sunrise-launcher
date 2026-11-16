//! The launcher's scenery: the pixel sunset scene, the themes' painted skies and Hypr Neon's
//! rain (each a GPUI view of its own), the glass plates, and how they move. Kept apart from the
//! launcher so its drawing compiles once and the launcher's own changes rebuild less.
pub mod art;
pub mod clock;
pub mod glass;
pub mod rain;
pub mod scene;
pub mod sky;
