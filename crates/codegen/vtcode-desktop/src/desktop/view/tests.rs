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
    assert_eq!(
        params,
        json!({ "sessionId": "session-b", "prompt": "Review **these** changes", "attachments": [] })
    );
}

#[gpui_kit::test]
fn attachments_ride_the_turn_request_as_removable_chips(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("workspace");
    std::fs::write(workspace.path().join("notes.md"), "attached notes").expect("write");

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(workspace.path().to_path_buf());
            view.selected_session_id = Some("session-att".to_string());
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "notes.md".to_string(),
                text: "attached notes".to_string(),
            });
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "scratch.txt".to_string(),
                text: "drop me".to_string(),
            });
            view.composer
                .update(cx, |state, cx| state.set_value("Summarize the attachment", window, cx));
        });
        window.render_frame(cx);
        // Remove the second chip before submitting.
        window.click("remove-attachment-1", cx);
        assert_eq!(view.read(cx).attachments.len(), 1);

        window.click("send-prompt", cx);
        let view = view.read(cx);
        assert!(view.attachments.is_empty(), "attachments clear after submit");
        assert!(view.transcript.get(0).unwrap().text.contains("Attached: notes.md"));
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected turn request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(
        params["attachments"],
        json!([{ "path": "notes.md", "text": "attached notes" }])
    );
}

#[gpui_kit::test]
fn selecting_a_model_switches_configuration_and_resets_the_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(std::env::temp_dir());
            view.selected_session_id = Some("session-old".to_string());
            view.models = vec![
                vtcode_desktop_protocol::ModelInfo {
                    provider: "ollama".to_string(),
                    id: "gpt-oss:20b".to_string(),
                    display_name: "GPT OSS 20B".to_string(),
                    context_window: Some(131_072),
                    current: true,
                },
                vtcode_desktop_protocol::ModelInfo {
                    provider: "anthropic".to_string(),
                    id: "claude-sonnet-5".to_string(),
                    display_name: "Claude Sonnet 5".to_string(),
                    context_window: Some(200_000),
                    current: false,
                },
            ];
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        window.click("model-1", cx);
        assert_eq!(view.read(cx).status, "Switching model…");
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected config select request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SELECT);
    assert_eq!(
        params,
        json!({ "provider": "anthropic", "model": "claude-sonnet-5" })
    );

    // The service answers with the new snapshot; the client resets session
    // state and refreshes the catalog.
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SELECT.to_string(),
                    result: Some(json!({
                        "provider": "anthropic",
                        "model": "claude-sonnet-5",
                        "authenticated": true,
                        "credentialSetupCommand": "vtcode login anthropic",
                        "sandbox": "Ask before protected actions",
                        "contextWindow": 200_000,
                    })),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.selected_session_id, None, "session resets after a model switch");
            assert_eq!(
                view.configuration.as_ref().map(|configuration| configuration.model.clone()),
                Some("claude-sonnet-5".to_string())
            );
        });
    })
    .unwrap();
    let BackendCommand::Request { method, params: _ } = requests.try_recv().unwrap() else {
        panic!("expected catalog refresh");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_MODELS);
}

#[gpui_kit::test]
fn enter_submits_the_draft_and_shift_enter_keeps_editing(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session-c".to_string());
            view.composer.update(cx, |state, cx| {
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
    assert_eq!(
        params,
        json!({ "sessionId": "session-c", "prompt": "Keyboard first\n", "attachments": [] })
    );
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
                event: vtcode_exec_events::VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::TurnCompleted(
                    vtcode_exec_events::TurnCompletedEvent {
                        completed_at: None,
                        usage: vtcode_exec_events::Usage {
                            input_tokens: 0,
                            cached_input_tokens: 0,
                            cache_creation_tokens: 0,
                            output_tokens: 0,
                        },
                        in_progress_exec_sessions: Vec::new(),
                    },
                )),
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
fn settings_popover_copies_the_credential_setup_command(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_DISCOVER.to_string(),
                    result: Some(json!({
                        "provider": "ollama",
                        "model": "gpt-oss:20b",
                        "authenticated": true,
                        "credentialSetupCommand": "vtcode login ollama",
                    })),
                    error: None,
                },
                cx,
            );
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::INITIALIZE.to_string(),
                    result: Some(json!({
                        "protocol": { "major": 1, "minor": 0 },
                        "workspace": std::env::temp_dir().display().to_string(),
                        "serverVersion": "0.180.5",
                    })),
                    error: None,
                },
                cx,
            );
        });
        window.render_frame(cx);
        window.click("open-settings", cx);
        window.render_frame(cx);
        window.click("copy-credential-command", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode login ollama"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_files_open_a_read_only_preview(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("README.md"), "# Notes\n\nhello from the workspace").expect("write file");
    std::fs::create_dir_all(workspace.path().join("target/debug")).expect("skip dir");
    std::fs::write(workspace.path().join("target/debug/artifact"), b"binary").expect("write artifact");

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let readme_index = view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.refresh_files(cx);
            assert!(
                !view.files.iter().any(|entry| entry.rel_path.starts_with("target")),
                "dependency directories are skipped"
            );
            view.files
                .iter()
                .position(|entry| entry.rel_path == "README.md")
                .expect("README listed")
        });
        window.render_frame(cx);
        // The file browser lives in the inspector's Files tab.
        window.click("inspector-tab-files", cx);
        window.render_frame(cx);
        window.click(format!("file-{readme_index}"), cx);
        window.render_frame(cx);
        window.click("copy-preview", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("# Notes\n\nhello from the workspace"));
        window.click("close-preview", cx);
        assert!(view.read(cx).file_preview.is_none(), "close dismisses the preview");
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_changes_list_files_and_open_a_diff_viewer(cx: &mut TestAppContext) {
    use vtcode_exec_events::{
        FileChangeItem, FileUpdateChange, ItemCompletedEvent, PatchApplyStatus, PatchChangeKind, ThreadEvent,
        ThreadItem, ThreadItemDetails,
    };

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.workspace = Some(std::env::temp_dir());
            let patch = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,2 +1,3 @@\n ctx\n-gone\n+here\n+added\n";
            view.handle_thread_event(vtcode_desktop_protocol::ThreadEventNotification {
                session_id: "session-f".to_string(),
                turn_id: Some("turn-1".to_string()),
                event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: ThreadItem {
                        context: None,
                        id: "change-9".to_string(),
                        details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                            diff_incomplete: Some(false),
                            changes: vec![FileUpdateChange {
                                path: "src/app.rs".to_string(),
                                kind: PatchChangeKind::Update,
                            }],
                            status: PatchApplyStatus::Completed,
                            unified_diff: Some(patch.to_string()),
                            additions: Some(2),
                            deletions: Some(1),
                        })),
                    },
                })),
            });
            assert_eq!(view.changes.entries().len(), 1);
            assert_eq!(view.changes.entries()[0].path, "src/app.rs");
            assert_eq!(view.changes.totals(), (2, 1));
        });
        window.render_frame(cx);
        window.click("change-row-0", cx);
        {
            let view = view.read(cx);
            assert_eq!(view.selected_change.as_deref(), Some("src/app.rs"));
            assert!(view.change_viewer.is_some(), "selecting a file builds the diff viewer");
        }
        window.click("copy-change-patch", cx);
        assert!(cx.read_from_clipboard().unwrap().text().unwrap().contains("+here"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn command_palette_confirms_the_highlighted_command(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(std::env::temp_dir());
            view.backend = Some(BackendHandle { commands });
            view.open_command_palette(window, cx);
            assert!(view.palette.is_some(), "palette state exists");
            assert_eq!(view.palette_groups[0].0, "Session");
            assert_eq!(view.palette_groups[0].1[0].label, "New session");
        });
        window.render_frame(cx);
        // Focus after the first render, matching the kit's draw-focus-draw
        // pattern (the palette installs its input state during render).
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        // Enter matches the palette's Command-context Confirm binding; the
        // confirm callback itself is deferred by the kit.
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).palette.is_none(), "confirm closes the palette");
    })
    .unwrap();
    let BackendCommand::Request { method, params: _ } = requests.try_recv().unwrap() else {
        panic!("expected session create request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CREATE);
}

#[gpui_kit::test]
fn file_change_items_render_as_toggleable_diff_cards(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::ThreadEventNotification;
    use vtcode_exec_events::{
        FileChangeItem, FileUpdateChange, ItemCompletedEvent, PatchApplyStatus, PatchChangeKind, ThreadEvent,
        ThreadItem, ThreadItemDetails,
    };

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.handle_thread_event(ThreadEventNotification {
                session_id: "session-d".to_string(),
                turn_id: Some("turn-1".to_string()),
                event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
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
                })),
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
            cx.read_from_clipboard().unwrap().text().unwrap().contains("+new line"),
            "copy patch must copy the raw unified diff"
        );
        view.read(cx)
            .transcript
            .get(0)
            .unwrap()
            .expanded
            .then_some(())
            .expect("diff expanded");

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
