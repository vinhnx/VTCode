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
        desktop_button(format!("activity-group-{}", head.id))
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
            .w_full()
            .justify_start()
            .text_color(status_color)
            .on_click(cx.listener(move |view, _, _, cx| {
                view.transcript.toggle_activity_group(&id);
                cx.notify();
            }))
            .into_any_element()
    }
}
