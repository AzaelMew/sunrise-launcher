use super::*;

#[test]
fn the_same_owner_file_can_admit_only_one_operation() {
    let folder = tempfile::tempdir().unwrap();
    let gate = folder.path().join("external.lock");
    let home = folder.path().join("profile");
    fs::create_dir(&home).unwrap();
    let owner = Admission::open(&home, Some(&gate)).unwrap();
    assert_eq!(owner.home(), home);
    assert!(Admission::open(&home, Some(&gate)).is_err());
    drop(owner);
    assert!(Admission::open(&home, Some(&gate)).is_ok());
    assert!(gate.is_file());
}

#[cfg(unix)]
mod inherited {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Child, Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    const HOME: &str = "SUNRISE_PROFILE_JOURNAL_TEST_HOME";
    const ACTION: &str = "SUNRISE_PROFILE_JOURNAL_TEST_ACTION";
    struct Actor(Child);
    impl Drop for Actor {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn pause_actor(home: &Path, action: &str, prepare: impl FnOnce(&mut Command)) -> Actor {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "profile_txn::tests::inherited::journal_actor",
                "--nocapture",
            ])
            .env(HOME, home)
            .env(ACTION, action)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        prepare(&mut command);
        let mut child = Actor(command.spawn().unwrap());
        let stdout = child.0.stdout.take().unwrap();
        let (send, recv) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line == "txn-paused" {
                    let _ = send.send(());
                }
            }
        });
        recv.recv_timeout(Duration::from_secs(20))
            .expect("actor reached the journal boundary");
        child
    }

    fn pause_here() {
        println!("txn-paused");
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input).unwrap();
        assert_eq!(
            input, "release\n",
            "parent should stop the child or release the inherited lease"
        );
    }

    #[test]
    fn journal_actor() {
        if std::env::var_os(HOME).is_some() {
            pause_here();
        }
    }
    #[test]
    #[cfg(unix)]
    fn a_child_inherits_the_profile_gate_and_blocks_an_already_open_launcher() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let owner = Admission::open(home, None).unwrap();
        let mut child = pause_actor(home, "hold", |cmd| owner.inherit(cmd).unwrap());
        drop(owner);
        assert!(Admission::open(home, None).is_err());
        child
            .0
            .stdin
            .take()
            .unwrap()
            .write_all(b"release\n")
            .unwrap();
        assert!(child.0.wait().unwrap().success());
        assert!(Admission::open(home, None).is_ok());
    }
}
