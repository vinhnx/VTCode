//! Consecutive-notice folding: one announcement callout for a run of
//! ephemeral notices, with a bounded, head-owned expansion state.

use super::super::item::NOTICE_STACK_LIMIT;
use super::super::{NoticeSeverity, Transcript, TranscriptItem};

#[test]
fn consecutive_plain_notices_fold_into_one_announcement() {
    let mut transcript = Transcript::default();
    let notice = |id: &str, title: &str, text: &str, severity| {
        TranscriptItem::notice(id.into(), title.into(), text.to_string(), severity)
    };
    transcript.push(notice("a", "Run status", "Turn blocked", NoticeSeverity::Warning));
    transcript.push(notice("b", "Run error, exit code: 2", "Command failed", NoticeSeverity::Error));
    transcript.push(notice("c", "Run status", "Verification pending", NoticeSeverity::Info));

    assert_eq!(transcript.len(), 1, "three notices occupy one callout row");
    let head = transcript.get(0).unwrap();
    assert_eq!(head.notice_count(), 3);
    assert_eq!(head.title.as_str(), "Run status", "the first notice stays the visible callout");
    assert_eq!(head.notice_severity, NoticeSeverity::Error, "the loudest folded severity styles the head");
    assert!(
        head.notice_stack
            .iter()
            .any(|entry| entry.id == "b" && entry.severity == NoticeSeverity::Error),
        "folded payloads keep their own severity"
    );

    transcript.toggle_notice_stack("a");
    assert!(transcript.get(0).unwrap().notice_stack_expanded);
    let before = transcript.revision();
    transcript.toggle_notice_stack("b");
    transcript.toggle_notice_stack("missing");
    assert_eq!(transcript.revision(), before, "only the head owns the stack toggle");

    // Diagnostics are upserted by id, so they break (never join) a stack.
    transcript.push(TranscriptItem::diagnostic_notice(
        "diag".into(),
        "Tool failed".into(),
        "Evidence".to_string(),
        NoticeSeverity::Warning,
    ));
    transcript.push(notice("d", "Run status", "Still blocked", NoticeSeverity::Warning));
    assert_eq!(transcript.len(), 3, "diagnostic and the notice after it stay separate rows");
    assert_eq!(transcript.get(2).unwrap().notice_count(), 1);

    // A very long run caps the stack so one callout cannot grow unbounded.
    let mut capped = Transcript::default();
    for index in 0..NOTICE_STACK_LIMIT + 3 {
        capped.push(notice(&format!("n-{index}"), "Run status", "Turn blocked", NoticeSeverity::Info));
    }
    assert_eq!(capped.len(), 2);
    assert_eq!(capped.get(0).unwrap().notice_count(), NOTICE_STACK_LIMIT);
    assert_eq!(capped.get(1).unwrap().notice_count(), 3);
}
