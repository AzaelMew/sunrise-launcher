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

// A Windows DLL with a long name, imported by `raw-dylib` as the `windows` crates do: linking
// the mod writes `<dll>.dll_imports.def` under Cargo's `deps` folder (`_imports_indirect.def`
// when the import is a dependency's, as in #396). In the default home this name takes that path
// past Windows' 260 characters, where the linker's `dlltool` can't open it.
#[cfg(windows)]
#[link(name = "api-ms-win-core-windowserrorreporting-l1-1-0", kind = "raw-dylib")]
unsafe extern "system" {
    fn WerSetFlags(flags: u32) -> i32;
}

/// Uses the import, so the build links it.
#[cfg(windows)]
pub fn report_flags(flags: u32) -> i32 {
    unsafe { WerSetFlags(flags) }
}
