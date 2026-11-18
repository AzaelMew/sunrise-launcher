//! The selected setup’s local collections. Reading a list never creates its folder. Choosing a
//! file targets the collection that opened the dialog, even if another setup exists.
use super::ui::{c, card, chip, dim, muted, primary, switch};
use super::*;
use crate::alt::{button, LINE, MUTED, SOFT, TEXT};
use gpui::ExternalPaths;
use std::path::Path;
use sunrise_launcher::packs::{self, Kind};

impl Launcher {
    pub(crate) fn reset_pack_view(&mut self) {
        let m = &mut self.alt.mods;
        m.have.set(String::new());
        m.row_filter.text.clear();
        m.selection.clear();
        m.anchor = None;
        m.confirm = None;
        self.alt_dismiss_notice();
        self.game_packs.confirm_remove = None;
        self.game_packs.stale();
    }

    pub(super) fn pack_page(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let focus = self
            .alt
            .mods
            .have_focus
            .get_or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone();
        let header = self.alt_mods_header(true, &focus, window, cx);
        let kind = self.alt.mods.collection;
        let home = self.alt_mods_home();
        div()
            .id("alt-packs-page")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(24.))
            .text_color(c(TEXT))
            .child(header)
            .children(self.alt_strip(cx))
            .child(self.collection_tabs(cx))
            .child(self.installed_packs(kind, cx))
            .drag_over::<ExternalPaths>(|s, _, _, _| s.border_1().border_color(c(MUTED)))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                this.import_pack_files(home.clone(), kind, paths.paths().to_vec(), cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    pub(super) fn collection_tabs(&self, cx: &mut Context<Self>) -> Div {
        let mut tabs = div().flex().flex_wrap().items_center().gap(px(8.));
        for (index, (kind, label)) in [
            (Kind::Resource, tr("Resource packs")),
            (Kind::Shader, tr("Shader packs")),
        ]
        .into_iter()
        .enumerate()
        {
            let chosen = self.alt.mods.collection == kind;
            tabs = tabs.child(
                chip(
                    ElementId::named_usize("alt-collection", index),
                    label,
                    chosen,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.alt.mods.collection != kind {
                        this.alt.mods.collection = kind;
                        this.reset_pack_view();
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
            && self.alt.page == Page::Packs
            && self.alt.mods.collection == kind
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
            multiple: true,
            prompt: Some(tr("Import from file").into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = pick.await {
                let _ = this.update(cx, |this, cx| this.import_pack_files(home, kind, paths, cx));
            }
        })
        .detach();
    }

    fn import_pack_files(
        &mut self,
        home: PathBuf,
        kind: Kind,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        // A dialog or drop may outlive the view that offered it.
        if paths.is_empty() || !self.can_edit_packs(&home, kind) {
            return;
        }
        self.alt_dismiss_notice();
        self.alt.mods.busy = Some((tr("Importing packs…").into(), 0.));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let _lock =
                        sunrise_launcher::browser::try_lock_home(&home).ok_or_else(|| {
                            tr("This setup is busy. Try again when the operation finishes.")
                                .to_string()
                        })?;
                    let mut added = Vec::new();
                    let mut refused = Vec::new();
                    for path in paths {
                        match packs::add_local(&home, kind, &path) {
                            Ok(p) => added.push(p.title),
                            Err(e) => refused.push(e),
                        }
                    }
                    let mut parts = Vec::new();
                    if !added.is_empty() {
                        parts.push(trf("Imported {name}.", &[("name", &added.join(", "))]));
                    }
                    if refused.is_empty() {
                        Ok(parts.join(" "))
                    } else {
                        parts.extend(refused);
                        Err(parts.join("\n"))
                    }
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
        self.load_packs_list(home.clone());
        let editable = self.can_edit_packs(&home, kind);
        let query = self.alt.mods.have.text().trim().to_lowercase();
        let (detail, empty) = match kind {
            Kind::Resource => (
                tr("Higher packs take priority. A pack you turn on goes to the top."),
                tr("Packs you add from Discover show up here. You can also drop a resource pack (.zip) on this page."),
            ),
            Kind::Shader => (
                tr("Choose one shader pack at a time, or leave them all off."),
                tr("Packs you add from Discover show up here. You can also drop a shader pack (.zip or .sshader) on this page."),
            ),
        };
        let add = button("alt-pack-import", tr("Import from file…"));
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
            .child(div().flex_1().min_w_0().child(muted(detail)))
            .child(open.flex_none())
            .child(add.flex_none());
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
                            packs::remove(&dest, &p)?;
                            // Otherwise Discover thinks this version is still installed and
                            // will not download it again. change_pack holds the home lock.
                            let mut kept = installed::Installed::load(&dest);
                            let before = kept.entries.len();
                            kept.entries
                                .retain(|entry| entry.path(&dest).as_deref() != Some(&p.path));
                            if kept.entries.len() != before {
                                kept.save(&dest)?;
                            }
                            Ok(())
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
        if items.is_empty() {
            let (none, find) = match kind {
                Kind::Resource => (
                    tr("No resource packs installed yet"),
                    tr("Find resource packs in Discover"),
                ),
                Kind::Shader => (
                    tr("No shader packs installed yet"),
                    tr("Find shader packs in Discover"),
                ),
            };
            list = list.child(
                div()
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(12.))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(none))
                    .child(muted(empty))
                    .child(primary("alt-packs-browse", find).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            this.alt_choose_view(Page::PackFind);
                            cx.notify();
                        },
                    ))),
            );
        } else if shown == 0 {
            list = list.child(muted(tr("No packs match your search.")).p(px(20.)));
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
