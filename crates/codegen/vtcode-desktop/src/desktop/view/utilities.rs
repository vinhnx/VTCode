use super::*;

impl DesktopView {
    pub(super) fn render_utilities(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No workspace".into());
        let (additions, deletions) = self.changes.totals();
        let rows = inspector::activity_rows(&self.transcript, &self.restored_activity);
        let counts = ActivityCounts::from_rows(&rows);
        let summary = if counts.total() == 0 {
            "No activity yet".to_string()
        } else {
            counts.summary()
        };
        div()
            .id("utilities-pane")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label("Workspace utilities")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .child(accessible_group_box(
                "utilities-workspace-group",
                workspace.clone(),
                GroupBox::new()
                    .fill()
                    .title(workspace)
                    .child(
                        desktop_button("utilities-changes")
                            .ghost()
                            .w_full()
                            .justify_start()
                            .accessibility_label(format!(
                                "Session changes: {} files, {additions} additions, {deletions} deletions",
                                self.changes.entries().len()
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .w_full()
                                    .child(Icon::new(IconName::GitCompare).small())
                                    .child(div().flex_1().child("Changes"))
                                    .child(div().text_sm().text_color(colors.success).child(format!("+{additions}")))
                                    .child(div().text_sm().text_color(colors.danger).child(format!("−{deletions}"))),
                            )
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.set_inspector_tab(InspectorTab::Changes, cx);
                            })),
                    )
                    .child(
                        div()
                            .pt_4()
                            .pb_1()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child("RECENT ACTIVITY"),
                    )
                    .child(
                        desktop_button("utilities-activity")
                            .ghost()
                            .w_full()
                            .justify_start()
                            .accessibility_label(format!("Recent activity: {summary}"))
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
                            .child(div().text_xs().text_color(colors.muted_foreground).child("DRAFT SOURCES"))
                            .child(
                                desktop_button("utilities-add-source")
                                    .icon(IconName::Plus)
                                    .ghost()
                                    .small()
                                    .accessibility_label("Attach source files")
                                    .tooltip("Attach workspace files")
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
                                .child("Attach workspace files to include them with your next message."),
                        )
                    })
                    .children(self.attachments.iter().map(|attachment| {
                        let path = attachment.path.clone();
                        desktop_button(opaque_path_element_id("utilities-source", &path))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .accessibility_label(format!("Preview attached source: {path}"))
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
                            .label("Browse workspace files")
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
