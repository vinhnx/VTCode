//! Rendering tests for the unified severity announcement callouts.

use super::*;
use crate::desktop::transcript::NoticeSeverity;

#[gpui_kit::test]
fn transcript_notices_render_severity_without_message_chrome(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            for (id, severity, title, text) in [
                ("notice-error", NoticeSeverity::Error, "Runtime error", "Boom `code` here"),
                ("notice-warning", NoticeSeverity::Warning, "Run status", "Turn ended early"),
                ("notice-info", NoticeSeverity::Info, "Compaction", "120 into 8 messages"),
            ] {
                view.transcript
                    .push(TranscriptItem::notice(id.into(), title.into(), text.into(), severity));
            }
            cx.notify();
        });
        window.render_frame(cx);
        // Consecutive notices fold into one announcement; the loudest level
        // styles the head and announces as an alert.
        assert_eq!(view.read(cx).transcript.len(), 1);
        assert_eq!(window.find("notice-notice-error").role(), Some(gpui_kit::Role::Alert));
        let stacked = &view.read(cx).transcript.get(0).unwrap().notice_stack;
        assert_eq!(stacked.len(), 2);
        assert_eq!(stacked[0].severity, NoticeSeverity::Warning);
        assert_eq!(stacked[1].severity, NoticeSeverity::Info);
        window.click("notice-stack-toggle-notice-error", cx);
        window.render_frame(cx);
        assert_eq!(window.find("notice-stack-entry-notice-warning-0").role(), Some(gpui_kit::Role::Status));
        assert_eq!(window.find("notice-stack-entry-notice-info-1").role(), Some(gpui_kit::Role::Status));
        // No message chrome: notices carry no copy action, even when stacked.
        for id in ["notice-error", "notice-warning", "notice-info"] {
            assert!(window.try_find(format!("copy-message-{id}")).is_none(), "{id} must not offer message copy");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn notice_callout_keeps_its_title_row_stable_while_expanding(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::notice(
                "callout".into(),
                "run status".into(),
                "turn blocked by a failing check\ncheck the verify step for details".into(),
                NoticeSeverity::Warning,
            ));
        });
        window.render_frame(cx);

        let lead = window.find("notice-text-callout");
        assert!(
            lead.label().is_some_and(|label| label == "Turn blocked by a failing check"),
            "the visible lead capitalizes its first letter: {:?}",
            lead.label()
        );
        assert!(window.try_find("notice-details-callout").is_some(), "a body with hidden lines offers a CTA");
        let header_before = window.find("notice-header-callout").bounds();
        let toggle_before = window.find("notice-details-callout").bounds();

        window.click("notice-details-callout", cx);
        window.render_frame(cx);
        assert_eq!(
            window.find("notice-header-callout").bounds(),
            header_before,
            "the icon/title/CTA row never jumps between states"
        );
        assert_eq!(
            window.find("notice-details-callout").bounds(),
            toggle_before,
            "the CTA keeps its exact slot when the label flips"
        );
        let body = window.find("notice-text-callout");
        assert!(
            body.label()
                .is_some_and(|label| label.contains("check the verify step for details")),
            "expanding reveals the rest of the body"
        );
        assert!(window.try_find("notice-details-body-callout").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn short_notice_body_shows_in_full_without_a_details_cta(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::notice(
                "short".into(),
                "run status".into(),
                "turn cancelled".into(),
                NoticeSeverity::Warning,
            ));
        });
        window.render_frame(cx);

        assert_eq!(
            window.find("notice-text-short").label(),
            Some("Turn cancelled"),
            "a short body is shown verbatim with no truncated lead"
        );
        assert!(
            window.try_find("notice-details-short").is_none(),
            "there is no redundant expand step when the full body is already visible"
        );
        let header = window.find("notice-header-short").bounds();
        let body = window.find("notice-details-body-short").bounds();
        assert!(body.origin.y > header.origin.y, "the body sits on its own row under the title");
    })
    .unwrap();
}

#[gpui_kit::test]
fn long_single_line_notice_offers_a_cta_with_a_truncated_lead(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::notice(
                "long".into(),
                "run status".into(),
                "x".repeat(200),
                NoticeSeverity::Warning,
            ));
        });
        window.render_frame(cx);

        let lead_element = window.find("notice-text-long");
        let lead = lead_element.label().unwrap_or_default();
        assert_eq!(lead.chars().count(), 181, "the lead truncates at 180 chars plus an ellipsis");
        assert!(lead.ends_with('…'));
        assert!(window.try_find("notice-details-long").is_some(), "a truncated body offers a CTA");

        window.click("notice-details-long", cx);
        window.render_frame(cx);
        let body_element = window.find("notice-text-long");
        assert_eq!(body_element.label().unwrap_or_default().chars().count(), 200, "expanding reveals the full body");
    })
    .unwrap();
}

#[gpui_kit::test]
fn notice_title_shares_the_transcript_title_spine(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "spine-tool".into(),
                "tool".into(),
                "Read Cargo.toml".into(),
                "completed".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "spine-answer".into(),
                "assistant".into(),
                "VT Code".into(),
                "Sharing the spine.".into(),
            ));
            view.transcript.push(TranscriptItem::notice(
                "spine-notice".into(),
                "run status".into(),
                "turn blocked".into(),
                NoticeSeverity::Warning,
            ));
        });
        window.render_frame(cx);

        let tool_title = window.find("tool-title-spine-tool").bounds();
        let answer_body = window.find("message-body-spine-answer").bounds();
        let notice_title = window.find("notice-title-spine-notice").bounds();
        assert!(
            (notice_title.origin.x - tool_title.origin.x).abs() <= px(1.),
            "notice title shares the tool title spine: notice={notice_title:?} tool={tool_title:?}"
        );
        assert!(
            (notice_title.origin.x - answer_body.origin.x).abs() <= px(1.),
            "notice title shares the assistant prose spine: notice={notice_title:?} answer={answer_body:?}"
        );
        let notice_body = window.find("notice-text-spine-notice").bounds();
        assert!(
            (notice_body.origin.x - notice_title.origin.x).abs() <= px(1.),
            "notice body starts on its own title spine: body={notice_body:?} title={notice_title:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn notice_stack_cta_reveals_folded_announcements(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, title) in [("first", "run status"), ("second", "run error")] {
                view.transcript.push(TranscriptItem::notice(
                    id.into(),
                    title.into(),
                    format!("{id} details"),
                    NoticeSeverity::Info,
                ));
            }
        });
        window.render_frame(cx);

        let toggle = window.find("notice-stack-toggle-first");
        assert_eq!(toggle.role(), Some(gpui_kit::Role::Button));
        assert!(
            toggle.label().is_some_and(|label| label.contains("Show 1 more")),
            "the CTA names how many announcements are folded: {:?}",
            toggle.label()
        );
        assert!(window.try_find("notice-stack-entry-second-0").is_none());

        window.click("notice-stack-toggle-first", cx);
        window.render_frame(cx);
        let entry = window.find("notice-stack-entry-second-0");
        assert!(
            entry.label().is_some_and(|label| label.contains("Run error")),
            "folded titles capitalize their first letter: {:?}",
            entry.label()
        );
        assert!(
            window
                .find("notice-stack-toggle-first")
                .label()
                .is_some_and(|label| label.contains("Hide 1 more")),
            "the CTA flips to hide once expanded"
        );
    })
    .unwrap();
}
