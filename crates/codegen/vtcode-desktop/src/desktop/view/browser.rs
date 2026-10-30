//! Full-window thread browser: workspace-grouped triage over cached sessions.

use super::*;

/// One flattened browser row. Group headers stay interleaved with their
/// sessions so a single uniform virtual list renders the whole browser.
/// Headers borrow their workspace from the session list through a
/// representative index instead of cloning workspace strings per render.
#[derive(Debug, PartialEq)]
enum BrowserRow {
    Group { index: usize, count: usize },
    Session { index: usize, ordinal: usize },
}

/// Flatten sessions into browser rows: current workspace first, remaining
/// groups alphabetical, sessions newest-first with unknown timestamps last.
/// Pure over borrowed sessions so unit tests pin the order without a window.
fn browser_rows(sessions: &[SessionSummary], current_workspace: Option<&str>) -> Vec<BrowserRow> {
    let mut order: Vec<usize> = (0..sessions.len()).collect();
    order.sort_by(|&a, &b| {
        // Indices originate from `0..sessions.len()`, so both lookups hold.
        let (Some(a_session), Some(b_session)) = (sessions.get(a), sessions.get(b)) else {
            return std::cmp::Ordering::Equal;
        };
        current_group_first(a_session.workspace.as_str(), b_session.workspace.as_str(), current_workspace)
            .then(a_session.workspace.cmp(&b_session.workspace))
            .then(recency_order(a_session.updated_at, b_session.updated_at))
            .then(a_session.session_id.cmp(&b_session.session_id))
    });
    let mut rows = Vec::with_capacity(order.len() + 1);
    let mut group: Option<&str> = None;
    let mut group_header = 0;
    for (ordinal, &index) in order.iter().enumerate() {
        let Some(session) = sessions.get(index) else {
            continue;
        };
        if group != Some(session.workspace.as_str()) {
            group = Some(session.workspace.as_str());
            group_header = rows.len();
            rows.push(BrowserRow::Group { index, count: 0 });
        }
        if let Some(BrowserRow::Group { count, .. }) = rows.get_mut(group_header) {
            *count += 1;
        }
        rows.push(BrowserRow::Session { index, ordinal });
    }
    rows
}

fn current_group_first(a: &str, b: &str, current: Option<&str>) -> std::cmp::Ordering {
    match current {
        Some(workspace) => (a != workspace).cmp(&(b != workspace)),
        None => std::cmp::Ordering::Equal,
    }
}

fn recency_order(a: Option<u64>, b: Option<u64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => y.cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Locale-neutral compact age ("45s", "12m", "3h", "9d") for triage lanes.
/// Unknown timestamps yield no lane rather than a wrong one, and clock skew
/// saturates to zero instead of wrapping.
fn compact_age(updated_at: Option<u64>, now_secs: u64) -> Option<String> {
    let age = now_secs.saturating_sub(updated_at?);
    let (value, unit) = match age {
        0..60 => (age, "s"),
        60..3_600 => (age / 60, "m"),
        3_600..86_400 => (age / 3_600, "h"),
        _ => (age / 86_400, "d"),
    };
    Some(format!("{value}{unit}"))
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

impl DesktopView {
    /// Full-window thread browser over the cached session list. Selecting a
    /// row loads the thread and dismisses the browser; Escape and backdrop
    /// click only dismiss.
    pub(super) fn render_thread_browser(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let title = rust_i18n::t!(i18n::BROWSER_BROWSE_THREADS).to_string();
        let row_height = scale_row_height_for_ui_font(SESSION_ROW_HEIGHT, cx.theme().font_size);
        let current = self.workspace.as_ref().map(|path| path.display().to_string());
        let now = now_unix_secs();
        let rows = browser_rows(&self.sessions, current.as_deref());
        let sizes = self.browser_row_sizes.get(rows.len(), row_height);
        let (primary, secondary, foreground, muted) =
            (colors.primary, colors.secondary, colors.foreground, colors.muted_foreground);
        let row_colors = colors;
        let can_new_thread = self.session_change_allowed(None);
        let can_close_thread = self.selected_session_id.is_some() && can_new_thread;
        let content = if self.sessions.is_empty() {
            empty_state(
                rust_i18n::t!("app.browser.empty_title").to_string(),
                rust_i18n::t!("app.browser.empty_description").to_string(),
                IconName::MessageSquare,
            )
            .into_any_element()
        } else {
            div()
                .id("thread-browser-list")
                .test_support()
                .role(gpui_kit::Role::List)
                .aria_label(rust_i18n::t!("app.workspace.recent_threads").to_string())
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .overflow_hidden()
                .child(
                    v_virtual_list(
                        cx.entity(),
                        "thread-browser-virtual-list",
                        sizes,
                        move |view, visible_range, _, cx| {
                            visible_range
                                .filter_map(|row| match rows.get(row)? {
                                    BrowserRow::Group { index, count } => {
                                        let workspace = view.sessions.get(*index)?.workspace.as_str();
                                        let name = workspace_short_name(workspace).to_string();
                                        Some(
                                            div()
                                                .id(opaque_path_element_id("browser-group", workspace))
                                                .test_support()
                                                .role(gpui_kit::Role::Group)
                                                .aria_label(name.clone())
                                                .h(px(row_height))
                                                .w_full()
                                                .flex()
                                                .items_center()
                                                .justify_between()
                                                .gap_2()
                                                .px_3()
                                                .child(eyebrow_label(name, &row_colors))
                                                .child(div().text_xs().text_color(muted).child(count.to_string()))
                                                .into_any_element(),
                                        )
                                    }
                                    BrowserRow::Session { index, ordinal } => {
                                        let session = view.sessions.get(*index)?;
                                        let session_id = session.session_id.clone();
                                        let selected = view.selected_session_id.as_deref() == Some(session_id.as_str());
                                        let label = session_display_title(&session_id, session.title.as_deref());
                                        let size_bytes = session.size_bytes;
                                        let mut meta = compact_age(session.updated_at, now).unwrap_or_default();
                                        if size_bytes > 0 {
                                            if !meta.is_empty() {
                                                meta.push_str(" · ");
                                            }
                                            meta.push_str(storage::format_bytes(size_bytes).as_str());
                                        }
                                        let enabled = view.session_change_allowed(Some(&session_id));
                                        let accessible_label = if selected {
                                            rust_i18n::t!(
                                                "app.navigation.current_thread_label",
                                                thread = label.as_str()
                                            )
                                            .to_string()
                                        } else {
                                            label.clone()
                                        };
                                        Some(
                                            div()
                                                .id(format!("browser-session-row-{session_id}"))
                                                .test_support()
                                                .role(gpui_kit::Role::ListItem)
                                                .aria_label(accessible_label.clone())
                                                .aria_position_in_set(ordinal + 1)
                                                .aria_size_of_set(view.sessions.len())
                                                .aria_selected(selected)
                                                .h(px(row_height))
                                                .w_full()
                                                .child(
                                                    desktop_button(format!("browser-session-{session_id}"))
                                                        .accessibility_label(accessible_label)
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .flex()
                                                                .items_center()
                                                                .gap_2()
                                                                .child(
                                                                    Icon::new(IconName::MessageSquare).text_color(
                                                                        if selected { primary } else { muted },
                                                                    ),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .truncate()
                                                                        .text_sm()
                                                                        .child(label),
                                                                )
                                                                .when(!meta.is_empty(), |row| {
                                                                    row.child(
                                                                        div()
                                                                            .id(format!(
                                                                                "browser-session-meta-{session_id}"
                                                                            ))
                                                                            .test_support()
                                                                            .flex_shrink_0()
                                                                            .text_xs()
                                                                            .text_color(muted)
                                                                            .child(meta),
                                                                    )
                                                                }),
                                                        )
                                                        .ghost()
                                                        .toggled(selected)
                                                        .desktop_disabled(!enabled)
                                                        .w_full()
                                                        .justify_start()
                                                        .when(selected, |button| {
                                                            button.bg(secondary).text_color(foreground)
                                                        })
                                                        .on_click(cx.listener(move |this, _, window, cx| {
                                                            if this.load_session(session_id.clone(), cx) {
                                                                this.close_browser(window, cx);
                                                            }
                                                        })),
                                                )
                                                .into_any_element(),
                                        )
                                    }
                                })
                                .collect::<Vec<_>>()
                        },
                    )
                    .track_scroll(&self.browser_scroll),
                )
                .child(Scrollbar::vertical(&self.browser_scroll))
                .into_any_element()
        };
        div()
            .absolute()
            .inset_0()
            .p_6()
            .bg(colors.background.opacity(0.85))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, cx.listener(|view, _, window, cx| view.close_browser(window, cx)))
            .child(
                div()
                    .w_full()
                    .h_full()
                    .max_w(px(1500.))
                    .child(
                        div()
                            .id("thread-browser")
                            .test_support()
                            .track_focus(&self.browser_focus)
                            .role(gpui_kit::Role::Dialog)
                            .aria_label(title.clone())
                            .size_full()
                            .flex()
                            .flex_col()
                            .rounded_xl()
                            .bg(colors.secondary)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(panel_header(
                                div().text_lg().font_semibold().child(title),
                                div()
                                    .flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        desktop_button("browser-new-session")
                                            .tooltip_with_action(
                                                rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string(),
                                                &NewSession,
                                                Some(key_context::DESKTOP),
                                            )
                                            .label(rust_i18n::t!(i18n::NAVIGATION_NEW_THREAD).to_string())
                                            .icon(IconName::Plus)
                                            .ghost()
                                            .small()
                                            .desktop_disabled(!can_new_thread)
                                            .dispatches_desktop_action(NewSession, self.focus_handle.clone()),
                                    )
                                    .child(
                                        desktop_button("browser-close-session")
                                            .tooltip_with_action(
                                                rust_i18n::t!(i18n::NAVIGATION_CLOSE_THREAD).to_string(),
                                                &CloseSession,
                                                Some(key_context::DESKTOP),
                                            )
                                            .label(rust_i18n::t!(i18n::NAVIGATION_CLOSE_THREAD).to_string())
                                            .ghost()
                                            .small()
                                            .desktop_disabled(!can_close_thread)
                                            .dispatches_desktop_action(CloseSession, self.focus_handle.clone()),
                                    )
                                    .child(
                                        desktop_button("browser-dismiss")
                                            .icon(IconName::X)
                                            .accessibility_label(rust_i18n::t!("app.browser.close").to_string())
                                            .tooltip_with_action(
                                                rust_i18n::t!("app.browser.close").to_string(),
                                                &DismissOverlay,
                                                Some(key_context::DESKTOP),
                                            )
                                            .ghost()
                                            .small()
                                            .on_click(
                                                cx.listener(|view, _, window, cx| view.close_browser(window, cx)),
                                            ),
                                    ),
                            ))
                            .child(div().flex_1().min_h_0().w_full().flex().flex_col().px_3().pb_3().child(content)),
                    )
                    .focus_trap("thread-browser-trap", &self.browser_focus),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str, workspace: &str, updated_at: Option<u64>) -> SessionSummary {
        SessionSummary {
            session_id: id.to_string(),
            title: None,
            workspace: workspace.to_string(),
            completed: false,
            resumable: true,
            size_bytes: 0,
            updated_at,
        }
    }

    #[test]
    fn rows_pin_current_workspace_first_then_sort_alphabetical() {
        let sessions = vec![
            summary("zeta-old", "/work/zeta", Some(3)),
            summary("alpha-old", "/work/alpha", Some(2)),
            summary("cur-old", "/work/current", Some(1)),
        ];
        let rows = browser_rows(&sessions, Some("/work/current"));
        let groups: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                BrowserRow::Group { index, .. } => Some(sessions[*index].workspace.as_str()),
                BrowserRow::Session { .. } => None,
            })
            .collect();
        assert_eq!(groups, vec!["/work/current", "/work/alpha", "/work/zeta"]);
    }

    #[test]
    fn rows_sort_newest_first_with_unknown_timestamps_last() {
        let sessions = vec![
            summary("unknown", "/work/main", None),
            summary("old", "/work/main", Some(10)),
            summary("new", "/work/main", Some(30)),
        ];
        let rows = browser_rows(&sessions, None);
        let order: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                BrowserRow::Session { index, .. } => Some(sessions[*index].session_id.as_str()),
                BrowserRow::Group { .. } => None,
            })
            .collect();
        assert_eq!(order, vec!["new", "old", "unknown"]);
    }

    #[test]
    fn rows_break_recency_ties_by_session_id() {
        let sessions = vec![
            summary("b-id", "/work/main", Some(7)),
            summary("a-id", "/work/main", Some(7)),
        ];
        let rows = browser_rows(&sessions, None);
        let order: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                BrowserRow::Session { index, .. } => Some(sessions[*index].session_id.as_str()),
                BrowserRow::Group { .. } => None,
            })
            .collect();
        assert_eq!(order, vec!["a-id", "b-id"]);
    }

    #[test]
    fn rows_count_groups_and_number_sessions_across_them() {
        let sessions = vec![
            summary("a1", "/work/alpha", Some(2)),
            summary("b1", "/work/beta", Some(1)),
            summary("a2", "/work/alpha", Some(3)),
        ];
        let rows = browser_rows(&sessions, None);
        assert_eq!(
            rows,
            vec![
                BrowserRow::Group { index: 2, count: 2 },
                BrowserRow::Session { index: 2, ordinal: 0 },
                BrowserRow::Session { index: 0, ordinal: 1 },
                BrowserRow::Group { index: 1, count: 1 },
                BrowserRow::Session { index: 1, ordinal: 2 },
            ]
        );
    }

    #[test]
    fn rows_stay_empty_without_sessions() {
        assert!(browser_rows(&[], Some("/work/current")).is_empty());
    }

    #[test]
    fn compact_age_steps_through_units_at_both_sides_of_each_boundary() {
        const NOW: u64 = 10_000_000;
        let cases = [
            (0, "0s"),
            (59, "59s"),
            (60, "1m"),
            (3_599, "59m"),
            (3_600, "1h"),
            (86_399, "23h"),
            (86_400, "1d"),
            (86_400 * 45, "45d"),
        ];
        for (age, expected) in cases {
            assert_eq!(compact_age(Some(NOW - age), NOW).as_deref(), Some(expected), "age {age}");
        }
    }

    #[test]
    fn compact_age_hides_unknown_timestamps_and_saturates_future_skew() {
        assert_eq!(compact_age(None, 1_000_000), None);
        assert_eq!(compact_age(Some(1_000_001), 1_000_000).as_deref(), Some("0s"));
    }
}
