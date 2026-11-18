//! The redesign's three sheets (`alt/06`, `07`, `08`): Resolve (a held update), Build consent and
//! the game-build chooser. Everything here reads or acts through the existing workers:
//!
//! * **Resolve** reads [`native_mods::update_check`] (or the build's worlds stop, `native_worlds`)
//!   and acts with [`Launcher::build_native_on`]; Stay writes nothing.
//! * **Consent** is the real `pending_consent` and `native_consent::resolve`; the registry-crate
//!   permission is the worker's own mid-build question (`Msg::NativeRegistryConsent`).
//! * **Build** edits the selected instance's pin and channel with `instances::edit`.
//!
//! `SUNRISE_PREVIEW_EXAMPLES=resolve|consent` shows a labelled made-up sheet with no side effects.
use super::*;
use crate::alt::{button, BG, CARD, GOLD, LINE, MUTED, TEXT};
use gpui::{rgb, MouseButton, MouseDownEvent};
use native_consent::PendingConsent;
use std::cell::RefCell;
use std::sync::mpsc::Sender;
use sunrise_launcher::native_mods::{Consent, UpdateCheck};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sheet {
    /// The game-build chooser, a popover over the bar's build chip.
    Build,
    /// Choose what to do with a held update.
    Resolve,
    /// Permission for native code: shown whenever `pending_consent` is set.
    Consent,
    /// The worker's question before dependency build programs run: shown while one is pending.
    Libraries,
}

/// The update check for one instance and newer game, while it runs (`None`) and once it ended.
struct Memo {
    key: String,
    result: Option<Result<UpdateCheck, String>>,
}

#[derive(Default)]
pub(crate) struct AltSheetsState {
    update: RefCell<Option<Memo>>,
    registry: Option<(Consent, Sender<bool>)>,
    /// The unticked boxes of the sheet on screen: native code, dependency programs.
    ticked: (bool, bool),
    error: Option<String>,
    /// The Build chooser lists builds to keep.
    picking: bool,
    example_closed: bool,
}

/// A held update as the bottom bar and the Resolve sheet read it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Held {
    /// The build waiting, as people read it ("Nov 17").
    pub(crate) to: String,
    /// The build played now.
    pub(crate) plays: String,
    /// What the check said first: how many mods fit.
    pub(crate) head: String,
    /// One sentence per mod that does not fit, or per world at stake.
    pub(crate) lines: Vec<String>,
    /// Worlds that would lose a mod's content.
    pub(crate) worlds: Vec<String>,
    /// A mod the tool refuses on any build: there is nothing to update to.
    pub(crate) blocked: bool,
    /// The raw build the update goes to (`build_native_on`).
    base: String,
    /// It is a build that already stopped for worlds, not a check.
    stopped: bool,
}

/// `Nov 17` for a dated build, else the tag.
fn build_name(tag: &str) -> String {
    sunrise_launcher::nightly::date_of(tag)
        .map(sunrise_launcher::nightly::day)
        .unwrap_or_else(|| tag.to_owned())
}

impl Held {
    fn of_check(check: UpdateCheck, plays: &str) -> Option<Self> {
        // `update_summary` is one line when every mod fits and a head plus one line per mod that
        // does not otherwise; a refusal or a world at stake is a hold as well. A fit is an offer
        // (the old Build card's), not a hold.
        let held = check.blocked || !check.worlds.is_empty() || check.summary.contains('\n');
        let mut parts = check.summary.lines().map(str::to_owned);
        let head = parts.next().unwrap_or_default();
        held.then(|| Self {
            to: build_name(&check.to),
            plays: build_name(plays),
            head,
            lines: parts.collect(),
            worlds: check.worlds,
            blocked: check.blocked,
            base: check.to,
            stopped: false,
        })
    }
}

/// The made-up case of `alt/06` and `alt/07`.
#[derive(Clone, Copy, PartialEq)]
enum Example {
    Resolve,
    Consent,
}

fn example() -> Option<Example> {
    match std::env::var("SUNRISE_PREVIEW_EXAMPLES").as_deref() {
        Ok("consent") => Some(Example::Consent),
        Ok("") | Err(_) => None,
        Ok(_) => Some(Example::Resolve),
    }
}

fn card() -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(14.))
        .border_1()
        .border_color(rgb(LINE))
        .bg(rgb(BG))
}

fn muted(text: impl IntoElement) -> Div {
    div().text_size(px(13.)).text_color(rgb(MUTED)).child(text)
}

fn checkbox(id: &'static str, on: bool, label: String) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .cursor_pointer()
        .child(
            div()
                .size(px(16.))
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(MUTED))
                .when(on, |d| d.bg(rgb(GOLD))),
        )
        .child(div().text_size(px(14.)).text_color(rgb(TEXT)).child(label))
}

/// The scrim and the sheet's frame. A press outside closes the sheet; inside, nothing does.
fn frame(id: &'static str, width: f32, body: Div, cx: &mut Context<Launcher>) -> Stateful<Div> {
    div()
        .id(id)
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::black().opacity(0.55))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.close_alt_sheet();
                cx.notify();
            }),
        )
        .child(
            body.w(px(width))
                .max_w_full()
                .p(px(28.))
                .gap(px(16.))
                .rounded(px(16.))
                .border_1()
                .border_color(rgb(LINE))
                .bg(rgb(CARD))
                .text_color(rgb(TEXT))
                .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| {
                    cx.stop_propagation()
                }),
        )
}

fn title(text: String) -> Div {
    div()
        .text_size(px(24.))
        .font_weight(FontWeight::BOLD)
        .child(text)
}

fn banner() -> Div {
    div()
        .px(px(10.))
        .py(px(6.))
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(GOLD))
        .text_size(px(12.))
        .text_color(rgb(GOLD))
        .child(tr(
            "EXAMPLE with made-up data. Nothing here is yours and no button changes anything.",
        ))
}

fn row(label: &str, value: String) -> Div {
    div()
        .flex()
        .gap(px(12.))
        .text_size(px(13.))
        .child(
            div()
                .w(px(56.))
                .flex_none()
                .text_color(rgb(MUTED))
                .child(label.to_owned()),
        )
        .child(div().flex_1().text_color(rgb(TEXT)).child(value))
}

/// One outcome of the Resolve sheet: the same four rows each, and its own button.
fn column(
    title: String,
    badge: Option<&str>,
    safe: bool,
    rows: [String; 4],
    action: Option<Stateful<Div>>,
    note: Option<String>,
) -> Div {
    let [game, mods, worlds, after] = rows;
    div()
        .relative()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(16.))
        .rounded(px(14.))
        .border_1()
        .border_color(rgb(if safe { TEXT } else { LINE }))
        .bg(rgb(BG))
        .children(badge.map(|b| {
            div()
                .absolute()
                .top(px(-9.))
                .left(px(16.))
                .px(px(8.))
                .rounded(px(4.))
                .bg(rgb(TEXT))
                .text_color(rgb(BG))
                .text_size(px(10.))
                .font_weight(FontWeight::BOLD)
                .child(b.to_uppercase())
        }))
        .child(
            div()
                .text_size(px(17.))
                .font_weight(FontWeight::BOLD)
                .child(title),
        )
        .child(row(tr("Game"), game))
        .child(row(tr("Mods"), mods))
        .child(row(tr("Worlds"), worlds))
        .child(row(tr("After"), after))
        .child(div().flex_1())
        .children(note.map(|n| muted(n).text_size(px(12.))))
        .children(action)
}

impl Launcher {
    /// Opens `sheet`. Consent and Libraries open by themselves from their pending state.
    pub(crate) fn open_alt_sheet(&mut self, sheet: Sheet) {
        self.alt.sheets.error = None;
        self.alt.sheets.picking = false;
        self.alt.sheet = Some(sheet);
    }

    /// The worker asks before dependency build programs run. The sheet answers `reply`; losing the
    /// sheet answers no.
    pub(crate) fn alt_registry_consent(&mut self, inventory: Consent, reply: Sender<bool>) {
        if let Some((_, old)) = self.alt.sheets.registry.replace((inventory, reply)) {
            let _ = old.send(false);
        }
        self.alt.sheets.ticked.1 = false;
    }

    /// Closes the sheet on screen the way Escape or a press outside does. Nothing is allowed,
    /// built or changed by it: a pending permission is declined, a pending library question is
    /// answered no, a held update stays held.
    pub(crate) fn close_alt_sheet(&mut self) {
        if let Some((_, reply)) = self.alt.sheets.registry.take() {
            let _ = reply.send(false);
        } else if self.pending_consent.is_some() {
            self.cancel_native_consent();
        }
        self.alt.sheet = None;
        self.alt.sheets.ticked = (false, false);
        self.alt.sheets.error = None;
        self.alt.sheets.picking = false;
        self.alt.sheets.example_closed = true;
    }

    /// Escape: true when a sheet was open and is closed now.
    pub(crate) fn alt_sheet_escape(&mut self) -> bool {
        let open = self.alt.sheets.registry.is_some()
            || self.pending_consent.is_some()
            || self.alt.sheet.is_some()
            || (example().is_some() && !self.alt.sheets.example_closed);
        if open {
            self.close_alt_sheet();
        }
        open
    }

    /// The update the bar should call held, if there is one: the build waiting and the mods that
    /// don't fit it. It reads what [`native_mods::update_check`] says, on a worker (so `None`
    /// while that runs and when nothing is held), or a build that stopped for worlds
    /// (`native_worlds`). Call it from the bar's render; it starts the check at most once per
    /// state of the game folder. It needs native-mod permission, as the check does.
    pub(crate) fn alt_held_update(&self, cx: &mut Context<Self>) -> Option<Held> {
        let instance = self.selected_instance()?;
        let plays = self.version_of(instance)?.clone();
        if let Some(stake) = self
            .native_worlds
            .as_ref()
            .filter(|_| self.native_for.as_deref() == Some(instance.id.as_str()))
        {
            return Some(Held {
                to: build_name(&stake.base),
                plays: build_name(&plays),
                head: String::new(),
                lines: stake.text.split("\n\n").map(str::to_owned).collect(),
                worlds: stake.worlds.clone(),
                blocked: false,
                base: stake.base.clone(),
                stopped: true,
            });
        }
        if self.native_step.is_some() || self.settings.native_mods_allowed_at.is_none() {
            return None;
        }
        let to = native_mods::update_for(&plays, &self.installed)?;
        let home = self.mods.home();
        let key = format!(
            "{}{to}",
            native_status::fingerprint(&home, Some(to.as_str()))
        );
        let mut memo = self.alt.sheets.update.borrow_mut();
        if let Some(previous) = memo.as_ref().filter(|previous| previous.key == key) {
            let check = previous.result.as_ref()?.as_ref().ok()?.clone();
            return Held::of_check(check, &plays);
        }
        *memo = Some(Memo {
            key: key.clone(),
            result: None,
        });
        drop(memo);
        let work = cx
            .background_executor()
            .spawn(async move { native_mods::update_check(&home, &to).map_err(String::from) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                *this.alt.sheets.update.borrow_mut() = Some(Memo {
                    key,
                    result: Some(result),
                });
                cx.notify();
            });
        })
        .detach();
        None
    }

    /// The sheet on screen, if any. Real state wins over an example: a pending library question,
    /// a pending permission, the sheet asked for, then `SUNRISE_PREVIEW_EXAMPLES`.
    pub(crate) fn alt_sheet(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.alt.sheets.registry.is_some() {
            return Some(self.libraries_sheet(cx).into_any_element());
        }
        if self.pending_consent.is_some() {
            return Some(self.consent_sheet(cx).into_any_element());
        }
        match self.alt.sheet {
            Some(Sheet::Build) => Some(self.build_sheet(cx).into_any_element()),
            Some(Sheet::Resolve) => match self.alt_held_update(cx) {
                Some(held) => Some(self.resolve_sheet(held, false, cx).into_any_element()),
                None => {
                    // Nothing is held any more (it was resolved elsewhere): close quietly.
                    self.alt.sheet = None;
                    None
                }
            },
            Some(Sheet::Consent | Sheet::Libraries) => {
                self.alt.sheet = None;
                None
            }
            None => match example().filter(|_| !self.alt.sheets.example_closed)? {
                Example::Resolve => Some(
                    self.resolve_sheet(example_held(), true, cx)
                        .into_any_element(),
                ),
                Example::Consent => Some(self.example_consent_sheet(cx).into_any_element()),
            },
        }
    }

    // ---- Resolve ----

    fn resolve_sheet(&mut self, held: Held, sample: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let Held {
            to,
            plays,
            head,
            lines,
            worlds,
            blocked,
            ..
        } = held.clone();
        let heading = if blocked {
            trf("A mod blocks the update to {to}", &[("to", &to)])
        } else {
            trf("Some mods don't fit {to}", &[("to", &to)])
        };
        let what = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(14.))
            .rounded(px(12.))
            .border_1()
            .border_color(rgb(LINE))
            .bg(rgb(BG))
            .children((!head.is_empty()).then(|| div().text_size(px(13.)).child(head)))
            .children(lines.into_iter().map(|l| muted(l)));
        let stay = button("alt-resolve-stay", native_mods::fit::stay(&plays)).on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| {
                // Staying is what already happens: nothing is written.
                this.close_alt_sheet();
                cx.notify();
            }),
        );
        let stay_col = column(
            trf("Stay on {build}", &[("build", &plays)]),
            Some(tr("Safe · what you have now")),
            true,
            [
                plays.clone(),
                tr("As they are").into(),
                tr("Unchanged").into(),
                tr("Game updates wait until you update the mods or choose again").into(),
            ],
            Some(stay),
            None,
        );
        let first = column(
            tr("Update the mods first").into(),
            None,
            false,
            [
                plays.clone(),
                tr("Update any with a newer version in Mods").into(),
                tr("Unchanged").into(),
                tr("This is checked again and the update is offered once every mod fits").into(),
            ],
            None,
            Some(tr("Done in Mods; nothing to press here.").into()),
        );
        let third = if blocked {
            column(
                tr("Update").into(),
                None,
                false,
                [
                    plays.clone(),
                    tr("Refused on this build").into(),
                    tr("Unchanged").into(),
                    tr("Fix or remove that mod first").into(),
                ],
                None,
                Some(tr("There is nothing to update to until then.").into()),
            )
        } else if worlds.is_empty() {
            let go = button("alt-resolve-update", tr("Update game"));
            let go = if sample {
                go.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.close_alt_sheet();
                    cx.notify();
                }))
            } else {
                let base = held.base.clone();
                go.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.close_alt_sheet();
                    this.build_native_on(Some(base.clone()), false);
                    cx.notify();
                }))
            };
            column(
                tr("Update without them").into(),
                None,
                false,
                [
                    to.clone(),
                    tr("The ones that don't fit are turned off, not removed").into(),
                    tr("Unchanged: none use those mods").into(),
                    tr("They can come back when their authors publish a version that fits").into(),
                ],
                Some(go),
                None,
            )
        } else {
            let go = button(
                "alt-resolve-backup",
                if worlds.len() == 1 {
                    native_mods::fit::back_up_and_update(1)
                } else {
                    native_mods::fit::back_up_and_update(2)
                },
            )
            .text_color(gpui::red());
            let go = if sample {
                go.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.close_alt_sheet();
                    cx.notify();
                }))
            } else {
                let base = held.base.clone();
                go.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.close_alt_sheet();
                    // The worker backs each world up and checks it before anything is turned off;
                    // a backup that fails stops the build with nothing changed.
                    this.build_native_on(Some(base.clone()), true);
                    cx.notify();
                }))
            };
            column(
                tr("Back up and go on").into(),
                None,
                false,
                [
                    to.clone(),
                    tr("The ones that don't fit are turned off, not removed").into(),
                    trf(
                        "{worlds} is backed up first. Opened without the mod, it loses what the mod stored, for good.",
                        &[("worlds", &worlds.join(", "))],
                    ),
                    tr("If a backup can't be made and checked, nothing is changed").into(),
                ],
                Some(go),
                None,
            )
        };
        let body = div()
            .flex()
            .flex_col()
            .children(sample.then(banner))
            .child(title(heading))
            .child(muted(trf(
                "You are still playing {plays}. The update to {to} is on hold until you pick one of these.",
                &[("plays", &plays), ("to", &to)],
            )))
            .child(what)
            .child(div().flex().gap(px(12.)).child(first).child(stay_col).child(third))
            .child(muted(
                tr("Closing this sheet is the same as staying. Nothing changes until you choose."),
            ));
        frame("alt-resolve", 980., body, cx)
    }

    // ---- Consent ----

    fn consent_sheet(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let pending = self.pending_consent.as_ref();
        let name = pending
            .map(PendingConsent::name)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| tr("your mods").into());
        let building = matches!(
            pending,
            Some(PendingConsent::Build(_) | PendingConsent::Local(_))
        );
        let host = sunrise_launcher::rust_toolchain::host();
        let downloads = match (host, sunrise_launcher::rust_toolchain::installed()) {
            (None, _) => tr("Unavailable on this platform").into(),
            (Some(_), Some(_)) => tr("Nothing: the build tools are already here").into(),
            (Some(host), None) => trf(
                "{size} of build tools from Sunrise",
                &[("size", &storage::format_bytes(host.download_bytes()))],
            ),
        };
        let ticked = self.alt.sheets.ticked.0;
        let can = host.is_some() && ticked;
        let allow = button(
            "alt-consent-allow",
            if building {
                tr("Allow and build")
            } else {
                tr("Allow")
            },
        );
        let allow = if can {
            allow.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.allow_native_consent();
                cx.notify();
            }))
        } else {
            disabled(allow)
        };
        let body = div()
            .flex()
            .flex_col()
            .child(title(if building {
                trf("Build {name} on this computer?", &[("name", &name)])
            } else {
                trf("Allow {name} to run as native code?", &[("name", &name)])
            }))
            .child(muted(tr(
                "Native mods are programs that run inside your game. Like any program you install, they can read and change your files. Allow them only if you trust who made them.",
            )))
            .child(
                card()
                    .child(fact(tr("Mods"), name.clone(), false))
                    .child(fact(tr("Downloads"), downloads, true))
                    .child(fact(
                        tr("While building"),
                        tr("If a code library the mod uses wants to run its own programs, Sunrise stops and shows you the list first. Nothing runs before you say so.").into(),
                        true,
                    ))
                    .child(fact(
                        tr("When you play"),
                        tr("The mod, inside the game, with the same access to your files and programs as the game has.").into(),
                        true,
                    )),
            )
            .child(
                checkbox(
                    "alt-consent-native",
                    ticked,
                    trf("Let {name} run as native code in my game", &[("name", &name)]),
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.alt.sheets.ticked.0 ^= true;
                    cx.notify();
                })),
            )
            .children(
                self.consent_error
                    .clone()
                    .map(|e| div().text_color(gpui::red()).text_size(px(13.)).child(e)),
            )
            .child(consent_footer(
                tr("This stays allowed on this computer until you take it back in Settings. Progress shows in the bar below."),
                button("alt-consent-cancel", tr("Not now")).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.close_alt_sheet();
                        cx.notify();
                    },
                )),
                allow,
            ));
        frame("alt-consent", 760., body, cx)
    }

    /// Allow: persist the permission, then run the one pending action. The old overlay's match.
    fn allow_native_consent(&mut self) {
        let result = native_consent::resolve(
            &mut self.pending_consent,
            true,
            true,
            &mut self.settings,
            &self.mods.home(),
        );
        match result {
            Err(error) => self.consent_error = Some(error),
            Ok(None) => {}
            Ok(Some(action)) => {
                self.consent_error = None;
                self.storage_revoke_error = None;
                self.alt.sheets.ticked = (false, false);
                match action {
                    PendingConsent::Build(_) => {
                        let (base, back_up) = self.native_consent_build.take().unwrap_or_default();
                        self.build_native_on(base, back_up)
                    }
                    PendingConsent::Toggle(entry) => self.mods_switch(&entry),
                    PendingConsent::Install(source, id, known) => {
                        self.mods_install(source, id, known)
                    }
                    PendingConsent::Github(found, only) => self.github_install(found, only),
                    PendingConsent::Local(id) => {
                        self.mods.message = sunrise_launcher::browser::java::set_enabled(
                            &self.mods.home(),
                            &id,
                            true,
                        )
                        .err()
                        .map(Err);
                        self.instance_mods_changed();
                    }
                }
            }
        }
    }

    // ---- Libraries ----

    fn libraries_sheet(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let (inventory, _) = self
            .alt
            .sheets
            .registry
            .as_ref()
            .expect("shown only while a question is pending");
        let mut names: Vec<&String> = inventory
            .build_scripts
            .iter()
            .chain(&inventory.proc_macros)
            .collect();
        names.sort();
        names.dedup();
        let count = names.len();
        let heading = inventory.label();
        let rows = card()
            .child(fact(
                tr("Build scripts"),
                list_or_none(&inventory.build_scripts),
                false,
            ))
            .child(fact(
                tr("Proc macros"),
                list_or_none(&inventory.proc_macros),
                true,
            ));
        let ticked = self.alt.sheets.ticked.1;
        let go = button("alt-libraries-allow", tr("Allow and continue"));
        let go = if ticked {
            go.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                if let Some((_, reply)) = this.alt.sheets.registry.take() {
                    let _ = reply.send(true);
                }
                this.alt.sheets.ticked.1 = false;
                cx.notify();
            }))
        } else {
            disabled(go)
        };
        let body = div()
            .flex()
            .flex_col()
            .child(title(heading))
            .child(muted(tr(
                "Registry dependencies are native code. Their build scripts and proc macros run during compilation, before the game starts, with the same access to your files as the launcher has.",
            )))
            .child(rows)
            .child(
                checkbox(
                    "alt-libraries-tick",
                    ticked,
                    trf(
                        "Let those {count} libraries run their programs while building",
                        &[("count", &count.to_string())],
                    ),
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.alt.sheets.ticked.1 ^= true;
                    cx.notify();
                })),
            )
            .child(consent_footer(
                tr("Not now stops the build. Nothing has run."),
                button("alt-libraries-cancel", tr("Not now")).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.close_alt_sheet();
                        cx.notify();
                    },
                )),
                go,
            ));
        frame("alt-libraries", 760., body, cx)
    }

    /// `alt/07` as drawn, with both permissions on one sheet. The real flow asks the second one
    /// during the build, with the real list.
    fn example_consent_sheet(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let (a, b) = self.alt.sheets.ticked;
        let go = button("alt-example-go", tr("Build and turn on"));
        let go = if a && b {
            go.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.close_alt_sheet();
                cx.notify();
            }))
        } else {
            disabled(go)
        };
        let body = div()
            .flex()
            .flex_col()
            .child(banner())
            .child(title(
                trf("Build {name} on this computer?", &[("name", "Machinist’s Workbench")]),
            ))
            .child(muted(tr(
                "Its authors haven’t published a ready-made copy of 3.1.2 for your game. Sunrise can make one here from the source they published. Your game is not changed.",
            )))
            .child(
                card()
                    .child(fact(tr("Mod"), "Machinist’s Workbench 3.1.2 · Kiln Club (example)".into(), false))
                    .child(fact(tr("Downloads"), "318 MiB of build tools, and 14 code libraries the mod uses (example)".into(), true))
                    .child(fact(tr("While building"), "Programs from 3 of those libraries (example). In the real flow Sunrise names them in a second question during the build.".into(), true))
                    .child(fact(tr("When you play"), tr("The mod, inside the game, with the same access to your files and programs as the game has.").into(), true)),
            )
            .child(
                checkbox("alt-example-a", a, tr("Let those 3 libraries run their programs while building").into())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.alt.sheets.ticked.0 ^= true;
                        cx.notify();
                    })),
            )
            .child(
                checkbox("alt-example-b", b, tr("Let the mod run as native code in my game").into())
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.alt.sheets.ticked.1 ^= true;
                        cx.notify();
                    })),
            )
            .child(consent_footer(
                tr("Both stay allowed on this computer until you take them back in Settings."),
                button("alt-example-cancel", tr("Not now")).on_click(cx.listener(
                    |this, _: &ClickEvent, _, cx| {
                        this.close_alt_sheet();
                        cx.notify();
                    },
                )),
                go,
            ));
        frame("alt-example", 760., body, cx)
    }

    // ---- Game build ----

    fn pin_alt_build(&mut self, version: Option<String>, channel: instances::Channel) {
        let Some(id) = self.selected_instance().map(|i| i.id.clone()) else {
            return;
        };
        match instances::edit(&id, |m| {
            m.version = version;
            m.channel = Some(channel);
        }) {
            Ok(_) => {
                self.alt.sheets.error = None;
                self.reload_instances();
            }
            Err(e) => self.alt.sheets.error = Some(e),
        }
    }

    fn build_sheet(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let Some(instance) = self.selected_instance() else {
            return div().id("alt-build");
        };
        let pinned = instance.version.clone();
        let channel = instance.channel;
        let plays = self.version_of(instance).cloned();
        let stable = self.stable_builds().iter().any(|b| b.week.is_none());
        let mut builds: Vec<String> = self
            .installed
            .iter()
            .filter(|v| !native_mods::is_native(v))
            .filter(|v| sunrise_launcher::backend::launcher_version(v).is_none())
            .cloned()
            .collect();
        builds.sort_by(|a, b| sunrise_launcher::backend::compare_versions(b, a));
        let picking = self.alt.sheets.picking || pinned.is_some();
        let follow_nightly = pinned.is_none() && channel == instances::Channel::Nightly;
        let follow_stable = pinned.is_none() && channel == instances::Channel::Stable;
        let option = |id: &'static str, on: bool, head: String, sub: String| {
            div()
                .id(id)
                .flex()
                .gap(px(12.))
                .p(px(10.))
                .rounded(px(8.))
                .cursor_pointer()
                .when(on, |d| d.bg(rgb(BG)))
                .child(
                    div()
                        .mt(px(3.))
                        .size(px(14.))
                        .flex_none()
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(if on { GOLD } else { MUTED }))
                        .when(on, |d| d.bg(rgb(GOLD))),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(14.)).child(head))
                        .child(muted(sub).text_size(px(12.))),
                )
        };
        let mut list = div().flex().flex_col().gap(px(2.));
        list = list.child(
            option(
                "alt-build-nightly",
                follow_nightly,
                tr("Follow Nightly").into(),
                tr("The newest build each day.").into(),
            )
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.pin_alt_build(None, instances::Channel::Nightly);
                cx.notify();
            })),
        );
        if stable || follow_stable {
            list = list.child(
                option(
                    "alt-build-stable",
                    follow_stable,
                    tr("Follow Stable").into(),
                    tr("Only builds marked stable.").into(),
                )
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.pin_alt_build(None, instances::Channel::Stable);
                    cx.notify();
                })),
            );
        }
        let keep_now = plays.clone();
        list = list.child(
            option(
                "alt-build-keep",
                picking,
                tr("Stay on one build").into(),
                tr("Pick a build and keep it until you change this.").into(),
            )
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.alt.sheets.picking = true;
                if let Some(now) = keep_now.clone() {
                    this.pin_alt_build(Some(now), channel);
                }
                cx.notify();
            })),
        );
        if picking {
            let mut choices = div().flex().flex_col().pl(px(36.)).pb(px(6.));
            for (i, tag) in builds.into_iter().enumerate() {
                let on = pinned.as_deref() == Some(tag.as_str());
                let label = sunrise_launcher::nightly::label(&tag)
                    .map(|l| format!("{} · {l}", build_name(&tag)))
                    .unwrap_or_else(|| tag.clone());
                let pick = tag.clone();
                choices = choices.child(
                    div()
                        .id(("alt-build-choice", i))
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .text_size(px(13.))
                        .text_color(rgb(if on { GOLD } else { TEXT }))
                        .child(label)
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.pin_alt_build(Some(pick.clone()), channel);
                            cx.notify();
                        })),
                );
            }
            list = list.child(choices);
        }
        let extras = div()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(12.))
            .opacity(0.5)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().text_size(px(14.)).child(tr("Sunrise Extras")))
                    .child(
                        muted(tr(
                            "Sunrise’s additions on top of vanilla. This switch arrives with the build that supports it.",
                        ))
                        .text_size(px(12.)),
                    ),
            )
            .child(
                div()
                    .mt(px(2.))
                    .w(px(34.))
                    .h(px(20.))
                    .rounded_full()
                    .border_1()
                    .border_color(rgb(LINE)),
            );
        let body = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(rgb(MUTED))
                    .child(tr("GAME BUILD")),
            )
            .child(list)
            .child(div().h(px(1.)).bg(rgb(LINE)))
            .child(extras)
            .children(
                self.alt
                    .sheets
                    .error
                    .clone()
                    .map(|e| div().text_color(gpui::red()).text_size(px(12.)).child(e)),
            );
        // A popover over the bar's build chip: the scrim is clear.
        div()
            .id("alt-build")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.close_alt_sheet();
                    cx.notify();
                }),
            )
            .child(
                body.absolute()
                    .left(px(148.))
                    .bottom(px(78.))
                    .w(px(372.))
                    .p(px(14.))
                    .rounded(px(14.))
                    .border_1()
                    .border_color(rgb(LINE))
                    .bg(rgb(CARD))
                    .text_color(rgb(TEXT))
                    .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| {
                        cx.stop_propagation()
                    }),
            )
    }
}

fn list_or_none(names: &[String]) -> String {
    if names.is_empty() {
        tr("None").into()
    } else {
        names.join(", ")
    }
}

fn fact(label: &str, value: String, rule: bool) -> Div {
    div()
        .flex()
        .gap(px(16.))
        .px(px(16.))
        .py(px(10.))
        .text_size(px(13.))
        .when(rule, |d| d.border_t_1().border_color(rgb(LINE)))
        .child(
            div()
                .w(px(110.))
                .flex_none()
                .text_color(rgb(MUTED))
                .child(label.to_owned()),
        )
        .child(div().flex_1().child(value))
}

fn consent_footer(note: &str, cancel: Stateful<Div>, go: Stateful<Div>) -> Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .child(muted(note.to_owned()).flex_1())
        .child(div().flex().gap(px(10.)).child(cancel).child(go))
}

/// `alt/06`'s case, for `SUNRISE_PREVIEW_EXAMPLES`.
fn example_held() -> Held {
    Held {
        to: "Nov 17".into(),
        plays: "Nov 14".into(),
        head: "Move to Nov 17? 1 of your 2 native mods fit it. (example)".into(),
        lines: vec![
            "JEI 1.8.0 does not: it was made for the Nov 14 game. It will be switched off until its author makes a version that fits. (example)".into(),
        ],
        worlds: vec!["Harbor at Low Tide".into()],
        blocked: false,
        base: "example".into(),
        stopped: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(summary: String, worlds: &[&str], blocked: bool) -> UpdateCheck {
        UpdateCheck {
            to: "nightly-2026-11-17-abcdef0".into(),
            summary,
            worlds: worlds.iter().map(|w| w.to_string()).collect(),
            blocked,
        }
    }

    #[test]
    fn a_fit_is_an_offer_and_a_misfit_is_a_hold() {
        let fits = native_mods::fit::update_summary("Nov 17", 2, &[]);
        assert!(Held::of_check(check(fits, &[], false), "nightly-2026-11-14-0000000").is_none());
        let off = [("jei".to_string(), "made for another game".to_string())];
        let summary = native_mods::fit::update_summary("Nov 17", 2, &off);
        let held = Held::of_check(check(summary, &[], false), "nightly-2026-11-14-0000000")
            .expect("a mod that does not fit holds the update");
        assert_eq!(
            (held.to.as_str(), held.plays.as_str()),
            ("Nov 17", "Nov 14")
        );
        assert_eq!(held.lines.len(), 1);
        assert!(held.lines[0].starts_with("jei does not"));
        assert!(!held.head.is_empty());
    }

    #[test]
    fn a_refusal_or_a_world_at_stake_holds_even_on_one_line() {
        let refused = Held::of_check(check("one mod is refused".into(), &[], true), "x").unwrap();
        assert!(refused.blocked);
        let worlds = Held::of_check(check("Move?".into(), &["Harbor"], false), "x").unwrap();
        assert_eq!(worlds.worlds, ["Harbor"]);
    }

    #[test]
    fn dated_builds_read_as_a_day() {
        assert_eq!(build_name("nightly-2026-11-17-abcdef0"), "Nov 17");
        assert_eq!(build_name("v1.0.0"), "v1.0.0");
    }
}
