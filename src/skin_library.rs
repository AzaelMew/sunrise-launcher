//! Saved skins: the skins an account wore or its player added on the Skins tab, kept in the
//! launcher's data folder, each account's apart (`saved-skins/<uuid>/`: each skin's PNG under its
//! id, and `index.json` with their names and arm models) so they can be worn again with one
//! click. They leave the computer only when worn, and then only to the Minecraft services.
//!
//! Every change to an index (adding a skin, its arm model, removing one) reads it, changes it
//! and writes it back under one lock ([`changing`]), so two changes at once in this launcher
//! (two imports, an import beside a Save keeping the skin it replaces) can't each write over
//! what the other added.
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
        Err("This account has no valid profile id, so it can't keep saved skins.".into())
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

/// Every entry of the index in `dir`, those whose picture is gone included: what a change
/// writes back, so it leaves the entries it doesn't touch as they were.
fn list_all(dir: &Path) -> Result<Vec<Saved>, String> {
    let text = match std::fs::read_to_string(dir.join("index.json")) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("The saved skins couldn't be read: {e}")),
    };
    serde_json::from_str(&text)
        .map_err(|e| format!("The saved skins' list is damaged ({e}), so it was left as it is."))
}

/// A saved skin's picture.
pub fn png(dir: &Path, id: &str) -> Option<Vec<u8>> {
    valid_id(id)
        .then(|| read_skin_file(&png_path(dir, id)).ok())
        .flatten()
}

/// The file at `path`, if it is a regular file no bigger than a skin
/// ([`crate::account::skins::MAX_SKIN`]): nothing larger is read into memory, whatever its length
/// claims, and a pipe put in its place is never waited on.
pub fn read_skin_file(path: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read as _;
    let max = crate::account::skins::MAX_SKIN as u64;
    let mut open = std::fs::OpenOptions::new();
    open.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        open.custom_flags(libc::O_NONBLOCK);
    }
    let file = open.open(path).map_err(|e| e.to_string())?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err(crate::i18n::tr("Only a PNG file can be added as a skin.").to_string());
    }
    let mut bytes = Vec::new();
    if meta.len() <= max {
        file.take(max + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
    }
    if meta.len() > max || bytes.len() as u64 > max {
        return Err(crate::i18n::tr("This picture is too big for a skin.").to_string());
    }
    Ok(bytes)
}

fn png_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.png"))
}

fn valid_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Writes the index; only ever called under [`changing`].
fn save(dir: &Path, list: &[Saved]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    write_whole(&dir.join("index.json"), (text + "\n").as_bytes())
}

/// Saves the skin `png` as `name`, its arms as `slim` says, else as the picture suggests
/// ([`crate::player_model::looks_slim`]). When the same skin is saved already, it keeps its name,
/// and its arms unless `slim` says otherwise; but its picture is checked, and written anew when
/// it is gone, damaged or another skin's, so the saved skin returned is always there to wear
/// again.
pub fn add(dir: &Path, png: &[u8], name: &str, slim: Option<bool>) -> Result<Saved, String> {
    if png.len() > crate::account::skins::MAX_SKIN {
        return Err("This picture is too big for a skin.".into());
    }
    let id =
        id_of_png(png).ok_or("This isn't a Minecraft skin: it must be a 64×64 or 64×32 PNG.")?;
    let _changing = changing();
    let mut all = list_all(dir)?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = png_path(dir, &id);
    let there = read_skin_file(&path).ok().and_then(|p| id_of_png(&p));
    if there.as_deref() != Some(id.as_str()) {
        write_whole(&path, png)?;
    }
    if let Some(known) = all.iter_mut().find(|s| s.id == id) {
        if slim.is_none_or(|slim| slim == known.slim) {
            return Ok(known.clone());
        }
        known.slim = slim.unwrap_or(known.slim);
        let known = known.clone();
        save(dir, &all)?;
        return Ok(known);
    }
    let name: String = name.trim().chars().take(40).collect();
    let saved = Saved {
        id,
        name,
        slim: slim.unwrap_or_else(|| crate::player_model::looks_slim(png)),
    };
    all.push(saved.clone());
    save(dir, &all)?;
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
}
