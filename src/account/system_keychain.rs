//! The OS keychain on each platform: the Secret Service, the macOS Keychain and Windows Credential Manager.
use super::*;

/// The Secret Service (GNOME Keyring, KWallet) through oo7, as gpui itself uses it.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub struct SystemStore;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl SystemStore {
    fn keyring() -> Result<oo7::Keyring, String> {
        futures_lite::future::block_on(async {
            // `oo7::Keyring::new` takes `GTK_USE_PORTAL=1` (set by many KDE desktops) for a
            // sandbox, and then keeps the secrets in a file under a key the Secret portal gives
            // per app id. Outside a sandbox that id comes from how the launcher was started (the
            // menu or a terminal), so the file opened in one start and not the next (#3249).
            // Flatpak (`/.flatpak-info`) and Snap (`$SNAP`) keep the portal's file.
            let sandboxed =
                Path::new("/.flatpak-info").exists() || std::env::var_os("SNAP").is_some();
            let keyring = if sandboxed {
                oo7::Keyring::new().await.map_err(|e| e.to_string())?
            } else {
                let service = oo7::dbus::Service::new().await.map_err(|e| e.to_string())?;
                oo7::Keyring::DBus(
                    service
                        .default_collection()
                        .await
                        .map_err(|e| e.to_string())?,
                )
            };
            keyring.unlock().await.map_err(|e| e.to_string())?;
            Ok(keyring)
        })
    }
}
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
impl SecretStore for SystemStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        let keyring = Self::keyring()?;
        let read = |service| {
            futures_lite::future::block_on(async {
                let attributes = vec![("service", service), ("account", key)];
                let items = keyring
                    .search_items(&attributes)
                    .await
                    .map_err(|e| e.to_string())?;
                let Some(item) = items.into_iter().next() else {
                    return Ok(None);
                };
                item.unlock().await.map_err(|e| e.to_string())?;
                let secret = item.secret().await.map_err(|e| e.to_string())?;
                Ok(String::from_utf8(secret.to_vec()).ok())
            })
        };
        get_carried(read(SERVICE)?, read, |secret| self.set(key, secret))
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        let keyring = Self::keyring()?;
        futures_lite::future::block_on(async {
            let attributes = vec![("service", SERVICE), ("account", key)];
            keyring
                .create_item(
                    "Sunrise Launcher: Minecraft account",
                    &attributes,
                    secret.as_bytes(),
                    true,
                )
                .await
                .map_err(|e| e.to_string())
        })
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        let keyring = Self::keyring()?;
        futures_lite::future::block_on(async {
            let attributes = vec![("service", SERVICE), ("account", key)];
            keyring.delete(&attributes).await.map_err(|e| e.to_string())
        })
    }
    fn persistent(&self) -> bool {
        true
    }
    fn lock_file(&self) -> Option<PathBuf> {
        Some(crate::backend::data_dir().join("account-session.lock"))
    }
}

/// Keychain on macOS, Credential Manager on Windows (the keyring crate). Credential Manager takes at
/// most 2560 bytes of UTF-16 per item and a sign-in's secrets are longer, so on Windows a secret is
/// kept in pieces: the first under the account's key, the rest under `key.1`, `key.2`... The
/// Keychain has no such limit, and every item is a consent prompt for a launcher signature it
/// doesn't know yet — pieces meant several prompts after every self-update (#4912) — so on macOS a
/// secret is one item, and pieces an older launcher left fold into it as they are read.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct SystemStore;

/// Bytes of UTF-8 per keychain item: at most twice as many in UTF-16, well under 2560.
#[cfg(any(target_os = "windows", test))]
pub(super) const PIECE: usize = 1000;

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(super) fn piece_key(key: &str, n: usize) -> String {
    if n == 0 {
        key.to_string()
    } else {
        format!("{key}.{n}")
    }
}

/// `secret` cut into pieces of at most `PIECE` bytes, on character boundaries.
#[cfg(any(target_os = "windows", test))]
pub(super) fn split_secret(mut secret: &str) -> Vec<&str> {
    let mut pieces = Vec::new();
    while secret.len() > PIECE {
        let mut cut = PIECE;
        while !secret.is_char_boundary(cut) {
            cut -= 1;
        }
        let (head, tail) = secret.split_at(cut);
        pieces.push(head);
        secret = tail;
    }
    pieces.push(secret);
    pieces
}

/// One keychain item per key: what a secret's pieces are kept in.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(super) trait Items {
    fn read(&self, key: &str) -> Result<Option<String>, String>;
    fn write(&self, key: &str, piece: &str) -> Result<(), String>;
    /// Removes one item; false when there was none.
    fn remove(&self, key: &str) -> Result<bool, String>;
}

#[cfg(any(target_os = "windows", target_os = "macos", test))]
pub(super) fn get_pieces(items: &dyn Items, key: &str) -> Result<Option<String>, String> {
    let Some(mut secret) = items.read(key)? else {
        return Ok(None);
    };
    for n in 1.. {
        match items.read(&piece_key(key, n))? {
            Some(piece) => secret.push_str(&piece),
            None => break,
        }
    }
    Ok(Some(secret))
}

#[cfg(any(target_os = "windows", test))]
pub(super) fn set_pieces(items: &dyn Items, key: &str, secret: &str) -> Result<(), String> {
    let before = get_pieces(items, key).ok().flatten();
    let pieces = split_secret(secret);
    for (n, piece) in pieces.iter().enumerate() {
        if let Err(e) = items.write(&piece_key(key, n), piece) {
            // New pieces before old ones read back as no secret at all (the account would be
            // signed out at the next start): the old secret goes back, which fit before.
            if n > 0 {
                let _ = delete_pieces(items, key);
                if let Some(before) = &before {
                    for (n, piece) in split_secret(before).iter().enumerate() {
                        let _ = items.write(&piece_key(key, n), piece);
                    }
                }
            }
            return Err(e);
        }
    }
    // Pieces left over from a longer secret would be read back as part of this one.
    for n in pieces.len().. {
        match items.remove(&piece_key(key, n)) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                // Some leftovers are already gone, some are still there: the old secret goes back
                // whole, as on a refused write above.
                let _ = delete_pieces(items, key);
                if let Some(before) = &before {
                    for (n, piece) in split_secret(before).iter().enumerate() {
                        let _ = items.write(&piece_key(key, n), piece);
                    }
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
pub(super) fn delete_pieces(items: &dyn Items, key: &str) -> Result<(), String> {
    for n in 0.. {
        if !items.remove(&piece_key(key, n))? {
            break;
        }
    }
    Ok(())
}

/// One item per key, the whole secret in it (macOS). A secret an older launcher split over `key.1`,
/// `key.2`, ... reads whole and folds into the one item, so a read touches a single item — and asks
/// once — from then on. A merged item also reads fine as "piece 0" to launchers that still split.
#[cfg(any(target_os = "macos", test))]
pub(super) fn get_item(items: &dyn Items, key: &str) -> Result<Option<String>, String> {
    let Some(mut secret) = items.read(key)? else {
        return Ok(None);
    };
    let mut split = false;
    for n in 1.. {
        match items.read(&piece_key(key, n))? {
            Some(piece) => {
                secret.push_str(&piece);
                split = true;
            }
            None => break,
        }
    }
    if split {
        let _ = set_item(items, key, &secret);
    }
    Ok(Some(secret))
}

/// Writes `secret` to `key` and removes `key.1`, `key.2`, ... pieces an older, split secret left:
/// they would read back as part of this one. A removal the keychain refuses puts the old items
/// back: the merged secret beside the pieces left would read as a longer, unparseable one, and
/// pieces removed before the failure can't be had again except from here.
#[cfg(any(target_os = "macos", test))]
pub(super) fn set_item(items: &dyn Items, key: &str, secret: &str) -> Result<(), String> {
    let mut before = Vec::new();
    if let Some(piece) = items.read(key)? {
        before.push((key.to_string(), piece));
        for n in 1.. {
            match items.read(&piece_key(key, n))? {
                Some(piece) => before.push((piece_key(key, n), piece)),
                None => break,
            }
        }
    }
    items.write(key, secret)?;
    for n in 1.. {
        match items.remove(&piece_key(key, n)) {
            Ok(true) => {}
            Ok(false) => break,
            Err(e) => {
                if before.is_empty() {
                    let _ = items.remove(key);
                } else {
                    for (key, piece) in &before {
                        let _ = items.write(key, piece);
                    }
                }
                return Err(e);
            }
        }
    }
    Ok(())
}

/// Credential Manager's error 8 (`ERROR_NOT_ENOUGH_MEMORY`) on a write: not the computer's memory,
/// the player's credential store is full (an undocumented limit of its total size). The Xbox app
/// and the Minecraft Launcher are known to leave hundreds of items there (#4313, #4366).
#[cfg(any(target_os = "windows", test))]
pub const CREDENTIAL_MANAGER_FULL: &str = "Windows Credential Manager is full, so Sunrise can't \
     keep your sign-in (Windows error 8; it isn't your computer's memory). Open Credential Manager \
     from the Start menu, go to Windows Credentials and remove entries you no longer use, then sign \
     in again. The Xbox app and the Minecraft Launcher often leave many XblGrts and Xbl| entries \
     there; removing an entry can sign the app that made it out.";

/// What the launcher says for a Windows error code Credential Manager gave when writing an item,
/// when it says more than the code.
#[cfg(any(target_os = "windows", test))]
pub(super) fn credential_manager_says(code: u32) -> Option<&'static str> {
    const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
    (code == ERROR_NOT_ENOUGH_MEMORY).then_some(CREDENTIAL_MANAGER_FULL)
}

/// A running executable removed by an older macOS updater cannot create Keychain credentials.
#[cfg(any(target_os = "macos", test))]
pub const MACOS_LAUNCHER_RESTART: &str = "The launcher was updated while it was running. Restart the launcher, then sign in with Microsoft again.";

#[cfg(any(target_os = "macos", test))]
pub(super) fn macos_keychain_says(code: i32) -> Option<&'static str> {
    // CSSM's Unix ENOENT: Keychain cannot identify an executable removed by an older updater.
    (code == 100002).then_some(MACOS_LAUNCHER_RESTART)
}

/// The keyring crate's error as the player reads it.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub(super) fn write_error(e: keyring::Error) -> String {
    #[cfg(target_os = "macos")]
    if let keyring::Error::PlatformFailure(inner) = &e {
        if let Some(said) = inner
            .downcast_ref::<security_framework::base::Error>()
            .and_then(|error| macos_keychain_says(error.code()))
        {
            return said.to_string();
        }
    }
    #[cfg(target_os = "windows")]
    if let keyring::Error::PlatformFailure(inner) = &e {
        if let Some(said) = inner
            .downcast_ref::<keyring::windows::Error>()
            .and_then(|code| credential_manager_says(code.0))
        {
            return said.to_string();
        }
    }
    e.to_string()
}

/// `own`, the secret under [`SERVICE`], or else the first an earlier launcher's service holds
/// ([`crate::identity::EARLIER_KEYCHAIN_SERVICES`], read by `earlier` and never written), copied
/// under [`SERVICE`] by `keep`: a sign-in saved by the launcher before this one carries over. An
/// earlier service that fails reads as missing; a copy `keep` couldn't make is tried again on the
/// next read.
pub(super) fn get_carried(
    own: Option<String>,
    earlier: impl Fn(&'static str) -> Result<Option<String>, String>,
    keep: impl FnOnce(&str) -> Result<(), String>,
) -> Result<Option<String>, String> {
    if own.is_some() {
        return Ok(own);
    }
    let found = crate::identity::EARLIER_KEYCHAIN_SERVICES
        .iter()
        .find_map(|service| earlier(service).ok().flatten());
    if let Some(secret) = &found {
        let _ = keep(secret);
    }
    Ok(found)
}

/// The OS keychain's items under one service.
#[cfg(any(target_os = "macos", target_os = "windows"))]
struct KeyringItems(&'static str);

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl Items for KeyringItems {
    fn read(&self, key: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(self.0, key).and_then(|e| e.get_password()) {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn write(&self, key: &str, piece: &str) -> Result<(), String> {
        keyring::Entry::new(self.0, key)
            .and_then(|e| e.set_password(piece))
            .map_err(write_error)
    }
    fn remove(&self, key: &str) -> Result<bool, String> {
        match keyring::Entry::new(self.0, key).and_then(|e| e.delete_credential()) {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl SecretStore for SystemStore {
    #[cfg(target_os = "macos")]
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        get_carried(
            get_item(&KeyringItems(SERVICE), key)?,
            |service| get_pieces(&KeyringItems(service), key),
            |secret| set_item(&KeyringItems(SERVICE), key, secret),
        )
    }
    #[cfg(target_os = "windows")]
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        get_carried(
            get_pieces(&KeyringItems(SERVICE), key)?,
            |service| get_pieces(&KeyringItems(service), key),
            |secret| set_pieces(&KeyringItems(SERVICE), key, secret),
        )
    }
    #[cfg(target_os = "macos")]
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        set_item(&KeyringItems(SERVICE), key, secret)
    }
    #[cfg(target_os = "windows")]
    fn set(&self, key: &str, secret: &str) -> Result<(), String> {
        set_pieces(&KeyringItems(SERVICE), key, secret)
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        delete_pieces(&KeyringItems(SERVICE), key)
    }
    fn persistent(&self) -> bool {
        true
    }
    fn lock_file(&self) -> Option<PathBuf> {
        Some(crate::backend::data_dir().join("account-session.lock"))
    }
}
