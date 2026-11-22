//! Install the two signed SDK slices that make up a playable game without a Rust toolchain.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GameSlices {
    pub(crate) index: String,
    commit: String,
    slices: BTreeMap<String, String>,
}

/// `game_index` and its digest come from the signed game release, not from the SDK host.
pub(crate) fn stage_game(
    stage: &Path,
    version: &str,
    triple: &str,
    (game_index, digest): (&[u8], &str),
    base: &Base,
    keys: &[PublicKey],
    progress: &dyn Fn(f32),
) -> Result<(PathBuf, GameSlices), Refusal> {
    let commit = game_commit(
        version,
        sunrise_launcher_release_verify::Channel::of(version),
        game_index,
    )?;
    let index = signed_sdk_index(base, &commit, keys)?;
    let refused = |why| Refusal::Refused {
        build: version.to_owned(),
        why,
    };
    let sha256 = sha256(&index);
    sunrise_launcher_release_verify::check(INDEX, digest, &sha256).map_err(refused)?;
    let sdk = Sdk {
        index: Index::read(&index, &commit).map_err(|error| refused(error.to_string()))?,
        sha256,
        dir: stage.to_owned(),
    };
    let mut receipt = GameSlices {
        index: sdk.sha256.clone(),
        commit,
        slices: BTreeMap::new(),
    };
    let wanted = [Kind::Runtime, Kind::Programs].map(|kind| {
        (
            Asset::name(kind, triple, Side::Client),
            stage.join(kind.name()),
        )
    });
    let mut fetched = Fetched::default();
    for (name, _) in &wanted {
        let asset = sdk.index.asset(name).map_err(|_| Refusal::Host)?;
        fetched.total = fetched.total.saturating_add(asset.size);
        receipt.slices.insert(name.clone(), asset.sha256.clone());
    }
    let report = |p: Fetched| progress(0.9 * (p.done as f32 / p.total.max(1) as f32).min(1.0));
    // Always unpack into the private installation stage. SDK cache stamps check sizes, not
    // file hashes, and are not proof of the bytes in a newly installed game.
    for (name, into) in &wanted {
        fetch_into(&sdk, base, name, into, &mut fetched, &report)?;
    }
    let [(_, runtime), (_, programs)] = wanted;
    let bin = runtime.join("bin");
    let io = |error: std::io::Error| Refusal::Cache(error.to_string());
    // The executable and std are already together in bin/. Move the game libraries there
    // too: ELF's $ORIGIN, Mach-O's @executable_path and Windows all search beside the program.
    for folder in [runtime.join("deps"), programs] {
        for entry in fs::read_dir(&folder).map_err(io)? {
            let entry = entry.map_err(io)?;
            let to = bin.join(entry.file_name());
            if to.try_exists().map_err(io)? {
                return Err(io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    to.display().to_string(),
                )));
            }
            fs::rename(entry.path(), to).map_err(io)?;
        }
    }
    if !bin.join(crate::preparer::BINARY).is_file() {
        return Err(refused(
            sdk::Refusal::Missing(vec![crate::preparer::BINARY.into()]).to_string(),
        ));
    }
    Ok((bin, receipt))
}
