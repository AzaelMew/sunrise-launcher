//! A Java instance's part of the glass instance dialog (`glass_instance.rs`; the mockup's
//! `InstanceModal` for Vanilla and Modded): Mojang's versions to pick from, then for Modded the
//! loaders, each with its logo, as many as `loaders::Kind` knows.
use super::*;
use crate::glass_ui::{self as ui, white};
use glass_instance::{
    choice, list_box, list_choice, loaders, section, tick};
use new_instance::{NewKind, VersionList};
use servers_page::Field;
use sunrise_launcher::instances::{ Instance, Plays};
use sunrise_launcher::loaders::Loader;
use sunrise_launcher::minecraft_versions::Kind as VersionKind;
use sunrise_launcher_theme::glass::{fill, ink, radius};

/// Mojang versions listed at most: the search finds the others.
const ROWS: usize = 60;

impl Launcher {
    /// Mojang's versions: Snapshots and Old beside the title, the search, the list.
    pub(super) fn minecraft_versions(&mut self, cx: &mut Context<Self>) -> Div {
        let t = self.theme;
        let soft = self.look.accent_soft;
        let Some(choice) = self.instances.dialog.as_ref().map(|d| d.java.clone()) else {
            return div();
        };
        let typed = self
            .servers
            .value(Field::InstanceVersion)
            .trim()
            .to_string();
        let search = typed.to_lowercase();
        let chosen = self.draft_minecraft();
        let filter = |id: &'static str, label: &'static str, on: bool| {
            keyboard(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .h(px(32.))
                    .px(px(12.))
                    .rounded(px(crate::glass_ui::rad(radius::CONTROL)))
                    .cursor_pointer()
                    .font_family(sunrise_launcher_theme::glass::FONT)
                    .text_size(px(13.))
                    .when(on, |d| {
                        d.bg(white(fill::BUTTON_HOVER))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(white(ink::FULL))
                    })
                    .when(!on, |d| {
                        d.text_color(white(ink::MUTED))
                            .hover(|s| s.text_color(white(ink::FULL)))
                    })
                    .child(tr(label)),
                &t,
            )
        };
        let filters = div()
            .flex()
            .gap(px(6.))
            .child(
                filter("mc-snapshots", "Snapshots", choice.snapshots).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.change(|j| j.snapshots = !j.snapshots);
                        cx.notify();
                    },
                )),
            )
            .child(filter("mc-old", "Old", choice.old).on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    this.change(|j| j.old = !j.old);
                    cx.notify();
                },
            )));
        let field = div()
            .relative()
            .mb(px(10.))
            .child(
                self.field(Field::InstanceVersion, tr("Search versions"), 600., cx)
                    .w_full()
                    .pl(px(44.)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(16.))
                    .h_full()
                    .flex()
                    .items_center()
                    .child(ui::icon("search-01", 17., white(ink::DIM))),
            );
        let line = |s: String, a: f32| {
            ui::text(14., FontWeight::NORMAL, a)
                .px(px(16.))
                .py(px(24.))
                .child(s)
        };
        let list = match &self.instances.versions {
            VersionList::NotAsked | VersionList::Loading => list_box("mc-versions", 208.).child(
                line(tr("Asking Mojang for its versions…").into(), ink::QUIET),
            ),
            VersionList::Failed(e) => list_box("mc-versions", 208.)
                .child(line(e.clone(), ink::FULL).text_color(ui::danger())),
            VersionList::Ready(versions) => {
                let rows: Vec<_> = versions
                    .of(choice.snapshots, choice.old)
                    .filter(|v| search.is_empty() || v.id.to_lowercase().contains(&search))
                    .take(ROWS)
                    .map(|v| {
                        let on = chosen.as_deref() == Some(v.id.as_str());
                        let id = v.id.clone();
                        let kind = match v.kind {
                            VersionKind::Release => "Release",
                            VersionKind::Snapshot => "Snapshot",
                            VersionKind::OldBeta => "Beta",
                            VersionKind::OldAlpha => "Alpha",
                        };
                        list_choice(format!("mc-{}", v.id), on, &t)
                            .py(px(10.))
                            .child(
                                ui::text(15., FontWeight::SEMIBOLD, ink::FULL)
                                    .w(px(160.))
                                    .flex_none()
                                    .truncate()
                                    .child(v.id.clone()),
                            )
                            .child(
                                ui::text(13., FontWeight::NORMAL, ink::QUIET)
                                    .flex_1()
                                    .child(format!(
                                        "{} · {}",
                                        tr(kind),
                                        v.released.get(..10).unwrap_or("")
                                    )),
                            )
                            .child(tick(on, soft))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.change(|j| j.version = Some(id.clone()));
                                cx.notify();
                            }))
                    })
                    .collect();
                if rows.is_empty() {
                    list_box("mc-versions", 208.).child(line(
                        trf("No version matches “{search}”.", &[("search", &typed)]),
                        ink::QUIET,
                    ))
                } else {
                    list_box("mc-versions", 208.).children(rows)
                }
            }
        };
        section(
            tr("Minecraft version").into(),
            Some(filters),
            div().flex().flex_col().child(field).child(list),
        )
    }

    /// What the dialog has `editing` play: Sunrise, or a Java game with its loader. The loader
    /// keeps its version while its kind and Minecraft stay; a new one's (empty here) is the
    /// newest stable, asked of its service on Save.
    pub(super) fn draft_plays(&self, editing: &Instance) -> Option<Plays> {
        let d = self.instances.dialog.as_ref()?;
        if d.java.kind == NewKind::Sunrise {
            return Some(Plays::Sunrise);
        }
        let minecraft = self.draft_minecraft()?;
        let loader = (d.java.kind == NewKind::Modded).then(|| {
            editing
                .java
                .as_ref()
                .filter(|j| j.minecraft == minecraft)
                .and_then(|j| j.loader.clone())
                .filter(|l| l.kind == d.java.loader)
                .unwrap_or(Loader {
                    kind: d.java.loader,
                    version: String::new(),
                })
        });
        Some(Plays::Java { minecraft, loader })
    }

}
