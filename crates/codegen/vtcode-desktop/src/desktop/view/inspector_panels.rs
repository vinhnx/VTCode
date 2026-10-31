//! Inspector Files, Output, and Activity panels.

use super::*;

/// One nonzero status line in the activity summary legend.
struct ActivityLegendEntry {
    state: ActivityState,
    label: String,
    count: usize,
}

/// Stable per-status slug for activity-summary legend element ids.
fn activity_state_slug(state: ActivityState) -> &'static str {
    match state {
        ActivityState::Running => "running",
        ActivityState::Completed => "done",
        ActivityState::Failed => "failed",
        ActivityState::Interrupted => "interrupted",
        ActivityState::Other => "other",
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
        let mut body = div().w_full().flex_1().min_h_0().flex().flex_col();
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
        let directory_paths = self.file_directory_paths.clone();
        let menu_directory_paths = directory_paths.clone();
        let file_tree = self.file_tree.clone();
        let weak_view = cx.entity().downgrade();
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
                    .child(shimmer_label("file-scan-loading-label", rust_i18n::t!("app.files.scanning").to_string())),
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
                    .child(
                        Tree::new(&self.file_tree, move |_index, entry, selected, _window, app| {
                            let rel_path = entry.item().id.to_string();
                            let is_dir = directory_paths.contains(&rel_path);
                            let display_name = rel_path.rsplit('/').next().unwrap_or(&rel_path).to_string();
                            let icon = if is_dir {
                                if entry.is_expanded() {
                                    IconName::FolderOpen
                                } else {
                                    IconName::FolderClosed
                                }
                            } else {
                                IconName::FileText
                            };
                            let tree = file_tree.clone();
                            let view = weak_view.clone();
                            let open_path = rel_path.clone();
                            ListItem::new(format!("workspace-file-{rel_path}"))
                                .selected(selected)
                                .cursor_pointer()
                                .accessibility_label(if is_dir {
                                    rust_i18n::t!("app.files.folder_accessibility", path = rel_path.as_str())
                                        .to_string()
                                } else {
                                    rust_i18n::t!("app.files.file_accessibility", path = rel_path.as_str()).to_string()
                                })
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_x_2()
                                        .child(Icon::new(icon).small().text_color(app.theme().muted_foreground))
                                        .child(div().text_sm().child(display_name)),
                                )
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
                                let menu = menu.menu(
                                    "Copy path",
                                    Box::new(files_panel::CopyFilePath { path: rel_path.clone().into() }),
                                );
                                if directory_paths.contains(&rel_path) {
                                    menu
                                } else {
                                    menu.menu(
                                        "Open preview",
                                        Box::new(files_panel::OpenFilePath { path: rel_path.clone().into() }),
                                    )
                                }
                            }
                        }),
                    ),
            );
        }
        body.into_any_element()
    }

    /// Show a selected historical capture or the newest command/tool output.
    pub(super) fn render_output_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let colors = theme.colors;
        let mono = theme.mono_font_family.clone();
        let mut pane = div().w_full().flex_1().min_h_0().flex().flex_col();
        if self.restored_activity_history_incomplete {
            pane = pane.child(div().px_3().pt_2().child(warning_alert(
                "activity-history-warning",
                rust_i18n::t!("app.inspector.partial_activity_history").to_string(),
                &colors,
            )));
        }
        let (output, tail) = self.cached_output_view();
        let Some(output) = output else {
            return pane
                .child(empty_state(
                    rust_i18n::t!("app.inspector.empty_output_title").to_string(),
                    rust_i18n::t!("app.inspector.empty_output_description").to_string(),
                    IconName::Terminal,
                ))
                .into_any_element();
        };
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
        pane.child(
            div()
                .id("output-body")
                .lock_scroll_axis()
                .test_support()
                .role(gpui_kit::Role::Label)
                .aria_label(tail.clone())
                .flex_1()
                .min_h_0()
                .track_scroll(&self.output_scroll)
                .overflow_y_scroll()
                .vertical_scrollbar(&self.output_scroll)
                .p_3()
                .text_xs()
                .font_family(mono)
                .text_size(theme.mono_font_size)
                .child(tail),
        )
        .into_any_element()
    }

    pub(super) fn render_activity_tab(
        &mut self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let activity = self.cached_activity_projection();
        let counts = ActivityCounts::from_rows(&activity.rows);
        let has_older_activity = activity.has_older;
        let mut body = div()
            .id("activity-list")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap_2()
            .p_3();
        if self.restored_activity_history_incomplete {
            body = body.child(warning_alert(
                "activity-history-warning",
                rust_i18n::t!("app.inspector.partial_activity_history").to_string(),
                colors,
            ));
        }
        if activity.rows.is_empty() {
            return body
                .child(empty_state(
                    rust_i18n::t!(i18n::INSPECTOR_EMPTY_ACTIVITY_TITLE).to_string(),
                    rust_i18n::t!("app.inspector.empty_activity_description").to_string(),
                    IconName::Activity,
                ))
                .into_any_element();
        }
        let summary = counts.summary();
        let legend = [
            ActivityLegendEntry {
                state: ActivityState::Running,
                label: rust_i18n::t!("app.inspector.status_running").to_string(),
                count: counts.running,
            },
            ActivityLegendEntry {
                state: ActivityState::Completed,
                label: rust_i18n::t!("app.inspector.status_done").to_string(),
                count: counts.completed,
            },
            ActivityLegendEntry {
                state: ActivityState::Failed,
                label: rust_i18n::t!("app.inspector.status_failed").to_string(),
                count: counts.failed,
            },
            ActivityLegendEntry {
                state: ActivityState::Interrupted,
                label: rust_i18n::t!("app.inspector.status_interrupted").to_string(),
                count: counts.interrupted,
            },
            ActivityLegendEntry {
                state: ActivityState::Other,
                label: rust_i18n::t!("app.inspector.status_other").to_string(),
                count: counts.other,
            },
        ]
        .into_iter()
        .filter(|entry| entry.count > 0)
        .collect::<Vec<_>>();
        body = body.child(
            div()
                .id("activity-status-chart")
                .test_support()
                .role(gpui_kit::Role::Group)
                .aria_label({
                    let older = if has_older_activity {
                        rust_i18n::t!("app.inspector.older_excluded").to_string()
                    } else {
                        String::new()
                    };
                    rust_i18n::t!("app.inspector.statuses_label", summary = summary.as_str(), older = older.as_str())
                        .to_string()
                })
                .w_full()
                .flex_shrink_0()
                .flex()
                .flex_col()
                .gap_2()
                .rounded_md()
                .border_1()
                .border_color(colors.border)
                .px_3()
                .py_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(row_title(rust_i18n::t!("app.inspector.recent_activity").to_string(), colors)),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(rust_i18n::t!("app.inspector.shown_count", count = counts.total()).to_string()),
                        ),
                )
                .child(div().flex().flex_col().gap_1().children(legend.into_iter().map(|entry| {
                    div()
                        .id(format!("activity-summary-{}", activity_state_slug(entry.state)))
                        .test_support()
                        .aria_label(format!("{}: {}", entry.label, entry.count))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .size(px(8.))
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(Self::activity_status_color(entry.state, *colors)),
                        )
                        .child(div().flex_1().min_w_0().truncate().text_sm().child(entry.label))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_sm()
                                .font_medium()
                                .text_color(colors.foreground)
                                .child(entry.count.to_string()),
                        )
                }))),
        );

        if has_older_activity {
            let notice = rust_i18n::t!("app.activity.history.notice", count = inspector::MAX_ACTIVITY_ROWS).to_string();
            let search_label = rust_i18n::t!("app.activity.history.search").to_string();
            body = body.child(
                div()
                    .id("activity-history-notice")
                    .test_support()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_md()
                    .bg(colors.secondary)
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .id("activity-history-status")
                            .test_support()
                            .role(gpui_kit::Role::Status)
                            .aria_label(notice.clone())
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(notice),
                    )
                    .child(
                        desktop_button("activity-search-history")
                            .icon(IconName::Search)
                            .ghost()
                            .small()
                            .accessibility_label(search_label.clone())
                            .tooltip(search_label)
                            .desktop_disabled(self.selected_session_id.is_none())
                            .on_click(cx.listener(|view, _, window, cx| {
                                if let Some(session_id) = view.selected_session_id.clone() {
                                    view.open_conversation_search(session_id, window, cx);
                                }
                            })),
                    ),
            );
        }

        let activity_row_height = scale_row_height_for_ui_font(ACTIVITY_ROW_HEIGHT, cx.theme().font_size);
        let item_sizes = self.activity_row_sizes.get(activity.rows.len(), activity_row_height);
        let activity_count = activity.rows.len();
        let activity_rows = Rc::clone(&activity);
        let row_colors = *colors;
        let weak_view = cx.entity().downgrade();
        body = body.child(
            div()
                .id("activity-rows")
                .test_support()
                .role(gpui_kit::Role::List)
                .aria_label(rust_i18n::t!("app.inspector.recent_activity").to_string())
                .relative()
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(
                    v_virtual_list(cx.entity(), "activity-virtual-list", item_sizes, move |_, visible_range, _, _| {
                        visible_range
                            .map(|index| {
                                activity_rows.rows.get(index).map_or_else(
                                    || div().into_any_element(),
                                    |row| {
                                        let output_action = row.has_output.then(|| {
                                            let view = weak_view.clone();
                                            let output_id = row.id.clone();
                                            let output_title = row.title.to_string();
                                            desktop_button(format!("activity-output-{}", row.id))
                                                .icon(IconName::Terminal)
                                                .ghost()
                                                .small()
                                                .accessibility_label(
                                                    rust_i18n::t!(
                                                        "app.inspector.view_output_for",
                                                        title = output_title.as_str()
                                                    )
                                                    .to_string(),
                                                )
                                                .tooltip(
                                                    rust_i18n::t!("app.inspector.view_captured_output").to_string(),
                                                )
                                                .on_click(move |_, window, app| {
                                                    if let Some(view) = view.upgrade() {
                                                        view.update(app, |view, cx| {
                                                            view.select_activity_output(output_id.clone(), window, cx);
                                                        });
                                                    }
                                                })
                                                .into_any_element()
                                        });
                                        Self::render_activity_row(
                                            row,
                                            row_colors,
                                            activity_row_height,
                                            index + 1,
                                            activity_count,
                                            output_action,
                                        )
                                        .into_any_element()
                                    },
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .track_scroll(&self.activity_scroll),
                )
                .child(Scrollbar::vertical(&self.activity_scroll)),
        );
        body.into_any_element()
    }

    pub(super) fn render_activity_status_indicator(
        id_prefix: &str,
        row_id: &str,
        state: ActivityState,
        color: gpui_kit::Hsla,
    ) -> AnyElement {
        if state == ActivityState::Running {
            div()
                .id(format!("{id_prefix}-{row_id}-spinner"))
                .test_support()
                .flex()
                .items_center()
                .justify_center()
                .child(Spinner::new().small().color(color))
                .into_any_element()
        } else {
            let icon = match state {
                ActivityState::Completed => IconName::Check,
                ActivityState::Failed | ActivityState::Interrupted => IconName::X,
                ActivityState::Running | ActivityState::Other => IconName::Activity,
            };
            Icon::new(icon).small().text_color(color).into_any_element()
        }
    }

    pub(super) fn activity_status_color(
        state: ActivityState,
        colors: gpui_kit::component::theme::ThemeColor,
    ) -> gpui_kit::Hsla {
        match state {
            ActivityState::Running => colors.primary,
            ActivityState::Completed => colors.success,
            ActivityState::Failed => colors.danger,
            ActivityState::Interrupted => colors.warning,
            // Subdued rows still carry small text, so they use the brighter
            // secondary ramp instead of the caption-grade muted floor.
            ActivityState::Other => themes::muted_text(&colors),
        }
    }

    fn render_activity_row(
        row: &ActivityRow,
        colors: gpui_kit::component::theme::ThemeColor,
        row_height: f32,
        position: usize,
        set_size: usize,
        output_action: Option<AnyElement>,
    ) -> impl IntoElement {
        let status_color = Self::activity_status_color(row.state, colors);
        let status = gpui_kit::SharedString::from(row.status.as_str());
        let mut row_element = div()
            .id(format!("activity-{}", row.id))
            .test_support()
            .role(gpui_kit::Role::ListItem)
            .aria_label(row.title.clone())
            .aria_description(status.clone())
            .aria_position_in_set(position)
            .aria_size_of_set(set_size)
            .w_full()
            .h(px(row_height))
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(colors.secondary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.))
                    .flex_shrink_0()
                    .rounded_full()
                    .bg(colors.popover)
                    .child(Self::render_activity_status_indicator(
                        "activity-indicator",
                        &row.id,
                        row.state,
                        status_color,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .child(div().text_sm().font_medium().truncate().child(row.title.clone()))
                    .child(div().text_xs().truncate().text_color(themes::muted_text(&colors)).child(status)),
            );
        if let Some(output_action) = output_action {
            row_element = row_element.child(div().flex_shrink_0().child(output_action));
        }
        row_element
    }
}
