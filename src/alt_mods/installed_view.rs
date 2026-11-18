//! Mods › Installed (mocks 05, 05b): what needs the player first, then updates, then the rest;
//! one switch per row; bulk actions that name their count and scope.
use super::model::{self, Group, Make, Need, Row, Show};
use super::ui::*;
use super::*;
use crate::alt::{BG, GOLD, MUTED, SOFT, TEXT};
use std::rc::Rc;
use sunrise_launcher::browser::images;

const BLUE: u32 = 0x7fb0ff;
const DANGER: u32 = 0xf08a7a;

impl Launcher {
    pub(super) fn alt_installed(
        &mut self,
        window: &mut Window,
        focus: &FocusHandle,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = self.alt_mods_header(true, focus, window, cx);
        let mut page = div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(24.))
            .text_color(c(TEXT))
            .child(header)
            .children(self.alt.mods.examples.then(example_banner))
            .children(self.alt_strip(cx));

        let rows = self.alt_rows();
        if rows.is_empty() {
            return page
                .child(
                    card()
                        .p(px(24.))
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(px(12.))
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(tr("No mods installed yet")))
                        .child(muted(tr("Mods you add from Discover show up here, with their updates and a switch for each.")))
                        .child(primary("alt-installed-browse", tr("Find mods in Discover")).on_click(cx.listener(
                            |this, _: &ClickEvent, _, cx| {
                                this.alt_choose_view(Page::Discover);
                                cx.notify();
                            },
                        ))),
                )
                .into_any_element();
        }

        let filter = self.alt.mods.row_filter.clone();
        let visible = model::visible(&rows, &filter);
        let shown_ids: Rc<Vec<String>> = Rc::new(
            visible
                .iter()
                .filter(|&&i| rows[i].local.is_none())
                .map(|&i| rows[i].id.clone())
                .collect(),
        );
        let selected: Vec<&Row> = rows
            .iter()
            .filter(|r| r.local.is_none() && self.alt.mods.selection.contains(&r.id))
            .collect();

        // ---- what to show: the same row of chips Discover filters with
        let count = |f: &dyn Fn(&Row) -> bool| rows.iter().filter(|r| f(r)).count();
        let counted = |label: &str, n: usize| format!("{label} · {n}");
        let mut chips = div().flex().flex_wrap().items_center().gap(px(8.));
        for (i, (label, n, which)) in [
            (tr("All"), rows.len(), Show::All),
            (tr("Turned on"), count(&|r| r.on), Show::On),
            (tr("Turned off"), count(&|r| !r.on), Show::Off),
        ]
        .into_iter()
        .enumerate()
        {
            chips = chips.child(
                chip(("alt-show", i), counted(label, n), filter.show == which).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        let m = &mut this.alt.mods;
                        m.row_filter.show = which;
                        m.selection.clear();
                        m.anchor = None;
                        cx.notify();
                    }),
                ),
            );
        }
        chips = chips.child(
            div()
                .flex_none()
                .w(px(1.))
                .h(px(20.))
                .bg(c(crate::alt::LINE)),
        );
        for (i, make) in [Make::Native, Make::Content].into_iter().enumerate() {
            let n = count(&|r| r.make == Some(make));
            chips = chips.child(
                chip(
                    ("alt-kind", i),
                    counted(make.label(), n),
                    filter.makes.contains(&make),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let m = &mut this.alt.mods;
                    if !m.row_filter.makes.remove(&make) {
                        m.row_filter.makes.insert(make);
                    }
                    m.selection.clear();
                    m.anchor = None;
                    cx.notify();
                })),
            );
        }
        page = page.child(chips);

        // ---- toolbar: select-all and the bulk actions, naming their scope
        let selected_here = shown_ids
            .iter()
            .filter(|id| self.alt.mods.selection.contains(*id))
            .count();
        let all_on = selected_here > 0 && selected_here == shown_ids.len();
        let n_sel = selected.len();
        let real = !self.alt.mods.examples;
        let busy = self.alt_mods_busy();
        let with_update = selected.iter().filter(|r| r.update.is_some()).count();
        let bulk = |id: &'static str, label: String, enabled: bool, danger: bool| {
            let b = crate::alt::button(id, label);
            let b = if danger { b.text_color(c(DANGER)) } else { b };
            if enabled && real && !busy {
                b
            } else {
                dim(b)
            }
        };
        let toolbar = {
            let ids = shown_ids.clone();
            let select_all = checkbox("alt-select-all", all_on)
                .when(selected_here > 0 && !all_on, |d| {
                    d.bg(c(TEXT))
                        .border_color(c(TEXT))
                        .child(div().w(px(10.)).h(px(2.)).bg(c(BG)))
                })
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    let m = &mut this.alt.mods;
                    match ids.iter().all(|id| m.selection.contains(id)) {
                        true => m.selection.clear(),
                        false => m.selection.extend(ids.iter().cloned()),
                    }
                    m.anchor = None;
                    cx.notify();
                }));
            let mut bar = div()
                .flex()
                .items_center()
                .gap(px(10.))
                .px(px(14.))
                .h(px(48.))
                .border_b_1()
                .border_color(c(SOFT))
                .child(select_all);
            if n_sel == 0 {
                bar = bar.child(muted(trf(
                    "{shown} of {total} mods shown",
                    &[
                        ("shown", &shown_ids.len().to_string()),
                        ("total", &rows.len().to_string()),
                    ],
                )));
            } else {
                let on = bulk(
                    "alt-bulk-on",
                    trf("Turn on {count}", &[("count", &n_sel.to_string())]),
                    true,
                    false,
                );
                let off = bulk(
                    "alt-bulk-off",
                    trf("Turn off {count}", &[("count", &n_sel.to_string())]),
                    true,
                    false,
                );
                let upd = bulk(
                    "alt-bulk-update",
                    trf("Update {count}", &[("count", &with_update.to_string())]),
                    with_update > 0,
                    false,
                );
                let rm = bulk(
                    "alt-bulk-remove",
                    trf("Remove {count}…", &[("count", &n_sel.to_string())]),
                    true,
                    true,
                );
                bar = bar
                    .child(div().text_size(px(13.)).child(trf(
                        "{count} selected of {shown} shown",
                        &[
                            ("count", &n_sel.to_string()),
                            ("shown", &shown_ids.len().to_string()),
                        ],
                    )))
                    .child(if real && !busy {
                        on.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.alt_bulk_on();
                            cx.notify();
                        }))
                    } else {
                        on
                    })
                    .child(if real && !busy {
                        off.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.alt_bulk_off();
                            cx.notify();
                        }))
                    } else {
                        off
                    })
                    .child(if real && !busy && with_update > 0 {
                        upd.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.alt_ask_update(true);
                            cx.notify();
                        }))
                    } else {
                        upd
                    })
                    .child(if real && !busy {
                        rm.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.alt_ask_remove();
                            cx.notify();
                        }))
                    } else {
                        rm
                    })
                    .child(div().flex_1())
                    .child(
                        muted(tr("Clear"))
                            .id("alt-selection-clear")
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.alt.mods.selection.clear();
                                this.alt.mods.anchor = None;
                                cx.notify();
                            })),
                    );
            }
            bar
        };

        // ---- the confirmation, when one is open: names, counts, nothing started yet
        let confirm = self.alt.mods.confirm.as_ref().map(|confirm| {
            let panel = card().m(px(12.)).p(px(16.)).flex().flex_col().gap(px(10.)).border_color(c(GOLD));
            match confirm {
                Confirm::Remove(chosen) => {
                    let ids: HashSet<String> = chosen.iter().map(|(id, ..)| id.clone()).collect();
                    let index = sunrise::known_index();
                    let dependents = model::dependents(&rows, &ids, index.as_deref());
                    let names = chosen.iter().map(|(_, _, t)| t.as_str()).take(12).collect::<Vec<_>>().join(", ");
                    let more = chosen.len().saturating_sub(12);
                    let items = chosen.clone();
                    panel
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(trf("Remove {count} mods?", &[("count", &chosen.len().to_string())])))
                        .child(div().text_size(px(13.)).child(match more {
                            0 => names,
                            n => trf("{names} and {more} more", &[("names", &names), ("more", &n.to_string())]),
                        }))
                        .children(dependents.into_iter().map(|(who, needed)| {
                            div().text_size(px(13.)).text_color(c(GOLD)).child(trf(
                                "{who} needs {needed}: it may stop working.",
                                &[("who", &who), ("needed", &needed.join(", "))],
                            ))
                        }))
                        .child(muted(tr("Their files are deleted. Worlds saved with them may lose what they added.")))
                        .child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(crate::alt::button("alt-confirm-cancel", tr("Cancel")).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.alt.mods.confirm = None;
                                    cx.notify();
                                })))
                                .child(
                                    crate::alt::button("alt-confirm-remove", trf("Remove {count}", &[("count", &items.len().to_string())]))
                                        .text_color(c(DANGER))
                                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                            this.alt_run(actions::Action::Remove(items.clone()), false);
                                            cx.notify();
                                        })),
                                ),
                        )
                }
                Confirm::Stake(action, worlds) => {
                    let action = action.clone();
                    let n = worlds.len();
                    panel
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(trf(
                            "{count} of your worlds were saved with these mods",
                            &[("count", &n.to_string())],
                        )))
                        .children(worlds.iter().map(|(world, mods)| {
                            div().text_size(px(13.)).child(format!("{world}: {}", mods.join(", ")))
                        }))
                        .child(div().text_size(px(13.)).text_color(c(GOLD)).child(trf(
                            "{what} can permanently remove blocks, items and other content from these worlds. Nothing has been changed yet.",
                            &[("what", action.consequence())],
                        )))
                        .child(muted(tr("Sunrise backs up each of these worlds first (a zip in backups/) and reads the backup back. If one can't be made or checked, nothing is changed.")))
                        .child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(crate::alt::button("alt-confirm-cancel", tr("Cancel")).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.alt.mods.confirm = None;
                                    cx.notify();
                                })))
                                .child(primary("alt-confirm-backup", trf("Back up {count} worlds and continue", &[("count", &n.to_string())])).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                    this.alt_run(action.clone(), true);
                                    cx.notify();
                                }))),
                        )
                }
                Confirm::Update(plan) => {
                    let included = plan.included.clone();
                    let natives = included.iter().filter(|i| i.native).count();
                    panel
                        .child(div().font_weight(FontWeight::SEMIBOLD).child(trf("Update {count} mods?", &[("count", &included.len().to_string())])))
                        .children(included.iter().take(14).map(|i| {
                            div().text_size(px(13.)).child(format!(
                                "{} {} → {}{}",
                                i.title,
                                i.from,
                                i.to,
                                if i.native { format!("  ({})", tr("native code")) } else { String::new() }
                            ))
                        }))
                        .children((included.len() > 14).then(|| muted(trf("and {count} more", &[("count", &(included.len() - 14).to_string())]))))
                        .children((!plan.skipped.is_empty()).then(|| {
                            div().flex().flex_col().gap(px(2.)).child(muted(tr("Left out:"))).children(plan.skipped.iter().map(|(t, why)| {
                                div().text_size(px(12.)).text_color(c(GOLD)).child(format!("{t}: {why}"))
                            }))
                        }))
                        .child(muted(tr("Each file is checked against the registry's size, hash and signature before it replaces anything. Mods that are off stay off.")))
                        .children((natives > 0).then(|| {
                            div().text_size(px(12.)).text_color(c(GOLD)).child(trf(
                                "{count} of these run native code. A new build is made only when you Play or choose Build…, after the usual checks.",
                                &[("count", &natives.to_string())],
                            ))
                        }))
                        .child(
                            div()
                                .flex()
                                .gap(px(10.))
                                .child(crate::alt::button("alt-confirm-cancel", tr("Cancel")).on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.alt.mods.confirm = None;
                                    cx.notify();
                                })))
                                .child({
                                    let b = primary("alt-confirm-update", trf("Update {count} mods", &[("count", &included.len().to_string())]));
                                    if included.is_empty() || busy {
                                        dim(b)
                                    } else {
                                        b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                            this.alt_run(actions::Action::Update(included.clone()), false);
                                            cx.notify();
                                        }))
                                    }
                                }),
                        )
                }
            }
        });

        // ---- rows, in three groups
        let mut list = div().flex().flex_col();
        let mut last: Option<Group> = None;
        let mut position = 0;
        let group_counts = |g: Group| visible.iter().filter(|&&i| rows[i].group() == g).count();
        let updates_total = rows.iter().filter(|r| r.update.is_some()).count();
        for (n, &i) in visible.iter().enumerate() {
            let r = rows[i].clone();
            let g = r.group();
            if last != Some(g) {
                last = Some(g);
                let (title, count) = match g {
                    Group::NeedsYou => (tr("NEEDS YOU"), group_counts(g)),
                    Group::Updates => (tr("UPDATES"), group_counts(g)),
                    Group::Rest => (tr("EVERYTHING ELSE"), group_counts(g)),
                };
                let mut head = div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(14.))
                    .h(px(38.))
                    .bg(c(BG))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(c(MUTED))
                            .child(format!("{title} · {count}")),
                    );
                if g == Group::Updates {
                    let b = crate::alt::button(
                        "alt-update-all",
                        trf(
                            "Update {count} mods",
                            &[("count", &updates_total.to_string())],
                        ),
                    );
                    head = head.child(if real && !busy {
                        b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.alt_ask_update(false);
                            cx.notify();
                        }))
                    } else {
                        dim(b)
                    });
                }
                list = list.child(head);
            }
            let pos = position;
            if r.local.is_none() {
                position += 1;
            }
            list = list.child(self.alt_row(n, pos, r, shown_ids.clone(), real, busy, cx));
        }
        if visible.is_empty() {
            list = list.child(
                div()
                    .p(px(16.))
                    .child(muted(tr("No mods match this search or filter."))),
            );
        }
        let hidden = self
            .alt
            .mods
            .snapshot
            .as_ref()
            .map_or(0, |s| s.entries.len().saturating_sub(rows.len()));
        let right = card()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(toolbar)
            .children(confirm)
            .child(div().id("alt-installed-scroll").flex_1().min_h_0().overflow_y_scroll().child(list))
            .children((hidden > 0).then(|| {
                div().px(px(14.)).py(px(8.)).border_t_1().border_color(c(SOFT)).child(muted(trf(
                    "{count} other files the launcher installed (packs, or from other sources) aren't listed here.",
                    &[("count", &hidden.to_string())],
                )))
            }));
        page = page.child(right);
        page.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn alt_row(
        &mut self,
        n: usize,
        pos: usize,
        r: Row,
        ids: Rc<Vec<String>>,
        real: bool,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let selected = self.alt.mods.selection.contains(&r.id);
        let picture = r
            .icon
            .clone()
            .filter(|_| !r.example)
            .and_then(|u| self.mods_image(&u, images::ROW));
        let error = self.alt.mods.errors.get(&r.id).cloned();
        let project = r.entry.as_ref().map(|e| e.project.clone());

        let check = {
            let id = r.id.clone();
            checkbox(("alt-row-check", n), selected).on_click(cx.listener(
                move |this, ev: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    let m = &mut this.alt.mods;
                    match (ev.modifiers().shift, m.anchor) {
                        (true, Some(a)) => {
                            let (lo, hi) = (a.min(pos), a.max(pos));
                            m.selection
                                .extend(ids[lo..=hi.min(ids.len() - 1)].iter().cloned());
                        }
                        _ => {
                            if !m.selection.remove(&id) {
                                m.selection.insert(id.clone());
                            }
                        }
                    }
                    m.anchor = Some(pos);
                    cx.notify();
                },
            ))
        };

        // What the row says on the right: one verdict, with its one action.
        // Roomy, the row is one line of columns; narrow, what it says moves under the name and
        // wraps there, so nothing is pushed out of the row.
        let wide = self.alt_content_width() >= 860.;
        let say = move |text: String, color: u32| {
            div()
                .min_w_0()
                .text_size(px(13.))
                .text_color(c(color))
                .when(wide, |d| d.truncate())
                .child(text)
        };
        let mut notes: Vec<Div> = Vec::new();
        let act = |id: &'static str, label: &'static str, on: bool| {
            let b = crate::alt::button((id, n), tr(label));
            if on && real && !busy {
                b
            } else {
                dim(b)
            }
        };
        let mut verdict = div().flex().flex_none().items_center().gap(px(12.));
        if let Some(local) = &r.local {
            notes.push(say(
                match local.github {
                    true => tr("From GitHub, not the registry: read-only here").to_string(),
                    false => tr("A local file, not from the registry: read-only here").to_string(),
                },
                0x9a948a,
            ));
        }
        if let Some(why) = error {
            notes.push(say(why, GOLD));
        }
        match &r.need {
            Some(Need::TakenDown(why)) => {
                notes.push(say(trf("Taken down: {reason}", &[("reason", why)]), GOLD));
            }
            Some(Need::Build) => {
                let building = self.native_step.is_some();
                let b = act(
                    "alt-row-build",
                    if building { "Building…" } else { "Build…" },
                    true,
                );
                notes.push(say(tr("Not built for this computer yet").to_string(), GOLD));
                verdict = verdict.child(if real && !busy && !building {
                    b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.alt_build();
                        cx.notify();
                    }))
                } else {
                    b
                });
            }
            Some(Need::Permission) => {
                let b = act("alt-row-allow", "Allow…", true);
                notes.push(say(
                    tr("Needs your permission to run native code").to_string(),
                    GOLD,
                ));
                verdict = verdict.child(if real && !busy {
                    b.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.alt_allow();
                        cx.notify();
                    }))
                } else {
                    b
                });
            }
            None => {}
        }
        if let Some(to) = &r.update {
            let b = act("alt-row-update", "Update", true);
            let row = r.clone();
            notes.push(say(format!("→ {to}"), BLUE));
            verdict = verdict.child(if real && !busy {
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.alt_update_one(&row);
                    cx.notify();
                }))
            } else {
                b
            });
        }
        let sw = {
            let s = ui::switch(("alt-row-switch", n), r.on);
            if real && r.local.is_none() && !matches!(r.need, Some(Need::TakenDown(_))) {
                let row = r.clone();
                s.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.alt_toggle(&row);
                    cx.notify();
                }))
            } else {
                s.opacity(0.45).cursor_default()
            }
        };
        let title = {
            let d = div()
                .id(("alt-row-title", n))
                .min_w_0()
                .flex()
                .items_baseline()
                .gap(px(8.))
                .child(
                    div()
                        .min_w_0()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(r.title.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(12.))
                        .text_color(c(MUTED))
                        .truncate()
                        .child(r.authors.clone()),
                );
            match project.filter(|_| real) {
                Some(id) => {
                    d.cursor_pointer()
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.alt_mods_show_project(id.clone());
                            cx.notify();
                        }))
                }
                None => d,
            }
        };
        let kind = r.make.map_or("", Make::label);
        let row = div()
            .id(("alt-row", n))
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .min_h(px(52.))
            .border_t_1()
            .border_color(c(SOFT))
            .when(selected, |d| d.bg(c(0x2a2724)))
            .child(match r.local {
                Some(_) => div().flex_none().w(px(18.)),
                None => div().flex_none().child(check),
            })
            .child(tile(picture, &r.title, 32.));
        let row = if wide {
            row.child(title.flex_1().min_w(px(140.)))
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .text_size(px(13.))
                        .text_color(c(MUTED))
                        .child(kind),
                )
                .child(
                    div()
                        .w(px(64.))
                        .flex_none()
                        .text_size(px(13.))
                        .font_family(crate::theme::MONO)
                        .child(r.version.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(12.))
                        .flex_shrink(1.)
                        .min_w_0()
                        .children(notes),
                )
        } else {
            row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .py(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(title)
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_baseline()
                            .gap_x(px(10.))
                            .child(muted(match kind {
                                "" => r.version.clone(),
                                kind => format!("{kind} · {}", r.version),
                            }))
                            .children(notes),
                    ),
            )
        };
        match r.unfit.clone() {
            Some(unfit) => row
                .child(verdict)
                .child(self.alt_unfit(n, &r, unfit, real, cx)),
            None => row.child(verdict).child(sw),
        }
    }

    /// Where a row's switch would be, for a mod the game was made without: "Not on Nov 17" and
    /// a "?" that says why in one sentence, the changed items for its author, and the way on.
    fn alt_unfit(
        &mut self,
        n: usize,
        r: &Row,
        unfit: model::Unfit,
        real: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        use crate::alt::{button, CARD, LINE, TEXT};
        let open = self.alt.mods.unfit_open.as_deref() == Some(r.id.as_str());
        let id = r.id.clone();
        let help = div()
            .id(("alt-unfit-help", n))
            .tab_index(0)
            .flex_none()
            .size(px(16.))
            .rounded_full()
            .border_1()
            .border_color(c(if open { TEXT } else { LINE }))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(10.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(c(if open { TEXT } else { MUTED }))
            .cursor_pointer()
            .focus_visible(|s| s.border_color(c(GOLD)))
            .child("?")
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                let m = &mut this.alt.mods;
                m.unfit_open = match m.unfit_open.as_ref() == Some(&id) {
                    true => None,
                    false => Some(id.clone()),
                };
                m.unfit_details = false;
                cx.stop_propagation();
                cx.notify();
            }));
        let said = open.then(|| {
            let sentence = match (&unfit.made_for, unfit.changes.is_empty()) {
                (Some((_, made)), false) => trf(
                    "Made for {made}. {on} changed things it uses, so it waits for an update from its author. It stays installed.",
                    &[("made", made), ("on", &unfit.on)],
                ),
                (Some((_, made)), true) => trf(
                    "Made for {made}, so it waits for an update from its author. It stays installed.",
                    &[("made", made)],
                ),
                (None, false) => trf(
                    "Made for another build. {on} changed things it uses, so it waits for an update from its author. It stays installed.",
                    &[("on", &unfit.on)],
                ),
                (None, true) => tr(
                    "Made for another build, so it waits for an update from its author. It stays installed.",
                )
                .to_string(),
            };
            let details = self.alt.mods.unfit_details;
            let technical = (!unfit.changes.is_empty()).then(|| {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("alt-unfit-details")
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .text_color(c(MUTED))
                            .cursor_pointer()
                            .child(blocks::icon(
                                if details { "chevron-down" } else { "chevron-right" },
                                12.,
                                c(MUTED),
                            ))
                            .child(tr("Technical details"))
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.alt.mods.unfit_details = !this.alt.mods.unfit_details;
                                cx.stop_propagation();
                                cx.notify();
                            })),
                    )
                    .children(details.then(|| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .children(unfit.changes.iter().map(|change| {
                                div()
                                    .text_size(px(12.))
                                    .text_color(c(MUTED))
                                    .child(format!("• {change}"))
                            }))
                    }))
            });
            // One way on: a version that fits when the registry has a newer one, else back to
            // the build it was made for when that build is still here.
            let action = match (&r.update, &unfit.made_for) {
                (Some(_), _) => {
                    let b = button(
                        "alt-unfit-update",
                        trf("Update {mod}", &[("mod", &r.title)]),
                    );
                    let row = r.clone();
                    Some(if real {
                        b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.alt.mods.unfit_open = None;
                            this.alt_update_one(&row);
                            cx.stop_propagation();
                            cx.notify();
                        }))
                    } else {
                        dim(b)
                    })
                }
                (None, Some((tag, made))) if !real || self.installed.contains(tag) => {
                    let b = button(
                        "alt-unfit-stay",
                        trf("Stay on {build}", &[("build", made)]),
                    );
                    let tag = tag.clone();
                    Some(if real {
                        b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.alt.mods.unfit_open = None;
                            this.alt_stay_on(tag.clone());
                            cx.stop_propagation();
                            cx.notify();
                        }))
                    } else {
                        dim(b)
                    })
                }
                _ => None,
            };
            // Opens under the "?", or over it when the row is low in the window.
            let popup = div()
                    .w(px(360.))
                    .max_w(px(self.alt_content_width() - 28.))
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(c(LINE))
                    .bg(c(CARD))
                    .shadow_lg()
                    .occlude()
                    .child(div().text_size(px(13.)).child(sentence))
                    .children(technical)
                    .children(action.map(|a| div().flex().justify_end().child(a)));
            gpui::deferred(
                div().absolute().top(px(24.)).right_0().child(
                    gpui::anchored()
                        .anchor(gpui::Anchor::TopRight)
                        .snap_to_window_with_margin(px(8.))
                        .child(popup),
                ),
            )
            .with_priority(10)
        });
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .child(muted(trf("Not on {build}", &[("build", &unfit.on)])))
            .child(div().relative().flex_none().child(help).children(said))
    }

    /// "Stay on Nov 16" for a mod made for it: the setup stays on that build, where the mod
    /// runs. The next Play builds there as usual.
    fn alt_stay_on(&mut self, tag: String) {
        let Some(id) = self.selected_instance().map(|i| i.id.clone()) else {
            return;
        };
        if !self.alt_guard() {
            return;
        }
        match instances::edit(&id, |meta| meta.version = Some(tag)) {
            Ok(_) => self.reload_instances(),
            Err(e) => self.alt.mods.notice = Some(Err(e)),
        }
    }
}
