//! The outside addresses the launcher has built in, in one place: Sunrise's own, and the game's,
//! Mojang's and its sign-in's.

/// The signed v2 mod registry's index, published by sunrise-org/sunrise-mods
/// (`SUNRISE_MOD_INDEX_V2` overrides it).
pub const MOD_INDEX_V2: &str = "https://sunrise-org.github.io/sunrise-mods/v2/index.signed";

/// The registry's Nightly source listings. A listing does not vet the source or authorize code.
pub const MOD_SOURCES: &str = "https://sunrise-org.github.io/sunrise-mods/sources.json";

/// A registry project's page on the website is `<this>/<slug>`.
pub const MOD_PAGES: &str = "https://playsunrise.net/mods";

/// Where the registry's icons and screenshots are served from: the same GitHub Pages site.
pub const MOD_IMAGES: &str = "https://sunrise-org.github.io/sunrise-mods/";

/// The registry service's API, through the website: ratings (`/ratings`, `/<id>/rating`).
pub const MOD_API: &str = "https://playsunrise.net/api/mods";

/// The telemetry ingest server (`crates/sunrise-telemetry-server` in the game repository): opt-in
/// telemetry, the "I'm playing" ping, benchmark uploads (`SUNRISE_TELEMETRY_URL` overrides it).
pub const TELEMETRY: &str = "https://telemetry.playsunrise.net";

/// The GitHub profile of who made the launcher's look, credited in Settings' About (its picture is
/// `<this>.png`).
pub const LAUNCHER_LOOK_CREDIT: &str = "https://github.com/Xmilon";

/// The official launcher installer's product home.
pub const LAUNCHER_ABOUT: &str = "https://playsunrise.net";

/// Where the game repository publishes each game build's mod SDK (`sdk-<build id>`) and the game
/// releases whose `sdk.json` names their build; a release's tag and file follow.
/// `SUNRISE_SDK_BASE` overrides it.
pub const SDK_RELEASES: &str = "https://github.com/sunrise-org/sunrise/releases/download";

// ------------------------------------------------------------------------------------------------
// Launcher-only (not in the game's crate)
// ------------------------------------------------------------------------------------------------

/// GitHub's public API, for releases, commits and archives.
pub const GITHUB_API: &str = "https://api.github.com";

/// Mojang's asset objects, by their hash.
pub const MOJANG_RESOURCES: &str = "https://resources.download.minecraft.net";

/// The index of Mojang's Java runtimes the official launcher reads.
pub const MOJANG_JAVA_RUNTIMES: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// Microsoft's consumer sign-in endpoints (the device code and tokens).
pub const MICROSOFT_LOGIN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";

/// The Minecraft services: the profile, its skin and its capes.
pub const MINECRAFT_SERVICES: &str = "https://api.minecraftservices.com";

/// Xbox Live's user authentication.
pub const XBOX_USER_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";

/// Xbox Live's XSTS token exchange.
pub const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

/// The page that shows a device code to sign in with Microsoft.
pub const MICROSOFT_LINK: &str = "https://www.microsoft.com/link";

/// Where an adult adds a child account to a Microsoft family.
pub const MICROSOFT_FAMILY: &str = "https://account.microsoft.com/family";

/// Mojang's texture host and path; a texture id follows, without a scheme (profiles list `http://`).
pub const MINECRAFT_TEXTURE_PATH: &str = "textures.minecraft.net/texture/";

/// Tailscale's admin console.
pub const TAILSCALE_ADMIN: &str = "https://login.tailscale.com/admin";

/// Where a Tailscale API access token is made.
pub const TAILSCALE_KEYS: &str = "https://login.tailscale.com/admin/settings/keys";

/// Tailscale's access controls page.
pub const TAILSCALE_ACCESS: &str = "https://login.tailscale.com/admin/acls/file";

/// Tailscale's API.
pub const TAILSCALE_API: &str = "https://api.tailscale.com";

/// Tailscale's installers, per system.
pub const TAILSCALE_DOWNLOAD_MAC: &str = "https://tailscale.com/download/mac";
pub const TAILSCALE_DOWNLOAD_WINDOWS: &str = "https://tailscale.com/download/windows";
pub const TAILSCALE_DOWNLOAD_LINUX: &str = "https://tailscale.com/download/linux";

/// The Rust release native mods build with (the date is `rust_toolchain::VERSION`'s).
pub const RUST_DIST: &str = "https://static.rust-lang.org/dist/2026-10-15";

/// WinLibs' MinGW-w64 toolchain, and LLVM-MinGW's, as released for Windows.
pub const WINLIBS_MINGW: &str = "https://github.com/brechtsanders/winlibs_mingw/releases/download/16.2.0posix-23.1.3-14.0.0-msvcrt-r3/winlibs-x86_64-posix-seh-gcc-16.2.0-mingw-w64msvcrt-14.0.0-r3.zip";
pub const LLVM_MINGW: &str = "https://github.com/mstorsjo/llvm-mingw/releases/download/20261104/llvm-mingw-20261104-ucrt-aarch64.zip";
