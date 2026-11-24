//! A native mod with no hooks: its one `[[native.mixin]]` row is all CI's Windows build needs.
use mod_api::Hooks;

#[derive(Default)]
struct Mod;

#[mod_api::sunrise_mod]
impl Hooks for Mod {}

/// Uses the registry dependency, so the build compiles it.
pub fn digits(n: u32) -> usize {
    itoa::Buffer::new().format(n).len()
}

// A Windows DLL with a long name, imported as a crates.io dependency's `raw-dylib` import is:
// linking it writes `<dll>.dll_imports_indirect.def` into Cargo's `deps` folder, the file #396's
// build could not open past 260 characters.
#[cfg(windows)]
#[link(name = "api-ms-win-core-backgroundtask-l1-1-0", kind = "raw-dylib")]
unsafe extern "system" {
    fn RaiseCustomSystemEventTrigger(config: *const core::ffi::c_void) -> u32;
}

/// Uses the import, so the build links it.
#[cfg(windows)]
pub fn raise(config: *const core::ffi::c_void) -> u32 {
    unsafe { RaiseCustomSystemEventTrigger(config) }
}
