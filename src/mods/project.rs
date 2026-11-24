//! A mod's page (mock 04): one button, and a plain list of what adding it changes.
use super::model;
use super::ui::*;
use super::*;
use crate::paint::palette;
use sunrise_launcher::browser::images;

/// Show every registry screenshot. Pack feeds can list many more: the full-size viewer steps
/// through those too, without fetching all of a large pack gallery on opening its page.
impl Launcher {
    pub(super) fn gallery(
        &mut self,
        urls: Vec<super::viewer::Picture>,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if urls.is_empty() {
            return None;
        }
        let urls: Rc<[super::viewer::Picture]> = urls.into();
        const PREVIEWS: usize = sunrise_launcher_smod::index::page::MAX_SCREENSHOTS;
        let count = urls.len().min(PREVIEWS);
        let columns = if self.window_width < 950. { 3 } else { 4 };
        let height = if count == 1 { 220. } else { 104. };
        // A tile's tooltip would float over its own picture once that is open.
        let viewing = self.ui.mods.viewer.is_some();
        let mut tiles = urls.iter().take(PREVIEWS).enumerate().map(|(i, url)| {
            let photo = self.picture(url, images::GALLERY);
            // No wider than the picture's own shape at the row's height: a wide window would
            // otherwise blow it up and crop it.
            let ratio = photo
                .as_deref()
                .and_then(super::viewer::picture_size)
                .map_or(16. / 9., |(w, h)| w.max(1) as f32 / h.max(1) as f32);
            let all = urls.clone();
            let label = trf(
                "Picture {number} of {total}",
                &[
                    ("number", &(i + 1).to_string()),
                    ("total", &urls.len().to_string()),
                ],
            );
            div()
                .id(ElementId::named_usize("gallery-picture", i))
                .reach_edge()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .max_w(px(height * ratio))
                .cursor_pointer()
                .rounded(crate::tokens::r(crate::tokens::R_CONTROL))
                .border_1()
                .border_color(c(palette().divider))
                .bg(palette().card())
                .overflow_hidden()
                .when(!viewing, |d| d.tooltip(crate::glass_ui::tip(label)))
                .children(photo.map(|path| {
                    gpui::img(path)
                        .size_full()
                        .object_fit(gpui::ObjectFit::Contain)
                }))
                .when(i + 1 == PREVIEWS && urls.len() > PREVIEWS, |d| {
                    d.child(
                        div()
                            .absolute()
                            .right_0()
                            .bottom_0()
                            .px(px(6.))
                            .bg(c(palette().raised))
                            .child(format!("+{}", urls.len() - PREVIEWS)),
                    )
                })
                .track_scroll(&super::viewer::tile(i))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.viewer_open(all.clone(), i);
                    cx.notify();
                }))
        });
        let mut grid = div().flex().flex_col().flex_none().gap(px(6.));
        for _ in 0..count.div_ceil(columns) {
            grid = grid.child(
                div()
                    .flex()
                    .gap(px(6.))
                    .h(px(height))
                    .children(tiles.by_ref().take(columns)),
            );
        }
        Some(grid)
    }
}

/// A page's description, written in Markdown, in this look's colours.
pub(super) fn prose(theme: crate::theme::Theme, text: &str) -> AnyElement {
    let mut t = field_theme(theme);
    t.ink_soft = c(palette().text);
    t.plate_soft = c(palette().card_color);
    t.rule = c(palette().divider);
    crate::markdown::render(text, &t)
}

/// "‹ Discover": the way back from a page. Its padding widens the hit area and the negative
/// margin keeps the label lined up with the page.
pub(super) fn back(label: impl Into<SharedString>) -> Stateful<Div> {
    let p = palette();
    div()
        .id("alt-project-back")
        .group("alt-project-back")
        .reach()
        .self_start()
        .flex()
        .items_center()
        .gap(px(4.))
        .px(px(8.))
        .py(px(4.))
        .ml(px(-8.))
        .my(px(-4.))
        .rounded(px(6.))
        .text_size(px(tokens::LABEL))
        .text_color(c(p.muted))
        .cursor_pointer()
        .hover(|s| s.bg(c(p.text).opacity(0.06)).text_color(c(p.text)))
        .child(
            crate::blocks::icon("chevron-right", 14., c(p.muted))
                .with_transformation(gpui::Transformation::rotate(gpui::radians(
                    std::f32::consts::PI,
                )))
                .group_hover("alt-project-back", |s| s.text_color(c(p.text))),
        )
        .child(label.into())
}

/// One line of a page's About card.
pub(super) fn about_row(key: &'static str, value: String) -> Div {
    div()
        .flex()
        .justify_between()
        .gap(px(12.))
        .py(px(5.))
        .text_size(px(tokens::LABEL))
        .child(div().flex_none().text_color(c(palette().muted)).child(key))
        .child(div().min_w_0().truncate().child(value))
}

/// The line of an About card that opens a web page.
pub(super) fn about_link(id: &'static str, key: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .reach()
        .flex()
        .justify_between()
        .py(px(5.))
        .text_size(px(tokens::LABEL))
        .cursor_pointer()
        .child(div().text_color(c(palette().muted)).child(key))
        .child(
            div()
                .text_color(c(palette().accent_ink))
                .child(tr("Open ↗")),
        )
}

fn project_frame() -> Div {
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap(px(14.))
        .p(px(24.))
        .text_color(c(palette().text))
}

impl Launcher {
    pub(super) fn installed_project_controls(
        &self,
        row: &Row,
        cells: &model::Cells,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        use crate::frame::button;
        let hold = self
            .ui
            .mods
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.switch_held.get(&row.id))
            .cloned();
        let status = if let Some(hold) = &hold {
            Some(note(match hold.detail() {
                Some(detail) => format!("{} {detail}", hold.label()),
                None => hold.label().to_owned(),
            }))
        } else if cells.popover {
            row.unfit
                .clone()
                .map(|unfit| self.unfit(0, row, unfit, busy, cx))
        } else {
            cells.status.as_ref().map(|status| note(status.tip.clone()))
        };
        let toggle = ui::switch("alt-project-switch", row.on);
        let toggle = if busy || !cells.switch {
            dim(toggle)
        } else {
            let row = row.clone();
            toggle.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.toggle(&row);
                cx.notify();
            }))
        };
        let manual_badge = row.manual_badge();
        let manual_refresh = manual_badge.map(|_| {
            if let Some(progress) = self.registry_card_control(&row.id, |label| {
                dim(button("alt-project-github-progress", label)).into_any_element()
            }) {
                return progress;
            }
            let button = button("alt-project-manual-refresh", tr("Add from source"));
            if busy {
                dim(button).into_any_element()
            } else {
                button
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                        this.open_add_from_source();
                        cx.notify();
                    }))
                    .into_any_element()
            }
        });
        let remove = row
            .selectable_archive()
            .then(|| self.remove_mod_button(&row.id, busy, cx));
        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(manual_badge.map(|badge| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(ui::manual_badge_pill(badge))
                    .child(muted(tr(
                        "No automatic updates. Refresh via Add from source.",
                    )))
            }))
            .children(status)
            .children(
                hold.as_ref()
                    .and_then(|reason| reason.detail())
                    .map(|detail| note(detail.to_owned())),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(8.))
                    .child(toggle)
                    .child(note(if row.on {
                        tr("Turned on")
                    } else {
                        tr("Turned off")
                    }))
                    .children(manual_refresh)
                    .children(remove),
            )
    }

    pub(super) fn project(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let back_to = self
            .ui
            .mods
            .open
            .as_ref()
            .map_or(Page::Discover, |o| o.from);
        let parent = self.ui.mods.open.as_ref().and_then(|o| o.parent.clone());
        let parent_name = parent
            .as_ref()
            .and_then(|id| match &self.ui.mods.v2_catalog {
                V2Catalog::Ready { collections, .. } => collections
                    .iter()
                    .find(|c| c.id == *id)
                    .map(|c| c.name.clone()),
                _ => None,
            });
        let back_label = match (back_to, &parent_name) {
            (_, Some(name)) => name.clone(),
            (Page::Installed, _) => tr("Installed").to_string(),
            _ => tr("Discover").to_string(),
        };
        // The back link scrolls away with the page, above the side panel.
        let header = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                back(back_label).on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    match &parent {
                        Some(collection) => {
                            this.mods_show_v2_collection(collection.clone(), back_to)
                        }
                        None => this.ui.page = back_to,
                    }
                    cx.notify();
                })),
            )
            .children(self.strip(cx));
        let page = project_frame();
        let Some(open) = self.ui.mods.open.as_ref() else {
            return page
                .child(header)
                .child(muted(tr("No mod is open.")))
                .into_any_element();
        };
        if open.collection {
            return self.v2_collection_page(header, page, cx);
        }
        if open.v2 {
            return self.v2_project_page(header, page, cx);
        }
        let (rows, labels) = self.labeled_installed_rows();
        let Some(i) = rows.iter().position(|row| match &open.installed {
            Some(InstalledProject::Local(file)) => {
                row.id == open.id && row.local.as_ref().is_some_and(|local| &local.file == file)
            }
            Some(InstalledProject::Registry) | None => row
                .entry
                .as_ref()
                .is_some_and(|entry| entry.project == open.id),
        }) else {
            return page
                .child(header)
                .child(muted(tr("No mod is open.")))
                .into_any_element();
        };
        let row = &rows[i];
        // These controls mutate by mod ID, so showing a specific archive mustn't choose
        // which duplicate to mutate.
        let conflict = rows
            .iter()
            .any(|other| other.id == row.id && other.archive_file() != row.archive_file());
        let title = labels[i].as_ref().unwrap_or(&row.title);
        let pending = self
            .discover_installed()
            .is_some_and(|rows| rows.registry_pending(&row.id));
        let uses_registry = !matches!(open.installed, Some(InstalledProject::Local(_)));
        let package = self
            .archive_display(row)
            .and_then(|archive| archive.icon.clone());
        let picture = mod_artwork(
            row.icon
                .as_ref()
                .and_then(|url| self.mods_image(url, images::ICON)),
            package.clone(),
        );
        let head =
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(tile(picture, &row.id, title, 64.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(tokens::DISPLAY))
                                .font_weight(FontWeight::BOLD)
                                .child(crate::selectable::selectable(title.clone())),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_x(px(8.))
                                .gap_y(px(4.))
                                .children((!row.authors.is_empty()).then(|| {
                                    note(trf("by {authors}", &[("authors", &row.authors)]))
                                }))
                                .children(row.make.map(|make| tag(make.label()))),
                        ),
                );

        // The right column: the action, then the installed mod's controls.
        let busy = conflict
            || self.mods_busy()
            || self.mods_locked().is_some()
            || self
                .ui
                .mods
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| !snapshot.ready());
        let cells = model::cells(
            row,
            self.ui.mods.errors.get(&row.id).map(String::as_str),
            self.build_here()
                .filter(|_| row.on)
                .and_then(|run| run.state_of(&row.id, &row.version)),
        );
        let update = row
            .update
            .as_ref()
            .filter(|_| cells.action == Some(model::RowAction::Update));
        let action: AnyElement = if let Some(progress) = uses_registry
            .then(|| {
                self.registry_card_control(&row.id, |label| {
                    dim(primary("alt-project-add", label)
                        .w_full()
                        .h(px(tokens::ACTION)))
                    .into_any_element()
                })
            })
            .flatten()
        {
            progress
        } else if let Some(to) = update {
            let b = primary(
                "alt-project-update",
                trf(
                    "Update {from} → {to}",
                    &[("from", &row.version), ("to", to)],
                ),
            )
            .w_full()
            .h(px(tokens::ACTION));
            let stale_source =
                nightly::installed_source(row).is_some() && !self.nightly_catalogue_current();
            if busy || stale_source {
                dim(b).into_any_element()
            } else {
                let row = row.clone();
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    if nightly::installed_source(&row).is_some() {
                        this.submit_source_update(&row.id);
                    } else {
                        this.update_one(&row);
                    }
                    cx.notify();
                }))
                .into_any_element()
            }
        } else {
            div()
                .h(px(tokens::ACTION))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .rounded(crate::tokens::r(crate::tokens::R_CONTROL))
                .border_1()
                .border_color(c(palette().divider))
                .child(super::discover::install_fact(&row.version, pending))
                .into_any_element()
        };
        let panel_head = div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(tile(
                mod_artwork(
                    row.icon
                        .as_ref()
                        .and_then(|url| self.mods_image(url, images::ROW)),
                    package,
                ),
                &row.id,
                &row.title,
                40.,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::BOLD)
                            .child(row.title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::SMALL))
                            .font_family(crate::theme::MONO)
                            .text_color(c(palette().muted))
                            .child(row.version.clone()),
                    ),
            );
        let refusal = nightly::installed_source(row)
            .filter(|_| !conflict)
            .and_then(|origin| {
                let problem = self
                    .source_outcome(&row.id)
                    .and_then(|status| status.as_ref().err())
                    .or_else(|| self.source_update_issue(&row.id))?;
                let repo = format!("{}/{}", origin.owner, origin.repo);
                let refused = super::refused::Refused::one(&row.title, Some(repo), true, problem)?;
                Some(self.refused_line("alt-project-source-refused", refused, cx))
            });
        let origin = row.local.as_ref().and_then(|local| local.github.as_ref());
        let installed_controls = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .children(origin.map(|origin| note(nightly::source_provenance(origin))))
            .children(
                origin
                    .and_then(|origin| origin.tag.as_ref())
                    .map(|tag| note(trf("Release: {tag}", &[("tag", tag)]))),
            )
            .children(refusal)
            .children(conflict.then(|| note(tr(nightly::SOURCE_ID_CONFLICT))))
            .child(self.installed_project_controls(row, &cells, busy, cx));
        let source_button = origin
            .and_then(|origin| model::SourceLink::github(&origin.owner, &origin.repo))
            .and_then(|link| match link {
                model::SourceLink::GitHub(url) => Some(url),
                model::SourceLink::Other(_) => None,
            })
            .map(|url| {
                crate::frame::button("alt-project-github", tr("Open on GitHub"))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.open_url(&url)))
            });
        let right = div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                card()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(panel_head)
                    .child(action)
                    .children(
                        uses_registry
                            .then(|| self.registry_card_outcome(&row.id, cx))
                            .flatten(),
                    )
                    .child(installed_controls)
                    .children(source_button),
            )
            .children(self.other_ports_card(&row.id, Some(row), busy, cx));
        self.project_layout(header, head, None, std::iter::empty(), right)
    }

    /// The one project layout for registry, source and local packages. On a narrow window the
    /// action moves above the description; otherwise it stays beside it as the player scrolls.
    pub(super) fn project_layout(
        &self,
        header: Div,
        head: Div,
        gallery: Option<Div>,
        body: impl IntoIterator<Item = AnyElement>,
        side: Div,
    ) -> AnyElement {
        let scroll = crate::frame::scrolls("project");
        let (above, beside) = if self.content_width() < 860. {
            (Some(side), None)
        } else {
            (
                None,
                Some(
                    side.relative()
                        .top(px(self.mods_sticky.shift(&scroll, 1, 1)))
                        .w(px(360.))
                        .flex_none(),
                ),
            )
        };
        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(head)
            .children(above)
            .children(gallery)
            .children(body);
        project_frame()
            .child(crate::scrollbar::wrap_page_in_margin(
                "project",
                div()
                    .id("alt-project-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(header)
                    .child(
                        div()
                            .id("alt-project-columns")
                            .track_scroll(self.mods_sticky.row())
                            .flex()
                            .gap(px(24.))
                            .items_start()
                            .child(left)
                            .children(beside),
                    ),
                &self.theme,
            ))
            .into_any_element()
    }

    /// A v2 registry mod's page: summary, captioned screenshots (fetched only when this page
    /// opens), Markdown description, categories, "Published from github.com/owner/repo",
    /// publisher-pin note, and versions that fit the selected profile's exact build.
    fn v2_project_page(&mut self, header: Div, page: Div, cx: &mut Context<Self>) -> AnyElement {
        use super::model::SourceLink;
        use sunrise_launcher::browser::details::category_label;

        let p = palette();
        let open_id = self.ui.mods.open.as_ref().map(|o| o.id.clone());
        // Opened from Installed or Discover alike, an installed mod keeps its own controls here.
        let (rows, _) = self.labeled_installed_rows();
        let installed_row = rows.iter().find(|row| {
            row.v2_installed()
                && row
                    .entry
                    .as_ref()
                    .is_some_and(|e| Some(&e.project) == open_id.as_ref())
        });
        let busy = self.mods_busy()
            || self.mods_locked().is_some()
            || self
                .ui
                .mods
                .snapshot
                .as_ref()
                .is_none_or(|snapshot| !snapshot.ready());
        let cells = installed_row.map(|row| {
            model::cells(
                row,
                self.ui.mods.errors.get(&row.id).map(String::as_str),
                self.build_here()
                    .filter(|_| row.on)
                    .and_then(|run| run.state_of(&row.id, &row.version)),
            )
        });
        let controls = installed_row
            .zip(cells.as_ref())
            .map(|(row, cells)| self.installed_project_controls(row, cells, busy, cx));
        let vp = match self.ui.mods.open.as_ref().and_then(|o| o.v2_page.as_ref()) {
            None => {
                return page
                    .child(header)
                    .child(muted(tr("Loading…")))
                    .into_any_element();
            }
            Some(Err(error)) => {
                return page
                    .child(header)
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr("This mod can't be shown")),
                    )
                    .child(note(error.clone()))
                    // An installed mod the registry no longer lists can still be removed here.
                    .children(controls.map(|controls| controls.max_w(px(320.))))
                    .into_any_element();
            }
            Some(Ok(vp)) => (**vp).clone(),
        };

        let profile_name = self
            .selected_instance()
            .map(|i| i.name.clone())
            .unwrap_or_else(|| tr("Default").to_owned());
        let build_label = self
            .selected_exact_build()
            .as_deref()
            .map(crate::sheets::build_name);
        let target_label = match &build_label {
            Some(b) => trf(
                "{profile} ({build})",
                &[("profile", &profile_name), ("build", b)],
            ),
            None => profile_name.clone(),
        };

        let installed = self.discover_installed();
        let entry = installed.and_then(|rows| rows.registry_entry(&vp.id));
        let installed_version = entry.map(|entry| entry.number.clone());
        let pending = installed.is_some_and(|rows| rows.registry_pending(&vp.id));
        let package = installed
            .zip(entry)
            .and_then(|(rows, entry)| rows.archives.get(&entry.file)?.icon.clone());

        let picture = mod_artwork(
            vp.icon
                .as_deref()
                .and_then(|rel| self.v2_mod_image(&vp.id, rel, images::ICON)),
            package,
        );

        let credit = if vp.authors.is_empty() {
            String::new()
        } else {
            trf("by {authors}", &[("authors", &vp.authors)])
        };
        let categories = vp.categories.iter().map(|k| {
            div()
                .px(px(8.))
                .py(px(2.))
                .rounded(crate::tokens::r(crate::tokens::R_SMALL))
                .border_1()
                .border_color(c(p.divider))
                .bg(c(p.field))
                .text_size(px(tokens::CAPTION))
                .text_color(c(p.muted))
                .child(category_label(k))
        });

        let head = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(16.))
                    .child(tile(picture.clone(), &vp.id, &vp.name, 64.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(tokens::DISPLAY))
                                    .font_weight(FontWeight::BOLD)
                                    .child(crate::selectable::selectable(vp.name.clone())),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_center()
                                    .gap(px(8.))
                                    .when(!credit.is_empty(), |d| {
                                        d.child(
                                            div()
                                                .text_size(px(tokens::LABEL))
                                                .text_color(c(p.muted))
                                                .child(crate::selectable::selectable(credit)),
                                        )
                                    })
                                    .child(tag(vp.make.label()))
                                    .children(categories),
                            ),
                    ),
            )
            .when(!vp.summary.trim().is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(tokens::BODY))
                        .text_color(c(p.text))
                        .child(crate::selectable::selectable(vp.summary.clone())),
                )
            });

        // Captioned screenshot gallery: fetched ONLY here when the mod page opens.
        let gallery = self.v2_gallery(&vp.id, &vp.screenshots, cx);

        // Body: Markdown description + fitting versions section.
        let mut body: Vec<AnyElement> = Vec::new();
        if !vp.description.trim().is_empty() {
            body.push(prose(self.theme, &vp.description));
        }
        if !vp.dependencies.is_empty() {
            let mut list = card().flex().flex_col();
            for (n, dep) in vp.dependencies.iter().enumerate() {
                list = list.child(self.v2_dependency_line(n, dep, cx));
            }
            body.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .pt(px(8.))
                    .child(
                        div()
                            .text_size(px(tokens::HEADING))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tr("Dependencies")),
                    )
                    .child(list)
                    .into_any_element(),
            );
        }

        let versions_heading = div()
            .flex()
            .flex_wrap()
            .items_baseline()
            .justify_between()
            .gap(px(8.))
            .pt(px(8.))
            .child(
                div()
                    .text_size(px(tokens::HEADING))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(trf(
                        "Versions that fit {target}",
                        &[("target", &target_label)],
                    )),
            )
            .when(!vp.other_versions.is_empty(), |d| {
                let count = vp.other_versions.len();
                let label = if count == 1 {
                    tr("1 release for other builds hidden").to_owned()
                } else {
                    trf(
                        "{count} releases for other builds hidden",
                        &[("count", &count.to_string())],
                    )
                };
                d.child(muted(label))
            });

        let versions_card = if !vp.fitting_versions.is_empty() {
            let mut list = card().flex().flex_col();
            for (idx, line) in vp.fitting_versions.iter().enumerate() {
                let is_installed_ver = installed_version.as_deref() == Some(line.version.as_str());
                let badge_el = if is_installed_ver {
                    super::discover::install_fact(&line.version, pending)
                } else if idx == 0 {
                    div()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded(crate::tokens::r(crate::tokens::R_SMALL))
                        .bg(c(p.success).opacity(0.16))
                        .border_1()
                        .border_color(c(p.success).opacity(0.35))
                        .text_size(px(tokens::CAPTION))
                        .text_color(c(p.success))
                        .child(line.verdict.fit_label())
                } else {
                    div()
                        .text_size(px(tokens::SMALL))
                        .text_color(c(p.muted))
                        .child(line.verdict.fit_label())
                };
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(10.))
                        .when(idx > 0, |d| d.border_t_1().border_color(c(p.divider)))
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .font_family(crate::theme::MONO)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_size(px(tokens::LABEL))
                                        .child(line.version.clone()),
                                )
                                .child(muted(line.published.clone()))
                                .child(muted(model::size(line.size))),
                        )
                        .child(badge_el),
                );
            }
            list
        } else {
            let mut list = card().flex().flex_col();
            if let Some(reason) = vp.fit.reason() {
                list = list.child(
                    div()
                        .px(px(14.))
                        .py(px(12.))
                        .text_size(px(tokens::LABEL))
                        .text_color(c(p.muted))
                        .child(reason),
                );
            }
            for line in &vp.other_versions {
                let reason = line.verdict.reason().unwrap_or_default();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(10.))
                        .border_t_1()
                        .border_color(c(p.divider))
                        .opacity(0.75)
                        .child(
                            div()
                                .flex()
                                .items_baseline()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .font_family(crate::theme::MONO)
                                        .text_size(px(tokens::LABEL))
                                        .child(line.version.clone()),
                                )
                                .child(muted(line.published.clone())),
                        )
                        .child(muted(reason)),
                );
            }
            list
        };
        body.push(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(versions_heading)
                .child(versions_card)
                .into_any_element(),
        );

        // Right sticky side panel: action card + publisher provenance + pin callout + About card.
        // An update runs under the installed mod's id, which can differ from the project's.
        let installed_id = installed_row
            .map(|row| row.id.as_str())
            .filter(|id| *id != vp.id);
        let working_id = installed_id
            .filter(|id| {
                matches!(
                    self.registry_card(id),
                    Some(crate::mods_page::install::Card::Progress(_))
                        | Some(crate::mods_page::install::Card::Waiting)
                )
            })
            .unwrap_or(&vp.id);
        let working = self.registry_card_control(working_id, |label| {
            dim(crate::frame::button("alt-project-add", label).w_full()).into_any_element()
        });
        let update = installed_row
            .zip(cells.as_ref())
            .filter(|(_, cells)| cells.action == Some(model::RowAction::Update))
            .and_then(|(row, _)| row.update.as_ref().map(|to| (row, to)));
        let primary_control: AnyElement = if let Some(control) = working {
            control
        } else if let Some((row, to)) = update {
            let b = primary(
                "alt-project-update",
                trf(
                    "Update {from} → {to}",
                    &[("from", &row.version), ("to", to)],
                ),
            )
            .w_full();
            if busy {
                dim(b).into_any_element()
            } else {
                let row = row.clone();
                b.on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.update_one(&row);
                    cx.notify();
                }))
                .into_any_element()
            }
        } else if let Some(version) = &installed_version {
            super::discover::install_fact(version, pending).into_any_element()
        } else if vp.will_install.is_some() {
            let id = vp.id.clone();
            primary("alt-project-add", self.registry_install_label())
                .w_full()
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.add_v2(id.clone());
                    cx.notify();
                }))
                .into_any_element()
        } else {
            dim(crate::frame::button("alt-project-add", tr("Install")))
                .w_full()
                .into_any_element()
        };

        let meta_sub = match &vp.will_install {
            Some(v) => trf(
                "{version} · {size}",
                &[("version", &v.version), ("size", &model::size(v.size))],
            ),
            None => vp.fit.reason().unwrap_or_default(),
        };
        let repo_href = vp.repository_url.as_ref().map(|link| match link {
            SourceLink::GitHub(url) | SourceLink::Other(url) => url.clone(),
        });
        let published_from_text = trf(
            "Published from github.com/{repo}",
            &[("repo", &vp.repository_name)],
        );
        let published_row = {
            let row = div()
                .id("alt-project-publisher")
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(tokens::SMALL))
                .text_color(c(p.muted))
                .child(crate::glass_ui::icon("github", 14., c(p.muted)))
                .child(div().min_w_0().truncate().child(published_from_text));
            if let Some(url) = repo_href {
                row.cursor_pointer()
                    .hover(|s| s.text_color(c(p.accent_ink)))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        Self::open_url(&url, cx);
                    }))
            } else {
                row
            }
        };

        let action_card = card()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(tile(picture, &vp.id, &vp.name, 40.))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_size(px(tokens::BODY))
                                    .truncate()
                                    .child(vp.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(tokens::SMALL))
                                    .text_color(c(p.muted))
                                    .truncate()
                                    .child(meta_sub),
                            )
                            // Under the name, where a long label wraps instead of squeezing it.
                            .children(vp.will_install.as_ref().map(|line| {
                                div()
                                    .text_size(px(tokens::CAPTION))
                                    .text_color(c(p.success))
                                    .child(line.verdict.fit_label())
                            })),
                    ),
            )
            .child(primary_control)
            .children(self.registry_card_outcome(&vp.id, cx))
            .children(installed_id.and_then(|id| self.registry_card_outcome(id, cx)))
            .when(vp.will_install.is_none(), |d| {
                d.children(vp.fit.reason().map(note))
            })
            .children(controls)
            .child(div().h(px(1.)).bg(c(p.divider)))
            .child(self.rating_block(&vp.id, cx))
            .child(div().h(px(1.)).bg(c(p.divider)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(tokens::SMALL))
                    .text_color(c(p.muted))
                    .child(crate::glass_ui::icon("tick-02", 14., c(p.muted)))
                    .child(tr("Reviewed by Sunrise")),
            )
            .child(published_row);

        let link_url = |link: &Option<SourceLink>| -> Option<String> {
            link.as_ref().map(|l| match l {
                SourceLink::GitHub(url) | SourceLink::Other(url) => url.clone(),
            })
        };
        let source_url = link_url(&vp.source).or_else(|| link_url(&vp.repository_url));
        let homepage_url = link_url(&vp.homepage);
        let issues_url = link_url(&vp.issues);

        let mut about = card()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .child(
                div()
                    .pb(px(6.))
                    .text_size(px(tokens::CAPTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(c(p.muted))
                    .child(tr("ABOUT")),
            )
            .child(about_row(tr("In-game ID"), vp.mod_id.clone()))
            .when(!vp.updated.is_empty(), |d| {
                d.child(about_row(tr("Updated"), vp.updated.clone()))
            })
            .when(!vp.license.is_empty(), |d| {
                d.child(about_row(tr("Licence"), vp.license.clone()))
            });

        if let Some(url) = source_url {
            about = about.child(
                about_link("alt-v2-source", tr("Source")).on_click(cx.listener(
                    move |_, _: &ClickEvent, _, cx| {
                        Self::open_url(&url, cx);
                    },
                )),
            );
        }
        if let Some(url) = homepage_url {
            about = about.child(about_link("alt-v2-homepage", tr("Homepage")).on_click(
                cx.listener(move |_, _: &ClickEvent, _, cx| {
                    Self::open_url(&url, cx);
                }),
            ));
        }
        if let Some(url) = issues_url {
            about = about.child(
                about_link("alt-v2-issues", tr("Issues")).on_click(cx.listener(
                    move |_, _: &ClickEvent, _, cx| {
                        Self::open_url(&url, cx);
                    },
                )),
            );
        }

        let side = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(action_card)
            .child(about);

        self.project_layout(header, head, gallery, body, side)
    }

    /// One dependency, as Modrinth lists them: its picture and name, which open its page when the
    /// registry has it, the versions it must be, and whether it is required.
    fn v2_dependency_line(
        &mut self,
        n: usize,
        dep: &super::model::discover_v2::V2Dependency,
        cx: &mut Context<Self>,
    ) -> Div {
        use super::model::discover_v2::V2Provider;
        let p = palette();
        let (picture, id, name, detail) = match &dep.provider {
            V2Provider::Listed { id, name, icon } => (
                icon.as_deref()
                    .and_then(|rel| self.v2_mod_image(id, rel, images::ROW)),
                id.clone(),
                name.clone(),
                None,
            ),
            V2Provider::Several(count) => (
                None,
                dep.mod_id.clone(),
                dep.mod_id.clone(),
                Some(trf(
                    "{count} mods on the registry provide it: installing asks which one",
                    &[("count", &count.to_string())],
                )),
            ),
            V2Provider::Missing => (
                None,
                dep.mod_id.clone(),
                dep.mod_id.clone(),
                Some(tr("Not on the registry").to_owned()),
            ),
        };
        let mut title = div()
            .id(("alt-v2-dependency", n))
            .truncate()
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(tokens::LABEL))
            .child(name.clone());
        if let V2Provider::Listed { id, .. } = &dep.provider {
            let project = id.clone();
            title = title
                .reach()
                .cursor_pointer()
                .hover(|s| s.text_color(c(p.accent_ink)))
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.mods_show_v2_project(project.clone());
                    cx.notify();
                }));
        }
        let range = dep
            .range
            .clone()
            .unwrap_or_else(|| tr("any version").to_owned());
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .px(px(14.))
            .py(px(10.))
            .when(n > 0, |d| d.border_t_1().border_color(c(p.divider)))
            .child(tile(picture, &id, &name, 36.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .gap(px(8.))
                            .min_w_0()
                            .child(title)
                            .child(
                                div()
                                    .flex_none()
                                    .font_family(crate::theme::MONO)
                                    .text_size(px(tokens::SMALL))
                                    .text_color(c(p.muted))
                                    .child(range),
                            ),
                    )
                    .children(detail.map(|text| div().truncate().child(muted(text)))),
            )
            .child(match dep.required {
                true => tag(tr("Required")),
                false => tag(tr("Optional")),
            })
    }

    /// Captioned screenshot gallery for a v2 mod page: fetches verified content-addressed images
    /// only when the mod page is open.
    fn v2_gallery(
        &mut self,
        project_id: &str,
        screenshots: &[super::model::discover_v2::V2Screenshot],
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        if screenshots.is_empty() {
            return None;
        }
        let p = palette();
        let pictures: Rc<[super::viewer::Picture]> = screenshots
            .iter()
            .map(|shot| super::viewer::Picture::V2 {
                project_id: project_id.to_owned(),
                relative_path: shot.path.clone(),
                caption: shot.caption.clone(),
            })
            .collect::<Vec<_>>()
            .into();
        let count = screenshots.len();
        let content = self.content_width();
        let avail = if content < 860. {
            (content - 62.).max(240.)
        } else {
            (content - 442.).max(280.)
        };
        let columns = if count == 1 {
            1
        } else if avail < 680. {
            2
        } else {
            count.min(3)
        };
        let tile_w = (avail - 10. * (columns - 1) as f32) / columns as f32;
        let max_h = if count == 1 { 320. } else { 240. };
        let height = (tile_w * (9. / 16.)).clamp(110., max_h).round();
        let viewing = self.ui.mods.viewer.is_some();

        let mut items = screenshots.iter().enumerate().map(|(i, shot)| {
            let photo = self.v2_mod_image(project_id, &shot.path, images::GALLERY);
            let all = pictures.clone();
            let tip_text = if shot.caption.is_empty() {
                trf(
                    "Picture {number} of {total}",
                    &[
                        ("number", &(i + 1).to_string()),
                        ("total", &count.to_string()),
                    ],
                )
            } else {
                shot.caption.clone()
            };
            div()
                .flex_1()
                .min_w_0()
                .max_w(px(height * (16. / 9.)))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div()
                        .id(ElementId::named_usize("v2-gallery-picture", i))
                        .reach_edge()
                        .h(px(height))
                        .w_full()
                        .cursor_pointer()
                        .rounded(crate::tokens::r(crate::tokens::R_CONTROL))
                        .border_1()
                        .border_color(c(p.divider))
                        .bg(p.card())
                        .overflow_hidden()
                        .when(!viewing, |d| d.tooltip(crate::glass_ui::tip(tip_text)))
                        .children(photo.map(|path| {
                            gpui::img(path)
                                .size_full()
                                .object_fit(gpui::ObjectFit::Contain)
                        }))
                        .track_scroll(&super::viewer::tile(i))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.viewer_open(all.clone(), i);
                            cx.notify();
                        })),
                )
                .when(!shot.caption.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(tokens::SMALL))
                            .text_color(c(p.muted))
                            .line_clamp(2)
                            .text_ellipsis()
                            .child(shot.caption.clone()),
                    )
                })
        });

        let mut grid = div().flex().flex_col().flex_none().gap(px(10.));
        for _ in 0..count.div_ceil(columns) {
            grid = grid.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(10.))
                    .children(items.by_ref().take(columns)),
            );
        }
        Some(grid)
    }
}
