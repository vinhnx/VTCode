use super::*;

impl DesktopView {
    pub(super) fn render_utilities(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
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
        div()
            .id("utilities-pane")
            .lock_scroll_axis()
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.utilities.group_aria").to_string())
            .w_full()
            .flex_1()
            .min_h_0()
            .track_scroll(&self.utilities_scroll)
            .overflow_y_scroll()
            .vertical_scrollbar(&self.utilities_scroll)
            .p_3()
            .child(accessible_group_box(
                "utilities-workspace-group",
                workspace.clone(),
                settings_group_box(workspace, &colors)
                    .child(
                        desktop_button("utilities-changes")
                            .ghost()
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
                            .child(eyebrow_label(rust_i18n::t!("app.utilities.recent_activity"), &colors)),
                    )
                    .child(
                        desktop_button("utilities-activity")
                            .ghost()
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
                                desktop_button("utilities-add-source")
                                    .icon(IconName::Plus)
                                    .ghost()
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
                        desktop_button(opaque_path_element_id("utilities-source", &path))
                            .ghost()
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
                        desktop_button("utilities-browse-files")
                            .label(rust_i18n::t!("app.utilities.browse_files").to_string())
                            .icon(IconName::Folder)
                            .ghost()
                            .w_full()
                            .justify_start()
                            .desktop_disabled(self.workspace.is_none())
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.set_inspector_tab(InspectorTab::Files, cx);
                            })),
                    ),
            ))
            .into_any_element()
    }
}
