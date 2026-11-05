//! Storage settings: usage overview plus session and worktree cleanup.

use super::*;
use gpui_kit::component::chart::BarChart;
use gpui_kit::component::plot::shape::BarAlignment;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum StorageView {
    #[default]
    Summary,
    ReviewSessions,
    ManageWorktrees,
}

#[derive(Clone, Debug, Default)]
pub(super) struct StorageState {
    pub(super) view: StorageView,
    pub(super) summary: Option<protocol::StorageSummary>,
    pub(super) loading: bool,
    pub(super) error: Option<String>,
    pub(super) worktrees: Vec<protocol::WorktreeEntry>,
    pub(super) worktrees_loading: bool,
    pub(super) worktrees_error: Option<String>,
}

/// Human-readable byte count: "0 bytes", "1 byte", "1.6 GB".
pub(super) fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return if bytes == 1 {
            rust_i18n::t!("app.settings.storage.byte_one").to_string()
        } else {
            rust_i18n::t!("app.settings.storage.bytes_many", n = bytes).to_string()
        };
    }
    let unit = (bytes.ilog(1024) as usize).min(UNITS.len() - 1);
    #[expect(clippy::cast_possible_truncation, reason = "unit is clamped to 0..=4 by min() above")]
    let divisor = 1024u64.pow(unit as u32);
    let whole = bytes / divisor;
    let tenths = (bytes % divisor) * 10 / divisor;
    #[expect(clippy::indexing_slicing, reason = "unit is clamped to UNITS indices by min() above")]
    let suffix = UNITS[unit];
    if tenths == 0 {
        format!("{whole} {suffix}")
    } else {
        format!("{whole}.{tenths} {suffix}")
    }
}

fn format_archive_date(updated_at: Option<u64>) -> String {
    updated_at
        .and_then(|seconds| i64::try_from(seconds).ok())
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "—".to_string())
}

/// Sessions oldest first so cleanup candidates lead; unknown dates sort last.
pub(super) fn review_order(sessions: &[SessionSummary]) -> Vec<&SessionSummary> {
    let mut ordered: Vec<_> = sessions.iter().collect();
    ordered.sort_by_key(|session| session.updated_at.unwrap_or(u64::MAX));
    ordered
}

impl DesktopView {
    pub(super) fn render_storage_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.storage.view {
            StorageView::Summary => self.render_storage_summary(cx),
            StorageView::ReviewSessions => self.render_storage_review(cx),
            StorageView::ManageWorktrees => self.render_storage_worktrees(cx),
        }
    }

    pub(super) fn maybe_request_storage_summary(&mut self, cx: &mut Context<Self>) {
        if self.storage.summary.is_none() && !self.storage.loading {
            self.request_storage_summary(cx);
        }
    }

    pub(super) fn request_storage_summary(&mut self, cx: &mut Context<Self>) {
        self.storage.loading = true;
        self.storage.error = None;
        let _ = self.request(protocol::method::STORAGE_SUMMARY, json!({}));
        cx.notify();
    }

    /// Storage-scoped summary error: the inline error carries it; the
    /// generic banner would duplicate it away from the tab.
    pub(super) fn fail_storage_summary(&mut self, error: String, cx: &mut Context<Self>) {
        self.storage.loading = false;
        self.storage.error = Some(error);
        cx.notify();
    }

    pub(super) fn fail_storage_worktrees(&mut self, error: String, cx: &mut Context<Self>) {
        self.storage.worktrees_loading = false;
        self.storage.worktrees_error = Some(error);
        cx.notify();
    }

    pub(super) fn apply_storage_summary(&mut self, result: Value) {
        match serde_json::from_value::<protocol::StorageSummary>(result) {
            Ok(summary) => {
                self.storage.summary = Some(summary);
                self.storage.loading = false;
                self.storage.error = None;
            }
            Err(_) => {
                self.storage.loading = false;
                self.storage.error = Some(rust_i18n::t!("app.service.invalid_summary").to_string());
            }
        }
    }

    pub(super) fn apply_session_delete(&mut self, result: Value, cx: &mut Context<Self>) {
        match serde_json::from_value::<protocol::SessionDeleteResult>(result) {
            Ok(outcome) => {
                self.sessions.retain(|session| session.session_id != outcome.session_id);
                self.remove_tab(&outcome.session_id);
                self.request_storage_summary(cx);
            }
            Err(_) => {
                self.storage.error = Some(rust_i18n::t!("app.service.invalid_delete").to_string());
            }
        }
    }

    pub(super) fn apply_worktree_list(&mut self, result: Value) {
        match serde_json::from_value::<Vec<protocol::WorktreeEntry>>(result) {
            Ok(entries) => {
                self.storage.worktrees = entries;
                self.storage.worktrees_loading = false;
                self.storage.worktrees_error = None;
            }
            Err(_) => {
                self.storage.worktrees_loading = false;
                self.storage.worktrees_error = Some(rust_i18n::t!("app.service.invalid_worktrees").to_string());
            }
        }
    }

    pub(super) fn apply_worktree_remove(&mut self, result: Value, cx: &mut Context<Self>) {
        match serde_json::from_value::<protocol::WorktreeRemoveResult>(result) {
            Ok(outcome) => {
                self.storage.worktrees.retain(|entry| entry.name != outcome.name);
                self.request_storage_summary(cx);
            }
            Err(_) => {
                self.storage.worktrees_error = Some(rust_i18n::t!("app.service.invalid_remove").to_string());
            }
        }
    }

    fn open_storage_review(&mut self, cx: &mut Context<Self>) {
        self.storage.view = StorageView::ReviewSessions;
        self.storage.error = None;
        // Deletion review manages the full set, not the visible page.
        let _queued = self.request_session_list(protocol::MAX_SESSION_LIST_LIMIT);
        cx.notify();
    }

    fn open_storage_worktrees(&mut self, cx: &mut Context<Self>) {
        self.storage.view = StorageView::ManageWorktrees;
        self.storage.worktrees_loading = true;
        self.storage.worktrees_error = None;
        let _ = self.request(protocol::method::WORKTREE_LIST, json!({}));
        cx.notify();
    }

    fn back_to_storage_summary(&mut self, cx: &mut Context<Self>) {
        self.storage.view = StorageView::Summary;
        cx.notify();
    }

    /// Delete one thread from a context menu, reusing the storage review's
    /// warning prompt so the destructive path stays confirm-gated everywhere.
    /// The selected thread is refused outright in every state: the server
    /// rejects selected-session deletes, so prompting would only bait a
    /// failure.
    pub(super) fn confirm_delete_thread(&mut self, session_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_session_id.as_deref() == Some(session_id) {
            self.error = Some(rust_i18n::t!("app.session.delete_selected_blocked").to_string());
            cx.notify();
            return;
        }
        let title = self
            .sessions
            .iter()
            .find(|session| session.session_id == session_id)
            .and_then(|session| session.title.clone());
        self.confirm_delete_session(
            session_id.to_string(),
            session_display_title(session_id, title.as_deref()),
            window,
            cx,
        );
    }

    fn confirm_delete_session(&self, session_id: String, title: String, window: &mut Window, cx: &mut Context<Self>) {
        let title = rust_i18n::t!("app.settings.storage.delete_title", name = title).to_string();
        let detail = rust_i18n::t!("app.settings.storage.delete_detail").to_string();
        let delete = rust_i18n::t!("app.settings.storage.delete").to_string();
        let cancel = rust_i18n::t!("app.settings.storage.cancel").to_string();
        let answer = window.prompt(
            gpui_kit::PromptLevel::Warning,
            title.as_str(),
            Some(detail.as_str()),
            &[delete.as_str(), cancel.as_str()],
            cx,
        );
        cx.spawn(async move |view, cx| {
            if matches!(answer.await, Ok(0)) {
                let _updated = view.update(cx, |view, cx| {
                    view.storage.error = None;
                    let _ = view.request(protocol::method::SESSION_DELETE, json!({ keys::SESSION_ID: session_id }));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn confirm_remove_worktree(&self, name: String, is_dirty: bool, window: &mut Window, cx: &mut Context<Self>) {
        let title = rust_i18n::t!("app.settings.storage.remove_title", name = name).to_string();
        let detail = if is_dirty {
            rust_i18n::t!("app.settings.storage.remove_dirty_detail").to_string()
        } else {
            rust_i18n::t!("app.settings.storage.remove_detail").to_string()
        };
        let remove = rust_i18n::t!("app.settings.storage.remove").to_string();
        let cancel = rust_i18n::t!("app.settings.storage.cancel").to_string();
        let answer = window.prompt(
            gpui_kit::PromptLevel::Warning,
            title.as_str(),
            Some(detail.as_str()),
            &[remove.as_str(), cancel.as_str()],
            cx,
        );
        cx.spawn(async move |view, cx| {
            if matches!(answer.await, Ok(0)) {
                let _updated = view.update(cx, |view, cx| {
                    view.storage.worktrees_error = None;
                    let _ = view.request(protocol::method::WORKTREE_REMOVE, json!({ "name": name }));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn render_storage_summary(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let device_title = rust_i18n::t!("app.settings.storage.device_title").to_string();
        let cleanup_title = rust_i18n::t!("app.settings.storage.cleanup_title").to_string();
        let worktrees_title = rust_i18n::t!("app.settings.storage.worktrees_title").to_string();
        div()
            .id("storage-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_6()
            .child(accessible_group_box(
                "storage-device-group",
                device_title.clone(),
                settings_group_box(device_title, &colors)
                    .child(row_description(
                        rust_i18n::t!("app.settings.storage.device_description").to_string(),
                        &colors,
                    ))
                    .child(self.render_storage_total(cx, &colors))
                    .child(self.render_storage_bar(cx, &colors)),
            ))
            .child(accessible_group_box(
                "storage-cleanup-group",
                cleanup_title.clone(),
                settings_group_box(cleanup_title.clone(), &colors).child(settings_row(
                    cleanup_title,
                    rust_i18n::t!("app.settings.storage.cleanup_description").to_string(),
                    desktop_button("storage-review")
                        .label(rust_i18n::t!("app.settings.storage.review").to_string())
                        .accessibility_label(rust_i18n::t!("app.settings.storage.cleanup_title").to_string())
                        .on_click(cx.listener(|view, _, _, cx| view.open_storage_review(cx))),
                    &colors,
                )),
            ))
            .child(accessible_group_box(
                "storage-worktrees-group",
                worktrees_title.clone(),
                settings_group_box(worktrees_title.clone(), &colors).child(settings_row(
                    worktrees_title,
                    rust_i18n::t!("app.settings.storage.worktrees_description").to_string(),
                    desktop_button("storage-manage-worktrees")
                        .label(rust_i18n::t!("app.settings.storage.manage").to_string())
                        .accessibility_label(rust_i18n::t!("app.settings.storage.worktrees_title").to_string())
                        .on_click(cx.listener(|view, _, _, cx| view.open_storage_worktrees(cx))),
                    &colors,
                )),
            ))
            .into_any_element()
    }

    fn render_storage_total(
        &self,
        cx: &mut Context<Self>,
        colors: &gpui_kit::component::theme::ThemeColor,
    ) -> AnyElement {
        let total = match self.storage.summary.as_ref() {
            Some(summary) => {
                rust_i18n::t!("app.settings.storage.used_by", size = format_bytes(summary.total_bytes)).to_string()
            }
            None => rust_i18n::t!("app.settings.storage.device_title").to_string(),
        };
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(row_title(total, colors))
            .child(
                desktop_button("storage-refresh")
                    .label(rust_i18n::t!("app.settings.storage.refresh").to_string())
                    .small()
                    .disabled(self.storage.loading)
                    .on_click(cx.listener(|view, _, _, cx| view.request_storage_summary(cx))),
            )
            .into_any_element()
    }

    fn render_storage_bar(
        &self,
        cx: &mut Context<Self>,
        colors: &gpui_kit::component::theme::ThemeColor,
    ) -> AnyElement {
        if self.storage.loading && self.storage.summary.is_none() {
            return div()
                .flex()
                .items_center()
                .gap_2()
                .child(Spinner::new().small().color(colors.muted_foreground))
                .child(div().text_sm().text_color(themes::muted_text(colors)).child(shimmer_label(
                    "storage-loading-label",
                    rust_i18n::t!("app.settings.storage.loading").to_string(),
                    themes::muted_text(colors),
                    cx,
                )))
                .into_any_element();
        }
        if let Some(error) = self.storage.error.as_ref() {
            let retry = desktop_button("storage-retry")
                .label(rust_i18n::t!("app.settings.storage.retry").to_string())
                .small()
                .on_click(cx.listener(|view, _, _, cx| view.request_storage_summary(cx)));
            return div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().text_sm().text_color(colors.danger).child(error.clone()))
                .child(retry)
                .into_any_element();
        }
        let Some(summary) = self.storage.summary.as_ref() else {
            return div().into_any_element();
        };
        let chart_data = [
            (rust_i18n::t!("app.settings.storage.sessions").to_string(), summary.sessions_bytes, colors.chart_1),
            (rust_i18n::t!("app.settings.storage.worktrees").to_string(), summary.worktrees_bytes, colors.chart_2),
            (rust_i18n::t!("app.settings.storage.memory").to_string(), summary.memory_bytes, colors.chart_3),
            (rust_i18n::t!("app.settings.storage.other").to_string(), summary.other_bytes, colors.chart_4),
        ];
        let breakdown = [
            (rust_i18n::t!("app.settings.storage.sessions").to_string(), summary.sessions_bytes),
            (rust_i18n::t!("app.settings.storage.worktrees").to_string(), summary.worktrees_bytes),
            (rust_i18n::t!("app.settings.storage.memory").to_string(), summary.memory_bytes),
            (rust_i18n::t!("app.settings.storage.other").to_string(), summary.other_bytes),
        ]
        .into_iter()
        .map(|(label, bytes)| format!("{label} {}", format_bytes(bytes)))
        .collect::<Vec<_>>()
        .join(", ");
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .id("storage-usage-bar")
                    .test_support()
                    .role(gpui_kit::Role::Group)
                    .aria_label(rust_i18n::t!("app.settings.storage.usage", breakdown = breakdown).to_string())
                    .w_full()
                    .when(summary.total_bytes > 0, |chart| {
                        chart.h(px(144.)).child(
                            BarChart::new(chart_data.to_vec())
                                .band(|datum| datum.0.clone())
                                .value(|datum| datum.1 as f64)
                                .alignment(BarAlignment::Left)
                                .value_axis(false)
                                .grid(false)
                                .interactive(false)
                                .appear(false)
                                .fill(|datum, _, _, _| datum.2),
                        )
                    }),
            )
            .child(
                div().flex().flex_col().gap_1().children(
                    [
                        (rust_i18n::t!("app.settings.storage.sessions").to_string(), summary.sessions_bytes),
                        (rust_i18n::t!("app.settings.storage.worktrees").to_string(), summary.worktrees_bytes),
                        (rust_i18n::t!("app.settings.storage.memory").to_string(), summary.memory_bytes),
                        (rust_i18n::t!("app.settings.storage.other").to_string(), summary.other_bytes),
                    ]
                    .into_iter()
                    .map(|(label, bytes)| {
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(div().text_sm().text_color(colors.foreground).child(label))
                            .child(
                                div()
                                    .text_sm()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_color(themes::muted_text(colors))
                                    .child(format_bytes(bytes)),
                            )
                    }),
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(themes::muted_text(colors))
                    .truncate()
                    .child(summary.state_dir.clone()),
            )
            .into_any_element()
    }

    fn render_storage_review(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let title = rust_i18n::t!("app.settings.storage.cleanup_title").to_string();
        let mut content = div()
            .id("storage-review-view")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                desktop_button("storage-back")
                    .label(rust_i18n::t!("app.settings.storage.back").to_string())
                    .ghost()
                    .small()
                    .on_click(cx.listener(|view, _, _, cx| view.back_to_storage_summary(cx))),
            )
            .child(accessible_group_box(
                "storage-review-group",
                title.clone(),
                settings_group_box(title, &colors).child(row_description(
                    rust_i18n::t!("app.settings.storage.cleanup_description").to_string(),
                    &colors,
                )),
            ));
        if let Some(error) = self.storage.error.as_ref() {
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_sm().text_color(colors.danger).child(error.clone()))
                    .child(
                        desktop_button("storage-review-retry")
                            .label(rust_i18n::t!("app.settings.storage.retry").to_string())
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| view.open_storage_review(cx))),
                    ),
            );
        }
        let sessions = review_order(&self.sessions);
        if sessions.is_empty() {
            content =
                content.child(row_description(rust_i18n::t!("app.settings.storage.no_sessions").to_string(), &colors));
        }
        content
            .child(div().flex().flex_col().gap_4().children(sessions.into_iter().map(|session| {
                let name = session
                    .title
                    .clone()
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| session.session_id.clone());
                let meta =
                    format!("{} · {}", format_archive_date(session.updated_at), format_bytes(session.size_bytes));
                settings_row(name.clone(), meta, self.storage_review_action(session, cx), &colors)
            })))
            .into_any_element()
    }

    fn storage_review_action(&self, session: &SessionSummary, cx: &mut Context<Self>) -> AnyElement {
        if self.selected_session_id.as_deref() == Some(session.session_id.as_str()) {
            return row_description(
                rust_i18n::t!("app.settings.storage.current_session").to_string(),
                &cx.theme().colors,
            )
            .into_any_element();
        }
        let session_id = session.session_id.clone();
        let name = session
            .title
            .clone()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| session.session_id.clone());
        desktop_danger_button(format!("delete-session-{session_id}"))
            .label(rust_i18n::t!("app.settings.storage.delete").to_string())
            .accessibility_label(rust_i18n::t!("app.settings.storage.delete_title", name = name).to_string())
            .small()
            .on_click(cx.listener(move |view, _, window, cx| {
                view.confirm_delete_session(session_id.clone(), name.clone(), window, cx);
            }))
            .into_any_element()
    }

    fn render_storage_worktrees(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let title = rust_i18n::t!("app.settings.storage.worktrees_title").to_string();
        let mut content = div()
            .id("storage-worktrees")
            .test_support()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                desktop_button("storage-back")
                    .label(rust_i18n::t!("app.settings.storage.back").to_string())
                    .ghost()
                    .small()
                    .on_click(cx.listener(|view, _, _, cx| view.back_to_storage_summary(cx))),
            )
            .child(accessible_group_box(
                "storage-worktrees-list-group",
                title.clone(),
                settings_group_box(title, &colors).child(row_description(
                    rust_i18n::t!("app.settings.storage.worktrees_description").to_string(),
                    &colors,
                )),
            ));
        if self.storage.worktrees_loading && self.storage.worktrees.is_empty() {
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(Spinner::new().small().color(colors.muted_foreground))
                    .child(div().text_sm().text_color(themes::muted_text(&colors)).child(shimmer_label(
                        "storage-worktrees-loading-label",
                        rust_i18n::t!("app.settings.storage.loading").to_string(),
                        themes::muted_text(&colors),
                        cx,
                    ))),
            );
        }
        if let Some(error) = self.storage.worktrees_error.as_ref() {
            content = content.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().text_sm().text_color(colors.danger).child(error.clone()))
                    .child(
                        desktop_button("storage-worktrees-retry")
                            .label(rust_i18n::t!("app.settings.storage.retry").to_string())
                            .small()
                            .on_click(cx.listener(|view, _, _, cx| view.open_storage_worktrees(cx))),
                    ),
            );
        }
        if !self.storage.worktrees_loading && self.storage.worktrees.is_empty() {
            content =
                content.child(row_description(rust_i18n::t!("app.settings.storage.no_worktrees").to_string(), &colors));
        }
        content
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .children(self.storage.worktrees.iter().map(|entry| {
                        let head = entry.head.as_deref().map(|head| head.chars().take(7).collect::<String>());
                        let meta = if entry.is_dirty {
                            rust_i18n::t!("app.settings.storage.dirty").to_string()
                        } else {
                            head.unwrap_or_else(|| "—".to_string())
                        };
                        let name = entry.name.clone();
                        let is_dirty = entry.is_dirty;
                        settings_row(
                            entry.name.clone(),
                            meta,
                            desktop_danger_button(format!("remove-worktree-{name}"))
                                .label(rust_i18n::t!("app.settings.storage.remove").to_string())
                                .accessibility_label(
                                    rust_i18n::t!("app.settings.storage.remove_title", name = name).to_string(),
                                )
                                .small()
                                .on_click(cx.listener(move |view, _, window, cx| {
                                    view.confirm_remove_worktree(name.clone(), is_dirty, window, cx);
                                })),
                            &colors,
                        )
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_scales_through_units() {
        assert_eq!(format_bytes(0), "0 bytes");
        assert_eq!(format_bytes(1), "1 byte");
        assert_eq!(format_bytes(172), "172 bytes");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(172 * 1024 * 1024 + 1024 * 103), "172.1 MB");
        assert_eq!(format_bytes(15 * 1024 * 1024 * 1024 + 1024 * 1024 * 103), "15.1 GB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024 * 1024), "2 TB");
    }

    #[test]
    fn review_order_sorts_oldest_first_with_unknown_last() {
        let sessions = [
            SessionSummary {
                session_id: "new".into(),
                title: None,
                workspace: String::new(),
                completed: true,
                resumable: true,
                size_bytes: 0,
                updated_at: Some(200),
            },
            SessionSummary {
                session_id: "unknown".into(),
                title: None,
                workspace: String::new(),
                completed: true,
                resumable: true,
                size_bytes: 0,
                updated_at: None,
            },
            SessionSummary {
                session_id: "old".into(),
                title: None,
                workspace: String::new(),
                completed: true,
                resumable: true,
                size_bytes: 0,
                updated_at: Some(100),
            },
        ];
        let ordered: Vec<_> = review_order(&sessions)
            .into_iter()
            .map(|session| session.session_id.as_str())
            .collect();
        assert_eq!(ordered, ["old", "new", "unknown"]);
    }

    #[test]
    fn format_archive_date_renders_iso_or_placeholder() {
        assert_eq!(format_archive_date(None), "—");
        assert_eq!(format_archive_date(Some(1_792_022_400)), "2026-10-15");
    }
}
