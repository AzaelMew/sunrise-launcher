//! One Linux compiler attempt's subreaper. A process group alone cannot account for a child
//! that creates a fresh session. The launcher owns this helper, not the untrusted compiler.
use std::{ffi::OsString, io, process::Command};

#[doc(hidden)]
pub const ARG: &str = "--internal-compiler-worker";
const NO_WORKER: i32 = 124;
const UNPROVED: i32 = 125;

/// Only the helper may report a proved ordinary tool exit. A signal or an unavailable proof
/// retains the private tree. The wire tool only has defined exit codes 0 through 3.
pub(crate) fn proved(status: &std::process::ExitStatus) -> bool {
    matches!(status.code(), Some(0..=3))
}

/// The helper failed before it launched any compiler process.
pub(crate) fn no_worker(status: &std::process::ExitStatus) -> bool {
    status.code() == Some(NO_WORKER)
}

/// Entry point reached before launcher initialization. It passes inherited stdio, process
/// group and environment to the compiler command it wraps.
#[doc(hidden)]
pub fn run(mut args: impl Iterator<Item = OsString>) -> i32 {
    let Some(program) = args.next() else {
        return NO_WORKER;
    };
    // SAFETY: this is a fresh launcher helper, not the GUI or any other worker. Linux
    // reparents orphan descendants here, even if they left the compiler's process group.
    if unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) } != 0 {
        return NO_WORKER;
    }
    let Ok(mut child) = Command::new(program).args(args).spawn() else {
        return NO_WORKER;
    };
    let Ok(status) = child.wait() else {
        return UNPROVED;
    };
    // Reap adopted dead children. Only ECHILD proves nothing in the tree can still write.
    loop {
        // SAFETY: waitpid waits on children of this helper only; no status pointer is needed.
        let who = unsafe { libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG) };
        if who > 0 {
            continue;
        }
        if who == 0 {
            return UNPROVED;
        }
        match io::Error::last_os_error().raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::ECHILD) if proved(&status) => return status.code().unwrap_or(UNPROVED),
            _ => return UNPROVED,
        }
    }
}
