use std::path::PathBuf;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, point, px, size,
};
use serde_json::json;

use super::{DesktopView, MAX_UI_EVENTS_PER_TICK, ThemeMode, workspace_picker_options};
use crate::desktop::transcript::TranscriptItem;
use crate::desktop::transport::{BackendCommand, BackendEvent, BackendHandle};

fn open_desktop(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DesktopView>) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1280.), px(900.)),
                ))),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    let mut view = DesktopView::new(window, cx, ThemeMode::Light);
                    view.workspace = Some(std::env::temp_dir());
                    view
                })
            },
        )
        .unwrap()
    })
}

#[test]
fn workspace_picker_only_allows_one_directory() {
    let options = workspace_picker_options();
    assert!(!options.files);
    assert!(options.directories);
    assert!(!options.multiple);
    assert!(options.prompt.is_some());
}

#[gpui_kit::test]
fn picker_completion_rejects_switching_after_turn_starts(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|window, cx| DesktopView::new(window, cx, ThemeMode::Light));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            let original_workspace = PathBuf::from("/original-workspace");
            view.workspace = Some(original_workspace.clone());
            view.running = true;
            view.select_workspace(std::env::temp_dir(), cx);
            assert_eq!(view.workspace.as_ref(), Some(&original_workspace));
            assert!(view.backend.is_none());
            assert!(view.error.as_deref().unwrap().contains("Cancel the active turn"));
        });
    });
}

#[gpui_kit::test]
fn event_drain_yields_to_input_with_a_busy_producer(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (view, cx) = cx.add_window_view(|window, cx| DesktopView::new(window, cx, ThemeMode::Light));
    cx.update(|_, cx| {
        view.update(cx, |view, cx| {
            for index in 0..MAX_UI_EVENTS_PER_TICK + 3 {
                view.event_tx
                    .try_send(BackendEvent::TurnStatus(json!({ "status": format!("event-{index}") })))
                    .unwrap();
            }
            view.drain_events(cx);
            assert_eq!(view.status, format!("event-{}", MAX_UI_EVENTS_PER_TICK - 1));
            view.drain_events(cx);
            assert_eq!(view.status, format!("event-{}", MAX_UI_EVENTS_PER_TICK + 2));
        });
    });
}

#[gpui_kit::test]
fn send_button_keeps_draft_and_transcript_when_queue_is_full(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    commands.try_send(BackendCommand::Stop).unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session-a".to_string());
            view.composer
                .update(cx, |state, cx| state.set_value("Keep this draft", window, cx));
        });
        window.render_frame(cx);
        window.click("send-prompt", cx);
        let view = view.read(cx);
        assert!(view.transcript.is_empty());
        assert!(!view.running);
        assert_eq!(view.composer.read(cx).value(), "Keep this draft");
        assert!(view.error.as_deref().unwrap().contains("queue is full"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn send_button_submits_exact_draft_and_clears_it_after_enqueue(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session-b".to_string());
            view.composer
                .update(cx, |state, cx| state.set_value("Review **these** changes", window, cx));
        });
        window.render_frame(cx);
        window.click("send-prompt", cx);
        let view = view.read(cx);
        assert!(view.running);
        assert_eq!(view.composer.read(cx).value(), "");
        assert_eq!(view.transcript.get(0).unwrap().text, "Review **these** changes");
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params, json!({ "sessionId": "session-b", "prompt": "Review **these** changes" }));
}

#[gpui_kit::test]
fn enter_submits_the_draft_and_shift_enter_keeps_editing(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session-c".to_string());
            view.composer
                .update(cx, |state, cx| {
                    state.set_value("", window, cx);
                    state.focus(window, cx);
                });
        });
        window.render_frame(cx);
        // Type the draft so the caret sits after the text, like real input.
        window.input("Keyboard first", cx);

        // Shift+Enter inserts a newline and never submits.
        window.press("shift-enter", cx);
        {
            let view = view.read(cx);
            assert!(!view.running, "shift-enter must not submit the draft");
            assert_eq!(view.composer.read(cx).value(), "Keyboard first\n");
        }

        // Plain Enter submits through the propagated action.
        window.press("enter", cx);
        {
            let view = view.read(cx);
            assert!(view.running, "enter must submit the draft");
            assert_eq!(view.composer.read(cx).value(), "");
            assert_eq!(view.transcript.get(0).unwrap().text, "Keyboard first\n");
        }
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params, json!({ "sessionId": "session-c", "prompt": "Keyboard first\n" }));
}

#[gpui_kit::test]
fn first_submit_names_the_session_and_close_deselects_it(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.sessions.insert(
                0,
                vtcode_desktop_protocol::SessionSummary {
                    session_id: "session-e".to_string(),
                    title: None,
                    workspace: std::env::temp_dir().display().to_string(),
                    completed: false,
                    resumable: true,
                },
            );
            view.selected_session_id = Some("session-e".to_string());
            view.composer.update(cx, |state, cx| {
                state.set_value("Fix the login timeout\n\nand add a regression test", window, cx);
            });
        });
        window.render_frame(cx);
        window.click("send-prompt", cx);
        {
            let view = view.read(cx);
            assert_eq!(
                view.sessions[0].title.as_deref(),
                Some("Fix the login timeout"),
                "sidebar entry is named from the first prompt"
            );
        }

        // A later archive refresh without a persisted title keeps the local one.
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_LIST.to_string(),
                    result: Some(serde_json::json!([{
                        "sessionId": "session-e",
                        "title": None::<String>,
                        "workspace": std::env::temp_dir().display().to_string(),
                        "completed": false,
                        "resumable": true,
                    }])),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.sessions[0].title.as_deref(), Some("Fix the login timeout"));
        });

        // Close is gated on the turn ending, like the button state.
        view.update(cx, |view, _| {
            view.handle_thread_event(vtcode_desktop_protocol::ThreadEventNotification {
                session_id: "session-e".to_string(),
                turn_id: Some("turn-9".to_string()),
                event: vtcode_exec_events::VersionedThreadEvent::new(
                    vtcode_exec_events::ThreadEvent::TurnCompleted(vtcode_exec_events::TurnCompletedEvent {
                        completed_at: None,
                        usage: vtcode_exec_events::Usage {
                            input_tokens: 0,
                            cached_input_tokens: 0,
                            cache_creation_tokens: 0,
                            output_tokens: 0,
                        },
                        in_progress_exec_sessions: Vec::new(),
                    }),
                ),
            });
            assert!(!view.running, "turn completion re-enables session controls");
        });

        window.click("close-session", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params: _ } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    let BackendCommand::Request { method, params: _ } = requests.try_recv().unwrap() else {
        panic!("expected session list refresh");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LIST);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected close request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
    assert_eq!(params, json!({ "sessionId": "session-e" }));
}

#[gpui_kit::test]
fn file_change_items_render_as_toggleable_diff_cards(cx: &mut TestAppContext) {
    use vtcode_exec_events::{
        FileChangeItem, FileUpdateChange, ItemCompletedEvent, PatchApplyStatus, PatchChangeKind, ThreadEvent,
        ThreadItem, ThreadItemDetails,
    };
    use vtcode_desktop_protocol::ThreadEventNotification;

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.handle_thread_event(ThreadEventNotification {
                session_id: "session-d".to_string(),
                turn_id: Some("turn-1".to_string()),
                event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(
                    ItemCompletedEvent {
                        item: ThreadItem {
                            context: None,
                            id: "change-1".to_string(),
                            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                                diff_incomplete: Some(false),
                                changes: vec![FileUpdateChange {
                                    path: "src/main.rs".to_string(),
                                    kind: PatchChangeKind::Update,
                                }],
                                status: PatchApplyStatus::Completed,
                                unified_diff: Some("@@ -1,2 +1,3 @@\n context\n-old line\n+new line\n".to_string()),
                                additions: Some(1),
                                deletions: Some(1),
                            })),
                        },
                    },
                )),
            });
            let item = view.transcript.get(0).expect("diff row");
            assert_eq!(item.role, "diff");
            let diff = item.diff.as_ref().expect("structured diff payload");
            assert_eq!(diff.files.len(), 1);
            assert_eq!(diff.files[0].path, "src/main.rs");
            assert_eq!(diff.additions, Some(1));
            assert_eq!(diff.deletions, Some(1));
            assert!(!diff.failed);
            assert!(diff.patch.contains("+new line"));
        });
        window.render_frame(cx);

        // Expanding the card exposes the patch text and the copy action.
        window.click("diff-change-1", cx);
        window.click("copy-patch-change-1", cx);
        assert!(
            cx.read_from_clipboard()
                .unwrap()
                .text()
                .unwrap()
                .contains("+new line"),
            "copy patch must copy the raw unified diff"
        );
        view.read(cx).transcript.get(0).unwrap().expanded.then_some(()).expect("diff expanded");

        // Collapsing again hides the patch view but keeps the card.
        window.click("diff-change-1", cx);
        assert!(!view.read(cx).transcript.get(0).unwrap().expanded);
    })
    .unwrap();
}

#[gpui_kit::test]
fn measured_transcript_follows_tall_messages_and_preserves_scroll_away(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let list = cx.update(|cx| {
        view.update(cx, |view, _| {
            for index in 0..20 {
                view.transcript.push(TranscriptItem::new(
                    format!("message-{index}"),
                    "assistant".to_string(),
                    "VT Code".to_string(),
                    format!("Message {index}\n\n{}", "wrapped words ".repeat(index + 1)),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "stream".to_string(),
                "assistant".to_string(),
                "VT Code".to_string(),
                "A tall paragraph.\n\n".repeat(100),
            ));
            view.transcript.list_state()
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let last = window.find("transcript-row-stream").bounds();
        let viewport = list.viewport_bounds();
        assert!(last.size.height > viewport.size.height);
        assert!(last.bottom() <= viewport.bottom() + px(1.));
        assert!(last.bottom() >= viewport.bottom() - px(32.), "last={last:?}, viewport={viewport:?}");

        list.scroll_by(px(-120.));
        window.render_frame(cx);
        let anchor = list.logical_scroll_top();
        assert!(!list.is_following_tail());
        view.update(cx, |view, _| {
            view.append_or_update_item(
                "stream".to_string(),
                "assistant",
                "VT Code",
                &"Additional text.\n\n".repeat(150),
                false,
            );
        });
        window.render_frame(cx);
        let after = list.logical_scroll_top();
        assert_eq!(after.item_ix, anchor.item_ix);
        assert!((after.offset_in_item - anchor.offset_in_item).abs() <= px(1.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn rich_transcript_copy_actions_keep_raw_markdown_and_exact_code(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let source = "# Review\n\n**Bold** and <b>inline HTML</b>\n\n```rust\nfn sample() -> u8 { 7 }\n```";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "copy-test".to_string(),
                "assistant".to_string(),
                "VT Code".to_string(),
                source.to_string(),
            ));
        });
        window.render_frame(cx);
        window.click("copy-message-copy-test", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(source));
        window.click("copy-code", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap().trim_end(), "fn sample() -> u8 { 7 }");
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_remeasures_wrapped_markdown_when_window_narrows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let wide_height = cx
        .update_window(window, |_, window, cx| {
            view.update(cx, |view, _| {
                view.transcript.push(TranscriptItem::new(
                    "resize".to_string(),
                    "assistant".to_string(),
                    "VT Code".to_string(),
                    format!("## Summary\n\n{}", "A longer paragraph that wraps naturally. ".repeat(50)),
                ));
            });
            window.render_frame(cx);
            window.find("transcript-row-resize").bounds().size.height
        })
        .unwrap();

    cx.simulate_window_resize(window, size(px(720.), px(900.)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let row = window.find("transcript-row-resize").bounds();
        assert!(row.size.height > wide_height);
        let list = view.read(cx).transcript.list_state();
        assert!(list.is_following_tail());
        assert!(row.bottom() <= list.viewport_bounds().bottom() + px(1.));
    })
    .unwrap();
}
