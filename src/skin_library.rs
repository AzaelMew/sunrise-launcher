//! Saved skins: the skins an account wore or its player added on the Skins tab, kept in the
//! launcher's data folder, each account's apart (`saved-skins/<uuid>/`: each skin's PNG under its
//! id, and `index.json` with their names and arm models) so they can be worn again with one
//! click. They leave the computer only when worn, and then only to the Minecraft services.
//!
//! Every change to an index (adding a skin, its arm model, removing one) reads it, changes it
//! and writes it back under one lock ([`changing`]), so two changes at once in this launcher
//! (two imports, an import beside a Save keeping the skin it replaces) can't each write over
//! what the other added.
use crate::i18n::{tr, trf};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Held for the whole of a change to any account's index: changes are rare and quick, so one
/// lock for every account costs nothing.
fn changing() -> MutexGuard<'static, ()> {
    static CHANGES: Mutex<()> = Mutex::new(());
    CHANGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A temporary file beside `path`, named for this process and this write alone.
fn temporary(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{name}.{}.{n}.tmp", std::process::id()))
}

/// Writes `bytes` to `path` whole or not at all.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = temporary(path);
    std::fs::write(&tmp, bytes)
        .and_then(|()| std::fs::rename(&tmp, path))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
}

/// One saved skin.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Saved {
    /// The first 16 hex digits of the SHA-1 of its texels ([`id_of`]): the same skin is saved once.
    pub id: String,
    /// What its tile is called: the file it came from, or nothing for a skin kept because it was
    /// worn.
    pub name: String,
    pub slim: bool,
}

/// The folder the account `uuid` keeps its saved skins in: another account never sees them.
pub fn folder(uuid: &str) -> Result<PathBuf, String> {
    folder_in(&crate::backend::data_dir(), uuid)
}

fn folder_in(data: &Path, uuid: &str) -> Result<PathBuf, String> {
    // Only a profile UUID (32 hex digits) names a folder: an account record without one has no
    // saved skins, rather than a folder it would share with every other such record.
    if uuid.len() == 32 && uuid.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(data.join("saved-skins").join(uuid))
    } else {
        Err(tr("This account has no valid profile id, so it can't keep saved skins.").into())
    }
}

/// The saved skins in `dir`, oldest first; those whose picture is gone are left out. No index
/// yet is no saved skins; an index that can't be read or understood is an error, and nothing
/// here writes over it.
pub fn list(dir: &Path) -> Result<Vec<Saved>, String> {
    Ok(list_all(dir)?
        .into_iter()
        .filter(|s| valid_id(&s.id) && png_path(dir, &s.id).is_file())
        .collect())
}

/// A listing of an account's saved skins ([`list`]) and the number of the request it answers,
/// so the page takes in only the latest.
pub type Listing = (u64, Result<Vec<Saved>, String>);

/// The most an index may grow to: a thousand saved skins take about a tenth of it.
const MAX_INDEX: u64 = 1 << 20;
/// The most an index is read up to: more than it may grow to, so one already bigger (written
/// by hand, or by an older launcher) is still listed, and can be made smaller, rather than
/// locking its skins away.
const MAX_INDEX_READ: u64 = 16 << 20;

/// Every entry of the index in `dir`, those whose picture is gone included: what a change
/// writes back, so it leaves the entries it doesn't touch as they were. The index is read as
/// [`read_bounded`] reads a file: a regular file of at most [`MAX_INDEX_READ`] bytes.
fn list_all(dir: &Path) -> Result<Vec<Saved>, String> {
    let bytes = match read_bounded(&dir.join("index.json"), MAX_INDEX_READ) {
        Ok(bytes) => bytes,
        Err(Unread::Missing) => return Ok(Vec::new()),
        Err(why) => {
            return Err(trf(
                "The saved skins couldn't be read: {why}",
                &[("why", &why.to_string())],
            ))
        }
    };
    serde_json::from_slice(&bytes).map_err(|e| {
        trf(
            "The saved skins' list is damaged ({why}), so it was left as it is.",
            &[("why", &e.to_string())],
        )
    })
}

/// A saved skin's picture.
pub fn png(dir: &Path, id: &str) -> Option<Vec<u8>> {
    valid_id(id)
        .then(|| read_skin_file(&png_path(dir, id)).ok())
        .flatten()
}

/// Why [`read_bounded`] read nothing.
#[derive(Debug)]
enum Unread {
    Missing,
    NotRegular,
    TooBig,
    Io(String),
}

impl std::fmt::Display for Unread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unread::Missing => f.write_str(tr("The file isn't there.")),
            Unread::NotRegular => f.write_str(tr("It isn't a regular file.")),
            Unread::TooBig => f.write_str(tr("The file is too big.")),
            Unread::Io(why) => f.write_str(why),
        }
    }
}

/// The file at `path`, if it is a regular file of at most `max` bytes: nothing larger is read
/// into memory, whatever its length claims, and a pipe put in its place is never waited on
/// (it is opened without blocking, and the open file is checked before anything is read).
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, Unread> {
    use std::io::Read as _;
    let mut open = std::fs::OpenOptions::new();
    open.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        open.custom_flags(libc::O_NONBLOCK);
    }
    let file = open.open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Unread::Missing,
        _ => Unread::Io(e.to_string()),
    })?;
    let meta = file.metadata().map_err(|e| Unread::Io(e.to_string()))?;
    if !meta.is_file() {
        return Err(Unread::NotRegular);
    }
    if meta.len() > max {
        return Err(Unread::TooBig);
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Unread::Io(e.to_string()))?;
    if bytes.len() as u64 > max {
        return Err(Unread::TooBig);
    }
    Ok(bytes)
}

/// The file at `path`, if it is a regular file no bigger than a skin
/// ([`crate::account::skins::MAX_SKIN`]), read as [`read_bounded`] reads one.
pub fn read_skin_file(path: &Path) -> Result<Vec<u8>, String> {
    read_bounded(path, crate::account::skins::MAX_SKIN as u64).map_err(|why| match why {
        Unread::NotRegular => tr("Only a PNG file can be added as a skin.").to_string(),
        Unread::TooBig => tr("This picture is too big for a skin.").to_string(),
        other => other.to_string(),
    })
}

fn png_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.png"))
}

fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The index `list` as written. With a new entry (`adds`) it is refused when it would weigh
/// more than [`MAX_INDEX`], before anything is written, so the saved skins stay as they were
/// and can still be listed. A change that adds no entry (a removal, other arms) always goes
/// through, so an index already over it can still be changed and made smaller.
fn index_bytes(list: &[Saved], adds: bool) -> Result<Vec<u8>, String> {
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())? + "\n";
    if adds && text.len() as u64 > MAX_INDEX {
        return Err(tr("There are too many saved skins. Remove some, then try again.").into());
    }
    Ok(text.into_bytes())
}

/// Writes the index, with no new entry in it; only ever called under [`changing`].
fn save(dir: &Path, list: &[Saved]) -> Result<(), String> {
    let bytes = index_bytes(list, false)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    write_whole(&dir.join("index.json"), &bytes)
}

/// The arms a skin is saved with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arms {
    /// As the picture suggests ([`crate::player_model::looks_slim`]), for a skin not saved yet;
    /// those it has for one that is.
    Guess,
    /// These, for a skin not saved yet; those it has for one that is: a skin kept because it was
    /// seen worn never changes the arms saved for it, which a Save may have set since.
    IfNew(bool),
    /// These, whether it is saved already or not: the arms it is worn with as it is replaced.
    Worn(bool),
}

/// Saves the skin `png` as `name`, its arms as `slim` says, else as the picture suggests
/// ([`crate::player_model::looks_slim`]). When the same skin is saved already, it keeps its name,
/// and its arms unless `slim` says otherwise; but its picture is checked, and written anew when
/// it is gone, damaged or another skin's, so the saved skin returned is always there to wear
/// again.
pub fn add(dir: &Path, png: &[u8], name: &str, slim: Option<bool>) -> Result<Saved, String> {
    add_with(dir, png, name, slim.map_or(Arms::Guess, Arms::Worn))
}

/// Keeps the skin `png`, seen worn with slim or classic arms, among the saved skins: as
/// [`add`] does, but a skin saved already keeps the arms it has, so a keep that ends after a
/// Save gave it other arms never puts the old ones back.
pub fn keep_seen(dir: &Path, png: &[u8], slim: bool) -> Result<Saved, String> {
    add_with(dir, png, "", Arms::IfNew(slim))
}

fn add_with(dir: &Path, png: &[u8], name: &str, arms: Arms) -> Result<Saved, String> {
    if png.len() > crate::account::skins::MAX_SKIN {
        return Err(tr("This picture is too big for a skin.").into());
    }
    let id = id_of_png(png).ok_or(tr(
        "This isn't a Minecraft skin: it must be a 64×64 or 64×32 PNG.",
    ))?;
    let _changing = changing();
    let mut all = list_all(dir)?;
    // What the index becomes, worked out (and refused when too big) before anything is written.
    let (saved, changed, adds) = match all.iter_mut().find(|s| s.id == id) {
        Some(known) => {
            let changed = match arms {
                Arms::Worn(slim) if slim != known.slim => {
                    known.slim = slim;
                    true
                }
                _ => false,
            };
            (known.clone(), changed, false)
        }
        None => {
            let saved = Saved {
                id: id.clone(),
                name: name.trim().chars().take(40).collect(),
                slim: match arms {
                    Arms::Guess => crate::player_model::looks_slim(png),
                    Arms::IfNew(slim) | Arms::Worn(slim) => slim,
                },
            };
            all.push(saved.clone());
            (saved, true, true)
        }
    };
    let index = changed.then(|| index_bytes(&all, adds)).transpose()?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = png_path(dir, &id);
    let there = read_skin_file(&path).ok().and_then(|p| id_of_png(&p));
    if there.as_deref() != Some(id.as_str()) {
        write_whole(&path, png)?;
    }
    if let Some(index) = index {
        write_whole(&dir.join("index.json"), &index)?;
    }
    Ok(saved)
}

/// A skin's id: the first 16 hex digits of the SHA-1 of its texels as the player model wears
/// them ([`crate::player_model::skin`], completed as the game completes it), the colour of those
/// still clear then ignored. Not the file's bytes: Mojang's texture server sends a skin back in
/// another PNG than the one worn, and it must still be the same saved skin. Nor the texels as
/// the file has them: the game draws some texels opaque that a file may leave clear, and two
/// files that differ in their colour there show two different skins.
pub fn id_of(skin: &crate::player_model::Texture) -> String {
    use sha1::Digest;
    let mut sha = sha1::Sha1::new();
    sha.update((skin.w as u32).to_le_bytes());
    for t in &skin.texels {
        sha.update(if t[3] == 0 { [0; 4] } else { *t });
    }
    sha.finalize()
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The id of the skin `png` ([`id_of`]); none when it isn't a skin.
pub fn id_of_png(png: &[u8]) -> Option<String> {
    Some(id_of(&crate::player_model::skin(png)?))
}

/// The saved skin `png` is, if it is one.
pub fn find(dir: &Path, png: &[u8]) -> Option<Saved> {
    let id = id_of_png(png)?;
    list(dir).ok()?.into_iter().find(|s| s.id == id)
}

/// Gives the saved skin `id` slim or classic arms.
pub fn set_slim(dir: &Path, id: &str, slim: bool) -> Result<(), String> {
    let _changing = changing();
    let mut all = list_all(dir)?;
    match all.iter_mut().find(|s| s.id == id) {
        Some(s) if s.slim != slim => s.slim = slim,
        _ => return Ok(()),
    }
    save(dir, &all)
}

/// Forgets the saved skin `id` and deletes its picture.
pub fn remove(dir: &Path, id: &str) -> Result<(), String> {
    let _changing = changing();
    let mut all = list_all(dir)?;
    all.retain(|s| s.id != id);
    save(dir, &all)?;
    if valid_id(id) {
        let _ = std::fs::remove_file(png_path(dir, id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32, slim: bool) -> Vec<u8> {
        let image = image::RgbaImage::from_fn(w, h, |x, y| {
            let clear = slim && x == 54 && (20..32).contains(&y);
            image::Rgba(if clear {
                [0; 4]
            } else {
                [x as u8, y as u8, 3, 255]
            })
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn skins_are_saved_once_listed_changed_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().join("saved-skins");
        assert!(list(&dir).unwrap().is_empty());
        let first = add(&dir, &picture(64, 64, false), "  Knight  ", None).unwrap();
        assert_eq!((first.name.as_str(), first.slim), ("Knight", false));
        let slim = add(&dir, &picture(64, 64, true), "", None).unwrap();
        assert!(slim.slim, "the clear arm column says slim");
        assert_eq!(slim.name, "", "a skin kept without a name has none");
        assert_eq!(
            add(&dir, &picture(64, 64, false), "Again", None).unwrap(),
            first
        );
        assert_eq!(list(&dir).unwrap(), [first.clone(), slim.clone()]);
        assert_eq!(png(&dir, &first.id), Some(picture(64, 64, false)));
        set_slim(&dir, &first.id, true).unwrap();
        assert!(list(&dir).unwrap()[0].slim);
        remove(&dir, &first.id).unwrap();
        assert_eq!(list(&dir).unwrap(), [slim]);
        assert!(png(&dir, &first.id).is_none());
    }

    #[test]
    fn only_skins_are_saved_and_odd_ids_never_reach_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        assert!(add(dir.path(), &picture(46, 22, false), "Cape", None).is_err());
        assert!(add(dir.path(), b"text", "Text", None).is_err());
        assert!(png(dir.path(), "../../etc/passwd").is_none());
        assert!(remove(dir.path(), "../x").is_ok());
        assert!(list(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn the_same_skin_in_another_png_is_the_same_saved_skin() {
        let dir = tempfile::tempdir().unwrap();
        // A clear hat, and the texture server's copy of it: other PNG bytes, other colours
        // under the texels the game leaves clear.
        let hat = |colour: [u8; 4]| {
            png_of(move |x, y, texel| {
                if (40..48).contains(&x) && y < 8 {
                    colour
                } else {
                    texel
                }
            })
        };
        let worn = add(dir.path(), &hat([1, 2, 3, 0]), "Worn", None).unwrap();
        let copy = hat([99, 1, 2, 0]);
        assert_ne!(copy, hat([1, 2, 3, 0]));
        assert_eq!(find(dir.path(), &copy), Some(worn.clone()));
        assert_eq!(add(dir.path(), &copy, "Again", None).unwrap(), worn);
        assert_eq!(list(dir.path()).unwrap().len(), 1);
        assert!(find(dir.path(), &picture(64, 64, false)).is_none());
    }

    /// `picture(64, 64, true)` with each texel as `f` makes it.
    fn png_of(f: impl Fn(u32, u32, [u8; 4]) -> [u8; 4]) -> Vec<u8> {
        let base = image::load_from_memory(&picture(64, 64, true))
            .unwrap()
            .to_rgba8();
        let image =
            image::RgbaImage::from_fn(64, 64, |x, y| image::Rgba(f(x, y, base.get_pixel(x, y).0)));
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn skins_that_look_different_are_different_saved_skins() {
        // The texel the slim arm leaves clear is drawn opaque, in its own colour: a file with
        // another colour there is another skin, though both files leave it clear.
        let red = png_of(|x, y, texel| {
            if x == 54 && (20..32).contains(&y) {
                [200, 0, 0, 0]
            } else {
                texel
            }
        });
        assert_ne!(id_of_png(&red), id_of_png(&picture(64, 64, true)));
        let dir = tempfile::tempdir().unwrap();
        add(dir.path(), &picture(64, 64, true), "", None).unwrap();
        add(dir.path(), &red, "", None).unwrap();
        assert_eq!(list(dir.path()).unwrap().len(), 2);
    }

    #[test]
    fn a_saved_skin_added_again_gets_back_its_picture_and_the_arms_given() {
        let dir = tempfile::tempdir().unwrap();
        let skin = picture(64, 64, false);
        let saved = add(dir.path(), &skin, "Knight", Some(false)).unwrap();
        let file = dir.path().join(format!("{}.png", saved.id));
        // Damaged, then another skin's picture under its name, then gone.
        for broken in [
            Some(b"not a png".to_vec()),
            Some(picture(64, 64, true)),
            None,
        ] {
            match broken {
                Some(bytes) => std::fs::write(&file, bytes).unwrap(),
                None => std::fs::remove_file(&file).unwrap(),
            }
            let again = add(dir.path(), &skin, "", Some(true)).unwrap();
            assert_eq!(again.id, saved.id);
            assert_eq!(again.name, "Knight", "its name kept");
            assert!(again.slim, "the arms it was worn with");
            assert_eq!(png(dir.path(), &saved.id), Some(skin.clone()));
            assert_eq!(list(dir.path()).unwrap(), std::slice::from_ref(&again));
            set_slim(dir.path(), &saved.id, false).unwrap();
        }
        // No arms given: the ones it has stay.
        assert!(!add(dir.path(), &skin, "", None).unwrap().slim);
    }

    #[test]
    fn a_skin_file_is_read_only_when_it_is_a_small_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_skin_file(dir.path()).is_err(), "a folder");
        let big = dir.path().join("big.png");
        std::fs::write(&big, vec![0; crate::account::skins::MAX_SKIN + 1]).unwrap();
        assert!(read_skin_file(&big).is_err());
        let small = dir.path().join("small.png");
        std::fs::write(&small, picture(64, 64, false)).unwrap();
        assert_eq!(read_skin_file(&small).unwrap(), picture(64, 64, false));
        #[cfg(unix)]
        {
            // A pipe is refused at once, not waited on for a writer.
            let fifo = dir.path().join("fifo.png");
            let path = std::ffi::CString::new(fifo.to_str().unwrap()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            assert!(read_skin_file(&fifo).is_err());
        }
    }

    #[test]
    fn each_account_keeps_its_own_skins() {
        let data = tempfile::tempdir().unwrap();
        let (a, b) = (
            folder_in(data.path(), "069a79f444e94726a5befca90e38aaf5").unwrap(),
            folder_in(data.path(), "b09600dff4354d4fb56776eec1e55083").unwrap(),
        );
        add(&a, &picture(64, 64, false), "Mine", None).unwrap();
        assert_eq!(list(&a).unwrap().len(), 1);
        assert!(
            list(&b).unwrap().is_empty(),
            "another account sees none of them"
        );
        // A malformed account record gets no folder, rather than one shared with others.
        for bad in [
            "../../x",
            "",
            "no-account",
            "069a79f444e94726a5befca90e38aaf",
        ] {
            assert!(folder_in(data.path(), bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn changes_made_at_once_all_survive() {
        // Skins told apart by one texel each, added and given arms by threads let go together.
        let skin = |n: u8| {
            let image = image::RgbaImage::from_fn(64, 64, |x, y| {
                image::Rgba(if (x, y) == (0, 0) {
                    [n, 0, 0, 255]
                } else {
                    [1, 2, 3, 255]
                })
            });
            let mut out = std::io::Cursor::new(Vec::new());
            image.write_to(&mut out, image::ImageFormat::Png).unwrap();
            out.into_inner()
        };
        for _ in 0..5 {
            let dir = tempfile::tempdir().unwrap();
            let first = add(dir.path(), &skin(200), "First", Some(false)).unwrap();
            let start = std::sync::Arc::new(std::sync::Barrier::new(17));
            let threads: Vec<_> = (0..17u8)
                .map(|n| {
                    let (dir, start, png) = (dir.path().to_owned(), start.clone(), skin(n));
                    let first = first.id.clone();
                    std::thread::spawn(move || {
                        start.wait();
                        if n == 16 {
                            set_slim(&dir, &first, true)
                        } else {
                            add(&dir, &png, &format!("Skin {n}"), None).map(|_| ())
                        }
                    })
                })
                .collect();
            for t in threads {
                t.join().unwrap().unwrap();
            }
            let all = list(dir.path()).unwrap();
            assert_eq!(all.len(), 17, "every skin added is listed: {all:?}");
            assert!(all[0].slim, "the arm change survived the adds");
            let leftovers: Vec<_> = std::fs::read_dir(dir.path())
                .unwrap()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
                .collect();
            assert!(leftovers.is_empty(), "no temporary file left behind");
        }
    }

    #[test]
    fn a_damaged_index_is_reported_and_never_written_over() {
        let dir = tempfile::tempdir().unwrap();
        let first = add(dir.path(), &picture(64, 64, false), "First", None).unwrap();
        let index = dir.path().join("index.json");
        std::fs::write(&index, "{ not json").unwrap();
        assert!(list(dir.path()).is_err());
        for refused in [
            add(dir.path(), &picture(64, 64, true), "Second", None).map(|_| ()),
            set_slim(dir.path(), &first.id, true),
            remove(dir.path(), &first.id),
        ] {
            assert!(refused.is_err());
        }
        assert_eq!(std::fs::read_to_string(&index).unwrap(), "{ not json");
        assert!(
            png(dir.path(), &first.id).is_some(),
            "its picture is still there"
        );
    }

    #[test]
    fn an_add_that_would_make_the_index_too_big_is_refused_and_leaves_it_usable() {
        let dir = tempfile::tempdir().unwrap();
        let first = add(dir.path(), &picture(64, 64, false), "First", None).unwrap();
        // An entry with a long name brings the index to just under the cap.
        let filler = |name: String| Saved {
            id: "ffffffffffffffff".into(),
            name,
            slim: false,
        };
        let len = |all: &[Saved]| serde_json::to_string_pretty(all).unwrap().len() as u64 + 1;
        let base = len(&[first.clone(), filler(String::new())]);
        let all = [
            first.clone(),
            filler("x".repeat((MAX_INDEX - 10 - base) as usize)),
        ];
        std::fs::write(
            dir.path().join("index.json"),
            serde_json::to_string_pretty(&all).unwrap() + "\n",
        )
        .unwrap();
        let before = std::fs::read(dir.path().join("index.json")).unwrap();
        assert_eq!(before.len() as u64, MAX_INDEX - 10);
        let refused = add(dir.path(), &picture(64, 64, true), "One more", None);
        assert!(refused.is_err(), "{refused:?}");
        assert_eq!(
            std::fs::read(dir.path().join("index.json")).unwrap(),
            before,
            "the index as it was"
        );
        let id = id_of_png(&picture(64, 64, true)).unwrap();
        assert!(
            !dir.path().join(format!("{id}.png")).exists(),
            "and no picture written for it"
        );
        // Still listed, and a removal still works; then the add fits.
        assert_eq!(list(dir.path()).unwrap(), std::slice::from_ref(&first));
        remove(dir.path(), "ffffffffffffffff").unwrap();
        add(dir.path(), &picture(64, 64, true), "One more", None).unwrap();
    }

    #[test]
    fn an_index_already_too_big_is_still_listed_and_can_be_made_smaller() {
        let dir = tempfile::tempdir().unwrap();
        let first = add(dir.path(), &picture(64, 64, false), "First", None).unwrap();
        let mut all = list_all(dir.path()).unwrap();
        all.extend((0..20_000).map(|i| Saved {
            id: format!("{i:016x}"),
            name: "x".repeat(40),
            slim: false,
        }));
        std::fs::write(
            dir.path().join("index.json"),
            serde_json::to_string_pretty(&all).unwrap() + "\n",
        )
        .unwrap();
        assert!(
            std::fs::metadata(dir.path().join("index.json"))
                .unwrap()
                .len()
                > MAX_INDEX
        );
        assert_eq!(
            list(dir.path()).unwrap(),
            std::slice::from_ref(&first),
            "listed"
        );
        assert!(
            add(dir.path(), &picture(64, 64, true), "", None).is_err(),
            "no bigger"
        );
        remove(dir.path(), "0000000000000000").unwrap();
        assert_eq!(
            list_all(dir.path()).unwrap().len(),
            20_000,
            "smaller by one"
        );
        // Other arms add no entry: they go through, though "false" is longer than "true".
        set_slim(dir.path(), &first.id, true).unwrap();
        set_slim(dir.path(), &first.id, false).unwrap();
        let worn = add(dir.path(), &picture(64, 64, false), "", Some(true)).unwrap();
        assert!(
            worn.slim,
            "the arms it is worn with, on an index over the cap"
        );
        remove(dir.path(), &first.id).unwrap();
        assert!(list(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn a_skin_kept_as_seen_never_changes_the_arms_saved_for_it() {
        let dir = tempfile::tempdir().unwrap();
        let skin = picture(64, 64, false);
        let saved = add(dir.path(), &skin, "Knight", Some(true)).unwrap();
        assert!(keep_seen(dir.path(), &skin, false).unwrap().slim);
        assert!(list(dir.path()).unwrap()[0].slim, "the arms it was given");
        assert_eq!(list(dir.path()).unwrap()[0].id, saved.id);
        // A skin not saved yet takes the arms it was seen with.
        assert!(
            !keep_seen(dir.path(), &picture(64, 64, true), false)
                .unwrap()
                .slim
        );
    }
}
