//! The wrapper receives a one-use invocation, never the profile or cache capabilities.
use super::{
    child::Child,
    leases::{ChildLeases, ProfileHandoff},
    relay_wrapper::Wrapper,
};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{OsStr, OsString},
    io,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use sunrise_launcher_smod::{
    holder,
    local_control::{self, Channel, Endpoint, Marker, MAX_RELAY_CLAIMS},
};

const ARG: &str = "--internal-game-relay";
const TIMEOUT: Duration = Duration::from_secs(10);
const PROTECTED: &[&str] = &[
    "SUNRISE_HOME",
    "SUNRISE_SESSION_FILE",
    "SUNRISE_LAUNCHER_EXE",
    "SUNRISE_ACCOUNT_SESSIONS",
    "SUNRISE_NO_DATA_FILE",
    "SUNRISE_WINDOWS_RUNTIME_SLOT",
    "SUNRISE_RUNTIME_LEASE_FDS",
    "SUNRISE_RUNTIME_LEASE_HANDLES",
    "SUNRISE_RUNTIME_DATA_ROOT",
    crate::developer_offline::ENV,
    crate::run_leases::SOURCE,
    crate::run_leases::GAME,
    holder::ENV,
    holder::ENTRY,
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Invocation {
    marker: Marker,
    #[cfg(windows)]
    issuer: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Offer {
    count: usize,
    program: OsString,
    args: Vec<OsString>,
    directory: OsString,
    // Corresponds to PROTECTED. The wrapper may set normal environment, never these fields.
    environment: Vec<Option<OsString>>,
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
enum Signal {
    Acquired,
    Commit,
    #[cfg(windows)]
    Started,
    #[cfg(unix)]
    ExecFailed,
}

#[derive(PartialEq, Eq)]
enum Grant {
    Pending,
    HandedOut,
    Revoked,
}

struct Token {
    state: Mutex<Grant>,
    revoked: Arc<AtomicBool>,
}
impl Token {
    fn new() -> Self {
        Self {
            state: Mutex::new(Grant::Pending),
            revoked: Arc::new(AtomicBool::new(false)),
        }
    }
    fn revoke(&self) {
        let mut state = self.state.lock().unwrap();
        if *state == Grant::Pending {
            *state = Grant::Revoked;
            self.revoked.store(true, Ordering::Release);
        }
    }
    fn handout<T>(
        &self,
        deadline: Instant,
        transfer: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        let mut state = self.state.lock().unwrap();
        if *state != Grant::Pending {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let sent = if Instant::now() >= deadline {
            Err(io::ErrorKind::TimedOut.into())
        } else {
            transfer()
        };
        if sent.is_ok() {
            *state = Grant::HandedOut;
        } else {
            *state = Grant::Revoked;
            self.revoked.store(true, Ordering::Release);
        }
        sent
    }
}

pub(crate) fn start(
    game: &Command,
    prefix: &[OsString],
    leases: &ChildLeases,
    profile: ProfileHandoff<'_>,
    piped: bool,
) -> Result<Child, String> {
    let invocation = Invocation {
        marker: Marker::runtime().map_err(|e| e.to_string())?,
        #[cfg(windows)]
        issuer: std::process::id(),
    };
    let mut endpoint = Endpoint::bind(&invocation.marker).map_err(|e| e.to_string())?;
    let encoded = serde_json::to_string(&invocation).map_err(|e| e.to_string())?;
    let args: Vec<_> = game.get_args().map(OsStr::to_owned).collect();
    let launcher = std::env::current_exe().map_err(|e| e.to_string())?;
    let words = [launcher.into_os_string(), ARG.into(), encoded.into()]
        .into_iter()
        .chain(args.iter().cloned());
    let (program, prefix) = prefix.split_first().expect("this route has a wrapper");
    let mut command = Command::new(program);
    command.args(prefix).args(words);
    crate::launch::same_place(game, &mut command);
    let label = program.to_string_lossy();
    let failure = |e: io::Error| {
        crate::i18n::trf(
            "Could not start Sunrise through {wrapper}: {reason}",
            &[("wrapper", &label), ("reason", &e.to_string())],
        )
    };
    let files: Vec<_> = leases.files().into_iter().chain([profile.file]).collect();
    if files.len() > MAX_RELAY_CLAIMS {
        return Err(failure(io::ErrorKind::InvalidInput.into()));
    }
    let offer = Offer {
        count: files.len(),
        program: game.get_program().to_owned(),
        args,
        directory: game
            .get_current_dir()
            .expect("the admitted working directory")
            .as_os_str()
            .to_owned(),
        environment: PROTECTED
            .iter()
            .map(|key| {
                game.get_envs()
                    .find(|(name, _)| {
                        if cfg!(windows) {
                            name.to_str()
                                .is_some_and(|name| name.eq_ignore_ascii_case(key))
                        } else {
                            name == key
                        }
                    })
                    .map_or_else(
                        || std::env::var_os(key),
                        |(_, value)| value.map(OsStr::to_owned),
                    )
            })
            .collect(),
    };
    let wrapper = Wrapper::start(command, piped).map_err(failure)?;
    #[cfg(windows)]
    let job = wrapper
        .child
        .wrapper_job()
        .expect("Windows wrapper started in its Job");
    let wrapper = Mutex::new(wrapper);
    let token = Token::new();
    let done = AtomicBool::new(false);
    let deadline = Instant::now() + TIMEOUT;
    let outcome = std::thread::scope(|scope| -> io::Result<()> {
        std::thread::Builder::new()
            .name("wrapper-leader".into())
            .spawn_scoped(scope, || {
                while !done.load(Ordering::Acquire) && Instant::now() < deadline {
                    let exited = wrapper.lock().unwrap().leader_exited();
                    if !matches!(exited, Ok(false)) {
                        token.revoke();
                        return;
                    }
                    std::thread::park_timeout(Duration::from_millis(2));
                }
                if !done.load(Ordering::Acquire) {
                    token.revoke();
                }
            })?;
        let handshake = (|| {
            let stream =
                endpoint.accept_while(deadline, || !token.revoked.load(Ordering::Acquire))?;
            let channel = Channel::authenticate_relay(
                stream,
                &invocation.marker,
                true,
                deadline,
                Some(token.revoked.clone()),
            )?
            .send(&offer)?;
            if wrapper.lock().unwrap().leader_exited()? {
                token.revoke();
            }
            let channel = token.handout(deadline, || {
                #[cfg(unix)]
                {
                    let channel = channel.send_relay_claims(&files)?;
                    profile.handed_off.store(true, Ordering::Relaxed);
                    Ok(channel)
                }
                #[cfg(windows)]
                windows::send_claims(channel, &job, &files)
            })?;
            let (channel, ack) = channel.receive::<Signal>()?;
            if ack != Signal::Acquired {
                return Err(io::ErrorKind::InvalidData.into());
            }
            let channel = channel.send(&Signal::Commit)?;
            #[cfg(unix)]
            return channel.expect_relay_exec();
            #[cfg(windows)]
            {
                let (_, ack) = channel.receive::<Signal>()?;
                if ack != Signal::Started {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                Ok(())
            }
        })();
        done.store(true, Ordering::Release);
        handshake
    });
    // Close the endpoint before returning failure (particularly if no relay ever connected).
    token.revoke();
    drop(endpoint);
    let wrapper = wrapper.into_inner().unwrap();
    match outcome {
        Ok(()) => Ok(wrapper.child),
        Err(error) => {
            wrapper.retire();
            Err(failure(error))
        }
    }
}

/// Consume private modes before the launcher initializes, including before GUI/keychain work.
/// Ordinary invocations don't enter this function's process-control paths.
#[doc(hidden)]
pub fn private(mode: &OsStr, args: impl Iterator<Item = OsString>) -> Option<i32> {
    #[cfg(any(target_os = "linux", windows))]
    if mode == super::relay_wrapper::ARG {
        return Some(super::relay_wrapper::run(args).unwrap_or_else(|error| {
            eprintln!(
                "{}",
                crate::i18n::trf(
                    "The game wrapper stopped: {reason}",
                    &[("reason", &error.to_string())]
                )
            );
            78
        }));
    }
    if mode != ARG {
        return None;
    }
    Some(run(args).unwrap_or_else(|_| {
        eprintln!("{}", crate::i18n::tr("Sunrise didn't start: the game launch is no longer active or could not be verified."));
        78
    }))
}

fn run(mut args: impl Iterator<Item = OsString>) -> io::Result<i32> {
    let marker = args.next().ok_or(io::ErrorKind::InvalidInput)?;
    let encoded = marker
        .to_str()
        .filter(|s| s.len() <= 4096)
        .ok_or(io::ErrorKind::InvalidInput)?;
    let invocation: Invocation = serde_json::from_str(encoded)?;
    let marker = &invocation.marker;
    let deadline = Instant::now() + TIMEOUT;
    #[cfg(unix)]
    let stream = local_control::connect(marker, deadline)?;
    #[cfg(windows)]
    let stream = local_control::connect_relay(marker, invocation.issuer, deadline)?;
    let channel = Channel::authenticate_relay(stream, marker, false, deadline, None)?;
    let (channel, offer) = channel.receive::<Offer>()?;
    let directory = PathBuf::from(offer.directory);
    let program = PathBuf::from(offer.program);
    if !program.is_absolute()
        || !directory.is_absolute()
        || offer.environment.len() != PROTECTED.len()
        || offer.count == 0
        || offer.count > MAX_RELAY_CLAIMS
        || !args.eq(offer.args.iter().cloned())
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut game = Command::new(program);
    game.current_dir(directory).args(offer.args);
    for (name, value) in PROTECTED.iter().zip(offer.environment) {
        match value {
            Some(value) => game.env(name, value),
            None => game.env_remove(name),
        };
    }
    #[cfg(unix)]
    let (channel, claims) = channel.receive_relay_claims(offer.count)?;
    #[cfg(windows)]
    let (channel, claims) = windows::receive_claims(channel, offer.count)?;
    let (channel, permit) = channel.send(&Signal::Acquired)?.receive::<Signal>()?;
    if permit != Signal::Commit {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    #[cfg(unix)]
    return exec(channel, game, claims);
    #[cfg(windows)]
    {
        let mut child = windows::spawn(game, &claims)?;
        // The game now owns its own copies. After Commit, loss of the reply never kills it.
        drop(claims);
        let _ = channel.send(&Signal::Started);
        Ok(child.wait()?.code().unwrap_or(1))
    }
}

#[cfg(unix)]
fn exec(channel: Channel, mut game: Command, claims: Vec<std::fs::File>) -> io::Result<i32> {
    use std::os::{fd::AsRawFd, unix::process::CommandExt};
    let started = (|| {
        channel.ensure_relay_exec_closes()?;
        for file in &claims {
            // SAFETY: private relay has no other threads; each descriptor is exclusively owned.
            if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        Err(game.exec())
    })();
    let _ = channel.send(&Signal::ExecFailed);
    started
}

#[cfg(windows)]
#[path = "relay_windows.rs"]
mod windows;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;

    #[test]
    fn revoke_and_handout_have_one_winner() {
        for _ in 0..32 {
            let token = Token::new();
            let barrier = Barrier::new(2);
            let handout = std::thread::scope(|scope| {
                let send = scope.spawn(|| {
                    barrier.wait();
                    token.handout(Instant::now() + TIMEOUT, || Ok(42))
                });
                barrier.wait();
                token.revoke();
                send.join().unwrap()
            });
            assert_eq!(handout.is_ok(), !token.revoked.load(Ordering::Acquire));
            token.revoke();
            assert_eq!(handout.is_ok(), !token.revoked.load(Ordering::Acquire));
            assert!(token
                .handout::<()>(Instant::now() + TIMEOUT, || panic!("reuse"))
                .is_err());
        }
    }
}
