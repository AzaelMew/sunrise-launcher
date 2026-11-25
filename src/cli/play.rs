//! `play` and `server`: starting the game or a dedicated server in this terminal.
use super::*;

pub(super) fn server(
    dir: Option<PathBuf>,
    version: Option<String>,
    accept_eula: bool,
    allow_jar_downloads: bool,
) -> Result<i32, Error> {
    let tag = backend::resolve_version(version.as_deref())?;
    let dir = backend::server_dir(dir);
    // The server inherits this terminal, so typed lines reach its console.
    let mut cmd = backend::server(&tag, &dir, accept_eula)?;
    sunrise_launcher::jars::prepare_launch(
        &mut cmd,
        &Default::default(),
        sunrise_launcher::jars::Permission::headless(allow_jar_downloads),
    )
    .map_err(|error| error.to_string())?;
    let (cmd, _runtime) = backend::prepare_server_command(&tag, &dir, cmd)?;
    attached_prepared(cmd).map_err(Error::from)
}
