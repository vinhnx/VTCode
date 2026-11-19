//! Severity announcements: one shared callout for runtime errors,
//! compaction/reset notices, run status, and ThreadEvent diagnostic notices.
//!
//! Every severity uses the same card geometry, typography, icon slot, and CTA
//! placement; only the tone-resolved color and icon differ. The body always
//! sits on its own row below a constant-height title row; a Show/Hide details
//! CTA appears only when the body holds more than the visible lead, so short
//! notices never offer a redundant expanded state. Consecutive ephemeral
//! notices fold into one callout whose count CTA reveals the rest.

use super::*;

/// Announcement body lead length before the Details CTA takes over. Long
/// verification paragraphs are announcements, not transcripts: the first
/// sentence-sized line identifies the condition and the rest stays on demand.
const NOTICE_LEAD_CHARS: usize = 180;

fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

fn notice_lead(text: &str) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("").trim();
    let mut chars = line.chars();
    let mut lead: String = chars.by_ref().take(NOTICE_LEAD_CHARS).collect();
    if chars.next().is_some() {
        lead.push('…');
    }
    capitalize_first(&lead)
}

/// Whether the collapsed lead hides any of the body. Short single-line bodies
/// render in full with no Details CTA, so the expanded state never repeats
/// content the collapsed callout already shows verbatim.
fn notice_has_hidden_details(text: &str) -> bool {
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());
    let Some(first) = lines.next() else {
        return false;
    };
    lines.next().is_some() || first.trim().chars().nth(NOTICE_LEAD_CHARS).is_some()
}

/// Shared callout shell, used by both the head notice and each folded entry.
/// Keeps the surface, border, padding, icon tile, and title spine identical
/// across severities so the three levels read as one announcement family.
struct CalloutChrome {
    surface: gpui_kit::Hsla,
    border: gpui_kit::Hsla,
    icon_surface: gpui_kit::Hsla,
    title_color: gpui_kit::Hsla,
    body_color: gpui_kit::Hsla,
    icon: IconName,
}

fn callout_chrome(severity: NoticeSeverity, colors: &gpui_kit::component::theme::ThemeColor) -> CalloutChrome {
    let style = severity_marker_style(severity, colors);
    let (surface, border) = themes::tone_callout_surface(style.tone, colors);
    let (icon_surface, _) = themes::tone_chip_surface(style.tone, colors);
    CalloutChrome {
        surface,
        border,
        icon_surface,
        title_color: themes::readable_indicator_color(surface, colors.foreground, style.tone),
        body_color: themes::muted_text(colors),
        icon: style.icon,
    }
}

fn callout_icon_tile(chrome: &CalloutChrome) -> gpui_kit::Div {
    // The 16px slot is `transcript_icon_slot`'s geometry, so a callout title
    // lands on the same 32px spine as tool, group, and assistant rows.
    transcript_icon_slot(
        div()
            .size_4()
            .rounded_md()
            .flex()
            .items_center()
            .justify_center()
            .bg(chrome.icon_surface)
            .child(Icon::new(chrome.icon).small().text_color(chrome.title_color)),
    )
}

impl DesktopView {
    /// Severity rows for runtime errors, compaction/reset notices, run status,
    /// and collapsible diagnostic notices. The wrapper keeps test identity,
    /// roles, and aria labels; the card is the shared announcement treatment.
    pub(super) fn render_notice_item(&self, item: &TranscriptItem, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let chrome = callout_chrome(item.notice_severity, &colors);
        let style = severity_marker_style(item.notice_severity, &colors);
        let title = capitalize_first(item.title.as_str());
        let has_body = item.collapsible_details && !item.text.trim().is_empty();
        let has_details = has_body && notice_has_hidden_details(item.text.as_str());
        let stack_count = item.notice_count();
        let has_stack = stack_count > 1;

        // Aria text carries every folded message so assistive tech never loses
        // an announcement just because it is visually collapsed.
        let mut aria = format!("{}: {}", title, item.accessibility_text.as_str());
        for entry in &item.notice_stack {
            aria.push_str(": ");
            aria.push_str(entry.title.as_str());
            aria.push_str(": ");
            aria.push_str(entry.text.as_str());
        }

        // Title owns the whole header row; the body lives in its own row below
        // so the icon and trailing CTA never shift between collapsed/expanded.
        let heading = div()
            .min_w_0()
            .flex_1()
            .truncate()
            .text_sm()
            .font_semibold()
            .text_color(chrome.title_color)
            .child(title.clone());

        let mut header = div()
            .id(format!("notice-header-{}", item.id))
            .test_support()
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .child(callout_icon_tile(&chrome))
            .child(heading);

        if has_stack {
            let label = if item.notice_stack_expanded {
                rust_i18n::t!("app.transcript.notice_stack_hide", n = item.notice_stack.len())
            } else {
                rust_i18n::t!("app.transcript.notice_stack_show", n = item.notice_stack.len())
            }
            .to_string();
            let toggle_word = if item.notice_stack_expanded {
                rust_i18n::t!("app.transcript.hide_details").to_string()
            } else {
                rust_i18n::t!("app.transcript.show_details").to_string()
            };
            let stack_id = item.id.clone();
            header = header.child(
                desktop_ghost_button(format!("notice-stack-toggle-{}", item.id))
                    .accessibility_label(
                        rust_i18n::t!(
                            "app.transcript.details_label",
                            toggle = toggle_word.as_str(),
                            title = label.as_str(),
                            status = style.status_label
                        )
                        .to_string(),
                    )
                    .small()
                    .flex_shrink_0()
                    .text_color(chrome.title_color)
                    .child(label)
                    .child(transcript_chevron_lane(item.notice_stack_expanded))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.transcript.toggle_notice_stack(&stack_id);
                        cx.notify();
                    })),
            );
        }

        if has_details {
            let toggle = if item.expanded {
                rust_i18n::t!("app.transcript.hide_details").to_string()
            } else {
                rust_i18n::t!("app.transcript.show_details").to_string()
            };
            let details_id = item.id.clone();
            header = header.child(
                desktop_ghost_button(format!("notice-details-{}", item.id))
                    .accessibility_label(
                        rust_i18n::t!(
                            "app.transcript.details_label",
                            toggle = toggle.as_str(),
                            title = title.as_str(),
                            status = style.status_label
                        )
                        .to_string(),
                    )
                    .small()
                    .flex_shrink_0()
                    .text_color(chrome.title_color)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_xs()
                            .child(toggle)
                            .child(transcript_chevron_lane(item.expanded)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.transcript.toggle_notice(&details_id);
                        cx.notify();
                    })),
            );
        }

        let mut card = div()
            .id(format!("notice-{}", item.id))
            .test_support()
            .role(style.role)
            .aria_label(aria)
            .w_full()
            .max_w(px(self.content_width()))
            .rounded_md()
            .border_1()
            .border_color(chrome.border)
            .bg(chrome.surface)
            // Leading inset + icon slot + gap reproduce the 32px transcript
            // title spine; no right padding so the trailing CTA docks on the
            // same edge as group and tool disclosure rows.
            .py_3()
            .pl_2()
            .pr_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(header);

        if has_body {
            card = card.child(self.render_notice_body(
                item.id.as_str(),
                item.text.as_str(),
                item.expanded || (item.notice_is_diagnostic && !has_details),
                item.notice_is_diagnostic,
                &chrome,
                cx,
            ));
        }

        if has_stack && item.notice_stack_expanded {
            for (index, entry) in item.notice_stack.iter().enumerate() {
                card = card.child(self.render_folded_notice(entry, index, cx));
            }
        }

        card.into_any_element()
    }

    /// Body of one callout, always visible in its own row on the title spine.
    /// Collapsed notices show the short lead; the Details CTA reveals the full
    /// text. Diagnostic evidence keeps its Markdown treatment (fenced blocks
    /// still read as code); plain notices stay literal so backend backticks
    /// are never reinterpreted.
    fn render_notice_body(
        &self,
        id: &str,
        text: &str,
        show_full_body: bool,
        markdown: bool,
        chrome: &CalloutChrome,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div()
            .id(format!("notice-details-body-{id}"))
            .test_support()
            .pl_6()
            .text_sm()
            .text_color(chrome.body_color);
        if markdown && show_full_body {
            body = body.child(render_markdown_with_style(
                format!("notice-details-markdown-{id}"),
                text.to_string().into(),
                false,
                self.transcript_markdown_style.clone().unwrap_or_else(|| markdown_style(cx)),
            ));
        } else {
            let content = if show_full_body {
                capitalize_first(text)
            } else {
                notice_lead(text)
            };
            body = body.child(
                div()
                    .id(format!("notice-text-{id}"))
                    .test_support()
                    .aria_label(content.clone())
                    .child(content),
            );
        }
        body.into_any_element()
    }

    /// One folded announcement revealed by the stack count CTA. It shares the
    /// head's chrome; the body is already on demand, so no second toggle is
    /// needed inside the expanded stack.
    fn render_folded_notice(&self, entry: &NoticePayload, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let entry_chrome = callout_chrome(entry.severity, &colors);
        let title = capitalize_first(entry.title.as_str());
        let has_body = !entry.text.trim().is_empty();
        let mut body = div()
            .id(format!("notice-stack-entry-{}-{}", entry.id, index))
            .test_support()
            .role(if entry.severity == NoticeSeverity::Error {
                gpui_kit::Role::Alert
            } else {
                gpui_kit::Role::Status
            })
            .aria_label(format!("{}: {}", title, entry.text))
            .w_full()
            .rounded_md()
            .border_1()
            .border_color(entry_chrome.border)
            .bg(entry_chrome.surface)
            .py_2()
            .px_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(callout_icon_tile(&entry_chrome))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_sm()
                            .font_semibold()
                            .text_color(entry_chrome.title_color)
                            .child(title),
                    ),
            );
        if has_body {
            body = body.child(
                div().pl_6().text_sm().text_color(entry_chrome.body_color).child(
                    div()
                        .id(format!("notice-text-{}", entry.id))
                        .test_support()
                        .child(capitalize_first(entry.text.as_str())),
                ),
            );
        }
        body.into_any_element()
    }
}
