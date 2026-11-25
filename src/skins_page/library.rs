//! Saved skins and the game's default skins on disk.
use super::*;

/// The key of the saved skin `id` of account `uuid` among the files a page reads: each
/// account's own, as each has its own folder.
pub(super) fn saved_key(uuid: Option<&str>, id: &str) -> String {
    format!("saved:{}:{id}", uuid.unwrap_or_default())
}

/// A default skin's file in the prepared game files.
pub(super) fn default_skin_path(resources: &Path, name: &str, slim: bool) -> PathBuf {
    resources
        .join("assets/minecraft/textures/entity/player")
        .join(if slim { "slim" } else { "wide" })
        .join(format!("{name}.png"))
}

/// Reads the skin files at `paths` into `uuid`'s saved skins, off the UI thread: only regular
/// files (a pipe dropped on the page would block a read), none bigger than a skin. A file that
/// can't be added says so by its name.
pub(super) fn add_files(uuid: &str, paths: &[PathBuf]) -> Vec<Result<Saved, String>> {
    let dir = match skin_library::folder(uuid) {
        Ok(dir) => dir,
        Err(why) => return vec![Err(why)],
    };
    paths
        .iter()
        .map(|path| {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            skin_library::read_skin_file(path)
                .and_then(|png| skin_library::add(&dir, &png, &name, None))
                .map_err(|why| {
                    let file = path
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    trf(
                        "{file} couldn't be added: {why}",
                        &[("file", &file), ("why", &why)],
                    )
                })
        })
        .collect()
}

/// The file of the default skin the game gives `uuid`, with slim or classic arms: the same
/// skin's picture for the arms asked, since each default skin comes in both.
pub(super) fn account_default_path(resources: &Path, uuid: &str, slim: bool) -> Option<PathBuf> {
    let (path, _) = sunrise_launcher::heads::default_skin_file(resources, uuid)?;
    let name = path.file_stem()?.to_str()?;
    Some(default_skin_path(resources, name, slim))
}

/// Lists `uuid`'s saved skins, numbered `id`: what a job on the writes queue sends once it is
/// done, so the page shows the list as that job left it.
pub(super) fn listing(uuid: &str, id: u64) -> skin_library::Listing {
    (
        id,
        skin_library::folder(uuid).and_then(|dir| skin_library::list(&dir)),
    )
}
