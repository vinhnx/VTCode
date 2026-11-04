//! Severity notice rows: runtime errors, compaction/reset notices, run status,
//! and collapsible ThreadEvent diagnostic notices.

use super::*;

impl DesktopView {
    /// Severity rows for runtime errors, compaction/reset notices, and run
    /// status: one icon/title header line in severity colors, with no message
    /// bubble, Markdown, or copy action. Diagnostic notices carry their
    /// Show/Hide details toggle inline at the header's trailing edge, with
    /// the operational evidence below only while expanded. Weight is the last
    /// resort: icon + tone already carry severity, so only errors earn
    /// semibold.
    pub(super) fn render_notice_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let (icon, tone, role, status) = match item.notice_severity {
            NoticeSeverity::Error => (IconName::CircleAlert, colors.danger, gpui_kit::Role::Alert, "error"),
            NoticeSeverity::Warning => (IconName::TriangleAlert, colors.warning, gpui_kit::Role::Status, "warning"),
            NoticeSeverity::Info => (IconName::Info, themes::muted_text(&colors), gpui_kit::Role::Status, "info"),
        };
        let mut header = div()
            .id(format!("notice-header-{}", item.id))
            .test_support()
            .flex()
            .items_center()
            .gap_2()
            .child(Icon::new(icon).small().flex_shrink_0().text_color(tone))
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .text_sm()
                    .when(item.notice_severity == NoticeSeverity::Error, |title| title.font_semibold())
                    .text_color(tone)
                    .child(item.title.clone()),
            );
        if item.collapsible_details {
            let id = item.id.clone();
            let toggle = if item.expanded {
                rust_i18n::t!("app.transcript.hide_details").to_string()
            } else {
                rust_i18n::t!("app.transcript.show_details").to_string()
            };
            header = header.child(
                desktop_button(format!("notice-details-{}", item.id))
                    .accessibility_label(
                        rust_i18n::t!(
                            "app.transcript.details_label",
                            toggle = toggle.as_str(),
                            title = item.title.as_str(),
                            status = status
                        )
                        .to_string(),
                    )
                    .ghost()
                    .xsmall()
                    .flex_shrink_0()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Icon::new(if item.expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .small(),
                            )
                            .child(toggle),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.transcript.toggle_notice(&id);
                        cx.notify();
                    })),
            );
        }
        let mut panel = div()
            .id(format!("notice-{}", item.id))
            .test_support()
            .role(role)
            .aria_label(format!("{}: {}", item.title, item.accessibility_text))
            .w_full()
            .max_w(px(self.content_width()))
            .flex()
            .flex_col()
            .gap_1()
            .child(header);
        if item.collapsible_details {
            if item.expanded {
                panel = panel.child(
                    div()
                        .id(format!("notice-details-body-{}", item.id))
                        .test_support()
                        .text_sm()
                        .text_color(colors.foreground)
                        .child(item.text.clone()),
                );
            }
        } else if !item.text.trim().is_empty() {
            panel = panel.child(div().text_sm().text_color(colors.foreground).child(item.text.clone()));
        }
        panel.into_any_element()
    }
}
