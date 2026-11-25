//! `account …`: signing in and out, sessions, and the account that plays.
use super::*;

#[derive(Subcommand)]
pub(super) enum AccountCmd {
    /// The signed-in accounts and which one plays
    List,
    /// Sign in with a Microsoft account in your browser (device code). Play needs one that owns
    /// Minecraft: Java Edition
    SignIn {
        /// Sign this account (name or UUID) in again: another Microsoft account is refused and
        /// who plays stays the same
        #[arg(long = "for")]
        target: Option<String>,
    },
    /// Renew an account's session now, even when its access token is still fresh
    Refresh { who: String },
    /// Renew an account's session for a game already running: prints the read-once session file
    /// it hands over (`--json`: {"session_file"} or {"authentication"} when it can't)
    Session { who: String },
    /// Sign out: the account's tokens leave the keychain (and the secrets file, where it kept them)
    SignOut {
        /// Its name or UUID
        who: String,
    },
    /// Play as this account
    Select { who: String },
}

/// `account …`: names and UUIDs only; tokens are never printed.
pub(super) fn account_command(action: AccountCmd, as_json: bool) -> Result<(), Error> {
    let mut accounts = Accounts::load();
    let find = |accounts: &Accounts, who: &str| {
        accounts
            .find(who)
            .cloned()
            .ok_or_else(|| format!("No signed-in account {who}. See `sunrise-launcher account`."))
    };
    match action {
        AccountCmd::List => {
            if as_json {
                json(serde_json::json!({
                    "accounts": accounts.accounts.iter().map(|a| serde_json::json!({
                        "name": a.name, "uuid": a.uuid,
                        "selected": accounts.selected.as_ref() == Some(&a.uuid),
                        "authentication": account::session::status(account::store(), &a.uuid),
                    })).collect::<Vec<_>>(),
                    "sign_in_configured": account::client_id().is_some(),
                    "owner": accounts.has_owner(account::store()),
                }));
            } else if accounts.accounts.is_empty() {
                println!(
                    "No Microsoft account signed in. Play needs one that owns Minecraft: Java \
                     Edition: sunrise-launcher account sign-in"
                );
            } else {
                for a in &accounts.accounts {
                    let mark = if accounts.selected.as_ref() == Some(&a.uuid) {
                        "*"
                    } else {
                        " "
                    };
                    let status = account::session::status(account::store(), &a.uuid);
                    // Why it couldn't be checked (a keyring that didn't answer, #12179).
                    let why = match status.state {
                        account::session::State::Retry => format!(": {}", status.message),
                        _ => String::new(),
                    };
                    println!("{mark} {}  {}  {}{why}", a.name, a.uuid, status.label());
                }
                if !accounts.has_owner(account::store()) {
                    println!(
                        "{} {}",
                        account::NoOwner::default(),
                        tr(backend::SIGN_IN_HINT)
                    );
                }
            }
        }
        AccountCmd::Refresh { who } => {
            let status = account::session::refresh(&who)?;
            if as_json {
                json(serde_json::to_value(&status).map_err(|e| e.to_string())?);
            } else if status.state != account::session::State::Ready {
                return Err(format!("{}: {}", status.label(), status.message).into());
            } else {
                println!("{}: {}", status.label(), status.message);
            }
        }
        AccountCmd::Session { who } => {
            let dir = backend::game_home().join("sessions");
            match (account::session::renew_for_game(&dir, &who), as_json) {
                (Ok(file), true) => json(serde_json::json!({ "session_file": file })),
                (Ok(file), false) => println!("{}", file.display()),
                (Err(status), true) => json(serde_json::json!({ "authentication": status })),
                (Err(status), false) => return Err(status.message.into()),
            }
        }
        AccountCmd::SignIn { target: Some(who) } => {
            let a = find(&accounts, &who)?;
            let sign_in = account::SignIn::start_for(a.clone());
            let mut shown = false;
            let signed_in = loop {
                match sign_in.state() {
                    account::SignInState::Code(code) if !shown => {
                        shown = true;
                        if as_json {
                            json(serde_json::json!({
                                "link": code.verification_uri,
                                "code": code.user_code,
                            }));
                        } else {
                            eprintln!(
                                "To sign {} in again, open {} in a browser and enter the code {}",
                                a.name, code.verification_uri, code.user_code
                            );
                        }
                    }
                    account::SignInState::Done(result) => break result?,
                    _ => {}
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            };
            if as_json {
                json(serde_json::json!({ "signed_in": signed_in.uuid }));
            } else {
                println!("Signed in as {} again.", signed_in.name);
            }
        }
        AccountCmd::SignIn { target: None } => {
            let client_id = account::client_id().ok_or(account::NOT_CONFIGURED)?;
            let ep = account::Endpoints::live();
            let code = account::device_code(&ep, &client_id)?;
            eprintln!(
                "To sign in, open {} in a browser and enter the code {}",
                code.verification_uri, code.user_code
            );
            let ms = account::wait_for_sign_in(&ep, &client_id, &code, &|| false)?;
            let (signed_in, secrets) = account::finish_sign_in(&ep, ms)?;
            accounts.add(account::store(), signed_in.clone(), &secrets)?;
            accounts.save()?;
            println!("Signed in as {}.", signed_in.name);
            if let Some(said) = account::no_keychain() {
                eprintln!("{said}");
            }
        }
        AccountCmd::SignOut { who } => {
            let a = find(&accounts, &who)?;
            accounts.remove(account::store(), &a.uuid)?;
            accounts.save()?;
            println!("Signed out {}.", a.name);
        }
        AccountCmd::Select { who } => {
            let a = find(&accounts, &who)?;
            accounts.play_as(&a.uuid)?;
            accounts.save()?;
            println!("The game plays as {}.", a.name);
        }
    }
    Ok(())
}
