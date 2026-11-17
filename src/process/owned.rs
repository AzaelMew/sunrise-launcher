//! Children a worker waits on (a version's preparer and porter): the launcher stops those still running when it quits, so none outlives it.
use std::io;
use std::process::{Child, Command, ExitStatus};
use std::sync::{Mutex, MutexGuard, PoisonError};

static RUNNING: Mutex<Vec<u32>> = Mutex::new(Vec::new());

fn running() -> MutexGuard<'static, Vec<u32>> {
    RUNNING.lock().unwrap_or_else(PoisonError::into_inner)
}

fn forget(pid: u32) {
    running().retain(|&running| running != pid);
}

/// A child the launcher stops when it quits; one dropped before it ended is stopped then.
pub struct Owned(Child);

impl Owned {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        let child = command.spawn()?;
        running().push(child.id());
        Ok(Self(child))
    }

    pub fn child(&mut self) -> &mut Child {
        &mut self.0
    }

    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        let status = self.0.wait();
        forget(self.0.id());
        status
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
        forget(self.0.id());
    }
}

/// Stops every owned child still running: the launcher calls it as it quits.
pub fn stop_all() {
    for pid in std::mem::take(&mut *running()) {
        stop(pid);
    }
}

#[cfg(unix)]
fn stop(pid: u32) {
    // SAFETY: a signal to a process id; the id is a child this launcher started and hasn't reaped.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
}

#[cfg(windows)]
fn stop(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    // SAFETY: the handle is checked before use and closed after.
    unsafe {
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "daily"]
    fn quitting_stops_the_children_still_running() {
        let mut child = Owned::spawn(Command::new("sleep").arg("30")).unwrap();
        let pid = child.child().id();
        assert!(running().contains(&pid));
        stop_all();
        let status = child.wait().unwrap();
        assert!(!status.success(), "stopped by its signal: {status:?}");
        assert!(!running().contains(&pid));
    }
}
