use super::*;

impl DesktopView {
    pub(super) fn render_activity_group(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> AnyElement {
        let Some(head) = self.transcript.get(range.start) else {
            return div().into_any_element();
        };
        let id = head.id.clone();
        let summary = head.activity_group_summary;
        let failed = summary.failed;
        let running = summary.running;
        let colors = cx.theme().colors;
        let status = if failed > 0 {
            ActivityState::Failed
        } else if running > 0 {
            ActivityState::Running
        } else {
            ActivityState::Other
        };
        let status_color = Self::activity_status_color(status, colors);
        let mut label = rust_i18n::t!("app.transcript.activity_total", total = summary.total).to_string();
        if running > 0 {
            label.push_str(display::MIDDOT);
            label.push_str(rust_i18n::t!("app.transcript.activity_running", count = running).as_ref());
        }
        if failed > 0 {
            label.push_str(display::MIDDOT);
            label.push_str(rust_i18n::t!("app.transcript.activity_failed", count = failed).as_ref());
        }
        let toggle = desktop_button(format!("activity-group-{}", head.id))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .justify_start()
                    .gap_2()
                    .child(Icon::new(IconName::Wrench).small().flex_shrink_0())
                    .when(running > 0, |row| {
                        row.child(
                            div()
                                .id(format!("activity-group-running-indicator-{}", head.id))
                                .test_support()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Spinner::new()
                                        .small()
                                        .color(Self::activity_status_color(ActivityState::Running, colors)),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id(format!("activity-group-label-{}", head.id))
                            .test_support()
                            .min_w_0()
                            .flex_1()
                            .truncate()
                            .child(label.clone()),
                    )
                    .child(
                        Icon::new(if head.activity_expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .small()
                        .flex_shrink_0(),
                    ),
            )
            .accessibility_label(format!(
                "{} {label}",
                if head.activity_expanded {
                    rust_i18n::t!("app.transcript.collapse")
                } else {
                    rust_i18n::t!("app.transcript.expand")
                }
            ))
            .toggled(head.activity_expanded)
            .ghost()
            .small()
            .flex_1()
            .min_w_0()
            .justify_start()
            .text_color(status_color)
            .on_click(cx.listener(move |view, _, _, cx| {
                view.transcript.toggle_activity_group(&id);
                cx.notify();
            }));
        // The toggle and its CTAs are siblings, never nested buttons: the
        // toggle keeps the flexible width and truncates first on narrow rows.
        div()
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .child(toggle)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .flex_shrink_0()
                    .child(Self::activity_group_cta("activity", InspectorTab::Activity, &head.id, cx))
                    .child(Self::activity_group_cta("output", InspectorTab::Output, &head.id, cx))
                    .child(Self::activity_group_cta("utilities", InspectorTab::Utilities, &head.id, cx)),
            )
            .into_any_element()
    }

    /// Small transcript CTA that reveals one inspector tab beside the reading
    /// position. Labels reuse the inspector tab vocabulary so both surfaces
    /// name the same destination.
    fn activity_group_cta(suffix: &str, tab: InspectorTab, head_id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        desktop_button(format!("activity-group-open-{suffix}-{head_id}"))
            .label(tab.label())
            .ghost()
            .xsmall()
            .on_click(cx.listener(move |view, _, _, cx| view.open_inspector_tab(tab, cx)))
    }

    /// Jump button beside an expanded tool row's copy action, opening that
    /// exact capture in Output. Same affordance as the Activity tab's per-row
    /// output action; callers gate on [`TranscriptItem::has_output`] so the
    /// CTA never lands on an unrelated fallback.
    pub(super) fn tool_output_cta(item: &TranscriptItem, cx: &mut Context<Self>) -> impl IntoElement {
        let output_id = item.id.clone();
        let output_title = item.title.to_string();
        desktop_button(format!("tool-open-output-{}", item.id))
            .icon(IconName::Terminal)
            .ghost()
            .xsmall()
            .accessibility_label(
                rust_i18n::t!("app.inspector.view_output_for", title = output_title.as_str()).to_string(),
            )
            .tooltip(rust_i18n::t!("app.inspector.view_captured_output").to_string())
            .on_click(cx.listener(move |view, _, window, cx| {
                view.select_activity_output(output_id.clone(), window, cx);
            }))
    }

    /// Jump button in a diff card header, opening Changes on the change-set's
    /// first previewable file, or the tab alone when nothing previews. The
    /// target resolves at click time from the live transcript (like the
    /// copy-patch action), so a rebuilt projection cannot strand it.
    pub(super) fn diff_changes_cta(item_id: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let id = item_id.to_string();
        desktop_button(format!("diff-open-changes-{item_id}"))
            .label(rust_i18n::t!("app.inspector.tab_changes").to_string())
            .ghost()
            .on_click(cx.listener(move |view, _, _, cx| {
                let open_path = view
                    .transcript
                    .items_newest_first()
                    .find(|candidate| candidate.id == id)
                    .and_then(|candidate| candidate.diff.as_ref())
                    .and_then(|diff| {
                        diff.files
                            .iter()
                            .find(|file| view.changes.has_preview(&file.path))
                            .map(|file| file.path.clone())
                    });
                match open_path {
                    Some(path) => view.select_change(path, cx),
                    None => view.open_inspector_tab(InspectorTab::Changes, cx),
                }
            }))
    }
}
