//! Severity announcements: one shared callout for runtime errors,
//! compaction/reset notices, run status, and ThreadEvent diagnostic notices.
//!
//! Every severity is one transcript row with the same lane geometry as tool
//! and activity rows: the shared icon slot, title gap, small semibold title,
//! and trailing ghost CTA. There is no card chrome — no surface, border, or icon.
//! A short tone marker sits in the leading slot (the lane the focus rail never
//! touches), so the notice blends into the transcript flow yet reads as a
//! system row rather than a message. Severity comes from the marker, the title
//! text, and the accessibility role, never from color alone. The body sits on
//! its own row below a constant-height title row; a Show/Hide details CTA
//! appears only when the body holds more than the visible lead, so short
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
/// Keeps the marker, lane geometry, and title spine identical across severities
/// so the three levels read as one announcement family. No surface or border:
/// the notice inherits the transcript background and stays legible by tone and
/// typography alone.
struct CalloutChrome {
    accent: gpui_kit::Hsla,
    marker_outlined: bool,
    body_color: gpui_kit::Hsla,
}

fn callout_chrome(severity: NoticeSeverity, colors: &gpui_kit::component::theme::ThemeColor) -> CalloutChrome {
    let style = severity_marker_style(severity, colors);
    CalloutChrome {
        // Contrast resolves against the transcript's own background, not a
        // tinted card. One readable tone drives the marker, title, and CTA.
        accent: themes::readable_indicator_color(colors.background, colors.foreground, style.tone),
        // Solid vs outlined keeps error and warning distinct without an icon,
        // so a diagnostic whose title omits the severity is not color-only.
        marker_outlined: style.marker_outlined,
        body_color: themes::muted_text(colors),
    }
}

/// Tone marker centered in the 16px leading slot. Tool rows place their kind
/// icon here; notices use a solid or outlined pill so the row shares that lane
/// without borrowing an icon. Staying inside the slot keeps the focus rail at
/// the row's leading edge clear and the title spine unchanged.
fn callout_marker(chrome: &CalloutChrome) -> gpui_kit::Div {
    let marker = div().w_1_5().h_4().rounded_full();
    if chrome.marker_outlined {
        marker.border_1().border_color(chrome.accent)
    } else {
        marker.bg(chrome.accent)
    }
}

impl DesktopView {
    /// Severity rows for runtime errors, compaction/reset notices, run status,
    /// and collapsible diagnostic notices. The wrapper keeps test identity,
    /// roles, and aria labels; the row is the shared announcement treatment.
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
        // so the title and trailing CTA never shift between collapsed/expanded.
        // ROW_INSET + ICON_SLOT + TITLE_GAP is the shared title spine, the same
        // one tool rows and assistant prose use.
        let heading = div()
            .id(format!("notice-title-{}", item.id))
            .test_support()
            .min_w_0()
            .flex_1()
            .truncate()
            .text_size(themes::layout::typography::ROW)
            .font_semibold()
            .text_color(chrome.accent)
            .child(title.clone());

        let mut header = div()
            .id(format!("notice-header-{}", item.id))
            .test_support()
            .w_full()
            .flex()
            .items_center()
            .gap(themes::layout::gap::TITLE)
            .child(transcript_icon_slot(callout_marker(&chrome)))
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
                    .text_color(chrome.accent)
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
                    .text_color(chrome.accent)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(themes::layout::gap::COMPACT)
                            .text_size(themes::layout::typography::ACTION)
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
            // No surface or border: the notice sits in the transcript flow as
            // one more row. ROW_INSET matches the small-button inset that tool
            // and activity headers carry; the row wrapper owns vertical rhythm.
            // No right padding so the CTA docks on the shared disclosure edge.
            .pl(themes::layout::padding::ROW_INSET)
            .pr_0()
            .flex()
            .flex_col()
            .gap(themes::layout::gap::COMPACT)
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
            for entry in &item.notice_stack {
                card = card.child(self.render_folded_notice(entry, cx));
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
        // BODY_INSET completes ROW_INSET + ICON_SLOT + TITLE_GAP, so the body
        // starts on the shared title spine and expanding never re-indents it.
        let mut body = div()
            .id(format!("notice-details-body-{id}"))
            .test_support()
            .pl(themes::layout::padding::BODY_INSET)
            .text_size(themes::layout::typography::ROW)
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
    fn render_folded_notice(&self, entry: &NoticePayload, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.theme().colors;
        let entry_chrome = callout_chrome(entry.severity, &colors);
        let title = capitalize_first(entry.title.as_str());
        let has_body = !entry.text.trim().is_empty();
        let mut body = div()
            .id(format!("notice-stack-entry-{}", entry.id))
            .test_support()
            .role(if entry.severity == NoticeSeverity::Error {
                gpui_kit::Role::Alert
            } else {
                gpui_kit::Role::Status
            })
            .aria_label(format!("{}: {}", title, entry.text))
            .w_full()
            .pl(themes::layout::padding::ROW_INSET)
            .pr_0()
            .flex()
            .flex_col()
            .gap(themes::layout::gap::COMPACT)
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(themes::layout::gap::TITLE)
                    .child(transcript_icon_slot(callout_marker(&entry_chrome)))
                    .child(
                        div()
                            .id(format!("notice-stack-title-{}", entry.id))
                            .test_support()
                            .min_w_0()
                            .flex_1()
                            .text_size(themes::layout::typography::ROW)
                            .font_semibold()
                            .text_color(entry_chrome.accent)
                            .child(title),
                    ),
            );
        if has_body {
            body = body.child(
                div()
                    .pl(themes::layout::padding::BODY_INSET)
                    .text_size(themes::layout::typography::ROW)
                    .text_color(entry_chrome.body_color)
                    .child(
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
