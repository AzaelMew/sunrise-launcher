//! Which architecture a game build is for: the platform's choices, the installed build's PE
//! header, and what to say when they don't match.
use super::{
    asset_installed, installation, platform_assets, same, version_dir, Release, Settings, CLIENT,
    COMPLETE,
};
use std::fs;
use std::path::Path;

/// The game architecture selected in Settings. Auto preserves the launcher's existing behavior:
/// it follows the process architecture, even when an x64 launcher runs on Windows ARM.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameArchitecture {
    #[default]
    Auto,
    X64,
    Arm64,
}

impl GameArchitecture {
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::X64 => "x64",
            Self::Arm64 => "arm64",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" | "default" => Some(Self::Auto),
            "x64" | "x86_64" | "amd64" => Some(Self::X64),
            "arm64" | "aarch64" => Some(Self::Arm64),
            _ => None,
        }
    }
}

pub(crate) const GAME_BUILD_ARCHITECTURE: &str = ".sunrise-game-architecture.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct InstalledGameArchitecture {
    /// The architecture requested when this version was installed.
    pub(super) requested: GameArchitecture,
    /// The archive's actual architecture (x64 may be the safe ARM64 fallback).
    pub(super) binary: GameArchitecture,
}

fn architecture_for_platform(architecture: &str) -> Option<GameArchitecture> {
    match architecture {
        "x86_64" => Some(GameArchitecture::X64),
        "aarch64" => Some(GameArchitecture::Arm64),
        _ => None,
    }
}

pub(super) fn architecture_for_asset(asset: &str) -> Option<GameArchitecture> {
    match asset.rsplit('/').next()? {
        "sunrise-windows-x64.zip" => Some(GameArchitecture::X64),
        "sunrise-windows-arm64.zip" => Some(GameArchitecture::Arm64),
        _ => None,
    }
}

/// Detect the native Windows machine even if this launcher process is running under emulation.
#[cfg(windows)]
pub(super) fn native_windows_architecture() -> Option<&'static str> {
    #[repr(C)]
    struct SystemInfo {
        processor_architecture: u16,
        _reserved: u16,
        _page_size: u32,
        _minimum_application_address: *mut std::ffi::c_void,
        _maximum_application_address: *mut std::ffi::c_void,
        _active_processor_mask: usize,
        _number_of_processors: u32,
        _processor_type: u32,
        _allocation_granularity: u32,
        _processor_level: u16,
        _processor_revision: u16,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetModuleHandleA(module_name: *const std::ffi::c_char) -> *mut std::ffi::c_void;
        fn GetProcAddress(
            module: *mut std::ffi::c_void,
            name: *const std::ffi::c_char,
        ) -> Option<unsafe extern "system" fn()>;
        fn GetNativeSystemInfo(info: *mut SystemInfo);
    }

    type IsWow64Process2 =
        unsafe extern "system" fn(*mut std::ffi::c_void, *mut u16, *mut u16) -> i32;

    // IsWow64Process2 reports the ARM64 host for x64 emulation; GetNativeSystemInfo reports the
    // emulated processor for compatibility. Resolve it dynamically for older Windows versions.
    let kernel32 = unsafe { GetModuleHandleA(c"kernel32.dll".as_ptr()) };
    let is_wow64_process2 = if kernel32.is_null() {
        None
    } else {
        unsafe { GetProcAddress(kernel32, c"IsWow64Process2".as_ptr()) }
    };
    if let Some(function) = is_wow64_process2 {
        // SAFETY: GetProcAddress resolved this exact Kernel32 API, whose signature is declared
        // above with the documented system calling convention.
        let is_wow64_process2: IsWow64Process2 = unsafe { std::mem::transmute(function) };
        let (mut process_machine, mut native_machine) = (0u16, 0u16);
        // SAFETY: GetCurrentProcess returns a pseudo-handle accepted by IsWow64Process2, and both
        // output pointers refer to live USHORTs.
        if unsafe {
            is_wow64_process2(
                GetCurrentProcess(),
                &mut process_machine,
                &mut native_machine,
            )
        } != 0
        {
            return match native_machine {
                0x8664 => Some("x86_64"),  // IMAGE_FILE_MACHINE_AMD64
                0xaa64 => Some("aarch64"), // IMAGE_FILE_MACHINE_ARM64
                _ => None,
            };
        }
    }

    // SAFETY: GetNativeSystemInfo writes the documented SYSTEM_INFO layout into this repr(C)
    // buffer and has no failure return value. This fallback preserves existing behavior on older
    // Windows versions without IsWow64Process2.
    let mut info: SystemInfo = unsafe { std::mem::zeroed() };
    unsafe { GetNativeSystemInfo(&mut info) };
    match info.processor_architecture {
        9 => Some("x86_64"),   // PROCESSOR_ARCHITECTURE_AMD64
        12 => Some("aarch64"), // PROCESSOR_ARCHITECTURE_ARM64
        _ => None,
    }
}

#[cfg(not(windows))]
pub(super) fn native_windows_architecture() -> Option<&'static str> {
    None
}

pub(super) fn supports_game_architecture_choice(os: &str, native: Option<&str>) -> bool {
    os == "windows" && native == Some("aarch64")
}

/// Resolves the game's asset and fallback. Auto follows the process architecture, preserving
/// existing x64, Linux and macOS behavior. Manual choices are honored on native Windows ARM.
pub(super) fn game_assets_for(
    os: &str,
    native: Option<&str>,
    process: &str,
    choice: GameArchitecture,
) -> Option<(&'static str, Option<&'static str>)> {
    let arch = if supports_game_architecture_choice(os, native) {
        match choice {
            GameArchitecture::Auto => process,
            GameArchitecture::X64 => "x86_64",
            GameArchitecture::Arm64 => "aarch64",
        }
    } else {
        process
    };
    let (asset, _) = platform_assets(os, arch)?;
    Some((asset, game_fallback_for(os, arch)))
}

pub(super) fn selected_game_architecture(settings: &Settings) -> Option<GameArchitecture> {
    if std::env::consts::OS != "windows" {
        return None;
    }
    let arch = if supports_game_architecture_choice("windows", native_windows_architecture()) {
        match settings.game_architecture {
            GameArchitecture::Auto => std::env::consts::ARCH,
            GameArchitecture::X64 => "x86_64",
            GameArchitecture::Arm64 => "aarch64",
        }
    } else {
        std::env::consts::ARCH
    };
    architecture_for_platform(arch)
}

/// True for both native ARM64 launchers and x64 launchers under ARM64 emulation.
pub fn can_choose_game_architecture() -> bool {
    supports_game_architecture_choice("windows", native_windows_architecture())
}

/// A downloaded version records its requested and actual architectures. Old installs and imports
/// have no marker; their PE machine type is used for both.
pub(super) fn installed_game_architecture(tag: &str) -> Option<InstalledGameArchitecture> {
    let dir = version_dir(tag);
    fs::read(dir.join(GAME_BUILD_ARCHITECTURE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .or_else(|| {
            let binary = pe_architecture_file(&dir.join(CLIENT))?;
            Some(InstalledGameArchitecture {
                requested: binary,
                binary,
            })
        })
}

fn pe_architecture_file(path: &Path) -> Option<GameArchitecture> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = fs::File::open(path).ok()?;
    let mut dos = [0u8; 0x40];
    file.read_exact(&mut dos).ok()?;
    if &dos[..2] != b"MZ" {
        return None;
    }
    let pe = u32::from_le_bytes(dos[0x3c..0x40].try_into().ok()?);
    file.seek(SeekFrom::Start(u64::from(pe))).ok()?;
    let mut header = [0u8; 6];
    file.read_exact(&mut header).ok()?;
    (&header[..4] == b"PE\x00\x00")
        .then(|| architecture_for_pe_machine(u16::from_le_bytes([header[4], header[5]])))
        .flatten()
}

pub(super) fn architecture_for_pe_machine(machine: u16) -> Option<GameArchitecture> {
    match machine {
        0x8664 => Some(GameArchitecture::X64),
        0xaa64 => Some(GameArchitecture::Arm64),
        _ => None,
    }
}

pub(super) fn installed_architecture_matches(
    target: GameArchitecture,
    installed: InstalledGameArchitecture,
) -> bool {
    installed.requested == target || installed.binary == target
}

/// The exact installed build can be repaired only while it is still offered for this setting.
/// This is a same-build operation, not an update candidate.
pub fn architecture_reinstall<'a>(
    releases: &'a [Release],
    tag: &str,
    choice: GameArchitecture,
) -> Option<&'a Release> {
    if !can_choose_game_architecture() || !version_dir(tag).join(COMPLETE).is_file() {
        return None;
    }
    architecture_reinstall_for(
        releases,
        tag,
        choice,
        std::env::consts::ARCH,
        installed_game_architecture(tag),
    )
}

pub(super) fn architecture_reinstall_for<'a>(
    releases: &'a [Release],
    tag: &str,
    choice: GameArchitecture,
    process: &str,
    installed: Option<InstalledGameArchitecture>,
) -> Option<&'a Release> {
    let channel = crate::instances::Channel::of_build(tag)?;
    let release = channel.builds(releases).find(|r| r.tag == tag)?;
    let (asset, fallback) = game_assets_for("windows", Some("aarch64"), process, choice)?;
    let expected = architecture_for_asset(asset_installed(release, Some(asset), fallback)?)?;
    let target = match choice {
        GameArchitecture::Auto => architecture_for_platform(process)?,
        explicit => explicit,
    };
    // A recorded x64 fallback for ARM64 is valid until an ARM64 archive is offered.
    installed
        .is_none_or(|have| !installed_architecture_matches(target, have) || have.binary != expected)
        .then_some(release)
}

pub(super) fn native_mod_architecture_message(
    process: GameArchitecture,
    selected: GameArchitecture,
) -> Option<&'static str> {
    (process != selected).then_some(if process == GameArchitecture::X64 {
        "Native mods cannot build for the selected ARM64 game yet. The available compiler builds x64 only."
    } else {
        "Native mods cannot build for the selected x64 game yet. This ARM64 launcher has no matching compiler."
    })
}

pub(crate) fn native_mod_architecture_error() -> Option<String> {
    if !can_choose_game_architecture() {
        return None;
    }
    let process = architecture_for_platform(std::env::consts::ARCH)?;
    let selected = selected_game_architecture(&Settings::load())?;
    native_mod_architecture_message(process, selected)
        .map(|message| crate::i18n::tr(message).to_string())
}

/// Refuse an explicit game architecture mismatch. CLI Play and server startup do not fetch a
/// catalogue to guess whether the exact installation can still be repaired.
pub fn game_architecture_launch_error(tag: &str) -> Option<String> {
    game_architecture_launch_error_with_catalogue(tag, &[])
}

/// The window can recommend a reinstall from the version picker only when it offers that exact repair.
pub fn game_architecture_launch_error_with_catalogue(
    tag: &str,
    releases: &[Release],
) -> Option<String> {
    if !can_choose_game_architecture() {
        return None;
    }
    let choice = Settings::load().game_architecture;
    if choice == GameArchitecture::Auto {
        return None;
    }
    architecture_mismatch_for(
        tag,
        releases,
        choice,
        std::env::consts::ARCH,
        installed_game_architecture(tag),
    )
}

pub(crate) fn managed_game_architecture_launch_error(
    tag: &str,
    variant: Option<&installation::BoundVariant>,
) -> Option<String> {
    if !can_choose_game_architecture() {
        return None;
    }
    let choice = Settings::load().game_architecture;
    if choice == GameArchitecture::Auto {
        return None;
    }
    check_game_architecture(tag, variant, choice)
}

pub(crate) fn check_game_architecture(
    tag: &str,
    variant: Option<&installation::BoundVariant>,
    choice: GameArchitecture,
) -> Option<String> {
    let installed = match variant {
        Some(variant) => variant.architecture(),
        None => installed_game_architecture(tag),
    };
    // A saved binding can't silently adopt a different archive, even for the same tag.
    architecture_mismatch_for(tag, &[], choice, std::env::consts::ARCH, installed)
}

pub(super) fn architecture_mismatch_for(
    tag: &str,
    releases: &[Release],
    choice: GameArchitecture,
    process: &str,
    installed: Option<InstalledGameArchitecture>,
) -> Option<String> {
    if installed.is_some_and(|have| installed_architecture_matches(choice, have)) {
        return None;
    }
    let label = |arch| match arch {
        GameArchitecture::X64 => "x64".to_owned(),
        GameArchitecture::Arm64 => "ARM64".to_owned(),
        GameArchitecture::Auto => crate::i18n::tr("Automatic").to_string(),
    };
    let available = architecture_reinstall_for(releases, tag, choice, process, installed).is_some();
    let installed = installed
        .map(|have| label(have.binary))
        .unwrap_or_else(|| crate::i18n::tr("Unknown").to_string());
    Some(crate::i18n::trf(
        if available {
            "This version has a {installed} game build, but Settings selects {selected}. Reinstall it from the version picker beside Play before launching."
        } else {
            "This version has a {installed} game build, but Settings selects {selected}. Switch back to the installed architecture or Automatic, or choose another available build."
        },
        &[("installed", &installed), ("selected", &label(choice))],
    ))
}

/// The asset a platform's launcher falls back to when a release has no build of its own.
pub(super) const fn game_fallback_for(os: &str, arch: &str) -> Option<&'static str> {
    if same(os, "windows") && same(arch, "aarch64") {
        match platform_assets("windows", "x86_64") {
            Some((game, _)) => Some(game),
            None => None,
        }
    } else {
        None
    }
}
