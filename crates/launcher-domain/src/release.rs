//! One entry of a release feed, as GitHub publishes it.

/// A published build: the launcher's own releases and the game's, from the same feed shape.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Release {
    pub tag: String,
    pub name: String,
    pub body: String,
    pub date: String,
    /// When it was published, `HH:MM` UTC (empty if GitHub didn't say).
    pub time: String,
    /// Full publication timestamp (`published_at`, RFC 3339; empty if GitHub didn't say).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub published_at: String,
    /// The preferred archive, or the SDK index if a game release has no player archive here.
    pub asset: Option<(String, u64)>,
    /// Published on GitHub as a pre-release. The rolling Nightly is normalized separately;
    /// updates are chosen by the profile's channel, not by this flag alone.
    pub prerelease: bool,
    /// The GitHub tag for signed pre-release builds: a Nightly's may differ from the installed
    /// `tag`; a Dev build uses the same exact tag for both.
    /// None for a release.
    pub nightly: Option<String>,
}
