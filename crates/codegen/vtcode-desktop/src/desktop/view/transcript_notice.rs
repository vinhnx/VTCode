//! Severity notice rows: runtime errors, compaction/reset notices, run status,
//! and collapsible ThreadEvent diagnostic notices.

use super::*;

impl DesktopView {
    /// Severity rows for runtime errors, compaction/reset notices, and run
    /// status: a tone-tinted pill chip (icon + title) built from kit `Tag`,
    /// with the message body below and no bubble, Markdown, or copy action.
    /// The wrapper keeps test identity, roles, and aria labels. Diagnostic
    /// notices carry their Show/Hide details toggle docked at the row's
    /// trailing edge, with the operational evidence below only while
    /// expanded. Weight is the last resort: icon + tone already carry
    /// severity, so only errors earn semibold.
    pub(super) fn render_notice_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let style = severity_marker_style(item.notice_severity, &colors);
        let (surface, border) = themes::tone_chip_surface(style.tone, &colors);
        let title_tone = themes::readable_indicator_color(surface, colors.foreground, style.tone);
        let chip = Tag::custom(surface, title_tone, border)
            .rounded_full()
            .small()
            .text_xs()
            .px_2()
            .when(style.semibold_title, |tag| tag.font_semibold())
            .child(Icon::new(style.icon).small().text_color(title_tone))
            .child(item.title.clone());
        let header = div()
            .id(format!("notice-header-{}", item.id))
            .test_support()
            // The leading inset matches a small button's `px_2`, so chip
            // titles start on the same spine as group and tool headers
            // (inset + `transcript_icon_slot` + `gap_2`).
            .pl_2()
            .flex()
            .items_center()
            .gap_2()
            .child(chip)
            .when(item.collapsible_details, |row| {
                let id = item.id.clone();
                let toggle = if item.expanded {
                    rust_i18n::t!("app.transcript.hide_details").to_string()
                } else {
                    rust_i18n::t!("app.transcript.show_details").to_string()
                };
                row.child(
                    desktop_button(format!("notice-details-{}", item.id))
                        .accessibility_label(
                            rust_i18n::t!(
                                "app.transcript.details_label",
                                toggle = toggle.as_str(),
                                title = item.title.as_str(),
                                status = style.status_label
                            )
                            .to_string(),
                        )
                        .ghost()
                        // Small like the group and tool toggles: one row
                        // height, one text size, and one chevron inset
                        // across headers.
                        .small()
                        .ml_auto()
                        .flex_shrink_0()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_xs()
                                .italic()
                                .text_color(themes::muted_text(&colors))
                                .child(toggle)
                                .child(transcript_chevron_lane(item.expanded)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.transcript.toggle_notice(&id);
                            cx.notify();
                        })),
                )
            });
        let mut panel = div()
            .id(format!("notice-{}", item.id))
            .test_support()
            .role(style.role)
            .aria_label(format!("{}: {}", item.title, item.accessibility_text))
            .w_full()
            .max_w(px(self.content_width()))
            .flex()
            .flex_col()
            .gap_1()
            .child(header);
        if item.collapsible_details {
            if item.expanded {
                // Evidence renders as Markdown so fenced blocks keep their
                // code treatment, with quieter body text than conversation.
                panel = panel.child(
                    div()
                        .id(format!("notice-details-body-{}", item.id))
                        .test_support()
                        .text_sm()
                        .text_color(themes::muted_text(&colors))
                        .child(render_markdown_with_style(
                            format!("notice-details-markdown-{}", item.id),
                            item.text.clone(),
                            false,
                            self.transcript_markdown_style.clone().unwrap_or_else(|| markdown_style(cx)),
                        )),
                );
            }
        } else if !item.text.trim().is_empty() {
            panel = panel.child(
                div()
                    .id(format!("notice-text-{}", item.id))
                    .test_support()
                    .text_sm()
                    .text_color(themes::muted_text(&colors))
                    .child(item.text.clone()),
            );
        }
        panel.into_any_element()
    }
}
