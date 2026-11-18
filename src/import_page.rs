//! Files dropped anywhere on the window (`sunrise_launcher::import`): a shared instance becomes
//! a new instance, a Java modpack goes to the Modpacks import, a resource or shader pack into
//! the selected instance, a game build (a `sunrise-client` program, or an archive or an Actions
//! artifact holding one) installed as its own version. The Packs section and the world rows take
//! their own drops first.





#[derive(Default)]
pub(super) struct Importing {
    /// Shared instances being made.
    busy: usize,
    /// What the last drop did, or why not.
    note: Option<Result<String, String>>,
}


