//! Inspector Activity panel.

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

/// Whether the Activity pane sits at its live edge. Newest rows lead, so the
/// edge is the top: the offset runs from 0 (top) toward `-max` (bottom).
fn activity_pinned_to_top(scroll: &VirtualListScrollHandle) -> bool {
    scroll.offset().y + px(1.) >= px(0.)
}

impl DesktopView {
    pub(in super::super) fn render_activity_tab(
        &mut self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let activity = self.cached_activity_projection();
        let counts = ActivityCounts::from_rows(&activity.rows);
        let has_older_activity = activity.has_older;
        // The legend always reflects the full projection so its counts stay
        // stable; only the rendered rows narrow to the selected status.
        let filter = self.activity_filter;
        let visible: Rc<Vec<usize>> = Rc::new(
            activity
                .rows
                .iter()
                .enumerate()
                .filter(|(_, row)| filter.is_none_or(|state| state == row.state))
                .map(|(index, _)| index)
                .collect(),
        );
        let mut body = inspector_tab_body()
            .id("activity-list")
            .test_support()
            .overflow_hidden()
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
        let shown_count = if filter.is_some() {
            rust_i18n::t!("app.inspector.filtered_count", shown = visible.len(), total = counts.total()).to_string()
        } else {
            rust_i18n::t!("app.inspector.shown_count", count = counts.total()).to_string()
        };
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
        // The active filter stays rendered at zero so it never strands: the
        // toggle that set it is always the toggle that clears it.
        .filter(|entry| entry.count > 0 || filter == Some(entry.state))
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
                .px_2()
                .py_1()
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
                                .id("activity-shown-count")
                                .test_support()
                                .role(gpui_kit::Role::Label)
                                .aria_label(shown_count.clone())
                                .flex_shrink_0()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child(shown_count),
                        ),
                )
                .child(div().flex().flex_col().gap_1().children(legend.into_iter().map(|entry| {
                    let state = entry.state;
                    let active = filter == Some(state);
                    let name = format!("{}: {}", entry.label, entry.count);
                    let tooltip = if active {
                        rust_i18n::t!("app.inspector.show_all_activity").to_string()
                    } else {
                        rust_i18n::t!("app.inspector.filter_by_status", status = entry.label.as_str()).to_string()
                    };
                    desktop_button(format!("activity-summary-{}", activity_state_slug(state)))
                        .child(
                            div()
                                .w_full()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .size(px(8.))
                                        .flex_shrink_0()
                                        .rounded_full()
                                        .bg(Self::activity_status_color(state, *colors)),
                                )
                                .child(div().flex_1().min_w_0().truncate().text_sm().child(entry.label))
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_sm()
                                        .font_medium()
                                        .text_color(colors.foreground)
                                        .child(entry.count.to_string()),
                                ),
                        )
                        .accessibility_label(name)
                        .tooltip(tooltip)
                        .toggled(active)
                        .selected(active)
                        .ghost()
                        .small()
                        .w_full()
                        .justify_start()
                        .on_click(cx.listener(move |view, _, _, cx| {
                            view.toggle_activity_filter(state);
                            cx.notify();
                        }))
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

        // Follow the live edge while the reader stays pinned: newest rows lead,
        // so a changed head means entries arrived above. A reader who scrolled
        // down keeps their offset; the jump button below resumes following.
        let head_id = visible
            .first()
            .and_then(|index| activity.rows.get(*index))
            .map(|row| row.id.clone());
        if self.activity_follow_filter != self.activity_filter {
            self.activity_follow_filter = self.activity_filter;
            self.activity_seen_head = head_id.clone();
            self.activity_scroll.scroll_to_item(0, gpui_kit::ScrollStrategy::Top);
        } else if self.activity_seen_head != head_id {
            self.activity_seen_head = head_id.clone();
            if activity_pinned_to_top(&self.activity_scroll) {
                self.activity_scroll.scroll_to_item(0, gpui_kit::ScrollStrategy::Top);
            }
        }
        let pinned = activity_pinned_to_top(&self.activity_scroll);
        let activity_row_height = scale_row_height_for_ui_font(ACTIVITY_ROW_HEIGHT, cx.theme().font_size);
        let item_sizes = self.activity_row_sizes.get(visible.len(), activity_row_height);
        let activity_count = visible.len();
        let activity_rows = Rc::clone(&activity);
        let visible_rows = Rc::clone(&visible);
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
                                visible_rows
                                    .get(index)
                                    .and_then(|row_index| activity_rows.rows.get(*row_index))
                                    .map_or_else(
                                        || div().into_any_element(),
                                        |row| {
                                            let jump_action = row.in_transcript.then(|| {
                                                let view = weak_view.clone();
                                                let jump_id = row.id.clone();
                                                let jump_title = row.title.to_string();
                                                desktop_button(format!("activity-jump-{}", row.id))
                                                    .icon(IconName::MessageSquare)
                                                    .ghost()
                                                    .small()
                                                    .accessibility_label(
                                                        rust_i18n::t!(
                                                            "app.inspector.jump_to_message_for",
                                                            title = jump_title.as_str()
                                                        )
                                                        .to_string(),
                                                    )
                                                    .tooltip(rust_i18n::t!("app.inspector.jump_to_message").to_string())
                                                    .on_click(move |_, window, app| {
                                                        if let Some(view) = view.upgrade() {
                                                            view.update(app, |view, cx| {
                                                                let _jumped = view.jump_to_transcript_item(
                                                                    jump_id.as_str(),
                                                                    window,
                                                                    cx,
                                                                );
                                                            });
                                                        }
                                                    })
                                                    .into_any_element()
                                            });
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
                                                                view.select_activity_output(
                                                                    output_id.clone(),
                                                                    window,
                                                                    cx,
                                                                );
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
                                                jump_action,
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
                .child(Scrollbar::vertical(&self.activity_scroll))
                .when(!pinned, |wrap| {
                    // Top-centered overlay in the Output jump-button shape:
                    // icon-only circle, label carried by tooltip and accessible
                    // name. Newest rows lead, so the live edge is the top.
                    let label = rust_i18n::t!("app.inspector.jump_latest_activity").to_string();
                    wrap.child(
                        div().absolute().left_0().right_0().top_2().flex().justify_center().child(
                            desktop_button("activity-jump-latest")
                                .icon(IconName::ArrowUp)
                                .accessibility_label(label.clone())
                                .tooltip(label)
                                .secondary()
                                .rounded(cx.theme().radius_full())
                                .border_1()
                                .border_color(colors.border)
                                .text_color(colors.foreground)
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.activity_scroll.scroll_to_item(0, gpui_kit::ScrollStrategy::Top);
                                    cx.notify();
                                })),
                        ),
                    )
                }),
        );
        body.into_any_element()
    }

    pub(in super::super) fn render_activity_status_indicator(
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

    pub(in super::super) fn activity_status_color(
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

    /// Transcript tool-row title tone: finished work drops to the secondary
    /// ramp so primary prose keeps the column, while live rows stay at full
    /// foreground. The status indicator keeps its semantic color either way,
    /// so state is never carried by the title tone alone.
    pub(in super::super) fn tool_row_title_color(
        state: ActivityState,
        colors: gpui_kit::component::theme::ThemeColor,
    ) -> gpui_kit::Hsla {
        if state == ActivityState::Completed {
            themes::muted_text(&colors)
        } else {
            colors.foreground
        }
    }

    fn render_activity_row(
        row: &ActivityRow,
        colors: gpui_kit::component::theme::ThemeColor,
        row_height: f32,
        position: usize,
        set_size: usize,
        jump_action: Option<AnyElement>,
        output_action: Option<AnyElement>,
    ) -> impl IntoElement {
        let status_color = Self::activity_status_color(row.state, colors);
        let status = gpui_kit::SharedString::from(row.status.as_str());
        let mut row_element =
            div()
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
                .gap_2()
                .px_2()
                .py_1()
                .child(div().flex().items_center().justify_center().flex_shrink_0().child(
                    Self::render_activity_status_indicator("activity-indicator", &row.id, row.state, status_color),
                ))
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
        if let Some(jump_action) = jump_action {
            row_element = row_element.child(div().flex_shrink_0().child(jump_action));
        }
        if let Some(output_action) = output_action {
            row_element = row_element.child(div().flex_shrink_0().child(output_action));
        }
        row_element
    }
}
