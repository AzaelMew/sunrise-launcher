//! Who plays: the top bar's quick switch (a menu under the account chip) between the signed-in
//! Microsoft accounts.
use super::*;

impl Launcher {
    pub(super) fn play_as(&mut self, uuid: &str) {
        let mut accounts = Accounts::load();
        let done = accounts.play_as(uuid).and_then(|()| accounts.save());
        match done {
            Ok(()) => self.accounts = accounts,
            Err(e) => self.sign_in_state = Some(SignInState::Done(Err(e))),
        }
    }
}
