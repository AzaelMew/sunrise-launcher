//! Where the launcher keeps its data, and the build folder its tools can open.
#[cfg(windows)]
use std::fs;
use std::path::{Path, PathBuf};

/// The launcher's data folder; `SUNRISE_LAUNCHER_HOME` overrides it (for tests and scripts).
pub fn data_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("SUNRISE_LAUNCHER_HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(home);
    }
    if cfg!(test) {
        return crate::data_location::test_home().join("launcher");
    }
    // A location that does not open stands in as the default folders, as
    // `data_location::startup` makes it for the window: never a panic here.
    if let Ok(Some(roots)) = crate::data_location::configured_roots() {
        return roots.launcher;
    }
    crate::identity::launcher_folder_in(&dirs::data_dir().unwrap_or_else(std::env::temp_dir))
}

/// A canonical path without Windows' verbatim prefix: `canonicalize` gives `\\?\C:\...` there,
/// and some tools stat `\\?\C:` — the volume itself — and fail. A drive path loses its `\\?\`;
/// any other path is left as it is.
pub(crate) fn plain(path: PathBuf) -> PathBuf {
    crate::data_location::plain_drive(path)
}

/// `path` spelled only in ASCII: what Windows' MinGW-w64 tools can open (they read paths
/// through ANSI APIs, so a Cyrillic user name in %APPDATA% breaks them, #5189).
fn ascii(path: &Path) -> bool {
    crate::data_location::ascii(path)
}

/// `path` as Windows' MinGW-w64 tools can take it: [`ascii`], and on Windows without a space.
/// `windres` hands its include folders to the preprocessor unquoted, so a user name with a space
/// in %APPDATA% splits there ("cc1.exe: fatal error: v\: No such file or directory").
fn tool_safe(path: &Path) -> bool {
    ascii(path) && !(cfg!(windows) && path.as_os_str().to_str().is_some_and(|p| p.contains(' ')))
}

/// Where builds happen — the Rust toolchain, its Cargo home and `native_mods::root` — as a
/// path every build tool can read: `data_dir()` resolved, so the tools are never handed a
/// link (horizon-mixins refuses one, and macOS's /tmp is one). On Windows, when the resolved
/// path isn't [`tool_safe`], [`windows_build_dir`] names the same folder a way MinGW-w64 can open.
pub fn build_dir() -> PathBuf {
    if let Some(root) = crate::data_location::admitted_build_root() {
        return root.to_owned();
    }
    resolve_build_dir(true, |_| Ok(()))
        .expect("unconditional build admission")
        .0
}

pub(crate) fn admit_build_dir<T>(
    admit: impl FnMut(&Path) -> Result<T, String>,
) -> Result<(PathBuf, T), String> {
    resolve_build_dir(true, admit)
}

/// The offline worker must not create a cache directory before acquiring Move's claims.
pub(crate) fn relocation_build_dir() -> PathBuf {
    resolve_build_dir(false, |_| Ok(()))
        .expect("read-only build path lookup")
        .0
}

fn resolve_build_dir<T>(
    #[cfg_attr(not(windows), allow(unused_variables))] provision: bool,
    mut admit: impl FnMut(&Path) -> Result<T, String>,
) -> Result<(PathBuf, T), String> {
    // Tests keep it inside their scratch launcher folder (`data_dir`).
    if !cfg!(test) && std::env::var_os("SUNRISE_LAUNCHER_HOME").is_none_or(|p| p.is_empty()) {
        // A location that does not open stands in as the default folders, as
        // `data_location::startup` makes it for the window: never a panic here.
        if let Ok(Some(roots)) = crate::data_location::configured_roots() {
            let lease = admit(&roots.build)?;
            return Ok((roots.build, lease));
        }
    }
    let data = data_dir();
    // Resolve existing ancestors without creating a root before first-start carry-over.
    let real = crate::data_location::canonical_data_path(&data).unwrap_or(data);
    if tool_safe(&real) {
        let lease = admit(&real)?;
        return Ok((real, lease));
    }
    #[cfg(windows)]
    return windows_build_dir(&real, provision, admit);
    #[cfg(not(windows))]
    {
        let lease = admit(&real)?;
        Ok((real, lease))
    }
}

/// [`build_dir`] when `real` isn't [`tool_safe`]: [`ascii_build_dir`], a real folder of
/// ProgramData's. Only when it can't be created, the data folder's short (8.3) name.
///
/// The short name came first until #6009, and wasn't enough: it is only another spelling of the
/// same folder, and rustc canonicalizes its own path to find its sysroot, which spells the long,
/// Cyrillic name again and hands it to the linker (`x86_64-w64-mingw32-gcc`: "No such file or
/// directory" on `C:/Users/\xca\xe8…`). A folder whose real name is ASCII stays ASCII however a
/// tool resolves it.
#[cfg(windows)]
fn windows_build_dir<T>(
    real: &Path,
    provision: bool,
    mut admit: impl FnMut(&Path) -> Result<T, String>,
) -> Result<(PathBuf, T), String> {
    let dir = ascii_build_dir(real, std::env::var_os("ProgramData").map(PathBuf::from));
    // ProgramData is writable by every account, so the folder may be another account's: its
    // `bin` and `cargo\config.toml` would run as this player. Only a folder made here, or owned
    // by this account, is used.
    // The parent is made with `create_dir_all` (ProgramData may be missing); only the final
    // component is made with `create_dir`, which fails on a folder that already exists.
    for candidate in build_dir_candidates(&dir) {
        if candidate.is_dir() && owned_by_this_account(&candidate) {
            let lease = admit(&candidate)?;
            return Ok((candidate, lease));
        }
        if !provision || candidate.exists() {
            continue;
        }
        let lease = admit(&candidate)?;
        if let Some(parent) = candidate.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::create_dir(&candidate).is_ok()
            || (candidate.is_dir() && owned_by_this_account(&candidate))
        {
            return Ok((candidate, lease));
        }
    }
    // Never an unclaimed folder: the short name of the data folder, else a folder of this
    // process's own in the temporary folder.
    let path = build_fallback(short_path(real), std::process::id(), &std::env::temp_dir());
    let lease = admit(&path)?;
    Ok((path, lease))
}

/// The build folder when no numbered name could be claimed: `short` if it is [`tool_safe`],
/// else a folder named for the process under `temp`, which no other account made.
#[cfg_attr(not(windows), allow(dead_code))]
fn build_fallback(short: Option<PathBuf>, pid: u32, temp: &Path) -> PathBuf {
    short
        .filter(|p| tool_safe(p))
        .unwrap_or_else(|| temp.join(format!("sunrise-launcher-{pid}")))
}

/// `base`, else `base-2`, `base-3`… (up to `base-9`).
#[cfg(any(windows, test))]
fn build_dir_candidates(base: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    let name = base.file_name().expect("cache base name").to_os_string();
    std::iter::once(base.to_path_buf()).chain((2..=9).map(move |n| {
        let mut next = name.clone();
        next.push(format!("-{n}"));
        base.with_file_name(next)
    }))
}

/// Whether the folder `path` is owned by the account this process runs as: the owner a folder
/// it creates gets (the token's default owner). A folder the same player made in an elevated
/// session is owned by Administrators, so it is passed over and a numbered folder is made
/// instead: safe, but the toolchain is built again there.
#[cfg(windows)]
fn owned_by_this_account(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
    use windows_sys::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, TokenOwner, OWNER_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, PSID, TOKEN_OWNER, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: every pointer handed over is to a live local; the token and the descriptor
    // GetNamedSecurityInfoW allocates are released before returning, and the owner SID is only
    // read while that descriptor is alive.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut buffer = vec![0u64; 128];
        let mut length = 0;
        let got = GetTokenInformation(
            token,
            TokenOwner,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * 8) as u32,
            &mut length,
        );
        CloseHandle(token);
        if got == 0 {
            return false;
        }
        let ours: PSID = (*buffer.as_ptr().cast::<TOKEN_OWNER>()).Owner;
        let mut owner: PSID = std::ptr::null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let status = GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut descriptor,
        );
        if status != 0 {
            return false;
        }
        let same = !owner.is_null() && EqualSid(owner, ours) != 0;
        LocalFree(descriptor);
        same
    }
}

/// The build folder for a data folder `real` whose path isn't [`ascii`]: in `program_data`
/// (`%ProgramData%`, `C:\ProgramData` when unset, relative or not ASCII), which every user may
/// create folders in, named for `real` so each user's is their own.
#[cfg_attr(not(windows), allow(dead_code))]
fn ascii_build_dir(real: &Path, program_data: Option<PathBuf>) -> PathBuf {
    use sha2::{Digest, Sha256};
    let public = program_data
        .filter(|p| p.is_absolute() && tool_safe(p))
        .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
    let key = Sha256::digest(real.as_os_str().as_encoded_bytes());
    let key: String = key[..4].iter().map(|b| format!("{b:02x}")).collect();
    public.join(format!("sunrise-launcher-{key}"))
}

/// `GetShortPathNameW`: `path`'s short (8.3) name, or None on error. A folder with no short
/// name (the volume may keep none) answers with its long name again, which [`ascii`] refuses.
#[cfg(windows)]
fn short_path(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    #[link(name = "kernel32")]
    extern "system" {
        fn GetShortPathNameW(long: *const u16, short: *mut u16, len: u32) -> u32;
    }
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let need = unsafe { GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0) } as usize;
    if need == 0 {
        return None;
    }
    let mut buf = vec![0u16; need];
    let got = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), need as u32) } as usize;
    (got != 0 && got < need).then(|| PathBuf::from(std::ffi::OsString::from_wide(&buf[..got])))
}

#[cfg(test)]
mod tests;
