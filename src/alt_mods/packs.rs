//! The selected setup’s local collections. Reading a list never creates its folder. Choosing a
//! file targets the collection that opened the dialog, even if another setup exists.
use super::ui::{c, card, dim, muted, primary, switch};
use super::*;
use crate::alt::{button, LINE, MUTED, SOFT, TEXT};
use std::path::Path;
use sunrise_launcher::packs::{self, Kind};

impl Launcher {
    pub(super) fn collection_tabs(&self, cx: &mut Context<Self>) -> Div {
        let mut tabs = div().flex().items_center().gap(px(8.));
        for (index, (kind, label)) in [
            (None, tr("Mods")),
            (Some(Kind::Resource), tr("Resource packs")),
            (Some(Kind::Shader), tr("Shader packs")),
        ]
        .into_iter()
        .enumerate()
        {
            let chosen = self.alt.mods.collection == kind;
            tabs = tabs.child(
                button(ElementId::named_usize("alt-collection", index), label)
                    .when(chosen, |d| d.bg(c(LINE)).border_color(c(TEXT)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.alt.mods.collection != kind {
                            let m = &mut this.alt.mods;
                            m.collection = kind;
                            m.have.set(String::new());
                            m.row_filter.text.clear();
                            m.selection.clear();
                            m.anchor = None;
                            m.confirm = None;
                            this.alt_dismiss_notice();
                            this.game_packs.confirm_remove = None;
                            this.game_packs.stale();
                        }
                        cx.notify();
                    })),
            );
        }
        tabs
    }

    fn can_edit_packs(&self, home: &Path, kind: Kind) -> bool {
        self.location_error.is_none()
            && self.alt_mods_locked().is_none()
            && !self.alt_mutating()
            && !self.alt.mods.examples
            && self.alt.page == Page::Installed
            && self.alt.mods.collection == Some(kind)
            && self
                .selected_instance()
                .is_some_and(|i| i.java.is_none() && i.home == home)
    }

    fn import_pack(&mut self, home: PathBuf, kind: Kind, cx: &mut Context<Self>) {
        if !self.can_edit_packs(&home, kind) {
            return;
        }
        let pick = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(tr("Import from file").into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = pick.await else {
                return;
            };
            let Some(path) = paths.pop() else {
                return;
            };
            // The file dialog is asynchronous: don't apply a past selection to today's setup.
            let started = this.update(cx, |this, cx| {
                if !this.can_edit_packs(&home, kind) {
                    return false;
                }
                this.alt_dismiss_notice();
                this.alt.mods.busy = Some((tr("Importing pack…").into(), 0.));
                cx.notify();
                true
            });
            if !matches!(started, Ok(true)) {
                return;
            }
            let result = cx
                .background_executor()
                .spawn(async move {
                    let _lock =
                        sunrise_launcher::browser::try_lock_home(&home).ok_or_else(|| {
                            tr("This setup is busy. Try again when the operation finishes.")
                                .to_string()
                        })?;
                    packs::add_local(&home, kind, &path)
                        .map(|p| trf("Imported {name}.", &[("name", &p.title)]))
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.alt.mods.busy = None;
                this.alt.mods.notice = Some(result);
                this.game_packs.stale();
                cx.notify();
            });
        })
        .detach();
    }

    fn change_pack(
        &mut self,
        home: &Path,
        kind: Kind,
        change: impl FnOnce() -> Result<(), String>,
    ) {
        if !self.can_edit_packs(home, kind) {
            return;
        }
        let result = match sunrise_launcher::browser::try_lock_home(home) {
            Some(_guard) => change(),
            None => Err(tr("This setup is busy. Try again when the operation finishes.").into()),
        };
        self.alt.mods.notice = result.err().map(Err);
        self.game_packs.confirm_remove = None;
        self.game_packs.stale();
    }

    pub(super) fn installed_packs(&mut self, kind: Kind, cx: &mut Context<Self>) -> Div {
        let Some(instance) = self.selected_instance() else {
            return div();
        };
        let home = instance.home.clone();
        let name = instance.name.clone();
        self.load_packs_list(home.clone());
        let editable = self.can_edit_packs(&home, kind);
        let query = self.alt.mods.have.text().trim().to_lowercase();
        let (title, detail, empty) = match kind {
            Kind::Resource => (tr("Resource packs"), tr("Higher packs take priority. A pack you turn on goes to the top."), tr("No resource packs yet. Import a local resource pack (.zip) to add it to this setup.")),
            Kind::Shader => (tr("Shader packs"), tr("Choose one shader pack at a time, or leave them all off."), tr("No shader packs yet. Import a local shader pack (.zip or .sshader) to add it to this setup.")),
        };
        let add = primary("alt-pack-import", tr("Import from file…"));
        let add = if editable {
            let dest = home.clone();
            add.on_click(
                cx.listener(move |this, _, _, cx| this.import_pack(dest.clone(), kind, cx)),
            )
        } else {
            dim(add)
        };
        let folder = home.join(kind.folder());
        let open = button("alt-pack-folder", tr("Open folder"));
        let open = if folder.is_dir() || editable {
            open.on_click(cx.listener(move |this, _, _, cx| {
                match std::fs::create_dir_all(&folder) {
                    Ok(()) => crate::glass_toast::open(cx, &folder),
                    Err(e) => this.alt.mods.notice = Some(Err(e.to_string())),
                }
                cx.notify();
            }))
        } else {
            dim(open)
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .text_size(px(19.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(trf(
                                "{collection} · {setup}",
                                &[("collection", title), ("setup", &name)],
                            )),
                    )
                    .child(muted(detail)),
            )
            .child(open)
            .child(add);
        let items: Vec<_> = match kind {
            Kind::Resource => self
                .game_packs
                .resource
                .iter()
                .map(|(p, icon)| (p, icon.clone()))
                .collect(),
            Kind::Shader => self.game_packs.shader.iter().map(|p| (p, None)).collect(),
        };
        let mut list = card()
            .id("alt-collection-list")
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let mut shown = 0;
        for (index, (pack, icon)) in items.iter().enumerate() {
            if !query.is_empty()
                && !pack.title.to_lowercase().contains(&query)
                && !pack.file.to_lowercase().contains(&query)
            {
                continue;
            }
            let mut row = div()
                .flex_none()
                .h(px(64.))
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(16.))
                .when(shown > 0, |d| d.border_t_1().border_color(c(SOFT)));
            shown += 1;
            let image = div()
                .size(px(36.))
                .flex_none()
                .rounded(px(6.))
                .bg(c(LINE))
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center();
            row = row
                .child(match icon {
                    Some(icon) => image.child(gpui::img(icon.clone()).size_full()),
                    None => image.child(blocks::icon("package", 16., c(MUTED))),
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .truncate()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(pack.title.clone()),
                        )
                        .when(!pack.description.is_empty(), |d| {
                            d.child(muted(tr(&pack.description).to_owned()).truncate())
                        }),
                );
            if kind == Kind::Resource && pack.enabled && query.is_empty() {
                for (label, adjacent) in [
                    (tr("Move up"), index.checked_sub(1)),
                    (tr("Move down"), Some(index + 1)),
                ] {
                    let Some((neighbor, _)) = adjacent
                        .and_then(|i| items.get(i))
                        .filter(|(p, _)| p.enabled)
                    else {
                        continue;
                    };
                    let b = button(
                        SharedString::from(format!("alt-order-{index}-{adjacent:?}")),
                        label,
                    );
                    let (file, neighbor, dest) =
                        (pack.file.clone(), neighbor.file.clone(), home.clone());
                    row = row.child(if editable {
                        b.on_click(cx.listener(move |this, _, _, cx| {
                            this.change_pack(&dest, kind, || {
                                packs::move_resource(&dest, &file, &neighbor)
                            });
                            cx.notify();
                        }))
                    } else {
                        dim(b)
                    });
                }
            }
            let key = (kind, pack.file.clone());
            let confirm = self.game_packs.confirm_remove.as_ref() == Some(&key);
            let remove = button(
                ElementId::named_usize("alt-pack-remove", index),
                if confirm {
                    tr("Move to trash?")
                } else {
                    tr("Remove")
                },
            );
            let remove = if editable {
                let dest = home.clone();
                remove.on_click(cx.listener(move |this, _, _, cx| {
                    if !this.can_edit_packs(&dest, kind) {
                        return;
                    }
                    if this.game_packs.confirm_remove.as_ref() == Some(&key) {
                        this.change_pack(&dest, kind, || {
                            let p = packs::find(&dest, kind, &key.1)?;
                            if p.file != key.1 {
                                return Err(
                                    tr("This pack changed. Check the refreshed list.").into()
                                );
                            }
                            packs::remove(&dest, &p)
                        });
                    } else {
                        this.game_packs.confirm_remove = Some(key.clone());
                    }
                    cx.notify();
                }))
            } else {
                dim(remove)
            };
            let toggle = switch(
                ElementId::named_usize("alt-pack-switch", index),
                pack.enabled,
            );
            let toggle = if editable {
                let (dest, file, was_on) = (home.clone(), pack.file.clone(), pack.enabled);
                toggle.on_click(cx.listener(move |this, _, _, cx| {
                    this.change_pack(&dest, kind, || {
                        let p = packs::find(&dest, kind, &file)?;
                        if p.file != file || p.enabled != was_on {
                            return Err(tr("This pack changed. Check the refreshed list.").into());
                        }
                        packs::set_enabled(&dest, &p, !was_on)
                    });
                    cx.notify();
                }))
            } else {
                dim(toggle)
            };
            list = list.child(row.child(remove).child(toggle));
        }
        if shown == 0 {
            list = list.child(
                muted(if items.is_empty() {
                    empty
                } else {
                    tr("No packs match your search.")
                })
                .p(px(20.)),
            );
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(header)
            .children(self.alt_mods_locked().map(muted))
            .child(list)
    }
}
