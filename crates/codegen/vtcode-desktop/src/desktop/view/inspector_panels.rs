//! Inspector Files and Output panels.

use super::*;
mod activity;

/// A workspace entry being dragged out of the file tree.
struct DragWorkspaceFile {
    rel_path: String,
    is_dir: bool,
}

/// Chip under the cursor while a workspace file drags toward another app.
struct FileDragPreview {
    name: String,
    is_dir: bool,
}

impl Render for FileDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let icon = if self.is_dir {
            IconName::FolderClosed
        } else {
            IconName::FileText
        };
        div()
            .flex()
            .items_center()
            .gap_x_2()
            .rounded_md()
            .bg(colors.popover)
            .border_1()
            .border_color(colors.border)
            .shadow_md()
            .px_3()
            .py_1()
            .child(Icon::new(icon).small().text_color(colors.muted_foreground))
            .child(div().text_sm().text_color(colors.foreground).child(self.name.clone()))
    }
}

impl DesktopView {
    /// Generation bump after in-place `restored_activity` edits that keep no
    /// helper (output-page appends); all other mutations go through the
    /// helpers below so the inspector caches stay sound.
    pub(super) fn bump_restored_activity_revision(&mut self) {
        self.restored_activity_revision = self.restored_activity_revision.wrapping_add(1);
    }

    pub(super) fn push_restored_activity(&mut self, item: TranscriptItem) {
        self.restored_activity.push(item);
        self.bump_restored_activity_revision();
    }

    pub(super) fn retain_restored_activity(&mut self, mut keep: impl FnMut(&TranscriptItem) -> bool) {
        let before = self.restored_activity.len();
        self.restored_activity.retain(|item| keep(item));
        if self.restored_activity.len() != before {
            self.bump_restored_activity_revision();
        }
    }

    pub(super) fn clear_restored_activity(&mut self) {
        if !self.restored_activity.is_empty() {
            self.restored_activity.clear();
            self.bump_restored_activity_revision();
        }
    }

    /// Toggle the Activity tab's status filter. Selecting the active status
    /// again clears the filter, so filtering stays visible and reversible.
    pub(super) fn toggle_activity_filter(&mut self, state: ActivityState) {
        self.activity_filter = if self.activity_filter == Some(state) {
            None
        } else {
            Some(state)
        };
    }

    /// Shared activity projection, rebuilt only when the transcript or the
    /// restored history changed since the last call.
    pub(super) fn cached_activity_projection(&mut self) -> Rc<inspector::ActivityProjection> {
        let transcript_revision = self.transcript.revision();
        let restored_revision = self.restored_activity_revision;
        if let Some(hit) = self
            .activity_projection_cache
            .as_ref()
            .and_then(|cache| cache.get(transcript_revision, restored_revision))
        {
            return hit;
        }
        let cache = inspector::ActivityProjectionCache::fresh(
            transcript_revision,
            restored_revision,
            inspector::activity_rows(&self.transcript, &self.restored_activity),
        );
        let shared = cache.shared();
        self.activity_projection_cache = Some(cache);
        shared
    }

    /// Shared output resolution plus its bounded tail, rebuilt only when the
    /// transcript, the restored history, or the selection changed.
    pub(super) fn cached_output_view(&mut self) -> (Option<Rc<inspector::OutputView>>, gpui_kit::SharedString) {
        let transcript_revision = self.transcript.revision();
        let restored_revision = self.restored_activity_revision;
        if let Some(hit) = self
            .output_view_cache
            .as_ref()
            .and_then(|cache| cache.get(transcript_revision, restored_revision, self.selected_output_id.as_deref()))
        {
            return hit;
        }
        let cache = inspector::OutputViewCache::fresh(
            transcript_revision,
            restored_revision,
            self.selected_output_id.clone(),
            inspector::output_for(&self.transcript, &self.restored_activity, self.selected_output_id.as_deref()),
        );
        let shared = cache.shared();
        self.output_view_cache = Some(cache);
        shared
    }

    /// Bounded tail for an expanded transcript tool row, memoized per
    /// transcript revision so chatty outputs are not rescanned every frame.
    pub(super) fn expanded_tool_tail(&mut self, index: usize) -> Option<gpui_kit::SharedString> {
        let item = self.transcript.get(index)?;
        if !(item.is_tool() && item.expanded) {
            return None;
        }
        let output = item
            .output
            .as_ref()
            .filter(|output| !output.is_empty())
            .cloned()
            .unwrap_or_else(|| item.text.clone());
        let id = item.id.clone();
        let revision = self.transcript.revision();
        Some(inspector::resolve_tool_tail(&id, &output, revision, &mut self.tool_tail_cache))
    }

    pub(super) fn render_inspector_files_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let mut body = inspector_tab_body().id("inspector-files-tab").test_support();
        body = body.child(panel_header(
            desktop_button("focus-files")
                .label(rust_i18n::t!("app.files.workspace_files").to_string())
                .accessibility_label(rust_i18n::t!("app.files.focus_tree").to_string())
                .tooltip_with_action(
                    rust_i18n::t!("app.files.focus_tree").to_string(),
                    &OpenWorkspaceFiles,
                    Some(key_context::DESKTOP),
                )
                .ghost()
                .dispatches_desktop_action(OpenWorkspaceFiles, self.focus_handle.clone()),
            desktop_button("refresh-files")
                .label(rust_i18n::t!("app.files.refresh").to_string())
                .accessibility_label(rust_i18n::t!(i18n::FILES_REFRESH_TOOLTIP).to_string())
                .tooltip_with_action(
                    rust_i18n::t!(i18n::FILES_REFRESH_TOOLTIP).to_string(),
                    &RefreshWorkspace,
                    Some(key_context::DESKTOP),
                )
                .ghost()
                .dispatches_desktop_action(RefreshWorkspace, self.focus_handle.clone()),
        ));
        let searching = self.file_search_hit_count.is_some();
        let filter_count = self.file_search_hit_count.map(|shown| {
            rust_i18n::t!("app.inspector.filtered_count", shown = shown, total = self.files.len()).to_string()
        });
        body = body.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .child(
                    Input::new(&self.file_filter_input)
                        .id("file-list-filter")
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
                            .id("file-list-filter-count")
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
        let directory_paths = self.file_directory_paths.clone();
        let menu_directory_paths = directory_paths.clone();
        let file_tree = self.file_tree.clone();
        let weak_view = cx.entity().downgrade();
        let drag_root = self.workspace.clone();
        if self.files.is_empty() && self.files_scan_cancel.is_some() {
            body = body.child(
                div()
                    .id("file-list-loading")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(rust_i18n::t!("app.files.scanning").to_string())
                    .aria_description(rust_i18n::t!("app.files.scanning_description").to_string())
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_3()
                    .text_sm()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(loading_marker_row(
                        "file-scan-loading-label",
                        rust_i18n::t!("app.files.scanning").to_string(),
                    )),
            );
        } else if self.files.is_empty() {
            let (title, description) = if self.workspace.is_some() {
                (
                    rust_i18n::t!("app.files.empty.title").to_string(),
                    rust_i18n::t!("app.files.empty.description").to_string(),
                )
            } else {
                (
                    rust_i18n::t!("app.files.no_workspace.title").to_string(),
                    rust_i18n::t!("app.files.no_workspace.description").to_string(),
                )
            };
            body = body.child(
                div()
                    .id("file-list-empty")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(title.clone())
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(empty_state(title, description, IconName::Folder)),
            );
        } else if searching && self.file_search_hit_count == Some(0) {
            let note = files_panel::NO_MATCHING_FILES_NOTE.to_string();
            body = body.child(
                div()
                    .id("file-list-filter-empty")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(note.clone())
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_3()
                    .text_sm()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(note),
            );
        } else {
            body = body.child(
                div()
                    .id("file-list")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .on_action(
                        cx.listener(|this, _: &OpenSelectedFile, window, cx| this.open_selected_file(window, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &RenameSelectedFile, window, cx| this.rename_selected_file(window, cx)),
                    )
                    .on_action(
                        cx.listener(|this, _: &DeleteSelectedFile, window, cx| this.delete_selected_file(window, cx)),
                    )
                    .child(
                        Tree::new(&self.file_tree, move |_index, entry, selected, _window, app| {
                            let rel_path = entry.item().id.to_string();
                            let is_dir = directory_paths.contains(&rel_path);
                            let display_name = rel_path.rsplit('/').next().unwrap_or(&rel_path).to_string();
                            // Ranked hits from different directories share
                            // names; the muted parent keeps them distinct.
                            let search_parent = if searching {
                                rel_path.rsplit_once('/').map(|(dir, _)| dir)
                            } else {
                                None
                            };
                            let icon = files_panel::file_tree_row_icon(is_dir, entry.is_expanded());
                            let tree = file_tree.clone();
                            let view = weak_view.clone();
                            let open_path = rel_path.clone();
                            let drag_root = drag_root.clone();
                            ListItem::new(format!("workspace-file-{rel_path}"))
                                .selected(selected)
                                .cursor_pointer()
                                .accessibility_label(if is_dir {
                                    rust_i18n::t!("app.files.folder_accessibility", path = rel_path.as_str())
                                        .to_string()
                                } else {
                                    rust_i18n::t!("app.files.file_accessibility", path = rel_path.as_str()).to_string()
                                })
                                .child(files_panel::file_tree_row_label(display_name, search_parent, icon, app))
                                // Dragging a row out of the window promotes the
                                // drag to a native session, so the file can be
                                // dropped into Finder or an editor.
                                .on_drag(DragWorkspaceFile { rel_path: rel_path.clone(), is_dir }, |drag, _, _, cx| {
                                    cx.new(|_| FileDragPreview {
                                        name: drag.rel_path.rsplit('/').next().unwrap_or(&drag.rel_path).to_string(),
                                        is_dir: drag.is_dir,
                                    })
                                })
                                .external_drag_payload::<DragWorkspaceFile>(move |drag, _, _| {
                                    drag_root.as_ref().and_then(|root| {
                                        files::external_drag_payload(root, &drag.rel_path, drag.is_dir)
                                    })
                                })
                                .on_click(move |_, window, app| {
                                    tree.update(app, |tree, app| tree.focus(window, app));
                                    if is_dir {
                                        return;
                                    }
                                    if let Some(view) = view.upgrade() {
                                        view.update(app, |view, cx| {
                                            view.open_file_preview(open_path.clone(), window, cx);
                                        });
                                    }
                                })
                        })
                        .context_menu({
                            let directory_paths = menu_directory_paths.clone();
                            move |_, entry, menu, _, _| {
                                let rel_path = entry.item().id.to_string();
                                let is_dir = directory_paths.contains(&rel_path);
                                files_panel::file_row_menu(menu, &rel_path, is_dir)
                            }
                        }),
                    ),
            );
            if self.files_truncated {
                // English literal: new user-visible strings stay untranslated
                // until full native-quality coverage ships for every locale.
                let notice = format!("Showing the first {} files", files::SNAPSHOT_MAX_ENTRIES);
                body = body.child(
                    div()
                        .id("file-list-truncated")
                        .test_support()
                        .role(gpui_kit::Role::Label)
                        .aria_label(notice.clone())
                        .w_full()
                        .px_3()
                        .pb_2()
                        .text_xs()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child(notice),
                );
            }
        }
        body.into_any_element()
    }

    /// Show a selected historical capture or the newest command/tool output.
    pub(super) fn render_output_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = theme.colors;
        let mono = theme.mono_font_family.clone();
        let mut pane = inspector_tab_body().id("inspector-output-tab").test_support();
        if self.restored_activity_history_incomplete {
            pane = pane.child(div().px_3().pt_2().child(warning_alert(
                "activity-history-warning",
                rust_i18n::t!("app.inspector.partial_activity_history").to_string(),
                &colors,
            )));
        }
        let (output, tail) = self.cached_output_view();
        let Some(output) = output else {
            self.output_tail_len = 0;
            return pane
                .child(empty_state(
                    rust_i18n::t!("app.inspector.empty_output_title").to_string(),
                    rust_i18n::t!("app.inspector.empty_output_description").to_string(),
                    IconName::Terminal,
                ))
                .into_any_element();
        };
        // Follow the live edge while the reader stays pinned: streaming
        // output only grows, so a longer tail plus a bottom-pinned viewport
        // means new content arrived below. A reader who scrolled up keeps
        // their offset; the jump button below resumes following.
        if self.output_follow_selection != self.selected_output_id {
            self.output_scroll.set_offset(point(px(0.), px(0.)));
            self.output_follow_selection = self.selected_output_id.clone();
        }
        let grown = tail.len() > self.output_tail_len;
        self.output_tail_len = tail.len();
        if output.running && grown && output_pinned_to_bottom(&self.output_scroll) {
            self.output_scroll.scroll_to_bottom();
        }
        let pinned = output_pinned_to_bottom(&self.output_scroll);
        let is_selected_output = self.selected_output_id.as_deref() == Some(output.id.as_str());
        let has_more_output = self.restored_output_cursors.contains_key(&output.id);
        let output_page_loading = self
            .output_page_pending
            .as_ref()
            .is_some_and(|pending| pending.item_id == output.id);
        let output_page_error = self
            .output_page_error
            .as_ref()
            .and_then(|(item_id, error)| (item_id == &output.id).then(|| error.clone()));
        let copy_output_id = output.id.clone();
        let copy_output_preview = output.text.clone();
        let output_header = div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_medium()
                    .child(output.title.clone()),
            )
            .when(is_selected_output, |header| {
                header
                    .child(
                        Tag::custom(colors.secondary, colors.muted_foreground, colors.border)
                            .small()
                            .rounded_full()
                            .child(rust_i18n::t!("app.inspector.history_tag").to_string()),
                    )
                    .child(
                        desktop_button("output-follow-latest")
                            .icon(IconName::Activity)
                            .ghost()
                            .small()
                            .accessibility_label(rust_i18n::t!("app.inspector.follow_latest").to_string())
                            .tooltip(rust_i18n::t!("app.inspector.follow_latest_tooltip").to_string())
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.selected_output_id = None;
                                cx.notify();
                            })),
                    )
            })
            .when(output.running, |header| {
                header.child(
                    Tag::custom(colors.secondary, colors.foreground, colors.border)
                        .small()
                        .child(rust_i18n::t!("app.inspector.status_running").to_string()),
                )
            })
            .child(
                desktop_button("copy-output")
                    .icon(IconName::Copy)
                    .accessibility_label(if has_more_output {
                        rust_i18n::t!("app.inspector.copy_loaded").to_string()
                    } else {
                        rust_i18n::t!("app.inspector.copy_output").to_string()
                    })
                    .tooltip(if has_more_output {
                        rust_i18n::t!("app.inspector.copy_loaded_notice").to_string()
                    } else {
                        rust_i18n::t!("app.inspector.copy_captured").to_string()
                    })
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |view, _, window, cx| {
                        let text = restored_output_for_copy(
                            &copy_output_id,
                            &copy_output_preview,
                            &view.restored_output_pages,
                            has_more_output,
                        );
                        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
                        super::super::notifications::copied_to_clipboard(window, cx);
                    })),
            );
        pane = pane.child(output_header);
        if has_more_output || output_page_loading || output_page_error.is_some() {
            let output_id = output.id.clone();
            let mut recovery = div().w_full().px_3().py_1().flex().items_center().gap_2();
            if has_more_output {
                recovery = recovery.child(
                    desktop_button("load-more-output")
                        .label(if output_page_loading {
                            rust_i18n::t!("app.inspector.loading_output").to_string()
                        } else {
                            rust_i18n::t!("app.inspector.load_more_output").to_string()
                        })
                        .ghost()
                        .small()
                        .accessibility_label(if output_page_loading {
                            rust_i18n::t!("app.inspector.loading_more_output").to_string()
                        } else {
                            rust_i18n::t!("app.inspector.load_more_output").to_string()
                        })
                        .desktop_disabled(self.output_page_pending.is_some())
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.request_more_captured_output(&output_id, cx);
                        })),
                );
            }
            if output_page_loading {
                recovery = recovery.child(
                    div()
                        .id("output-page-loading")
                        .role(gpui_kit::Role::Status)
                        .text_xs()
                        .text_color(colors.muted_foreground)
                        .child(shimmer_label(
                            "output-page-loading-label",
                            rust_i18n::t!("app.inspector.reading_next_page").to_string(),
                        )),
                );
            }
            if let Some(error) = output_page_error {
                recovery = recovery.child(warning_alert("output-page-error", error, &colors));
            }
            pane = pane.child(recovery);
        }
        // Line filter over the loaded tail; follow-tracking above keeps using
        // the unfiltered tail so live growth still pins while filtered.
        let filter_query = self.output_filter_input.read(cx).value().trim().to_string();
        let filtering = !filter_query.is_empty();
        let (shown_text, matched_lines, total_lines) = if filtering {
            let (text, matched, total) = inspector::filter_output_lines(&tail, &filter_query);
            (gpui_kit::SharedString::from(text), matched, total)
        } else {
            (tail.clone(), 0, 0)
        };
        let filter_count = filtering.then(|| {
            rust_i18n::t!("app.inspector.filtered_count", shown = matched_lines, total = total_lines).to_string()
        });
        pane = pane.child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_1()
                .child(
                    Input::new(&self.output_filter_input)
                        .id("output-filter")
                        .flex_1()
                        .min_w_0()
                        .small()
                        .aria_label(rust_i18n::t!("app.inspector.filter_output_lines").to_string())
                        .prefix(Icon::new(IconName::Search).size_4())
                        .cleanable(true),
                )
                .when_some(filter_count, |row, count| {
                    row.child(
                        div()
                            .id("output-filter-count")
                            .test_support()
                            .role(gpui_kit::Role::Label)
                            .aria_label(count.clone())
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(count),
                    )
                }),
        );
        let body_content = if filtering && matched_lines == 0 {
            let note = rust_i18n::t!("app.inspector.no_matching_output_lines").to_string();
            div()
                .id("output-filter-empty")
                .test_support()
                .role(gpui_kit::Role::Status)
                .aria_label(note.clone())
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .p_3()
                .text_sm()
                .text_color(colors.muted_foreground)
                .child(note)
                .into_any_element()
        } else {
            render_output_sections("output-body".to_string(), &shown_text, &colors, markdown_style(cx))
                .into_any_element()
        };
        pane.child(
            div()
                .relative()
                .w_full()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .id("output-body")
                        .lock_scroll_axis()
                        .test_support()
                        .role(gpui_kit::Role::Label)
                        .aria_label(shown_text.clone())
                        .flex_1()
                        .min_h_0()
                        .track_scroll(&self.output_scroll)
                        .overflow_y_scroll()
                        .vertical_scrollbar(&self.output_scroll)
                        .p_3()
                        .text_xs()
                        .font_family(mono)
                        .text_size(theme.mono_font_size)
                        .child(body_content),
                )
                .when(!pinned, |wrap| {
                    // Bottom-centered overlay in the transcript jump-button
                    // shape: icon-only circle, label carried by tooltip and
                    // accessible name.
                    let label = rust_i18n::t!("app.inspector.jump_latest").to_string();
                    wrap.child(
                        div().absolute().left_0().right_0().bottom_2().flex().justify_center().child(
                            desktop_button("output-jump-latest")
                                .icon(IconName::ArrowDown)
                                .accessibility_label(label.clone())
                                .tooltip(label)
                                .secondary()
                                .rounded(theme.radius_full())
                                .border_1()
                                .border_color(colors.border)
                                .text_color(colors.foreground)
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.output_scroll.scroll_to_bottom();
                                    cx.notify();
                                })),
                        ),
                    )
                }),
        )
        .into_any_element()
    }
}

/// Whether the Output pane sits at its live edge. The scroll offset runs
/// from 0 (top) to `-max` (bottom), so the remaining distance fits a pixel.
fn output_pinned_to_bottom(scroll: &ScrollHandle) -> bool {
    scroll.offset().y + scroll.max_offset().y <= px(1.)
}
