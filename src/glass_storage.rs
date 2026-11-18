//! Cached storage inventory; filesystem work never runs on the render thread.
use super::*;

use storage::Category;
use sunrise_launcher::browser;

impl Launcher {
    pub(super) fn refresh_storage(&mut self, clear: Option<Category>, all: bool) {
        if self.storage_pending {
            return;
        }
        self.storage_pending = true;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = storage::Storage::open()
                .and_then(|storage| storage.report(clear, all))
                .map(Box::new)
                .map_err(native_mods::cache_error);
            let _ = tx.send(Msg::Storage(result));
        });
    }

    /// A finished measurement or Clear from the worker.
    pub(super) fn storage_arrived(&mut self, result: Result<Box<storage::Report>, String>) {
        self.storage_pending = false;
        match result {
            Ok(report) => {
                self.storage_report = Some(*report);
                self.storage_error = None;
            }
            Err(error) => self.storage_error = Some(error),
        }
    }

    /// Counts the switched-on native mods and the instances holding them, on a worker, then
    /// asks before Turn off.
    pub(crate) fn storage_count_native(&mut self, cx: &mut Context<Self>) {
        let visit = self.storage_visit;
        self.storage_counting = Some(visit);
        self.storage_revoke_error = None;
        let task = cx.background_executor().spawn(async move {
            let counts: Vec<usize> = instances::list()
                .iter()
                .map(|i| native_status::enabled(&i.home).len())
                .filter(|n| *n > 0)
                .collect();
            (counts.iter().sum::<usize>(), counts.len())
        });
        cx.spawn(async move |this, cx| {
            let counted = task.await;
            let _ = this.update(cx, |this, cx| {
                // A count from a visit the player has left asks nothing.
                if this.storage_counting == Some(visit) {
                    this.storage_counting = None;
                    this.storage_revoke = Some(counted);
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// Takes the permission back (saved first: nothing changes if it can't be), then switches
    /// every native mod off, on a worker; a build card still open is cleared, so its buttons ask
    /// again rather than fail.
    pub(crate) fn turn_off_native_mods(&mut self, cx: &mut Context<Self>) {
        self.storage_revoke = None;
        if let Err(error) = self.settings.revoke_native_mods() {
            self.storage_revoke_error = Some(error);
            cx.notify();
            return;
        }
        self.native_error = None;
        self.native_worlds = None;
        self.native_failed_mods.clear();
        let task = cx.background_executor().spawn(async move {
            let mut failure = None;
            for instance in instances::list() {
                for (id, _, _) in native_status::enabled(&instance.home) {
                    if let Err(error) = browser::java::set_enabled(&instance.home, &id, false) {
                        failure.get_or_insert((id, instance.name.clone(), error));
                    }
                }
            }
            failure
        });
        cx.spawn(async move |this, cx| {
            let failure = task.await;
            let _ = this.update(cx, |this, cx| {
                this.storage_revoke_error = failure.map(|(id, instance, error)| {
                    trf(
                        "The permission is off, but {mod} in {instance} couldn't be switched off \
                         ({error}). Switch it off in {instance}'s Installed mods.",
                        &[
                            ("mod", &id),
                            ("instance", &instance),
                            ("error", error.trim_end_matches('.')),
                        ],
                    )
                });
                this.instance_mods_changed();
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

}

pub(crate) fn category_label(category: Category) -> &'static str {
    match category {
        Category::BuildFiles => "Build files",
        Category::PrebuiltGames => "Prebuilt games",
        Category::SpareGameBuilds => "Spare game builds",
        Category::RustCompiler => "Rust compiler",
        Category::OtherCaches => "Other caches",
    }
}

/// What a row holds, and that clearing it costs only a download or a rebuild.
pub(crate) fn category_about(category: Category) -> &'static str {
    match category {
        Category::BuildFiles => "What building native mods leaves behind: made again when needed.",
        Category::PrebuiltGames => {
            "The game packages native mods are built against: downloaded again when needed."
        }
        Category::SpareGameBuilds => {
            "Game builds kept for running servers: downloaded again when needed."
        }
        Category::RustCompiler => {
            "The compiler that builds native mods: downloaded again the next time one is built."
        }
        Category::OtherCaches => "Other downloads kept to make the launcher faster.",
    }
}

