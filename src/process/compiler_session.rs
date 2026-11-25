//! One macOS compiler attempt's session. macOS has no subreaper (Linux's
//! [`super::compiler_worker`]) and no job object (Windows'), so the tool starts a session of its
//! own and its workers are proved stopped once no process is left in it. Cargo moves workers to
//! their own process groups, never out of the session; a process that calls `setsid()` again
//! does leave it, and with it the check: on macOS only, a build script that daemonizes itself
//! goes unseen, where Linux's subreaper still finds it. That is code the player chose to run
//! with `--trust-native-code`. As before (launcher b755b9ba7), the compiler files are read or
//! published only after the whole tree is proved stopped, and a live member keeps them private.
use std::{io, os::unix::process::CommandExt, process::Command};

/// Starts `command` as the leader of a new session, which is also its process group.
pub(crate) fn lead(command: &mut Command) {
    // SAFETY: setsid is async-signal-safe; the child is not a process group leader, because
    // this command sets no process group of its own.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// Whether no process is left in the session `leader` led, after the leader itself was reaped.
/// A member still alive, or a process list that could not be read whole, is no proof.
pub(crate) fn ended(leader: u32) -> bool {
    let Ok(session) = libc::pid_t::try_from(leader) else {
        return false;
    };
    // SAFETY: a null buffer only asks how many processes there are.
    let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let Ok(count) = usize::try_from(count) else {
        return false;
    };
    // Room for processes started since the count; a full buffer may have left some out.
    let mut pids: Vec<libc::pid_t> = vec![0; count + 256];
    let Ok(bytes) = libc::c_int::try_from(std::mem::size_of_val(pids.as_slice())) else {
        return false;
    };
    // SAFETY: the buffer holds `bytes` bytes of pid_t, and the call writes at most that many.
    let listed = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
    let Ok(listed) = usize::try_from(listed) else {
        return false;
    };
    if listed == 0 || listed >= pids.len() {
        return false;
    }
    // A process that exited between the list and getsid answers ESRCH: it holds nothing open.
    // SAFETY: getsid only reads the session of the pid it is given.
    pids[..listed]
        .iter()
        .all(|&pid| unsafe { libc::getsid(pid) } != session)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The macOS proof itself, so CI's macOS check runs it on every pull request: a tool that
    /// exits with its workers is proved, one that leaves a worker behind is not.
    #[test]
    fn a_session_is_proved_only_once_every_member_has_exited() {
        for (script, proved) in [("exit 0", true), ("/bin/sleep 30 & exit 0", false)] {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            lead(&mut command);
            let mut child = command.spawn().unwrap();
            child.wait().unwrap();
            let leader = child.id();
            assert_eq!(ended(leader), proved, "{script}");
            // SAFETY: the group is the test's own session, led by the child it spawned.
            unsafe { libc::killpg(leader as libc::pid_t, libc::SIGKILL) };
        }
    }
}
