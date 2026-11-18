//! Machine-wide settings. Per-setup game choices live in the dock instead.
use super::*;
use crate::alt::{button, CARD, GOLD, LINE, MUTED, TEXT};
use gpui::rgb;

#[derive(Default)]
pub(super) struct SettingsView {
    account: bool,
    pub(super) privacy: bool,
    languages: bool,
    setup: bool,
    storage: bool,
    note: Option<String>,
}

fn section(label: &'static str, body: Div) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(20.))
        .py(px(15.))
        .border_b_1()
        .border_color(rgb(0x332d2e))
        .child(
            div()
                .w(px(116.))
                .flex_none()
                .font_weight(FontWeight::BOLD)
                .text_size(px(17.))
                .child(tr(label)),
        )
        .child(body.flex().flex_col().gap(px(12.)).flex_1().min_w_0())
}

fn row(title: impl Into<SharedString>, hint: impl Into<SharedString>, actions: Div) -> Div {
    let hint = hint.into();
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title.into()))
                .when(!hint.is_empty(), |d| {
                    d.child(div().text_color(rgb(MUTED)).text_size(px(12.)).child(hint))
                }),
        )
        .child(actions.flex().items_center().gap(px(6.)))
}

impl Launcher {
    pub(super) fn alt_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let st = &self.alt.settings;
        let (show_account, show_privacy, show_languages, show_setup, show_storage) =
            (st.account, st.privacy, st.languages, st.setup, st.storage);
        let account = self
            .accounts
            .selected()
            .map(|a| a.name.clone())
            .unwrap_or_else(|| tr("Not signed in").into());
        let mut accounts = div().child(row(
            account,
            tr("Microsoft account"),
            div()
                .child(
                    button(
                        "alt-manage-accounts",
                        tr(if show_account {
                            "Done"
                        } else {
                            "Manage accounts"
                        }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.alt.settings.account = !this.alt.settings.account;
                        cx.notify();
                    })),
                )
                .child(
                    button("alt-add-account", tr("Sign in")).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.sign_in_to_play();
                            cx.notify();
                        },
                    )),
                ),
        ));
        if show_account {
            let (list, _) = self.microsoft_rows(cx);
            accounts = accounts.child(list);
        }
        let mut look = div().child(row(
            tr("Language"),
            "",
            div().child(
                button(
                    "alt-language",
                    self.settings.language.map_or(tr("System"), |l| l.name()),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.alt.settings.languages = !this.alt.settings.languages;
                    cx.notify();
                })),
            ),
        ));
        if show_languages {
            let mut options = div()
                .id("alt-languages")
                .max_h(px(180.))
                .overflow_y_scroll()
                .flex()
                .flex_wrap()
                .gap(px(4.));
            for (n, language) in std::iter::once(None)
                .chain(i18n::Language::ALL.map(Some))
                .enumerate()
            {
                options = options.child(
                    button(
                        ElementId::named_usize("alt-lang", n),
                        language.map_or(tr("System"), |l| l.name()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings.language = language;
                        i18n::set(this.settings.language());
                        this.save_settings();
                        this.alt.settings.languages = false;
                        cx.notify();
                    })),
                );
            }
            look = look.child(options);
        }
        look = look.child(row(
            tr("Theme"),
            tr("This preview uses one dark palette."),
            div().text_color(rgb(MUTED)).child(tr("Daybreak")),
        ));
        let mut privacy = div();
        if !show_privacy {
            privacy = privacy.child(row(
                tr("Count me as playing"),
                tr("Once a minute while the game runs: build, system, LAN status and a random number."),
                div().child(self.ping_switch("alt-ping", cx)),
            ));
        }
        privacy = privacy.child(row(
            tr("Reports and permissions"),
            tr("Choose which reports, if any, to send."),
            div().child(
                button(
                    "alt-privacy",
                    tr(if show_privacy {
                        "Hide details"
                    } else {
                        "Privacy choices"
                    }),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.alt.settings.privacy = !this.alt.settings.privacy;
                    cx.notify();
                })),
            ),
        ));
        if show_privacy {
            privacy = privacy.child(self.collection_section(cx));
        }
        let dir = backend::game_home();
        let mut storage = div()
            .child(row(
                tr("Game files and worlds"),
                short(&dir),
                div().child(
                    button("alt-open-game", tr("Open folder"))
                        .on_click(move |_, _, cx| glass_toast::open(cx, &dir)),
                ),
            ))
            .child(row(
                tr("Downloads kept for reuse"),
                trf(
                    "Cache limit: {limit} GiB",
                    &[(
                        "limit",
                        &self.settings.cache_limit_gib.unwrap_or(25).to_string(),
                    )],
                ),
                div().child(
                    button(
                        "alt-storage",
                        tr(if show_storage { "Done" } else { "Manage…" }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.alt.settings.storage = !this.alt.settings.storage;
                        if this.alt.settings.storage {
                            this.refresh_storage(None, false);
                        }
                        cx.notify();
                    })),
                ),
            ));
        if show_storage {
            storage = storage.child(self.glass_storage(cx));
        }
        let mut launcher = div()
            .child(row(
                trf("Version {version}", &[("version", backend::VERSION)]),
                tr("Design preview · self-update is off"),
                div().text_color(rgb(MUTED)).child(tr("Local build")),
            ))
            .child(row(
                tr("A second set of mods"),
                tr("Keep different mods for different worlds or servers."),
                div().child(
                    button(
                        "alt-create",
                        tr(if show_setup { "Cancel" } else { "New setup…" }),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.alt.settings.setup = !this.alt.settings.setup;
                        this.alt.settings.note = None;
                        cx.notify();
                    })),
                ),
            ));
        if show_setup {
            let input = self.field(servers_page::Field::InstanceName, "Setup name", 250., cx);
            launcher = launcher.child(div().flex().items_center().gap(px(8.)).child(input).child(
                button("alt-create-empty", tr("Create empty setup")).on_click(cx.listener(
                    |this, _, _, cx| {
                        let name = this.servers.value(servers_page::Field::InstanceName).trim();
                        match instances::create(name, None) {
                            Ok(_) => {
                                this.reload_instances();
                                this.alt.settings.setup = false;
                                this.servers
                                    .text(servers_page::Field::InstanceName)
                                    .set(String::new());
                                this.alt.settings.note =
                                    Some(tr("Setup created. Select it in the play bar.").into());
                            }
                            Err(e) => this.alt.settings.note = Some(e),
                        };
                        cx.notify();
                    },
                )),
            ));
        }
        div()
            .id("alt-settings-scroll")
            .size_full()
            .overflow_y_scroll()
            .px(px(32.))
            .py(px(24.))
            .text_color(rgb(TEXT))
            .child(
                div()
                    .max_w(px(960.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .mb(px(6.))
                            .text_size(px(26.))
                            .font_weight(FontWeight::BOLD)
                            .child(tr("Settings")),
                    )
                    .children(
                        self.location_error
                            .as_ref()
                            .map(|e| div().text_color(rgb(GOLD)).child(e.clone())),
                    )
                    .child(section("Account", accounts))
                    .child(section("Look", look))
                    .child(section("Privacy", privacy))
                    .child(section("Storage", storage))
                    .child(section("Launcher", launcher))
                    .children(self.alt.settings.note.as_ref().map(|s| {
                        div()
                            .bg(rgb(CARD))
                            .border_1()
                            .border_color(rgb(LINE))
                            .p(px(10.))
                            .child(s.clone())
                    })),
            )
            .into_any_element()
    }
}
