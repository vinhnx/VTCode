use super::goal::{goal_live_elapsed_label, goal_state_icon, goal_state_label};
use super::*;

impl DesktopView {
    pub(super) fn render_utilities(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let workspace = self
            .workspace_file_name()
            .unwrap_or_else(|| rust_i18n::t!("app.utilities.no_workspace").to_string());
        let totals = self.changes.totals();
        let change_accessibility_label = totals.map_or_else(
            || rust_i18n::t!("app.utilities.changes_unavailable", n = self.changes.entries().len()).to_string(),
            |(additions, deletions)| {
                rust_i18n::t!(
                    "app.utilities.changes_totals",
                    n = self.changes.entries().len(),
                    a = additions,
                    d = deletions
                )
                .to_string()
            },
        );
        let activity = self.cached_activity_projection();
        let counts = ActivityCounts::from_rows(&activity.rows);
        let summary = if counts.total() == 0 {
            rust_i18n::t!(i18n::INSPECTOR_EMPTY_ACTIVITY_TITLE).to_string()
        } else {
            counts.summary()
        };
        inspector_tab_body()
            .id("utilities-pane")
            .lock_scroll_axis()
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.utilities.group_aria").to_string())
            .track_scroll(&self.utilities_scroll)
            .overflow_y_scroll()
            .vertical_scrollbar(&self.utilities_scroll)
            .p_3()
            .child(accessible_group_box(
                "utilities-workspace-group",
                workspace.clone(),
                settings_group_box(workspace, &colors)
                    .child(
                        desktop_ghost_button("utilities-changes")
                            .w_full()
                            .justify_start()
                            .accessibility_label(change_accessibility_label)
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .w_full()
                                    .child(Icon::new(IconName::GitCompare).small())
                                    .child(
                                        div().flex_1().child(rust_i18n::t!("app.utilities.changes_title").to_string()),
                                    )
                                    .when_some(totals, |row, (additions, deletions)| {
                                        row.child(
                                            div().text_sm().text_color(colors.success).child(format!("+{additions}")),
                                        )
                                        .child(div().text_sm().text_color(colors.danger).child(format!("−{deletions}")))
                                    })
                                    .when(totals.is_none(), |row| {
                                        row.child(
                                            div()
                                                .text_xs()
                                                .text_color(colors.muted_foreground)
                                                .child(rust_i18n::t!("app.utilities.counts_unavailable").to_string()),
                                        )
                                    }),
                            )
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.set_inspector_tab(InspectorTab::Changes, cx);
                            })),
                    )
                    .child(
                        div()
                            .pt_4()
                            .pb_1()
                            .child(eyebrow_label(rust_i18n::t!("app.utilities.goal_title"), &colors)),
                    )
                    .child(match self.goal_snapshot.as_ref() {
                        Some(snapshot) => div()
                            .id("utilities-goal")
                            .test_support()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(goal_state_icon("utilities-goal-state-icon", snapshot.state, &colors))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(colors.foreground)
                                            .child(goal_state_label(snapshot.state)),
                                    ),
                            )
                            .child({
                                let source: gpui_kit::SharedString = snapshot.objective.clone().into();
                                let prepared = prepared_markdown(&source);
                                div()
                                    .id("utilities-goal-markdown")
                                    .test_support()
                                    .aria_label(prepared.accessible_text.clone())
                                    .text_sm()
                                    .text_color(colors.muted_foreground)
                                    .child(render_markdown_with_style(
                                        "utilities-goal-objective-view".to_string(),
                                        prepared.markdown,
                                        false,
                                        markdown_style(cx).with_foreground(colors.muted_foreground),
                                    ))
                            })
                            .child(div().text_xs().text_color(colors.muted_foreground).child(format!(
                                "{} · {}",
                                goal_live_elapsed_label(snapshot, self.goal_snapshot_at),
                                snapshot.tokens_label()
                            )))
                            .into_any_element(),
                        None => div()
                            .id("utilities-goal")
                            .test_support()
                            .py_2()
                            .text_sm()
                            .text_color(colors.muted_foreground)
                            .child(rust_i18n::t!("app.utilities.goal_empty").to_string())
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .pt_4()
                            .pb_1()
                            .child(eyebrow_label(rust_i18n::t!("app.utilities.recent_activity"), &colors)),
                    )
                    .child(
                        desktop_ghost_button("utilities-activity")
                            .w_full()
                            .justify_start()
                            .accessibility_label(
                                rust_i18n::t!("app.utilities.recent_summary", summary = summary.as_str()).to_string(),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .min_w_0()
                                    .child(Icon::new(IconName::Activity).small())
                                    .child(div().text_sm().truncate().child(summary)),
                            )
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.set_inspector_tab(InspectorTab::Activity, cx);
                            })),
                    )
                    .child(
                        div()
                            .pt_4()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(eyebrow_label(rust_i18n::t!("app.utilities.draft_sources"), &colors))
                            .child(
                                desktop_ghost_button("utilities-add-source")
                                    .icon(IconName::Plus)
                                    .small()
                                    .accessibility_label(rust_i18n::t!("app.utilities.attach_sources").to_string())
                                    .tooltip(rust_i18n::t!("app.utilities.attach_tooltip").to_string())
                                    .desktop_disabled(
                                        self.workspace.is_none()
                                            || self.attachments_picker_open
                                            || self.session_request.is_some(),
                                    )
                                    .on_click(cx.listener(|view, _, window, cx| view.attach_files(window, cx))),
                            ),
                    )
                    .when(self.attachments.is_empty(), |card| {
                        card.child(
                            div()
                                .py_2()
                                .text_sm()
                                .text_color(colors.muted_foreground)
                                .child(rust_i18n::t!("app.utilities.attach_hint").to_string()),
                        )
                    })
                    .children(self.attachments.iter().map(|attachment| {
                        let path = attachment.path.clone();
                        desktop_ghost_button(opaque_path_element_id("utilities-source", &path))
                            .w_full()
                            .justify_start()
                            .accessibility_label(
                                rust_i18n::t!("app.utilities.open_source", path = path.as_str()).to_string(),
                            )
                            .tooltip(path.clone())
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .min_w_0()
                                    .child(Icon::new(IconName::File).small())
                                    .child(div().truncate().child(path.clone())),
                            )
                            .on_click(
                                cx.listener(move |view, _, window, cx| {
                                    view.open_file_preview(path.clone(), window, cx)
                                }),
                            )
                    }))
                    .child(
                        desktop_ghost_button("utilities-browse-files")
                            .label(rust_i18n::t!("app.utilities.browse_files").to_string())
                            .icon(IconName::Folder)
                            .w_full()
                            .justify_start()
                            .desktop_disabled(self.workspace.is_none())
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.set_inspector_tab(InspectorTab::Files, cx);
                            })),
                    )
                    .child(
                        desktop_ghost_button("utilities-terminal")
                            .label(rust_i18n::t!("app.navigation.terminal").to_string())
                            .icon(IconName::Terminal)
                            .w_full()
                            .justify_start()
                            .accessibility_label(rust_i18n::t!("app.navigation.terminal").to_string())
                            .tooltip_with_action(
                                rust_i18n::t!("app.navigation.terminal").to_string(),
                                &ToggleTerminal,
                                Some(key_context::DESKTOP),
                            )
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.toggle_terminal(window, cx);
                            })),
                    ),
            ))
            .into_any_element()
    }
}
