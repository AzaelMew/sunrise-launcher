//! The exact Keep this build action and its one decision. The plan owns the profile and saves
//! until this sheet declines it or passes it across the final, local publication boundary.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use sunrise_launcher::browser::sunrise::txn::{Lease, Receipt, Switch};

#[derive(Default)]
pub(super) struct State {
    plan: Option<Switch>,
    job: Option<Arc<AtomicBool>>,
    report: Option<(std::path::PathBuf, Receipt)>,
    details: bool,
    cache_download: Option<String>,
}

impl Launcher {
    pub(crate) fn switch_cache_download_finished(&mut self, tag: &str) -> bool {
        self.ui
            .sheets
            .switch_build
            .cache_download
            .take_if(|active| *active == tag)
            .is_some()
    }

    pub(super) fn cancel_switch_sheet(&mut self) {
        let state = &mut self.ui.sheets.switch_build;
        if let Some(job) = &state.job {
            job.store(true, Ordering::Release);
        }
        state.plan = None;
        state.report = None;
        state.details = false;
    }

    pub(super) fn open_removal_receipt(&mut self) -> bool {
        let Some(home) = self
            .selected_instance()
            .map(|instance| instance.home.clone())
        else {
            return false;
        };
        let receipt = Lease::admit(&home)
            .and_then(|owner| owner.active_build())
            .ok()
            .flatten()
            .and_then(|active| active.receipts().first().cloned());
        if let Some(receipt) = receipt {
            self.open_sheet(Sheet::SwitchBuild);
            self.ui.sheets.switch_build.report = Some((home, receipt));
            true
        } else {
            false
        }
    }

    pub(super) fn choose_switch_build(&mut self, tag: String, cx: &mut Context<Self>) {
        if self.launch_busy() || self.ui.sheets.switch_build.job.is_some() || self.client.is_some()
        {
            return;
        }
        if !self.installed.contains(&tag) {
            if let Some(release) = self.release_to_install(&tag) {
                self.ui.sheets.switch_build.cache_download = Some(tag);
                self.install(&release);
            }
            return;
        }
        if !backend::check_prepared(&tag) {
            self.prepare_tag(tag, backend::MinecraftFrom::Install);
            return;
        }
        let Some(instance) = self
            .selected_instance()
            .filter(|i| !i.is_pack_profile())
            .cloned()
        else {
            return;
        };
        let lease = match Lease::admit(&instance.home) {
            Ok(lease) => Arc::new(lease),
            Err(e) => {
                self.ui.sheets.error = Some(e);
                return;
            }
        };
        let token = Arc::new(AtomicBool::new(false));
        self.ui.sheets.switch_build.job = Some(token.clone());
        self.native_step = Some(tr("Checking your mods…").into());
        self.ui.sheets.error = None;
        let tx = self.tx.clone();
        let work = cx.background_executor().spawn(async move {
            Switch::stage(lease, &tag, instance.native_profile, |step| {
                let _ = tx.send(Msg::NativeProgress(step));
            })
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.native_step = None;
                this.native_progress = None;
                this.ui.sheets.switch_build.job = None;
                if !token.load(Ordering::Acquire) {
                    match result {
                        Ok(Some(plan)) if plan.question().needed() => {
                            this.open_sheet(Sheet::SwitchBuild);
                            this.ui.sheets.switch_build.plan = Some(plan);
                        }
                        Ok(Some(plan)) => this.commit_switch_plan(plan, cx),
                        Ok(None) => this.close_sheet(),
                        Err(error) => this.ui.sheets.error = Some(error),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn commit_switch_plan(&mut self, plan: Switch, cx: &mut Context<Self>) {
        let token = Arc::new(AtomicBool::new(false));
        self.ui.sheets.switch_build.job = Some(token.clone());
        self.native_step = Some(tr("Switching game builds…").into());
        self.open_sheet(Sheet::SwitchBuild);
        let cancelled = token.clone();
        let home = plan.home().to_owned();
        let work = cx.background_executor().spawn(async move {
            plan.commit(|| {
                if cancelled.load(Ordering::Acquire) {
                    Err(tr("Switch cancelled. Nothing was switched.").into())
                } else {
                    Ok(())
                }
            })
        });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.native_step = None;
                this.native_progress = None;
                this.ui.sheets.switch_build.job = None;
                match result {
                    Ok(report) => {
                        this.installed = backend::installed();
                        this.reload_instances();
                        if let Some(receipt) = report {
                            this.open_sheet(Sheet::SwitchBuild);
                            this.ui.sheets.switch_build.report = Some((home, receipt));
                        } else {
                            this.close_sheet();
                        }
                    }
                    Err(error) if !token.load(Ordering::Acquire) => {
                        this.open_sheet(Sheet::Versions);
                        this.ui.sheets.error = Some(error);
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn switch_build_sheet(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let state = &self.ui.sheets.switch_build;
        let plan = state.plan.as_ref();
        let report: Vec<(&str, &str)> = if let Some(plan) = plan {
            plan.removals()
                .iter()
                .map(|(n, r)| (n.as_str(), r.as_str()))
                .collect()
        } else {
            state
                .report
                .as_ref()
                .into_iter()
                .flat_map(|(_, receipt)| receipt.lines())
                .collect()
        };
        let shown = if plan.is_none() && report.is_empty() {
            div().child(title(tr("Switching game builds…").into()))
        } else {
            let headline = if plan.is_some() {
                tr("Switch game builds?")
            } else {
                tr("Switched game builds")
            };
            let mut panel = div()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(title(headline.into()));
            if let Some(plan) = plan {
                panel = panel
                    .child(muted(trf(
                        "{from} → {to}",
                        &[
                            ("from", &build_name(plan.prior())),
                            ("to", &build_name(plan.target())),
                        ],
                    )))
                    .children(
                        plan.question()
                            .lines()
                            .into_iter()
                            .map(|line| div().child(line)),
                    );
            }
            if !report.is_empty() {
                panel = panel.child(
                    div().child(trf(
                        "Removed mods: {mods}",
                        &[(
                            "mods",
                            &report
                                .iter()
                                .map(|(name, _)| *name)
                                .collect::<Vec<_>>()
                                .join(", "),
                        )],
                    )),
                );
            }
            let mut actions = div().flex().flex_wrap().justify_end().gap(px(8.));
            let close = if plan.is_some() {
                tr("Stay on this version")
            } else {
                tr("Close")
            };
            actions = actions.child(button("stay-on-build", close).on_click(cx.listener(
                |this, _: &ClickEvent, _, cx| {
                    this.close_sheet();
                    cx.notify();
                },
            )));
            if let Some((home, receipt)) = &state.report {
                let home = home.clone();
                let id = receipt.id();
                actions = actions.child(button("ack-switch-removals", tr("Got it")).on_click(
                    cx.listener(move |this, _: &ClickEvent, _, cx| {
                        match Lease::admit(&home).and_then(|owner| owner.acknowledge_removal(id)) {
                            Ok(_) => this.close_sheet(),
                            Err(error) => this.ui.sheets.error = Some(error),
                        }
                        cx.notify();
                    }),
                ));
            }
            if !report.is_empty() {
                actions = actions.child(button("switch-why", tr("Show why")).on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.ui.sheets.switch_build.details ^= true;
                        cx.notify();
                    }),
                ));
                if state.details {
                    let details = report
                        .iter()
                        .map(|(name, reason)| format!("{name}: {reason}"))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    panel = panel.child(div().child(details.clone()));
                    actions =
                        actions.child(button("switch-copy-why", tr("Copy details")).on_click(
                            cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    details.clone(),
                                ));
                            }),
                        ));
                }
            }
            if plan.is_some() {
                actions = actions.child(
                    button("confirm-switch-build", tr("Switch anyway")).on_click(cx.listener(
                        |this, _: &ClickEvent, _, cx| {
                            if let Some(plan) = this.ui.sheets.switch_build.plan.take() {
                                this.commit_switch_plan(plan, cx);
                            }
                            cx.notify();
                        },
                    )),
                );
            }
            panel
                .children(
                    self.ui
                        .sheets
                        .error
                        .clone()
                        .map(|e| div().text_color(rgb(palette().danger)).child(e)),
                )
                .child(actions)
        };
        frame("switch-build", 560., shown, cx)
    }
}
