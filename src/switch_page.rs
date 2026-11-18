//! Who plays: the top bar's quick switch (a menu under the account chip) between the signed-in
//! Microsoft accounts.
use super::*;

impl Launcher {
    pub(super) fn play_as(&mut self, uuid: &str) {
        if self.account_locked() {
            self.say_account_locked();
            return;
        }
        match Accounts::switch(uuid) {
            Ok(accounts) => self.accounts = accounts,
            Err(e) => self.sign_in_state = Some(SignInState::Done(Err(e))),
        }
    }
}
