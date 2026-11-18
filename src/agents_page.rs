//! Settings' AI rows: the No AI switch (`sunrise_launcher::no_ai`, shared with the game) and "Connect
//! an AI agent", the ready-made MCP config of each common client with Copy buttons, a one-click
//! add where the client has a CLI (run only on that click, its command shown first), and a test
//! that starts the MCP server and lists its tools. With No AI on, only the switch shows.



#[derive(Default)]
pub(crate) struct AgentsUi {
    /// The client shown, an index into `agents::clients`.
    client: usize,
    /// Which Copy button just copied.
    copied: Option<&'static str>,
    /// A command or test running.
    busy: bool,
    /// What the last add or test said, and whether it went well.
    status: Option<(bool, String)>,
}


