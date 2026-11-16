//! The launcher's frame (`Render for Launcher`) and its opening.
use super::*;

impl Render for Launcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        widgets::set_hover_view(cx.entity_id());
        if specimen::wanted() {
            return div()
                .flex()
                .flex_col()
                .size_full()
                .bg(t.side)
                .children(titlebar::render(&t, window))
                .child(specimen::render(&t));
        }
        if std::mem::take(&mut self.restyle) {
            self.theme = self.settings_theme(window);
        }
        glass_ui::set_on(self.theme.glass);
        glass::set_panel(self.theme.night.card);
        let corners = f32::from(self.settings.corners());
        glass_ui::set_corners(corners);
        glass::set_corners(corners);
        glass_ui::set_light(!self.theme.dark);
        glass::set_light(!self.theme.dark);
        self.look = self.theme.look;
        glass_ui::set_look(self.look);
        glass::set_look(self.look);
        // The plates are the theme's own solid card: nothing behind the pages to frost.
        glass::set_plain(self.settings.performance() || self.theme.dark);
        // Reduced motion (the player's or the system's) holds GPUI's own animations too.
        clock::set_preference(self.settings.reduce_motion);
        cx.set_reduce_motion(motion::still());
        self.scale = window.scale_factor();
        self.perf.frame();
        if self.bench.as_ref().is_some_and(perf::Bench::scrolling) {
            self.bench_scroll(window);
        }
        self.servers.note_focus(window, cx);
        self.friend_focused = self.friend_focus.is_focused(window);
        self.mods.note_focus(window);
        self.github.note_focus(window);
        self.args_focused = self.args_focus.is_focused(window);
        self.proxy_focused = self.proxy_focus.is_focused(window);
        self.settings_query_focused = self.settings_query_focus.is_focused(window);
        let size = window.viewport_size();
        self.window_width = f32::from(size.width);
        self.window_height = f32::from(size.height);
        self.window_long_side = f32::from(size.width).max(f32::from(size.height));
        self.scale_factor = window.scale_factor();
        let shown = if t.glass && self.page == Page::Settings {
            self.behind
        } else {
            self.page
        };
        let content = match shown {
            Page::Play => self.play_page(cx),
            Page::Versions => self.versions_page(cx),
            Page::Mods => self.mods_page(cx),
            Page::Servers => self.servers_page(cx),
            Page::Settings => self.settings_page(cx),
        };
        self.pictures.free_retired(window);
        // The new page fades in and rises a few pixels, as the website's tabs do (`hzl-pop`).
        let content = div()
            .relative()
            .w_full()
            // Let the page shrink inside the padded flex row so long notices can wrap.
            .min_w_0()
            .child(content)
            .with_animation(
                ElementId::named_usize("page", self.switches),
                motion::eased(motion::PAGE),
                |d, p| d.opacity(p).top(px(motion::PAGE_RISE * (1. - p))),
            );
        let (reveal, held) = self.opening_phase(window);
        let ui = ease_out_cubic((reveal - 0.3) / 0.7);
        if self.opening.is_none() && window.focused(cx).is_none() {
            window.focus(&self.root_focus, cx);
        }
        glass::begin_build(window);
        let sides = t.glass && self.outline_sides();
        let (right_sidebar, right_panel) = match sides.then(|| self.outline_side(false, window, cx))
        {
            Some((place, panel)) => (Some(place), panel),
            None => (None, None),
        };
        let (topbar, sidebar, left_panel) = if t.glass {
            let (place, panel) = if sides {
                self.outline_side(true, window, cx)
            } else {
                (div().into_any_element(), None)
            };
            (self.outline_topbar(window, cx), place, panel)
        } else {
            (
                self.topbar(cx),
                self.sidebar(window, cx).into_any_element(),
                None,
            )
        };
        let row_menu = if t.glass {
            self.row_menu(window, cx)
        } else {
            None
        };
        let page = self
            .content_scroll
            .wrap(
                div()
                    .id("content")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    // A scrolled page is drawn from its layer, not
                    // rendered: the Mods page's card asks, once the
                    // offset has moved, whether it must follow.
                    .on_scroll(cx.listener(|this, _: &gpui::ScrollEvent, window, cx| {
                        if this.page != Page::Mods {
                            return;
                        }
                        cx.on_next_frame(window, |this, _, cx| {
                            if this.mods_sticky.moved(&this.content_scroll.handle) {
                                cx.notify();
                            }
                        });
                    }))
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .min_w_0()
                            .px(px(theme::PAGE_GUTTER))
                            // The glass look's page heads hold their
                            // own 40 px above the title.
                            .pt(px(if self.page == Page::Play || t.glass {
                                0.
                            } else {
                                28.
                            }))
                            // Home's first screen fills the window and never scrolls.
                            .pb(px(if self.page == Page::Play && t.glass {
                                0.
                            } else {
                                48.
                            }))
                            // Past Home, a page keeps to a column it reads well at, centred
                            // under the top bar's pages.
                            .when(t.glass && self.page != Page::Play, |d| d.justify_center())
                            .child(if t.glass && self.page != Page::Play {
                                div()
                                    .flex()
                                    .w_full()
                                    .max_w(px(theme::PAGE_MAX))
                                    .min_w_0()
                                    .child(content)
                                    .into_any_element()
                            } else {
                                content.into_any_element()
                            }),
                    ),
                &t,
            )
            .flex_1()
            .min_h_0()
            .when(t.glass, |d| d.mt(px(crate::glass::CONTENT_INSET)));
        let mut root = div()
            .id("root")
            .track_focus(&self.root_focus)
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" && this.pending_consent.is_some() {
                    this.cancel_native_consent();
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let k = &ev.keystroke;
                match k.key.as_str() {
                    "tab" => {
                        KEYBOARD.store(true, Ordering::Relaxed);
                        if k.modifiers.shift {
                            window.focus_prev(cx);
                        } else {
                            window.focus_next(cx);
                        }
                    }
                    "escape" if this.welcome_escape() => {}
                    "escape" if this.consent_escape() => {}
                    "escape" if this.native_log_escape() => {}
                    "escape" if this.servers_escape() => {}
                    "escape" if this.instance_escape() => {}
                    "escape" if this.settings_escape() => {}
                    "escape" if this.screenshot_escape() => {}
                    "left" if this.screenshot_step(false) => {}
                    "right" if this.screenshot_step(true) => {}
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            // ⌘K (Ctrl+K elsewhere) wherever the focus is, even in a field: the palette.
            .capture_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                let k = &ev.keystroke;
                let command = if cfg!(target_os = "macos") {
                    k.modifiers.platform
                } else {
                    k.modifiers.control
                };
                if this.theme.glass && command && !k.modifiers.shift && k.key == "k" {
                    this.palette_toggle(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .capture_any_mouse_down(|_, _, _| KEYBOARD.store(false, Ordering::Relaxed))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .font_family(theme::FONT)
            .text_color(t.ink)
            .overflow_hidden()
            // The sidebar down the left, the top bar and the page beside it.
            .child(
                div()
                    .relative()
                    .flex()
                    .size_full()
                    .opacity(ui)
                    .top(px(theme::RISE * (1. - ui)))
                    .child(sidebar)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(topbar)
                            .child(page),
                    )
                    .children(right_sidebar)
                    // A narrow window's sidebars, opened over the page.
                    .children(left_panel)
                    .children(right_panel),
            );
        root = root.children(row_menu);
        if self.page == Page::Servers {
            if let Some(dialog) = self.create_dialog(cx) {
                root = root.child(dialog);
            }
        }
        root = self.import_drop_target(root, cx);
        root = root.children(self.import_note(cx));
        if let Some(dialog) = self.instance_dialog(cx) {
            root = root.child(dialog);
        }
        if t.glass && self.page == Page::Settings {
            root = root.child(self.settings_dialog(cx));
        }
        if let Some(panel) = self.native_log_panel(cx) {
            root = root.child(panel);
        }
        if self.page == Page::Play {
            if let Some(viewer) = self.screenshot_viewer(cx) {
                root = root.child(viewer);
            }
        }
        if t.glass {
            root = root.children(self.toast_float());
        }
        if let Some(palette) = self.palette_overlay(window, cx) {
            root = root.child(palette);
        }
        if self.pending_consent.is_some() {
            root = root.child(self.native_consent(cx));
        }
        if self.welcoming() {
            root = root.child(self.welcome_screen(cx));
        } else if self.asking_consent() {
            root = root.child(self.consent_screen(cx));
        }
        if self.asking_owner() {
            root = root.child(self.owner_screen(cx));
        }
        if let Some(opening) = &self.opening {
            // The wordmark arrives letter by letter, then gives way to the launcher; a click or
            // a key skips ahead.
            let gone = 1. - ease_out_cubic(reveal / 0.6);
            let word = div()
                .flex()
                .font_family(theme::PIXEL)
                .text_size(px(theme::WORDMARK.px * 0.5))
                .line_height(px(theme::WORDMARK.px * 0.5))
                .font_weight(FontWeight::BOLD)
                .text_color(t.ink)
                .children("SUNRISE".chars().enumerate().map(|(i, c)| {
                    let at = held - i as f32 * theme::STAGGER.as_secs_f32() * 1.5;
                    let p = ease_out_cubic(at / theme::ENTER.as_secs_f32());
                    div()
                        .relative()
                        .opacity(p)
                        .top(px(theme::RISE * 2. * (1. - p)))
                        .child(c.to_string())
                }));
            root = root.child(
                div()
                    .id("opening")
                    .track_focus(&opening.focus)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(t.page.opacity(gone))
                    .when(reveal == 0., |d| d.cursor_pointer())
                    .child(
                        div()
                            .relative()
                            .top(px(-24. * (1. - gone)))
                            .opacity(gone)
                            .child(word),
                    )
                    .when(reveal == 0., |d| {
                        d.on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.skip_opening();
                            cx.notify();
                        }))
                        .on_key_down(cx.listener(
                            |this, _: &KeyDownEvent, _, cx| {
                                this.skip_opening();
                                cx.notify();
                            },
                        ))
                    }),
            );
        }
        div()
            .flex()
            .flex_col()
            .relative()
            .size_full()
            .bg(t.side)
            .children(titlebar::render(&t, window))
            .children(self.settings_save_notice(cx))
            .child(root.flex_1().min_h_0())
            .children(titlebar::resize_handles(window))
    }
}

impl Launcher {
    /// Where the opening is: the reveal's progress (1 when there is no opening) and how long
    /// the wordmark has been showing, in seconds.
    pub(crate) fn opening_phase(&mut self, window: &mut Window) -> (f32, f32) {
        // Reduced motion: the wordmark's arrival and the reveal are all motion, so no opening.
        if motion::still() {
            self.opening = None;
        }
        let loaded = !matches!(self.releases, Load::Loading);
        let Some(opening) = &mut self.opening else {
            return (1., 10.);
        };
        let held = opening.started.get_or_insert_with(Instant::now).elapsed();
        if opening.reveal.is_none() && ((held >= OPENING_HOLD && loaded) || held >= OPENING_MAX) {
            opening.reveal = Some(Instant::now());
        }
        let reveal = opening.reveal.map_or(0., |at| {
            at.elapsed().as_secs_f32() / OPENING_REVEAL.as_secs_f32()
        });
        if reveal >= 1. {
            self.opening = None;
            return (1., 10.);
        }
        window.request_animation_frame();
        (reveal, held.as_secs_f32())
    }

    /// `sunrise-launcher benchmark`: scroll the page a little on every frame, back to the top at
    /// the bottom, so the frame times measure scrolling.
    pub(crate) fn bench_scroll(&mut self, window: &mut Window) {
        let handle = &self.content_scroll.handle;
        let mut offset = handle.offset();
        offset.y -= px(24.);
        if -offset.y > handle.max_offset().y {
            offset.y = px(0.);
        }
        handle.set_offset(offset);
        window.request_animation_frame();
    }

    pub(crate) fn skip_opening(&mut self) {
        if let Some(opening) = &mut self.opening {
            opening.reveal.get_or_insert_with(Instant::now);
        }
    }
}
