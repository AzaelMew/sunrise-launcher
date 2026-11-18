//! Blurring the screenshots on screen (the mockup's `PrivacyToggle`, `launcher-views.tsx`): the
//! eye button beside Overview's and the Screenshots tab's pictures, which draws them as
//! `blur-2xl` would. GPUI has no image filter, so each picture is made once, small and blurred,
//! in the background (`picture_cache`), and drawn stretched. For the session only, like the
//! mockup's.









#[derive(Default)]
pub(super) struct ShotBlur {
    /// Whether the screenshots are blurred.
    pub(super) hidden: bool,
}


