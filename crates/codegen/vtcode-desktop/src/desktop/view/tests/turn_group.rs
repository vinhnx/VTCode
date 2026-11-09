use super::*;

#[gpui_kit::test]
fn restored_file_summary_requires_unique_canonical_final_and_turn(cx: &mut TestAppContext) {
    use vtcode_exec_events::{
        AgentMessageItem, FileChangeItem, FileUpdateChange, ItemCompletedEvent, ItemContext, PatchApplyStatus,
        PatchChangeKind, ThreadEvent, ThreadItem, ThreadItemDetails, TurnCompletedEvent, Usage, VersionedThreadEvent,
    };
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let context = ItemContext {
            task_id: "task".into(),
            turn_id: "canonical-turn".into(),
            actor_id: "actor".into(),
            parent_actor_id: None,
            timestamp: "2026-11-05T00:00:00Z".into(),
            activity: None,
        };
        let mut final_row = TranscriptItem::new("archive".into(), "assistant".into(), "Agent".into(), "done".into());
        final_row.archived_phase = Some("final_answer".into());
        final_row.canonical_item_id = Some("canonical-final".into());
        view.load_history(vec![final_row]);
        let change = ThreadItem {
            id: "change".into(),
            context: Some(Box::new(context.clone())),
            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                changes: vec![FileUpdateChange {
                    path: "src/a.rs".into(),
                    kind: PatchChangeKind::Update,
                    terminators_normalized: None,
                }],
                status: PatchApplyStatus::Completed,
                unified_diff: None,
                additions: None,
                deletions: None,
                diff_incomplete: Some(true),
            })),
        };
        view.project_restored_review(&[change]);
        let final_event = VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                id: "canonical-final".into(),
                context: Some(Box::new(context)),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "done".into() }),
            },
        }));
        let completed = VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            final_response_item_id: Some(Box::new("canonical-final".into())),
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }));
        view.restore_review_summaries(&[final_event.clone(), completed.clone(), completed.clone()]);
        assert!(view.transcript.get(0).unwrap().turn_files.is_empty());
        view.restore_review_summaries(&[final_event, completed]);
        let row = view.transcript.get(0).unwrap();
        assert_eq!(row.turn_files.len(), 1);
        assert_eq!(row.turn_files[0].file.path, "src/a.rs");
        assert!(row.turn_files_incomplete);
        assert!(row.turn_file_totals.is_none());
    });
}

#[gpui_kit::test]
fn earlier_page_completion_uses_only_current_sessions_timeline(cx: &mut TestAppContext) {
    use std::sync::Arc;
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.restored_timeline = Arc::new(vec![VersionedThreadEvent::new(ThreadEvent::TurnCompleted(
            TurnCompletedEvent {
                final_response_item_id: Some(Box::new("final-id".into())),
                completed_at: Some(Box::new("2026-11-05T08:30:00Z".into())),
                usage: Usage::default(),
                in_progress_exec_sessions: Vec::new(),
            },
        ))]);
        let mut row = TranscriptItem::new("archive-row".into(), "assistant".into(), "Agent".into(), "done".into());
        row.archived_phase = Some("final_answer".into());
        row.canonical_item_id = Some("final-id".into());
        assert_eq!(view.transcript.prepend_prepared(vec![row]), 1);
        let timeline = Arc::clone(&view.restored_timeline);
        view.restore_completion_times(&timeline);
        assert!(view.transcript.get(0).unwrap().completed_at.is_some());
        view.reset_session_state();
        assert!(view.restored_timeline.is_empty());
    });
}

#[gpui_kit::test]
fn restored_completion_time_requires_unique_canonical_identity(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let mut final_item =
            TranscriptItem::new("archive-row".into(), "assistant".into(), "Agent".into(), "done".into());
        final_item.archived_phase = Some("final_answer".into());
        final_item.canonical_item_id = Some("final-item".into());
        view.load_history(vec![final_item]);
        let event = VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            final_response_item_id: Some(Box::new("final-item".into())),
            completed_at: Some(Box::new("2026-11-05T08:30:00Z".into())),
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }));
        view.restore_completion_times(&[event.clone(), event.clone()]);
        assert!(view.transcript.get(0).unwrap().completed_at.is_none());
        view.restore_completion_times(&[event]);
        let expected = chrono::DateTime::parse_from_rfc3339("2026-11-05T08:30:00Z")
            .unwrap()
            .with_timezone(&chrono::Local)
            .format("%H:%M")
            .to_string();
        assert_eq!(view.transcript.get(0).unwrap().completed_at.as_deref(), Some(expected.as_str()));
    });
}

#[test]
fn restored_final_keeps_archive_row_id_and_canonical_item_id() {
    let rows = super::super::sessions::prepare_session_history(
        "session",
        vec![vtcode_desktop_protocol::SessionHistoryMessage {
            hidden: false,
            archive_index: Some(7),
            role: "assistant".into(),
            text: "done".into(),
            phase: Some("final_answer".into()),
            event_item_id: Some("canonical-final".into()),
        }],
    );
    assert_ne!(rows[0].id, "canonical-final");
    assert_eq!(rows[0].canonical_item_id.as_deref(), Some("canonical-final"));
}

#[gpui_kit::test]
fn canonical_and_transport_turn_ids_are_kept_separate(cx: &mut TestAppContext) {
    use vtcode_exec_events::{AgentMessageItem, ItemContext, ThreadItem, ThreadItemDetails};
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let item = ThreadItem {
            id: "item".into(),
            context: Some(Box::new(ItemContext {
                task_id: "task".into(),
                turn_id: "turn-a".into(),
                actor_id: "actor".into(),
                parent_actor_id: None,
                timestamp: "2026-11-05T00:00:00Z".into(),
                activity: None,
            })),
            details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "original".into() }),
        };
        view.update_runtime_item_for_turn(&item, None);
        assert_eq!(view.transcript.get(0).unwrap().canonical_turn_id.as_deref(), Some("turn-a"));
        assert!(view.transcript.get(0).unwrap().turn_id.is_none());
        let mut conflict = item;
        conflict.details = ThreadItemDetails::AgentMessage(AgentMessageItem { text: "wrong".into() });
        view.update_runtime_item_for_turn(&conflict, Some("turn-b"));
        assert_eq!(view.transcript.get(0).unwrap().turn_id.as_deref(), Some("turn-b"));
        assert_eq!(view.transcript.get(0).unwrap().canonical_turn_id.as_deref(), Some("turn-a"));
        conflict.id = "new-item".into();
        view.update_runtime_item_for_turn(&conflict, Some("transport-turn"));
        assert_eq!(view.transcript.get(1).unwrap().turn_id.as_deref(), Some("transport-turn"));
    });
}

#[gpui_kit::test]
fn archived_turn_split_across_pages_groups_only_after_user_page_arrives(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::SessionHistoryMessage;
    let message = |index, role: &str, phase: Option<&str>| SessionHistoryMessage {
        event_item_id: None,
        hidden: false,
        archive_index: Some(index),
        role: role.into(),
        text: format!("row {index}"),
        phase: phase.map(str::to_owned),
    };
    let tail = super::super::sessions::prepare_session_history(
        "session",
        vec![
            message(2, "assistant", Some("commentary")),
            message(3, "assistant", Some("final_answer")),
        ],
    );
    let prefix = super::super::sessions::prepare_session_history(
        "session",
        vec![message(0, "user", None), message(1, "assistant", Some("commentary"))],
    );
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.load_history(tail);
        assert!(!view.transcript.get(0).unwrap().turn_grouped);
        let inserted = view.transcript.prepend_prepared(prefix);
        view.transcript.group_archived_prefix(inserted);
        view.transcript.group_archived_boundary(inserted);
        assert!(view.transcript.get(1).unwrap().turn_group_head);
        assert!(view.transcript.get(2).unwrap().turn_grouped);
        assert!(!view.transcript.get(3).unwrap().turn_grouped);
    });
}

#[gpui_kit::test]
fn prepended_archived_turn_groups_without_collapsing_existing_work(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::SessionHistoryMessage;
    let page = |offset, label: &str| {
        let message = |index, role: &str, phase: Option<&str>| SessionHistoryMessage {
            event_item_id: None,
            hidden: false,
            archive_index: Some(index),
            role: role.into(),
            text: label.into(),
            phase: phase.map(str::to_owned),
        };
        super::super::sessions::prepare_session_history(
            "session",
            vec![
                message(offset, "user", None),
                message(offset + 1, "assistant", Some("commentary")),
                message(offset + 2, "assistant", Some("final_answer")),
            ],
        )
    };
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.load_history(page(3, "later"));
        let later_turn = view.transcript.get(1).unwrap().turn_id.clone().unwrap();
        view.transcript.set_turn_group_expanded(&later_turn, true);
        let inserted = view.transcript.prepend_prepared(page(0, "earlier"));
        view.transcript.group_archived_prefix(inserted);
        assert!(view.transcript.get(1).unwrap().turn_group_head);
        assert!(!view.transcript.get(1).unwrap().turn_group_expanded);
        assert!(view.transcript.get(4).unwrap().turn_group_expanded);
    });
}

#[gpui_kit::test]
fn restored_explicit_commentary_groups_only_with_recorded_final(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::SessionHistoryMessage;
    let message = |index, role: &str, text: &str, phase: Option<&str>| SessionHistoryMessage {
        event_item_id: None,
        hidden: false,
        archive_index: Some(index),
        role: role.into(),
        text: text.into(),
        phase: phase.map(str::to_owned),
    };
    let history = super::super::sessions::prepare_session_history(
        "session",
        vec![
            message(0, "user", "first", None),
            message(1, "assistant", "progress", Some("commentary")),
            message(2, "assistant", "answer", Some("final_answer")),
            message(3, "user", "second", None),
            message(4, "assistant", "unknown", None),
            message(5, "assistant", "answer", Some("final_answer")),
        ],
    );
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.load_history(history);
        assert!(view.transcript.get(1).unwrap().turn_group_head);
        assert!(!view.transcript.get(1).unwrap().turn_group_expanded);
        assert!(!view.transcript.get(2).unwrap().turn_grouped);
        assert!(!view.transcript.get(4).unwrap().turn_grouped);
        assert!(!view.transcript.get(5).unwrap().turn_grouped);
    });
}

#[gpui_kit::test]
fn conflicting_turn_cannot_overwrite_existing_transcript_item(cx: &mut TestAppContext) {
    use vtcode_exec_events::{AgentMessageItem, ThreadItem, ThreadItemDetails};
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let item = |text: &str| ThreadItem {
            id: "shared".into(),
            context: None,
            details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: text.into() }),
        };
        view.update_runtime_item_for_turn(&item("first"), Some("turn-a"));
        view.update_runtime_item_for_turn(&item("foreign"), Some("turn-b"));
        let row = view.transcript.get(0).unwrap();
        assert_eq!(row.text.as_ref(), "first");
        assert_eq!(row.turn_id.as_deref(), Some("turn-a"));
        view.update_runtime_item_for_turn(&item("updated"), Some("turn-a"));
        assert_eq!(view.transcript.get(0).unwrap().text.as_ref(), "updated");
    });
}

#[gpui_kit::test]
fn context_notice_is_assigned_to_its_turn(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{ContextResetEvent, ContextResetTrigger, ThreadEvent, VersionedThreadEvent};
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.handle_thread_event(ThreadEventNotification {
            session_id: "session".into(),
            turn_id: Some("turn-a".into()),
            event: VersionedThreadEvent::new(ThreadEvent::ContextReset(ContextResetEvent {
                thread_id: "session".into(),
                turn_id: "turn-a".into(),
                trigger: ContextResetTrigger::PlanApproval,
                plan_preserved: true,
                previous_context_usage_percent: 80,
                tool_budget_reset: true,
            })),
        });
        assert_eq!(view.transcript.get(0).unwrap().turn_id.as_deref(), Some("turn-a"));
    });
}

#[gpui_kit::test]
fn restored_change_keeps_canonical_turn_identity(cx: &mut TestAppContext) {
    use vtcode_exec_events::{
        FileChangeItem, FileUpdateChange, ItemContext, PatchApplyStatus, PatchChangeKind, ThreadItem, ThreadItemDetails,
    };
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        let item = ThreadItem {
            id: "restored-change".into(),
            context: Some(Box::new(ItemContext {
                task_id: "task".into(),
                turn_id: "turn-a".into(),
                actor_id: "actor".into(),
                parent_actor_id: None,
                timestamp: "2026-11-05T00:00:00Z".into(),
                activity: None,
            })),
            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                changes: vec![FileUpdateChange {
                    path: "file.rs".into(),
                    kind: PatchChangeKind::Update,
                    terminators_normalized: None,
                }],
                status: PatchApplyStatus::Completed,
                unified_diff: None,
                additions: None,
                deletions: None,
                diff_incomplete: None,
            })),
        };
        let activity = ThreadItem {
            id: "restored-activity".into(),
            context: item.context.clone(),
            details: ThreadItemDetails::Error(vtcode_exec_events::ErrorItem { message: "failed".into() }),
        };
        view.project_restored_review(&[item]);
        view.project_runtime_item(&activity, true);
        assert_eq!(view.transcript.get(0).unwrap().turn_id.as_deref(), Some("turn-a"));
        assert_eq!(view.restored_activity[0].turn_id.as_deref(), Some("turn-a"));
    });
}

#[gpui_kit::test]
fn completed_turn_work_log_expands_without_hiding_final(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, text) in [("progress", "Working"), ("final", "Done")] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "assistant".into(), "Agent".into(), text.into()));
                view.transcript.set_item_turn(id, "turn");
            }
        });
        window.render_frame(cx);
        assert!(window.try_find("message-body-progress").is_some());
        view.update(cx, |view, _| {
            view.handle_thread_event(ThreadEventNotification {
                session_id: "session".into(),
                turn_id: Some("turn".into()),
                event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
                    final_response_item_id: Some(Box::new("final".into())),
                    completed_at: None,
                    usage: Usage::default(),
                    in_progress_exec_sessions: Vec::new(),
                })),
            });
        });
        window.render_frame(cx);
        assert!(window.try_find("message-body-progress").is_none());
        assert!(window.try_find("message-body-final").is_some());
        assert_eq!(window.find("turn-work-turn").label(), Some("Expand Activity"));
        window.click("turn-work-turn", cx);
        window.render_frame(cx);
        assert!(window.try_find("message-body-progress").is_some());
        assert!(window.try_find("message-body-final").is_some());
        assert_eq!(window.find("turn-work-turn").label(), Some("Collapse Activity"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn completed_turn_shows_time_only_on_identified_message(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "progress".into(),
                "assistant".into(),
                "Agent".into(),
                "Working".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "final".into(),
                "assistant".into(),
                "Agent".into(),
                "Done".into(),
            ));
            view.transcript.set_item_turn("final", "turn");
            view.handle_thread_event(ThreadEventNotification {
                session_id: "session".into(),
                turn_id: Some("turn".into()),
                event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
                    final_response_item_id: Some(Box::new("final".into())),
                    completed_at: Some(Box::new("2026-11-05T08:30:00Z".into())),
                    usage: Usage::default(),
                    in_progress_exec_sessions: Vec::new(),
                })),
            });
            assert!(view.transcript.get(0).unwrap().completed_at.is_none());
            let expected = chrono::DateTime::parse_from_rfc3339("2026-11-05T08:30:00Z")
                .unwrap()
                .with_timezone(&chrono::Local)
                .format("%H:%M")
                .to_string();
            assert_eq!(view.transcript.get(1).unwrap().completed_at.as_deref(), Some(expected.as_str()));
        });
        window.render_frame(cx);
        assert!(window.try_find("message-actions-final").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn only_final_turn_response_offers_message_copy(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // Asymmetric turn: intermediate commentary outside the turn id,
            // final answer inside it. Copy must follow the final only.
            view.transcript.push(TranscriptItem::new(
                "progress".into(),
                "assistant".into(),
                "Agent".into(),
                "Working".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "final".into(),
                "assistant".into(),
                "Agent".into(),
                "Done".into(),
            ));
            view.transcript.set_item_turn("final", "turn");
            view.handle_thread_event(ThreadEventNotification {
                session_id: "session".into(),
                turn_id: Some("turn".into()),
                event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
                    final_response_item_id: Some(Box::new("final".into())),
                    completed_at: Some(Box::new("2026-11-05T08:30:00Z".into())),
                    usage: Usage::default(),
                    in_progress_exec_sessions: Vec::new(),
                })),
            });
        });
        window.render_frame(cx);
        window.hover("transcript-row-progress", cx);
        window.render_frame(cx);
        assert!(window.try_find("message-actions-progress").is_some(), "footer slot stays for layout");
        assert!(
            window.try_find("copy-message-progress").is_none(),
            "intermediate commentary must not offer message copy"
        );
        // Leave to a neutral element before the next hover: back-to-back
        // row hovers lose the second enter in headless tests.
        window.hover("transcript-scroll", cx);
        window.render_frame(cx);
        window.hover("transcript-row-final", cx);
        window.render_frame(cx);
        assert!(window.try_find("copy-message-final").is_some(), "final turn response keeps message copy");
    })
    .unwrap();
}

#[gpui_kit::test]
fn assistant_bodies_share_the_header_title_spine(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            for (id, role, title, text) in [
                ("intro", "assistant", "VT Code", "I will inspect the build."),
                ("read", "tool", "Read Cargo.toml", "completed\nmanifest"),
                ("build", "tool", "cargo check", "failed (exit 1)\nstructured details"),
                ("comment", "assistant", "VT Code", "The build needs a fix."),
                ("retry", "tool", "cargo check", "in progress\nretrying"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), role.into(), title.into(), text.into()));
            }
        });
        window.render_frame(cx);
        // Assistant prose starts on the same left spine as group and tool
        // titles; the message insets to meet the header chrome.
        let first_body_x = window.find("message-body-intro").bounds().origin.x;
        let second_body_x = window.find("message-body-comment").bounds().origin.x;
        let header_x = window.find("activity-group-label-read").bounds().origin.x;
        let single_x = window.find("tool-title-retry").bounds().origin.x;
        assert_eq!(first_body_x, header_x, "first assistant body shares the header spine");
        assert_eq!(second_body_x, header_x, "second assistant body shares the header spine");
        assert_eq!(single_x, header_x, "single tool titles share the header spine");
    })
    .unwrap();
}

#[test]
fn hidden_history_marker_does_not_suppress_user_messages() {
    let messages = vec![
        vtcode_desktop_protocol::SessionHistoryMessage {
            event_item_id: None,
            phase: None,
            hidden: true,
            archive_index: Some(0),
            role: "system".into(),
            text: "model context".into(),
        },
        vtcode_desktop_protocol::SessionHistoryMessage {
            event_item_id: None,
            phase: None,
            hidden: true,
            archive_index: Some(1),
            role: "user".into(),
            text: "keep my message".into(),
        },
    ];
    let rows = super::super::prepare_session_history("session", messages);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].text.as_ref(), "keep my message");
}
