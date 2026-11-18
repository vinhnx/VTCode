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
fn notice_callout_capitalizes_and_hides_its_body_behind_a_cta(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::notice(
                "callout".into(),
                "run status".into(),
                "turn blocked by a failing check".into(),
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
        assert!(window.try_find("notice-details-callout").is_some(), "the details CTA is always reachable");
        assert!(window.try_find("notice-details-body-callout").is_none(), "the full message stays behind the CTA");

        window.click("notice-details-callout", cx);
        window.render_frame(cx);
        let body = window.find("notice-text-callout");
        assert!(
            body.label().is_some_and(|label| label == "Turn blocked by a failing check"),
            "the expanded body keeps capitalization"
        );
        assert!(window.try_find("notice-details-body-callout").is_some());
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
