//! Files dropped anywhere on the window (`sunrise_launcher::import`): a shared instance becomes
//! a new instance, a Java modpack goes to the Modpacks import, a resource or shader pack into
//! the selected instance, a game build (a `sunrise-client` program, or an archive or an Actions
//! artifact holding one) installed as its own version. The Packs section and the world rows take
//! their own drops first.
use super::*;

use std::path::PathBuf;
use sunrise_launcher::import::{self};

#[derive(Default)]
pub(super) struct Importing {
    /// Shared instances being made.
    busy: usize,
    /// What the last drop did, or why not.
    note: Option<Result<String, String>>,
}


impl Launcher {

    fn import_instance(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        self.importing.busy += 1;
        self.importing.note = None;
        let task = cx
            .background_executor()
            .spawn(async move { import::import_instance(&file, &|_, _| {}) });
        cx.spawn(async move |this, cx| {
            let made = task.await;
            let _ = this.update(cx, |this, cx| {
                this.importing.busy -= 1;
                this.importing.note = Some(made.map(|i| {
                    this.reload_instances();
                    this.select_instance(&i);
                    this.go(Page::Play);
                    trf(
                        "Made the instance {name} from the shared file.",
                        &[("name", &i.name)],
                    )
                }));
                cx.notify();
            });
        })
        .detach();
    }

}
