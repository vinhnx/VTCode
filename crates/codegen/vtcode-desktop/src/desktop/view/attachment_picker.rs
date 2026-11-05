//! Attachment picker: a tabbed tree surface for attaching workspace
//! files or picking from this computer, anchored above the composer.
use super::*;

/// Which source tab the picker shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AttachmentPickerTab {
    Workspace,
    Computer,
}

/// Open picker state: the active tab. Tree rows, filter text, and hit counts
/// live on the view beside the inspector's so both trees share one scan.
pub(super) struct AttachmentPickerState {
    tab: AttachmentPickerTab,
}

impl DesktopView {
    pub(super) fn open_attachment_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_none() || self.session_request.is_some() {
            return;
        }
        // Same-kind openers toggle: a second trigger closes the picker
        // instead of reopening it, mirroring `toggle_palette_kind`.
        if self.attachment_picker.is_some() {
            self.close_attachment_picker(window, cx);
            return;
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        }
        self.disarm_double_escape();
        self.attachment_picker = Some(AttachmentPickerState { tab: AttachmentPickerTab::Workspace });
        self.attachment_picker_focus.focus(window, cx);
        // Fresh query each opening; `set_value` emits no Change, so the
        // subscription handler runs explicitly.
        self.picker_filter_input.update(cx, |input, cx| input.set_value("", window, cx));
        self.rebuild_picker_tree(cx);
        cx.notify();
    }

    pub(super) fn close_attachment_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.attachment_picker.take().is_none() {
            return;
        }
        self.disarm_double_escape();
        self.focus_composer(window, cx);
        cx.notify();
    }

    pub(super) fn set_attachment_picker_tab(&mut self, tab: AttachmentPickerTab, cx: &mut Context<Self>) {
        if let Some(state) = self.attachment_picker.as_mut() {
            if state.tab != tab {
                state.tab = tab;
                cx.notify();
            }
        }
    }

    /// Rebuild picker rows from the current scan and filter query. Browse
    /// mode shows the hierarchy; a filter query shows flat file-only ranked
    /// hits (attaching a directory is meaningless, so dirs never rank).
    pub(super) fn rebuild_picker_tree(&mut self, cx: &mut Context<Self>) {
        if self.attachment_picker.is_none() {
            return;
        }
        let selected_path = self.picker_tree.read(cx).selected_item().map(|item| item.id.to_string());
        let query = self.picker_filter_input.read(cx).value().trim().to_string();
        let tree_items = if query.is_empty() {
            self.picker_search_hit_count = None;
            self.expanded_picker_directories
                .retain(|path| self.file_directory_paths.contains(path));
            file_tree_items(&self.files, None, &self.expanded_picker_directories)
        } else {
            let rows: Vec<TreeItem> = files::fuzzy_search_files(&self.files, &query, files::SEARCH_MAX_HITS)
                .iter()
                .map(|hit| TreeItem::new(hit.rel_path.clone(), hit.rel_path.clone()))
                .collect();
            self.picker_search_hit_count = Some(rows.len());
            rows
        };
        let selected_item = selected_path
            .as_deref()
            .and_then(|path| find_file_tree_item(&tree_items, path))
            .cloned();
        self.picker_tree.update(cx, |tree, cx| {
            tree.set_items(tree_items, cx);
            tree.set_selected_item(selected_item.as_ref(), cx);
        });
        cx.notify();
    }

    /// Attach the picker's keyboard selection (Enter in the tree). Folders
    /// no-op: Kit owns expand/collapse, mirroring the inspector tree.
    pub(super) fn attach_selected_picker_file(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.picker_tree.read(cx).selected_item().map(|item| item.id.to_string()) else {
            return;
        };
        self.toggle_picker_attachment(&path, cx);
    }

    /// Attach a file, or detach it when already attached. Clicking a row and
    /// pressing Enter share this; anything that is not a current workspace
    /// file (folders, stale rows) no-ops instead of erroring.
    fn toggle_picker_attachment(&mut self, rel_path: &str, cx: &mut Context<Self>) {
        let is_file = self
            .files
            .iter()
            .find(|entry| entry.rel_path == rel_path)
            .is_some_and(|entry| !entry.is_dir);
        if !is_file {
            return;
        }
        if self.attachments.iter().any(|attachment| attachment.path == rel_path) {
            self.remove_attachment(rel_path.to_string(), cx);
        } else {
            self.attach_context_file(rel_path.to_string(), cx);
        }
    }

    /// Tabbed picker anchored above the composer: the workspace tab filters
    /// a file tree with attach-on-toggle rows, the computer tab hands off to
    /// the native file picker.
    pub(super) fn render_attachment_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let ui_font_size = cx.theme().font_size;
        let popup_margin = ui_font_size * 0.75;
        let Some(state) = self.attachment_picker.as_ref() else {
            return div().into_any_element();
        };
        let weak = cx.entity().downgrade();

        // The wrapper owns the lookup id: Kit bars scope their id into child
        // paths but register nothing observable themselves.
        let tabs = div().id("attachment-picker-tabs").test_support().child(
            desktop_folder_tab_bar("attachment-picker-tabs-bar")
                .selected_index(match state.tab {
                    AttachmentPickerTab::Workspace => 0,
                    AttachmentPickerTab::Computer => 1,
                })
                .on_click(cx.listener(|this, index: &usize, _, cx| {
                    this.set_attachment_picker_tab(
                        if *index == 0 {
                            AttachmentPickerTab::Workspace
                        } else {
                            AttachmentPickerTab::Computer
                        },
                        cx,
                    );
                }))
                .child(
                    Tab::new()
                        .label(app::NAME.to_string())
                        .aria_label(app::NAME.to_string())
                        .prefix(Icon::new(IconName::FolderOpen).small()),
                )
                .child(
                    Tab::new()
                        .label(rust_i18n::t!("app.composer.picker_this_computer").to_string())
                        .aria_label(rust_i18n::t!("app.composer.picker_this_computer").to_string())
                        .prefix(Icon::new(IconName::LaptopMinimal).small()),
                ),
        );

        let body = match state.tab {
            AttachmentPickerTab::Workspace => self.render_picker_files(cx),
            AttachmentPickerTab::Computer => div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .p_4()
                .child(
                    desktop_primary_button("picker-choose-files")
                        .label(rust_i18n::t!("app.composer.attach_files").to_string())
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.close_attachment_picker(window, cx);
                            this.attach_files(window, cx);
                        })),
                )
                .into_any_element(),
        };

        deferred(
            anchored()
                .anchor(Anchor::BottomLeft)
                .offset(point(px(0.), ui_font_size * -0.5))
                .snap_to_window_with_margin(popup_margin)
                .child(
                    div()
                        .id("attachment-picker")
                        .test_support()
                        .track_focus(&self.attachment_picker_focus)
                        .focus_trap("attachment-picker-trap", &self.attachment_picker_focus)
                        .role(gpui_kit::Role::Dialog)
                        .accessibility_id("attachment-picker")
                        .aria_label(rust_i18n::t!("app.composer.attach_files").to_string())
                        .w(px(560.))
                        .max_w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .rounded_md()
                        .bg(colors.popover)
                        .border_1()
                        .border_color(colors.border)
                        .shadow_md()
                        // Click-away dismissal, mirroring the palette panel: a
                        // press outside the popover closes it. The close is
                        // idempotent and the listener unmounts with the panel,
                        // so no staleness guard is needed.
                        .on_mouse_down_out(move |_, window, cx| {
                            let _updated = weak.update(cx, |view, cx| {
                                view.close_attachment_picker(window, cx);
                            });
                        })
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .p_3()
                        .child(tabs)
                        .child(body),
                ),
        )
        .priority(2)
        .into_any_element()
    }

    /// Workspace tab: filter field over a file tree. Rows toggle attachments
    /// on click or Enter; attached rows pin a check. Rows share the
    /// inspector's label builder, and the check icon stays decorative: the
    /// accessible name matches the inspector's, and composer chips own the
    /// attached/remove announcements.
    fn render_picker_files(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.files.is_empty() {
            return div()
                .id("picker-files-empty")
                .test_support()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .p_4()
                .child(empty_state(
                    rust_i18n::t!("app.files.empty.title").to_string(),
                    rust_i18n::t!("app.files.empty.description").to_string(),
                    IconName::Folder,
                ))
                .into_any_element();
        }
        let searching = self.picker_search_hit_count.is_some();
        let filter_count = self.picker_search_hit_count.map(|shown| {
            let file_total = self.files.iter().filter(|entry| !entry.is_dir).count();
            rust_i18n::t!("app.inspector.filtered_count", shown = shown, total = file_total).to_string()
        });
        let attached_paths: HashSet<String> =
            self.attachments.iter().map(|attachment| attachment.path.clone()).collect();
        let directory_paths = self.file_directory_paths.clone();
        let picker_tree = self.picker_tree.clone();
        let weak_view = cx.entity().downgrade();
        let check_color = cx.theme().colors.primary;
        let mut body = div().w_full().flex().flex_col().gap_2().child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    Input::new(&self.picker_filter_input)
                        .id("picker-file-filter")
                        .flex_1()
                        .min_w_0()
                        .small()
                        .aria_label(files_panel::FILTER_FILES_LABEL.to_string())
                        .prefix(Icon::new(IconName::Search).size_4())
                        .cleanable(true),
                )
                .when_some(filter_count, |row, count| {
                    row.child(
                        div()
                            .id("picker-file-filter-count")
                            .test_support()
                            .role(gpui_kit::Role::Label)
                            .aria_label(count.clone())
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(count),
                    )
                }),
        );
        if searching && self.picker_search_hit_count == Some(0) {
            body = body.child(
                div()
                    .id("picker-file-filter-empty")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(files_panel::NO_MATCHING_FILES_NOTE.to_string())
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_3()
                    .text_sm()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(files_panel::NO_MATCHING_FILES_NOTE),
            );
        } else {
            body = body.child(
                div()
                    .id("picker-file-list")
                    .test_support()
                    .h(px(300.))
                    .w_full()
                    .overflow_hidden()
                    .on_action(cx.listener(|this, _: &OpenSelectedFile, _, cx| this.attach_selected_picker_file(cx)))
                    .child(Tree::new(&self.picker_tree, move |_index, entry, selected, _window, app| {
                        let rel_path = entry.item().id.to_string();
                        let is_dir = directory_paths.contains(&rel_path);
                        let display_name = rel_path.rsplit('/').next().unwrap_or(&rel_path).to_string();
                        let attached = attached_paths.contains(&rel_path);
                        let search_parent = if searching {
                            rel_path.rsplit_once('/').map(|(dir, _)| dir)
                        } else {
                            None
                        };
                        let icon = files_panel::file_tree_row_icon(is_dir, entry.is_expanded());
                        let tree = picker_tree.clone();
                        let view = weak_view.clone();
                        let toggle_path = rel_path.clone();
                        ListItem::new(format!("picker-file-{rel_path}"))
                            .selected(selected)
                            .cursor_pointer()
                            .accessibility_label({
                                let base = if is_dir {
                                    rust_i18n::t!("app.files.folder_accessibility", path = rel_path.as_str())
                                        .to_string()
                                } else {
                                    rust_i18n::t!("app.files.file_accessibility", path = rel_path.as_str()).to_string()
                                };
                                // The check icon is decorative, so attached
                                // state joins the spoken label instead.
                                if attached && !is_dir {
                                    format!("{base} (attached)")
                                } else {
                                    base
                                }
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap_2()
                                    .child(files_panel::file_tree_row_label(
                                        format!("picker-file-label-{rel_path}"),
                                        entry.depth(),
                                        display_name,
                                        search_parent,
                                        icon,
                                        app,
                                    ))
                                    .when(attached, |row| {
                                        row.child(Icon::new(IconName::Check).small().text_color(check_color))
                                    }),
                            )
                            .on_click(move |_, window, app| {
                                tree.update(app, |tree, app| tree.focus(window, app));
                                if is_dir {
                                    return;
                                }
                                if let Some(view) = view.upgrade() {
                                    view.update(app, |view, cx| view.toggle_picker_attachment(&toggle_path, cx));
                                }
                            })
                    })),
            );
        }
        body.into_any_element()
    }
}
