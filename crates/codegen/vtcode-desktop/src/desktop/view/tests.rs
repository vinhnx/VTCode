use gpui_kit::Focusable;
use gpui_kit::component::theme::ActiveTheme;
use std::path::PathBuf;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, point, px, size,
};
use serde_json::json;

use super::{DesktopView, MAX_UI_EVENTS_PER_TICK, ThemeMode, workspace_picker_options};
use crate::desktop::transcript::TranscriptItem;
use crate::desktop::transport::{BackendCommand, BackendEvent, BackendHandle};

/// Redirect preference writes away from the real user home for GUI tests.
fn isolate_preferences() -> PathBuf {
    crate::desktop::preferences::install_test_preferences_path()
}

fn open_desktop(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DesktopView>) {
    open_desktop_sized(cx, 1280., 900.)
}

fn open_desktop_sized(cx: &mut TestAppContext, width: f32, height: f32) -> (AnyWindowHandle, Entity<DesktopView>) {
    drop(isolate_preferences());
    cx.update(gpui_kit::init);
    cx.update(super::init_keybindings);
    cx.update(super::init_native_menus);
    cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(width), px(height)),
                ))),
                ..super::desktop_window_options()
            },
            cx,
            |window, cx| {
                cx.new(|cx| {
                    let mut view = DesktopView::new(window, cx, ThemeMode::Light);
                    view.workspace = Some(std::env::temp_dir());
                    view.sidebar_open = true;
                    view.inspector_open = true;
                    view
                })
            },
        )
        .unwrap()
    })
}

fn file_tree_paths(view: &Entity<DesktopView>, cx: &gpui_kit::App) -> Vec<String> {
    let files_count = view.read(cx).files.len();
    let tree = view.read(cx).file_tree.clone();
    let tree = tree.read(cx);
    (0..files_count)
        .map_while(|index| tree.entry(index).map(|entry| entry.item().id.to_string()))
        .collect()
}

fn file_tree_item_snapshot(window: &mut gpui_kit::Window, index: usize) -> gpui_kit::test::ElementSnapshot {
    window
        .within("file-list")
        .find_all(index)
        .into_iter()
        .find(|item| item.role() == Some(gpui_kit::Role::TreeItem))
        .expect("tree item exposes its semantic role")
}

fn menu_action(cx: &gpui_kit::App, menu_name: &str, item_name: &str) -> Box<dyn gpui_kit::Action> {
    cx.get_menus()
        .unwrap()
        .into_iter()
        .find(|menu| menu.name == menu_name)
        .unwrap()
        .items
        .into_iter()
        .find_map(|item| match item {
            gpui_kit::OwnedMenuItem::Action { name, action, .. } if name == item_name => Some(action),
            _ => None,
        })
        .unwrap()
}

#[gpui_kit::test]
fn native_session_menu_uses_live_action_availability_and_existing_requests(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("menu-session".into());
            view.active_turn_id = Some("menu-turn".into());
            view.running = true;
            cx.notify();
        });
        window.render_frame(cx);
        assert!(!window.is_action_available(menu_action(cx, "File", "New Session").as_ref(), cx));
        assert!(!window.is_action_available(menu_action(cx, "File", "Close Session").as_ref(), cx));
        assert!(window.is_action_available(menu_action(cx, "View", "Cancel Run").as_ref(), cx));
        window.dispatch_action(menu_action(cx, "View", "Cancel Run"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("cancel request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
    assert_eq!(params, json!({ "sessionId": "menu-session", "turnId": "menu-turn" }));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.running = false;
            view.active_turn_id = None;
            cx.notify();
        });
        window.render_frame(cx);
        assert!(window.is_action_available(menu_action(cx, "File", "New Session").as_ref(), cx));
        assert!(window.is_action_available(menu_action(cx, "File", "Close Session").as_ref(), cx));
        assert!(!window.is_action_available(menu_action(cx, "View", "Cancel Run").as_ref(), cx));
        window.dispatch_action(menu_action(cx, "File", "Close Session"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("close request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
    assert_eq!(params, json!({ "sessionId": "menu-session" }));
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn native_edit_and_settings_menus_operate_on_the_focused_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-l", cx);
        window.input("Menu editing keeps this exact text", cx);
        window.dispatch_action(menu_action(cx, "Edit", "Select All"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(menu_action(cx, "Edit", "Cut"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("Menu editing keeps this exact text"));
        window.dispatch_action(menu_action(cx, "Edit", "Undo"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "Menu editing keeps this exact text");
        window.dispatch_action(menu_action(cx, "VT Code", "Settings…"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).settings_open);
        assert_eq!(window.find("settings-panel").label(), Some("Settings"));
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(!view.read(cx).settings_open);
        window.press("cmd-,", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("settings-panel").label(), Some("Settings"));
    })
    .unwrap();
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
fn reopening_preserves_the_existing_window_and_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |state, cx| state.set_value("Keep this unsent draft", window, cx));
            view.selected_session_id = Some("existing-session".into());
        });
    })
    .unwrap();
    cx.update(|cx| {
        let reopened = super::open_or_activate_main_window(cx).unwrap();
        assert_eq!(reopened, window);
        assert_eq!(cx.windows().len(), 1);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep this unsent draft");
        assert_eq!(view.read(cx).selected_session_id.as_deref(), Some("existing-session"));
    });
}

#[gpui_kit::test]
fn reopening_after_last_window_closes_creates_one_new_window(cx: &mut TestAppContext) {
    let (closed, view) = open_desktop(cx);
    drop(view);
    cx.update_window(closed, |_, window, _| window.remove_window()).unwrap();
    cx.update(|cx| {
        assert!(cx.windows().is_empty());
        let reopened = super::open_or_activate_main_window(cx).unwrap();
        assert_ne!(reopened, closed);
        assert_eq!(cx.windows(), vec![reopened]);
        assert_eq!(super::open_or_activate_main_window(cx).unwrap(), reopened);
        assert_eq!(cx.windows().len(), 1);
    });
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
            view.selected_session_id = Some("session".into());
            for index in 0..MAX_UI_EVENTS_PER_TICK + 3 {
                view.event_tx
                    .try_send(BackendEvent::TurnStatus(
                        json!({ "sessionId": "session", "status": format!("event-{index}") }),
                    ))
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
    assert_eq!(params, json!({ "sessionId": "session-b", "prompt": "Review **these** changes", "attachments": [] }));
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
            view.attachments.reverse();
            view.composer
                .update(cx, |state, cx| state.set_value("Summarize the attachment", window, cx));
        });
        window.render_frame(cx);
        // Stable path IDs still target the same chip after the attachment order changes.
        window.click(super::opaque_path_element_id("remove-attachment", "scratch.txt"), cx);
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
    assert_eq!(params["attachments"], json!([{ "path": "notes.md", "text": "attached notes" }]));
}

#[gpui_kit::test]
fn selecting_a_model_switches_configuration_and_resets_the_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
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
        assert_eq!(window.find("command-palette").label(), Some("Model and reasoning effort"));
        view.update(cx, |view, cx| view.run_palette_action(super::PaletteAction::ChooseModel, window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
        assert_eq!(
            view.read(cx)
                .palette
                .as_ref()
                .unwrap()
                .read(cx)
                .selected_index()
                .unwrap()
                .section,
            0,
            "the active provider and model are placed first and selected"
        );
        let groups = &view.read(cx).palette_groups;
        assert_eq!(groups[0].1[0].label, "GPT OSS 20B");
        assert!(groups[0].1[0].description.as_ref().unwrap().contains("gpt-oss:20b"));
        assert_eq!(groups[1].1[0].label, "Claude Sonnet 5");
        window.input("claude-sonnet-5", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).matched_count(), 1);
        assert!(window.find("palette-label-1-0").label().unwrap().contains("200000 tokens"));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).status, "Switching model…");
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected config select request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SELECT);
    assert_eq!(params, json!({ "provider": "anthropic", "model": "claude-sonnet-5" }));

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
    assert_eq!(params, json!({ "sessionId": "session-c", "prompt": "Keyboard first\n", "attachments": [] }));
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
fn settings_dialog_copies_the_credential_setup_command(cx: &mut TestAppContext) {
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
                        "protocol": vtcode_desktop_protocol::PROTOCOL_VERSION,
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
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-360.))), cx);
        window.click("copy-credential-command", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode login ollama"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn escape_closes_cli_config_editor_before_the_settings_sheet(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![vtcode_desktop_protocol::ConfigSetting {
                    path: "agent.default_model".into(),
                    value: json!("model-a"),
                    origin: Some("workspace".into()),
                }],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
            view.open_settings(window, cx);
            view.open_cli_config_editor("agent.default_model".into(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_some());
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_none());
        assert!(view.read(cx).settings_open);
        assert!(view.read(cx).settings_focus.contains_focused(window, cx));
    })
    .unwrap();
}

#[gpui_kit::test]
fn sync_cli_theme_persists_through_the_protocol_and_preserves_composer_draft(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(directory.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.theme_id = "ciapre-blue".into();
            view.configuration = serde_json::from_value(json!({
                "provider": "ollama",
                "model": "gpt-oss:20b",
                "authenticated": true,
                "credentialSetupCommand": "vtcode login ollama",
                "sharedUi": { "theme": "ciapre", "shortcuts": {}, "shortcutWarnings": [] }
            }))
            .ok();
            view.composer
                .update(cx, |state, cx| state.set_value("Keep this draft", window, cx));
            assert!(view.can_sync_theme_to_cli(), "ordinary composer text remains safe during theme sync");
            let submission = || super::SubmissionDraft {
                prompt: "Do not lose this message".into(),
                attachments: Vec::new(),
                echo_id: "pending-echo".into(),
                delivery_uncertain: false,
            };
            view.pending_submission = Some(submission());
            assert!(!view.can_sync_theme_to_cli(), "a queued send must be resolved first");
            view.pending_submission = None;
            view.first_submission = Some(submission());
            assert!(!view.can_sync_theme_to_cli(), "a first-session send must be resolved first");
            view.first_submission = None;
            view.retained_submission = Some(submission());
            assert!(!view.can_sync_theme_to_cli(), "an uncertain send must be restored or dismissed first");
            view.retained_submission = None;
            view.attachments_picker_open = true;
            assert!(!view.can_sync_theme_to_cli(), "an active attachment picker must finish first");
            view.attachments_picker_open = false;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx);
        window.render_frame(cx);
        window.click("sync-cli-theme", cx);
        let BackendCommand::Request { method, params } = requests.try_recv().expect("theme request") else {
            panic!("expected a theme request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_THEME);
        assert_eq!(params, json!({ "theme": "ciapre-blue" }));
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_THEME.into(),
                    result: Some(json!({
                        "provider": "ollama", "model": "gpt-oss:20b", "authenticated": true,
                        "credentialSetupCommand": "vtcode login ollama",
                        "sharedUi": { "theme": "ciapre-blue", "shortcuts": {}, "shortcutWarnings": [] }
                    })),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.theme_id, "ciapre-blue");
            assert_eq!(view.preferences.follow_cli_theme, Some(true));
            assert_eq!(view.composer.read(cx).value().as_str(), "Keep this draft");
            assert!(view.selected_session_id.is_none());
            assert_eq!(view.status, "CLI theme saved. The next message starts a new session.");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_files_open_a_read_only_preview(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("README.md"), "# Notes\n\nhello from the workspace").expect("write file");
    std::fs::create_dir_all(workspace.path().join("src/nested")).expect("create nested sources");
    std::fs::write(workspace.path().join("src/nested/mod.rs"), "pub fn nested() {}").expect("write nested source");
    std::fs::create_dir_all(workspace.path().join("target/debug")).expect("skip dir");
    std::fs::write(workspace.path().join("target/debug/artifact"), b"binary").expect("write artifact");

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.refresh_files(cx);
            assert!(view.files.is_empty(), "scan does not block the UI callback");
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        {
            let view = view.read(cx);
            assert!(!view.files.iter().any(|entry| entry.rel_path.starts_with("target")));
            assert!(view.files.iter().any(|entry| entry.rel_path == "README.md"));
        };
        window.render_frame(cx);
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        assert_eq!(file_tree_paths(&view, cx), vec!["src", "README.md"]);
        let tree_id = format!("tree-{}", view.read(cx).file_tree.entity_id());
        let tree = window
            .find_all(tree_id)
            .into_iter()
            .find(|tree| tree.role() == Some(gpui_kit::Role::Tree))
            .expect("file tree exposes its semantic role");
        assert_eq!(tree.role(), Some(gpui_kit::Role::Tree));
        let root_item = file_tree_item_snapshot(window, 0);
        assert_eq!(root_item.role(), Some(gpui_kit::Role::TreeItem));
        assert_eq!(root_item.label(), Some("src"));
        assert_eq!(root_item.expanded(), Some(false));

        window.click("workspace-file-src", cx);
        window.render_frame(cx);
        assert_eq!(file_tree_item_snapshot(window, 0).expanded(), Some(true));
        assert!(file_tree_paths(&view, cx).contains(&"src/nested".to_string()));
        assert!(view.read(cx).file_preview.is_none(), "folder activation expands instead of previewing");
        window.click("workspace-file-src", cx);
        window.render_frame(cx);
        assert_eq!(file_tree_item_snapshot(window, 0).expanded(), Some(false));

        let mut reached_file_focus = false;
        for _ in 0..32 {
            window.press("tab", cx);
            window.render_frame(cx);
            if window.find("focus-files").focused() == Some(true) {
                reached_file_focus = true;
                break;
            }
        }
        assert!(reached_file_focus, "Tab reaches the file-tree focus control");
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        let tree = view.read(cx).file_tree.clone();
        let selected_path = tree.read(cx).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected_path.as_deref(), Some("src"));
        assert_eq!(file_tree_item_snapshot(window, 0).selected(), Some(true));
        window.press("right", cx);
        assert_eq!(file_tree_item_snapshot(window, 0).expanded(), Some(true));
        assert!(file_tree_paths(&view, cx).contains(&"src/nested".to_string()));
        window.press("down", cx);
        window.press("right", cx);
        assert_eq!(file_tree_item_snapshot(window, 1).expanded(), Some(true));
        assert!(file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));
        window.press("left", cx);
        assert!(!file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));
        window.press("right", cx);
        assert!(file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));

        view.update(cx, |view, cx| view.refresh_files(cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));
        let tree = view.read(cx).file_tree.clone();
        let selected_path = tree.read(cx).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected_path.as_deref(), Some("src/nested"));
        window.press("down", cx);
        let tree = view.read(cx).file_tree.clone();
        let selected_path = tree.read(cx).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected_path.as_deref(), Some("src/nested/mod.rs"));
        window.press("enter", cx);
        assert!(view.read(cx).file_preview.is_none(), "read does not block the UI callback");
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("copy-preview", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("pub fn nested() {}"));
        window.click("close-preview", cx);
        assert!(view.read(cx).file_preview.is_none(), "close dismisses the preview");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.click("workspace-file-README.md", cx);
        assert!(view.read(cx).file_preview.is_none(), "mouse activation also reads asynchronously");
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        let preview = view.read(cx).file_preview.as_ref().map(|preview| preview.text.clone());
        assert_eq!(preview.as_deref(), Some("# Notes\n\nhello from the workspace"));
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
        window.click(super::opaque_path_element_id("change-row", "src/app.rs"), cx);
        {
            let view = view.read(cx);
            assert_eq!(view.selected_change.as_deref(), Some("src/app.rs"));
            assert!(view.change_viewer.is_some(), "selecting a file builds the diff viewer");
        }
        window.click("copy-change-patch", cx);
        assert!(cx.read_from_clipboard().unwrap().text().unwrap().contains("+here"));
        let viewer = view.read(cx).change_viewer.as_ref().unwrap().clone();
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).review_open);
        assert_eq!(window.find("diff-review-panel").role(), Some(gpui_kit::Role::Dialog));
        assert!(view.read(cx).review_focus.contains_focused(window, cx));
        window.within("diff-review-panel").click("review-split", cx);
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).mode(), gpui_kit::component::diff::DiffMode::Split);
        window.within("diff-review-panel").click("review-unified", cx);
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).mode(), gpui_kit::component::diff::DiffMode::Unified);
        for _ in 0..8 {
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).review_focus.contains_focused(window, cx));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).review_open);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/app.rs"));
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        window.click("close-diff-review", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).review_open);
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(window.within("diff-review-panel").find("change-diff-view").bounds().size.width > px(900.));
        assert!(window.within("diff-review-panel").find("change-diff-view").bounds().size.height > px(400.));
        view.update(cx, |view, _| view.reset_session_state());
        window.render_frame(cx);
        assert!(!view.read(cx).review_open);
        assert!(view.read(cx).change_viewer.is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
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
        assert_eq!(window.find("command-palette").role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(window.find("command-palette").label(), Some("Command palette"));
        assert_eq!(window.find("palette-label-0-0").label(), Some("New session"));
        for key in ["tab", "shift-tab", "tab"] {
            window.press(key, cx);
            assert!(view.read(cx).palette_focus.contains_focused(window, cx), "{key} stays inside the palette");
        }
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
fn context_usage_percent_handles_unknown_and_saturated_windows() {
    use super::context_usage_percent;
    use vtcode_exec_events::Usage;

    let usage = Usage {
        input_tokens: 30_000,
        cached_input_tokens: 10_000,
        cache_creation_tokens: 5_000,
        output_tokens: 5_000,
    };
    assert_eq!(context_usage_percent(&usage, 200_000), Some(25));
    assert_eq!(context_usage_percent(&usage, 0), None, "unknown window hides the field");
    let huge = Usage {
        input_tokens: u64::MAX,
        cached_input_tokens: u64::MAX,
        cache_creation_tokens: 0,
        output_tokens: 0,
    };
    assert_eq!(context_usage_percent(&huge, 100), Some(100), "saturates at 100");
}

#[gpui_kit::test]
fn saved_preferences_restore_theme_widths_and_inspector_state(cx: &mut TestAppContext) {
    let path = isolate_preferences();
    // Seed a preferences file with a non-default theme and pane widths.
    std::fs::write(
        &path,
        json!({
            "theme": "gruvbox-dark",
            "inspectorOpen": false,
            "sidebarWidth": 230.0,
            "inspectorWidth": 400.0,
        })
        .to_string(),
    )
    .expect("seed prefs");

    cx.update(gpui_kit::init);
    let (window, view) = cx.update(|cx| {
        gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
            let theme_mode = ThemeMode::Light;
            let entity = cx.new(|cx| DesktopView::new(window, cx, theme_mode));
            entity.update(cx, |view, cx| view.load_preferences(window, cx));
            entity
        })
        .unwrap()
    });
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            assert_eq!(view.theme_id, "gruvbox-dark");
            assert_eq!(view.theme_mode, ThemeMode::Dark);
            assert!(!view.inspector_open);
            assert!((view.sidebar_width - 230.0).abs() < f32::EPSILON);
            assert!((view.inspector_width - 400.0).abs() < f32::EPSILON);
            cx.notify();
        });
    })
    .unwrap();
    drop(std::fs::remove_file(&path));
}

#[gpui_kit::test]
fn dragging_the_sidebar_divider_resizes_the_pane(cx: &mut TestAppContext) {
    use gpui_kit::{InputEvent, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent};

    let dispatch_mouse = |window: &mut gpui_kit::Window, position, event: MousePhase, cx: &mut gpui_kit::App| {
        match event {
            MousePhase::Down => {
                let _ = window.dispatch_event(
                    MouseDownEvent {
                        position,
                        button: MouseButton::Left,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    cx,
                );
            }
            MousePhase::Move => {
                let _ = window.dispatch_event(
                    MouseMoveEvent {
                        position,
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Modifiers::default(),
                    }
                    .to_platform_input(),
                    cx,
                );
            }
            MousePhase::Up => {
                let _ = window.dispatch_event(
                    MouseUpEvent {
                        position,
                        button: MouseButton::Left,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }
                    .to_platform_input(),
                    cx,
                );
            }
        }
        window.render_frame(cx);
    };

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let handle = window.find("resize-sidebar").bounds();
        let start = point(handle.center().x, handle.center().y);

        // Press on the divider, drag right by 60px, release.
        dispatch_mouse(window, start, MousePhase::Down, cx);
        dispatch_mouse(window, point(start.x + px(60.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x + px(60.), start.y), MousePhase::Up, cx);
        let width = view.read(cx).sidebar_width;
        assert!(width > 300.0 && width < 340.0, "sidebar grew by the drag delta: {width}");

        // Dragging past the bound clamps. Re-find the handle: the first
        // drag moved it.
        let handle = window.find("resize-sidebar").bounds();
        let start = point(handle.center().x, handle.center().y);
        dispatch_mouse(window, start, MousePhase::Down, cx);
        dispatch_mouse(window, point(start.x + px(600.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x + px(600.), start.y), MousePhase::Up, cx);
        let width = view.read(cx).sidebar_width;
        assert!((width - super::super::preferences::MAX_PANE_WIDTH).abs() < f32::EPSILON, "clamped width: {width}");
    })
    .unwrap();
}

enum MousePhase {
    Down,
    Move,
    Up,
}

#[gpui_kit::test]
fn theme_switches_refresh_both_directions_and_preserve_typography_and_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(19);
            view.preferences.code_font_size = Some(14);
            view.apply_theme("solarized-light".into(), window, cx);
            view.composer
                .update(cx, |input, cx| input.set_value("keep this draft", window, cx));
            view.transcript.push(TranscriptItem::new(
                "theme-content".into(),
                "assistant".into(),
                "Assistant".into(),
                "Text and `code` stay visible.".into(),
            ));
        });
        for palette in [false, true] {
            for (mode, id) in [(ThemeMode::Dark, "system-dark"), (ThemeMode::Light, "system-light")] {
                window.render_frame(cx);
                if palette {
                    view.update(cx, |view, cx| view.run_palette_action(super::PaletteAction::ToggleTheme, window, cx));
                } else {
                    window.click("toggle-theme", cx);
                }
                window.render_frame(cx);
                assert_eq!(view.read(cx).theme_mode, mode);
                assert_eq!(view.read(cx).theme_id, id);
                assert_eq!(crate::desktop::preferences::Preferences::load().theme.as_deref(), Some(id));
                assert_eq!(cx.theme().font_size, px(19.));
                assert_eq!(cx.theme().mono_font_size, px(14.));
                assert_eq!(gpui_kit::base::Theme::global(cx).tokens.colors.foreground, cx.theme().colors.foreground);
                assert_eq!(view.read(cx).composer.read(cx).value().as_str(), "keep this draft");
                assert_eq!(view.read(cx).transcript.get(0).unwrap().id, "theme-content");
            }
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_theme_picker_applies_a_theme_live(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "ciapre", "Ciapre is the default theme");
        window.scroll("theme-grid", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-180.))), cx);
        window.click("theme-github-dark", cx);
        {
            let view = view.read(cx);
            assert_eq!(view.theme_id, "github-dark");
            assert_eq!(view.theme_mode, ThemeMode::Dark);
        }
        window.scroll("theme-grid", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(180.))), cx);
        window.click("theme-solarized-light", cx);
        {
            let view = view.read(cx);
            assert_eq!(view.theme_id, "solarized-light");
            assert_eq!(view.theme_mode, ThemeMode::Light);
        }
    })
    .unwrap();
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

#[gpui_kit::test]
fn file_tasks_discard_obsolete_workspace_and_preview_results(cx: &mut TestAppContext) {
    let first = tempfile::TempDir::new().unwrap();
    let second = tempfile::TempDir::new().unwrap();
    std::fs::write(first.path().join("old.txt"), "old workspace").unwrap();
    std::fs::write(second.path().join("new.txt"), "new workspace").unwrap();
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(first.path().to_path_buf());
            view.refresh_files(cx);
            view.open_file_preview("old.txt".to_string(), window, cx);
            view.workspace = Some(second.path().to_path_buf());
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.files.is_empty());
            assert!(view.file_preview.is_none());
            assert!(view.error.is_none());
            view.refresh_files(cx);
            view.open_file_preview("missing.txt".to_string(), window, cx);
            view.open_file_preview("new.txt".to_string(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        let view = view.read(cx);
        assert_eq!(view.files.len(), 1);
        assert_eq!(view.files[0].rel_path, "new.txt");
        assert_eq!(view.file_preview.as_ref().unwrap().text, "new workspace");
        assert!(view.error.is_none(), "superseded read cannot publish an error");
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_transitions_clear_runtime_projections_and_model_switch_keeps_sources(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session = json!({
        "sessionId": "next-session", "title": "Next", "workspace": "/workspace",
        "completed": false, "resumable": true,
    });
    for (method, result) in [
        (vtcode_desktop_protocol::method::SESSION_CREATE, session.clone()),
        (vtcode_desktop_protocol::method::SESSION_CLOSE, json!({})),
        (
            vtcode_desktop_protocol::method::SESSION_LOAD,
            json!({
                "session": session,
                "messages": [{ "role": "assistant", "text": "Restored answer" }],
            }),
        ),
        (
            vtcode_desktop_protocol::method::CONFIG_SELECT,
            json!({
                "provider": "ollama", "model": "local", "authenticated": true,
                "credentialSetupCommand": "vtcode auth",
            }),
        ),
    ] {
        cx.update_window(window, |_, _, cx| {
            view.update(cx, |view, cx| {
                view.transcript.push(TranscriptItem::new(
                    "old".into(),
                    "assistant".into(),
                    "Old".into(),
                    "Old answer".into(),
                ));
                view.upsert_diff_card(
                    "old-change".into(),
                    &vtcode_exec_events::FileChangeItem {
                        diff_incomplete: Some(false),
                        changes: vec![vtcode_exec_events::FileUpdateChange {
                            path: "old.rs".into(),
                            kind: vtcode_exec_events::PatchChangeKind::Update,
                        }],
                        status: vtcode_exec_events::PatchApplyStatus::Completed,
                        unified_diff: Some("--- a/old.rs\n+++ b/old.rs\n@@ -1 +1 @@\n-before\n+after\n".into()),
                        additions: Some(1),
                        deletions: Some(1),
                    },
                );
                assert_eq!(view.changes.entries().len(), 1);
                view.select_change("old.rs".into(), cx);
                assert!(view.change_viewer.is_some());
                view.last_usage = Some(vtcode_exec_events::Usage {
                    input_tokens: 100,
                    output_tokens: 20,
                    cached_input_tokens: 0,
                    cache_creation_tokens: 0,
                });
                view.selected_change = Some("old.rs".into());
                view.attachments
                    .push(vtcode_desktop_protocol::TurnAttachment { path: "old.rs".into(), text: "old code".into() });
                view.interaction_choice_ids.push(Some("old-choice".into()));
                view.interaction_answer_texts.push(Some("old answer".into()));
                view.interaction_step_index = 2;
                view.attachments_picker_open = true;
                view.handle_backend_event(
                    BackendEvent::Response {
                        method: method.into(),
                        result: Some(result),
                        error: None,
                    },
                    cx,
                );
                assert!(view.last_usage.is_none(), "{method}");
                assert!(view.changes.entries().is_empty(), "{method}");
                assert!(view.selected_change.is_none(), "{method}");
                assert!(view.change_viewer.is_none(), "{method}");
                if method == vtcode_desktop_protocol::method::CONFIG_SELECT {
                    assert_eq!(view.attachments.len(), 1);
                    assert_eq!(view.attachments[0].path, "old.rs");
                } else {
                    assert!(view.attachments.is_empty(), "{method}");
                }
                assert!(view.interaction_choice_ids.is_empty(), "{method}");
                assert!(view.interaction_answer_texts.is_empty(), "{method}");
                assert_eq!(view.interaction_step_index, 0, "{method}");
                assert!(!view.attachments_picker_open, "{method}");
                if method == vtcode_desktop_protocol::method::SESSION_LOAD {
                    assert_eq!(view.transcript.get(0).unwrap().text, "Restored answer");
                } else {
                    assert!(view.transcript.is_empty(), "{method}");
                }
            });
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn pending_attachments_prevent_submitting_an_incomplete_prompt(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.attachments_picker_open = true;
            view.composer
                .update(cx, |state, cx| state.set_value("Review attached file", window, cx));
            view.submit_prompt(window, cx);
            assert!(!view.running);
            assert_eq!(view.composer.read(cx).value(), "Review attached file");
            assert!(view.transcript.is_empty());
        });
        window.render_frame(cx);
        assert_eq!(window.find("send-prompt").disabled(), Some(true));
    })
    .unwrap();
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn retained_submission_restores_exact_content_without_overwriting_a_new_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Original request", window, cx));
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "source.rs".into(),
                text: "original file".into(),
            });
            view.submit_prompt(window, cx);
            assert!(view.pending_submission.is_some());
            view.composer.update(cx, |state, cx| state.set_value("New draft", window, cx));
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: None,
                    error: Some("selected session runtime has stopped".into()),
                },
                cx,
            );
            assert!(view.pending_submission.is_none());
            assert!(view.retained_submission.is_some());
            assert!(view.transcript.is_empty(), "rejected content was never sent to the agent");
            assert_eq!(view.composer.read(cx).value(), "New draft");
        });
        window.render_frame(cx);
        assert_eq!(window.find("restore-submission").disabled(), Some(true));
        assert_eq!(window.find("send-prompt").disabled(), Some(true));
        window.click("copy-retained-prompt", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("Original request"));
        view.update(cx, |view, cx| {
            view.restore_submission(window, cx);
            assert_eq!(view.composer.read(cx).value(), "New draft", "handler also guards against overwrite");
            view.composer.update(cx, |state, cx| state.set_value("", window, cx));
        });
        window.render_frame(cx);
        window.click("restore-submission", cx);
        {
            let view = view.read(cx);
            assert!(view.retained_submission.is_none());
            assert_eq!(view.composer.read(cx).value(), "Original request");
            assert_eq!(view.attachments[0].path, "source.rs");
            assert_eq!(view.attachments[0].text, "original file");
        }
        window.render_frame(cx);
        window.click("send-prompt", cx);
    })
    .unwrap();
    let BackendCommand::Request { params: first, .. } = requests.try_recv().unwrap() else {
        panic!("first request")
    };
    let BackendCommand::Request { params: retry, .. } = requests.try_recv().unwrap() else {
        panic!("retry request")
    };
    assert_eq!(first, retry, "explicit retry preserves prompt and attachment bytes");
    assert!(requests.try_recv().is_err(), "no automatic replay");
}

#[gpui_kit::test]
fn oversized_encoded_submission_keeps_composer_attachments_and_connection(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Review files", window, cx));
            view.attachments = (0..5)
                .map(|index| vtcode_desktop_protocol::TurnAttachment {
                    path: format!("file-{index}.txt"),
                    text: "\u{1}".repeat(64 * 1024),
                })
                .collect();
            view.submit_prompt(window, cx);
            assert!(!view.running);
            assert!(view.backend.is_some());
            assert!(view.pending_submission.is_none());
            assert!(view.transcript.is_empty());
            assert_eq!(view.composer.read(cx).value(), "Review files");
            assert_eq!(view.attachments.len(), 5);
            assert!(view.error.as_deref().unwrap().contains("1 MiB"));
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "oversized frame never reaches the transport");
}

#[gpui_kit::test]
fn disconnect_before_acceptance_retains_draft_with_uncertain_delivery(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Unconfirmed request", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);
            assert!(!view.running);
            assert!(view.backend.is_none());
            let draft = view.retained_submission.as_ref().unwrap();
            assert!(draft.delivery_uncertain);
            assert_eq!(draft.prompt, "Unconfirmed request");
            assert!(!view.transcript.is_empty(), "unconfirmed delivery is not a confirmed rejection");
        });
        window.render_frame(cx);
        window.click("restore-submission", cx);
        let view = view.read(cx);
        assert_eq!(view.composer.read(cx).value(), "Unconfirmed request");
        assert!(view.transcript.is_empty());
    })
    .unwrap();
    assert!(requests.try_recv().is_ok());
    assert!(requests.try_recv().is_err(), "disconnect recovery never resends automatically");
}

#[gpui_kit::test]
fn acknowledged_submission_is_not_recovered_after_disconnect(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Accepted request", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: Some(json!({ "sessionId": "session", "turnId": "turn" })),
                    error: None,
                },
                cx,
            );
            assert!(view.pending_submission.is_none());
            assert_eq!(view.active_turn_id.as_deref(), Some("turn"));
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);
            assert!(view.retained_submission.is_none(), "accepted requests must not be offered as unsent drafts");
            assert_eq!(view.transcript.get(0).unwrap().text, "Accepted request");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn delayed_acknowledgement_cannot_restart_a_completed_turn(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("Quick request", window, cx));
            view.submit_prompt(window, cx);
            assert!(view.pending_submission.is_some());
            view.handle_thread_event(vtcode_desktop_protocol::ThreadEventNotification {
                session_id: "session".into(),
                turn_id: Some("finished-turn".into()),
                event: vtcode_exec_events::VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::TurnCompleted(
                    vtcode_exec_events::TurnCompletedEvent {
                        completed_at: None,
                        usage: vtcode_exec_events::Usage {
                            input_tokens: 1,
                            output_tokens: 2,
                            cached_input_tokens: 0,
                            cache_creation_tokens: 0,
                        },
                        in_progress_exec_sessions: Vec::new(),
                    },
                )),
            });
            assert!(view.pending_submission.is_none(), "terminal runtime event proves input was accepted");
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: Some(json!({ "sessionId": "session", "turnId": "finished-turn" })),
                    error: None,
                },
                cx,
            );
            assert!(!view.running);
            assert!(view.active_turn_id.is_none());
            assert_eq!(view.status, "Completed");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn malformed_acknowledgement_retains_unconfirmed_submission(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer.update(cx, |state, cx| state.set_value("Retain this", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: Some(json!({ "turnId": "unscoped" })),
                    error: None,
                },
                cx,
            );
            assert!(!view.running);
            assert!(view.active_turn_id.is_none());
            assert!(view.retained_submission.as_ref().unwrap().delivery_uncertain);
            assert_eq!(view.retained_submission.as_ref().unwrap().prompt, "Retain this");
            assert!(view.error.as_deref().unwrap().contains("invalid turn acknowledgement"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn keyboard_shortcuts_share_session_and_cancellation_guards(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.focus_composer(window, cx);
        });
        window.render_frame(cx);
        window.press("cmd-n", cx);
        view.update(cx, |view, cx| view.handle_backend_event(BackendEvent::Response {
            method: vtcode_desktop_protocol::method::SESSION_CREATE.into(),
            result: Some(json!({ "sessionId": "session", "title": null, "workspace": "/workspace", "completed": false, "resumable": true })), error: None,
        }, cx));
        window.press("cmd-shift-w", cx);
        view.update(cx, |view, cx| view.handle_backend_event(BackendEvent::Response {
            method: vtcode_desktop_protocol::method::SESSION_CLOSE.into(), result: Some(json!({ "closed": true })), error: None,
        }, cx));
        window.press("cmd-shift-i", cx);
        assert!(!view.read(cx).inspector_open);
        assert_eq!(view.read(cx).preferences.inspector_open, Some(false));
        window.press("cmd-shift-i", cx);
        assert!(view.read(cx).inspector_open);
        view.update(cx, |view, _| {
            view.selected_session_id = Some("session".into());
            view.running = true;
            view.active_turn_id = Some("turn".into());
        });
        window.press("cmd-n", cx);
        window.press("cmd-shift-w", cx);
        window.press("escape", cx);
        assert!(view.read(cx).running, "Escape without an overlay must not cancel a run");
        assert!(!view.read(cx).cancel_requested);
        window.press("cmd-.", cx);
        assert!(view.read(cx).cancel_requested);
        assert!(view.read(cx).cancel_request_sent);
    })
    .unwrap();
    for expected in [
        vtcode_desktop_protocol::method::SESSION_CREATE,
        vtcode_desktop_protocol::method::SESSION_CLOSE,
        vtcode_desktop_protocol::method::TURN_CANCEL,
    ] {
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("request")
        };
        assert_eq!(method, expected);
        if method == vtcode_desktop_protocol::method::TURN_CANCEL {
            assert_eq!(params, json!({ "sessionId": "session", "turnId": "turn" }));
        }
    }
    assert!(requests.try_recv().is_err(), "active work blocks session changes for shortcuts too");
}

#[gpui_kit::test]
fn escape_closes_preview_and_returns_keyboard_input_to_composer(cx: &mut TestAppContext) {
    use gpui_kit::Focusable;
    let workspace = tempfile::TempDir::new().unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "Preview text").unwrap();
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.focus_composer(window, cx);
            view.open_file_preview("notes.txt".into(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.is_some());
        assert!(view.read(cx).preview_focus.is_focused(window));
        window.press("escape", cx);
        assert!(view.read(cx).file_preview.is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        window.input("Continue editing", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Continue editing");
    })
    .unwrap();
}

#[gpui_kit::test]
fn palette_cancel_restores_focus_and_composer_shortcut_dismisses_overlay(cx: &mut TestAppContext) {
    use gpui_kit::Focusable;
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-l", cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        window.press("cmd-k", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).palette.is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        window.press("cmd-k", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.press("cmd-l", cx);
        assert!(view.read(cx).palette.is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        window.input("A focused draft", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "A focused draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn first_send_creates_one_session_then_submits_the_captured_message(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer.update(cx, |state, cx| state.set_value("First request", window, cx));
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment { path: "first.rs".into(), text: "first content".into() });
        });
        window.render_frame(cx);
        assert_ne!(window.find("send-prompt").disabled(), Some(true));
        window.click("send-prompt", cx);
        assert_eq!(view.read(cx).session_request, Some(vtcode_desktop_protocol::method::SESSION_CREATE));
        assert!(view.read(cx).first_submission.is_some());
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        view.update(cx, |view, cx| {
            view.composer.update(cx, |state, cx| state.set_value("Newer unsent draft", window, cx));
            view.submit_prompt(window, cx);
            view.create_session(cx);
            view.load_session("other-session".into(), cx);
            view.select_model("other".into(), "model".into(), cx);
            assert!(!view.workspace_switch_idle());
        });
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else { panic!("create") };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CREATE);
        assert!(requests.try_recv().is_err(), "transition blocks repeated Send and session/model changes");
        view.update(cx, |view, cx| view.handle_backend_event(BackendEvent::Response {
            method: vtcode_desktop_protocol::method::SESSION_CREATE.into(),
            result: Some(json!({ "sessionId": "created", "title": null, "workspace": "/workspace", "completed": false, "resumable": true })), error: None,
        }, cx));
        assert!(view.read(cx).first_submission.is_none());
        assert!(view.read(cx).session_request.is_none());
        assert!(view.read(cx).running);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Newer unsent draft");
        assert_eq!(view.read(cx).transcript.list_state().item_count(), 1);
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else { panic!("submit") };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
        assert_eq!(params, json!({ "sessionId": "created", "prompt": "First request", "attachments": [{ "path": "first.rs", "text": "first content" }] }));
        assert!(requests.try_recv().is_err(), "exactly one turn is submitted");
    }).unwrap();
}

#[gpui_kit::test]
fn session_creation_failure_retains_first_message_and_unlocks_transitions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer
                .update(cx, |state, cx| state.set_value("Keep first message", window, cx));
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "keep.txt".into(),
                text: "keep attachment".into(),
            });
            view.submit_prompt(window, cx);
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CREATE.into(),
                    result: None,
                    error: Some("Provider could not start".into()),
                },
                cx,
            );
            assert!(view.first_submission.is_none());
            assert!(view.session_request.is_none());
            assert!(!view.running);
            assert!(view.selected_session_id.is_none());
            assert!(view.transcript.is_empty());
            assert!(view.retained_submission.is_some());
            assert_eq!(view.error.as_deref(), Some("Provider could not start"));
        });
        window.render_frame(cx);
        window.click("restore-submission", cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep first message");
        assert_eq!(view.read(cx).attachments[0].text, "keep attachment");
        window.render_frame(cx);
        window.click("send-prompt", cx);
    })
    .unwrap();
    for _ in 0..2 {
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("create")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CREATE);
    }
    assert!(requests.try_recv().is_err(), "failed creation never submits a turn");
}

#[gpui_kit::test]
fn session_load_blocks_submission_until_the_new_session_is_selected(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(3);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("old-session".into());
            view.composer.update(cx, |state, cx| state.set_value("Send to loaded session", window, cx));
            view.load_session("new-session".into(), cx);
            view.submit_prompt(window, cx);
            view.close_session(cx);
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({ "session": { "sessionId": "new-session", "title": "New", "workspace": "/workspace", "completed": false, "resumable": true }, "messages": [] })), error: None,
            }, cx);
            view.submit_prompt(window, cx);
        });
    }).unwrap();
    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("load")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("submit")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params["sessionId"], "new-session");
    assert_eq!(params["prompt"], "Send to loaded session");
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn first_message_survives_failed_handoff_after_session_creation(cx: &mut TestAppContext) {
    for malformed in [false, true] {
        let (window, view) = open_desktop(cx);
        let (commands, mut requests) = tokio::sync::mpsc::channel(1);
        cx.update_window(window, |_, window, cx| {
            view.update(cx, |view, cx| {
                view.backend = Some(BackendHandle { commands: commands.clone() });
                view.composer.update(cx, |state, cx| state.set_value("Captured first message", window, cx));
                view.submit_prompt(window, cx);
                assert!(requests.try_recv().is_ok());
                if !malformed {
                    commands.try_send(BackendCommand::Stop).unwrap();
                }
                view.composer.update(cx, |state, cx| state.set_value("New draft stays", window, cx));
                view.handle_backend_event(BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CREATE.into(),
                    result: Some(if malformed { json!({}) } else { json!({ "sessionId": "created", "title": null, "workspace": "/workspace", "completed": false, "resumable": true }) }), error: None,
                }, cx);
                assert!(view.first_submission.is_none());
                assert!(view.session_request.is_none());
                assert!(view.pending_submission.is_none());
                assert!(!view.running);
                assert!(view.transcript.is_empty());
                assert_eq!(view.retained_submission.as_ref().unwrap().prompt, "Captured first message");
                assert_eq!(view.composer.read(cx).value(), "New draft stays");
                assert_eq!(view.selected_session_id.as_deref(), if malformed { None } else { Some("created") });
                assert!(view.error.is_some());
            });
        }).unwrap();
    }
}

#[gpui_kit::test]
fn blank_first_prompt_and_full_creation_queue_keep_the_composer_intact(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands: commands.clone() });
            view.composer.update(cx, |state, cx| state.set_value("   ", window, cx));
            view.submit_prompt(window, cx);
            assert!(requests.try_recv().is_err(), "blank Enter must not create empty sessions");
            commands.try_send(BackendCommand::Stop).unwrap();
            view.composer
                .update(cx, |state, cx| state.set_value("Keep unsent message", window, cx));
            view.attachments
                .push(vtcode_desktop_protocol::TurnAttachment { path: "notes.txt".into(), text: "notes".into() });
            view.submit_prompt(window, cx);
            assert!(view.session_request.is_none());
            assert!(view.first_submission.is_none());
            assert!(view.retained_submission.is_none());
            assert_eq!(view.composer.read(cx).value(), "Keep unsent message");
            assert_eq!(view.attachments[0].text, "notes");
            assert!(view.error.as_deref().unwrap().contains("queue is full"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn disconnect_during_creation_retains_a_message_that_was_never_submitted(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer
                .update(cx, |state, cx| state.set_value("Never submitted", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);
            assert!(view.session_request.is_none());
            assert!(view.first_submission.is_none());
            let retained = view.retained_submission.as_ref().unwrap();
            assert_eq!(retained.prompt, "Never submitted");
            assert!(!retained.delivery_uncertain, "only session creation crossed the wire");
            assert!(view.transcript.is_empty());
        });
    })
    .unwrap();
    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("create")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CREATE);
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn stale_runtime_notifications_cannot_finish_a_new_submission_or_reopen_a_closed_session(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ThreadEvent, TurnFailedEvent, TurnStartedEvent, VersionedThreadEvent};
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.composer
                .update(cx, |state, cx| state.set_value("First request", window, cx));
            view.submit_prompt(window, cx);
            let failure = || {
                BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
                    session_id: "session".into(),
                    turn_id: Some("old-turn".into()),
                    event: VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
                        completed_at: None,
                        message: "Old failure".into(),
                        usage: None,
                    })),
                })
            };
            view.handle_backend_event(failure(), cx);
            assert!(!view.running);
            assert_eq!(view.finished_turn_id.as_deref(), Some("old-turn"));
            view.composer
                .update(cx, |state, cx| state.set_value("Second request", window, cx));
            view.submit_prompt(window, cx);
            view.handle_backend_event(failure(), cx);
            assert!(view.running, "old terminal tails cannot finish an unacknowledged submission");
            assert_eq!(view.status, "Submitting turn…");
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
                    result: Some(json!({ "sessionId": "session", "turnId": "new-turn" })),
                    error: None,
                },
                cx,
            );
            view.handle_backend_event(failure(), cx);
            view.handle_backend_event(
                BackendEvent::TurnStatus(json!({ "sessionId": "session", "turnId": "old-turn", "status": "failed" })),
                cx,
            );
            assert!(view.running);
            assert_eq!(view.active_turn_id.as_deref(), Some("new-turn"));
            assert_eq!(view.status, "Running");
            view.handle_backend_event(
                BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
                    session_id: "other-session".into(),
                    turn_id: Some("other-turn".into()),
                    event: VersionedThreadEvent::new(ThreadEvent::TurnStarted(TurnStartedEvent::default())),
                }),
                cx,
            );
            assert_eq!(view.active_turn_id.as_deref(), Some("new-turn"));
            view.handle_backend_event(
                BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
                    session_id: "session".into(),
                    turn_id: Some("new-turn".into()),
                    event: VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
                        completed_at: None,
                        message: "New failure".into(),
                        usage: None,
                    })),
                }),
                cx,
            );
            assert!(!view.running, "the current turn still reaches a terminal state");
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_CLOSE.into(),
                    result: Some(json!({ "closed": true })),
                    error: None,
                },
                cx,
            );
            view.handle_backend_event(failure(), cx);
            view.handle_backend_event(
                BackendEvent::TurnStatus(json!({ "sessionId": "session", "turnId": "old-turn", "status": "failed" })),
                cx,
            );
            assert!(view.transcript.is_empty());
            assert!(!view.running);
            assert_eq!(view.status, "Session closed. Start a new session to continue.");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn interaction_notifications_preserve_startup_questions_and_reject_stale_turns(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{InteractionKind, InteractionRequest};
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.running = true;
            view.active_turn_id = Some("active-turn".into());
            let request = |session: &str, turn: &str, id: &str| {
                BackendEvent::Interaction(InteractionRequest {
                    session_id: session.into(),
                    turn_id: turn.into(),
                    interaction_id: id.into(),
                    kind: InteractionKind::Question,
                    wizard: false,
                    title: "Question".into(),
                    details: Vec::new(),
                    choices: Vec::new(),
                    steps: Vec::new(),
                    selected_choice_id: None,
                    selected_choice_ids: Vec::new(),
                })
            };
            view.handle_backend_event(request("other-session", "active-turn", "wrong-session"), cx);
            view.handle_backend_event(request("session", "old-turn", "wrong-turn"), cx);
            assert!(view.interaction.is_none());
            view.handle_backend_event(request("session", "session-start", "startup-question"), cx);
            assert_eq!(view.interaction.as_ref().unwrap().interaction_id, "startup-question");
            view.handle_backend_event(request("session", "active-turn", "current-question"), cx);
            assert_eq!(view.interaction.as_ref().unwrap().interaction_id, "current-question");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn rejected_draft_survives_same_session_reload_and_blocks_context_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let other_workspace = tempfile::TempDir::new().unwrap();
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let summary = json!({ "sessionId": "session", "title": "Current", "workspace": "/workspace", "completed": false, "resumable": true });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session".into());
            view.sessions = vec![serde_json::from_value(summary.clone()).unwrap(), serde_json::from_value(json!({
                "sessionId": "other", "title": "Other", "workspace": "/workspace", "completed": true, "resumable": true,
            })).unwrap()];
            view.composer.update(cx, |state, cx| state.set_value("Recover this request", window, cx));
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment { path: "source.rs".into(), text: "original source".into() });
            view.submit_prompt(window, cx);
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(), result: None,
                error: Some("selected session runtime has stopped; reload the session to continue".into()),
            }, cx);
        });
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else { panic!("submission") };
        assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
        window.render_frame(cx);
        assert_eq!(window.find("new-session").disabled(), Some(true));
        assert_eq!(window.find("close-session").disabled(), Some(true));
        assert_eq!(window.find("session-other").disabled(), Some(true));
        assert_ne!(window.find("session-session").disabled(), Some(true), "same-session reload stays available");
        view.update(cx, |view, cx| {
            let workspace_before = view.workspace.clone();
            view.create_session(cx);
            view.close_session(cx);
            view.load_session("other".into(), cx);
            view.select_model("other-provider".into(), "other-model".into(), cx);
            view.select_workspace(other_workspace.path().to_path_buf(), cx);
            assert_eq!(view.workspace, workspace_before);
            assert!(view.session_request.is_none());
            assert!(view.retained_submission.is_some());
        });
        assert!(requests.try_recv().is_err(), "guards protect keyboard and palette paths too");
        window.render_frame(cx);
        window.click("session-session", cx);
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else { panic!("reload") };
        assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LOAD);
        assert_eq!(params, json!({ "sessionId": "session" }));
        window.render_frame(cx);
        assert_eq!(window.find("restore-submission").disabled(), Some(true));
        view.update(cx, |view, cx| {
            view.restore_submission(window, cx);
            assert!(view.composer.read(cx).value().is_empty());
            assert!(view.attachments.is_empty());
            let draft = view.retained_submission.as_ref().unwrap();
            assert_eq!(draft.prompt, "Recover this request");
            assert_eq!(draft.attachments[0].text, "original source");
        });
        view.update(cx, |view, cx| view.handle_backend_event(BackendEvent::Response {
            method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
            result: Some(json!({ "session": summary, "messages": [{ "role": "assistant", "text": "Restored history" }] })), error: None,
        }, cx));
        assert_eq!(view.read(cx).transcript.get(0).unwrap().text, "Restored history");
        assert!(view.read(cx).retained_submission.is_some());
        window.render_frame(cx);
        window.click("restore-submission", cx);
        let view = view.read(cx);
        assert_eq!(view.composer.read(cx).value(), "Recover this request");
        assert_eq!(view.attachments.len(), 1);
        assert_eq!(view.attachments[0].path, "source.rs");
        assert_eq!(view.attachments[0].text, "original source");
        assert!(view.retained_submission.is_none());
        assert!(view.session_change_allowed(None));
    }).unwrap();
    assert!(requests.try_recv().is_err(), "reload and restoration never auto-resend");
}

#[gpui_kit::test]
fn initialization_gates_commands_until_the_negotiated_protocol_is_validated(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.initialize_backend(BackendHandle { commands });
            view.composer.update(cx, |state, cx| state.set_value("Wait for connection", window, cx));
            view.submit_prompt(window, cx);
            view.create_session(cx);
            assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::INITIALIZE));
            assert_eq!(view.composer.read(cx).value(), "Wait for connection");
            assert!(!view.running);
            assert!(view.service_info.is_none());
            let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else { panic!("initialize") };
            assert_eq!(method, vtcode_desktop_protocol::method::INITIALIZE);
            assert_eq!(params["protocol"]["minimum"], json!(vtcode_desktop_protocol::PROTOCOL_VERSION));
            assert!(requests.try_recv().is_err(), "no discovery or user commands before the handshake");
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::INITIALIZE.into(),
                result: Some(json!({ "protocol": vtcode_desktop_protocol::PROTOCOL_VERSION, "workspace": "/workspace", "serverVersion": "test" })),
                error: None,
            }, cx);
            assert!(view.service_info.is_some());
            assert!(view.session_request.is_none());
            assert!(view.session_change_idle());
            for expected in [
                vtcode_desktop_protocol::method::CONFIG_DISCOVER,
                vtcode_desktop_protocol::method::CONFIG_SETTINGS_READ,
                vtcode_desktop_protocol::method::CONFIG_MODELS,
                vtcode_desktop_protocol::method::SESSION_LIST,
            ] {
                let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else { panic!("discovery") };
                assert_eq!(method, expected);
            }
            assert!(requests.try_recv().is_err());
        });
    }).unwrap();
}

#[gpui_kit::test]
fn rejected_or_malformed_initialization_disconnects_without_discovery(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    for (result, error, expected_error) in [
        (None, Some("unsupported protocol version".to_string()), "unsupported protocol version"),
        (Some(json!({})), None, "invalid or incompatible initialization"),
        (
            Some(
                json!({ "protocol": { "major": 99, "minor": 0 }, "workspace": "/workspace", "serverVersion": "test" }),
            ),
            None,
            "invalid or incompatible initialization",
        ),
    ] {
        let (commands, mut requests) = tokio::sync::mpsc::channel(4);
        cx.update_window(window, |_, _, cx| {
            view.update(cx, |view, cx| {
                view.initialize_backend(BackendHandle { commands });
                let _initialize = requests.try_recv().unwrap();
                view.handle_backend_event(
                    BackendEvent::Response {
                        method: vtcode_desktop_protocol::method::INITIALIZE.into(),
                        result,
                        error,
                    },
                    cx,
                );
                assert!(view.backend.is_none());
                assert!(view.session_request.is_none());
                assert!(view.service_info.is_none());
                assert!(view.error.as_deref().unwrap().contains(expected_error));
                assert!(requests.try_recv().is_err(), "failed initialization must not enqueue discovery");
                view.handle_backend_event(BackendEvent::Exited { status: Some(0), authentication_required: false }, cx);
                assert!(
                    view.error.as_deref().unwrap().contains(expected_error),
                    "shutdown preserves the handshake error"
                );
            });
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn palette_filters_unavailable_commands_as_session_state_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_command_palette(window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.input("Close session", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).matched_count(), 0, "no selected session to close");
        assert!(palette.read(cx).selected_index().is_none());
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("selected-session".into());
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(palette.read(cx).matched_count(), 1, "the same query becomes actionable after selection");
        assert_eq!(window.find("palette-label-0-1").label(), Some("Close session"));
        view.update(cx, |view, cx| {
            view.running = true;
            view.run_palette_action(super::PaletteAction::CloseSession, window, cx);
            assert!(view.palette.is_some(), "deferred confirmation must recheck availability");
        });
        window.render_frame(cx);
        assert_eq!(palette.read(cx).matched_count(), 0, "active work prevents session changes");
        assert!(palette.read(cx).selected_index().is_none());
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "an unavailable confirmation cannot send a session-close request");
}

#[gpui_kit::test]
fn inspector_tabs_expose_selection_and_keyboard_navigation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.within("inspector-tabs").find(0usize).role(), Some(gpui_kit::Role::Tab));
        assert_eq!(window.within("inspector-tabs").find(0usize).selected(), Some(true));
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Files);
        assert_eq!(window.within("inspector-tabs").find(1usize).selected(), Some(true));
        for (key, expected) in [
            ("right", crate::desktop::inspector::InspectorTab::Activity),
            ("end", crate::desktop::inspector::InspectorTab::Utilities),
            ("home", crate::desktop::inspector::InspectorTab::Changes),
            ("left", crate::desktop::inspector::InspectorTab::Utilities),
            ("right", crate::desktop::inspector::InspectorTab::Changes),
        ] {
            window.press(key, cx);
            window.render_frame(cx);
            assert_eq!(view.read(cx).inspector_tab, expected, "{key}");
        }
        window.press("cmd-l", cx);
        window.input("composer text", cx);
        window.press("left", cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Changes);
        assert_eq!(view.read(cx).composer.read(cx).value(), "composer text");
    })
    .unwrap();
}

#[gpui_kit::test]
fn welcome_actions_browse_files_and_keep_current_model_without_resetting_a_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.inspector_open = false;
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("keep-session".into());
            // Custom active models need not appear in the built-in catalog.
            view.configuration = Some(
                serde_json::from_value(json!({
                    "provider": "provider-a", "model": "custom-current-id", "authenticated": true,
                    "credentialSetupCommand": "", "contextWindow": 123456,
                }))
                .unwrap(),
            );
            view.models = vec![vtcode_desktop_protocol::ModelInfo {
                provider: "provider-b".into(),
                id: "other-id".into(),
                display_name: "Other model".into(),
                context_window: Some(7000),
                current: false,
            }];
            view.composer
                .update(cx, |state, cx| state.set_value("Keep my draft", window, cx));
        });
        window.render_frame(cx);
        window.click("welcome-browse-files", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Files);
        window.click("welcome-choose-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
        assert!(
            window
                .find("palette-label-0-0")
                .label()
                .unwrap()
                .contains("123456 tokens · Current")
        );
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).palette.is_none());
        assert_eq!(view.read(cx).selected_session_id.as_deref(), Some("keep-session"));
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep my draft");
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "choosing the current model must not restart the session");
}

#[gpui_kit::test]
fn model_picker_revalidates_a_deferred_selection_against_the_current_catalog(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("original-session".into());
            view.models = vec![vtcode_desktop_protocol::ModelInfo {
                provider: "provider-b".into(),
                id: "outdated-model".into(),
                display_name: "Temporary model".into(),
                context_window: None,
                current: false,
            }];
            view.open_model_palette(window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").press("enter", cx);
        // The kit defers confirmation, so a catalog refresh can win this race.
        view.update(cx, |view, cx| {
            view.models.clear();
            cx.notify();
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).palette.is_some());
        assert_eq!(view.read(cx).selected_session_id.as_deref(), Some("original-session"));
        assert!(view.read(cx).session_request.is_none());
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "a removed model cannot be selected by a stale callback");
}

#[gpui_kit::test]
fn navigation_can_collapse_and_restore_without_losing_the_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("toggle-sidebar").label(), Some("Toggle navigation"));
        window.press("cmd-l", cx);
        window.input("keep this draft", cx);
        window.press("cmd-b", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).sidebar_open);
        assert_eq!(view.read(cx).composer.read(cx).value(), "keep this draft");
        window.click("toggle-sidebar", cx);
        window.render_frame(cx);
        assert!(view.read(cx).sidebar_open);
        assert_eq!(view.read(cx).composer.read(cx).value(), "keep this draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn font_pickers_confirm_defaults_and_cancel_back_to_appearance(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        window.within("settings-panel").click("choose-ui-font", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose UI font"));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert!(view.read(cx).settings_open);
        assert_eq!(cx.theme().font_family.as_str(), ".SystemUIFont");
        assert_eq!(cx.theme().font_size, px(16.));
        assert_eq!(cx.theme().mono_font_size, px(14.));
        window.within("settings-panel").click("choose-code-font", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose code font"));
        window.within("command-palette").press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert!(view.read(cx).settings_open);
        assert!(view.read(cx).preferences.code_font_family.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn appearance_sizes_apply_live_survive_theme_changes_and_reset_only_appearance(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.settings_open = true;
        });
        window.render_frame(cx);
        window.within("settings-panel").click("font-ui-1", cx);
        window.render_frame(cx);
        window.within("settings-panel").click("font-code-0", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.ui_font_size(), 17);
        assert_eq!(view.read(cx).preferences.code_font_size(), 13);
        assert_eq!(cx.theme().font_size, px(17.));
        assert_eq!(cx.theme().mono_font_size, px(13.));
        let saved = crate::desktop::preferences::Preferences::load();
        assert_eq!((saved.ui_font_size(), saved.code_font_size()), (17, 13));
        view.update(cx, |view, cx| {
            view.preferences.touch_workspace("/preserve-workspace");
            view.apply_theme("ciapre".into(), window, cx);
        });
        assert_eq!(cx.theme().font_size, px(17.));
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-240.))), cx);
        window.within("settings-panel").click("reset-appearance", cx);
        window.render_frame(cx);
        assert_eq!(cx.theme().font_size, px(16.));
        assert_eq!(cx.theme().mono_font_size, px(14.));
        assert_eq!(view.read(cx).preferences.recent_workspaces[0], "/preserve-workspace");
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(24);
            view.preferences.code_font_size = Some(10);
            view.apply_typography(cx);
        });
        window.render_frame(cx);
        assert_eq!(window.within("settings-panel").find("font-ui-1").disabled(), Some(true));
        assert_eq!(window.within("settings-panel").find("font-code-0").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn appearance_dialog_traps_focus_and_escape_restores_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-l", cx);
        window.input("preserve this draft", cx);
        window.press("cmd-,", cx);
        window.render_frame(cx);
        assert!(view.read(cx).settings_open);
        assert_eq!(window.find("settings-panel").role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(window.find("theme-ciapre-blue").role(), Some(gpui_kit::Role::Button));
        assert!(view.read(cx).settings_focus.contains_focused(window, cx));
        for _ in 0..8 {
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).settings_focus.contains_focused(window, cx));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).settings_open);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
        assert_eq!(view.read(cx).composer.read(cx).value(), "preserve this draft");
        window.press("cmd-,", cx);
        window.render_frame(cx);
        window.click("close-settings", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).settings_open);
    })
    .unwrap();
}

#[gpui_kit::test]
fn welcome_review_action_opens_changes_without_creating_a_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("welcome-review-changes", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Changes);
        assert!(view.read(cx).selected_session_id.is_none());
        assert_eq!(window.find("send-prompt").label(), Some("Send message"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn selected_diff_tracks_runtime_updates_and_keeps_unrelated_review_state(cx: &mut TestAppContext) {
    use gpui_kit::component::diff::{DiffLineRange, DiffMode, DiffSide};
    use vtcode_exec_events::{
        FileChangeItem, FileUpdateChange, ItemCompletedEvent, PatchApplyStatus, PatchChangeKind, ThreadEvent,
        ThreadItem, ThreadItemDetails,
    };

    fn notification(id: &str, path: &str, patch: &str) -> BackendEvent {
        BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
            session_id: "review-session".into(),
            turn_id: None,
            event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    context: None,
                    id: id.into(),
                    details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                        changes: vec![FileUpdateChange { path: path.into(), kind: PatchChangeKind::Update }],
                        status: PatchApplyStatus::Completed,
                        unified_diff: Some(patch.into()),
                        diff_incomplete: Some(false),
                        additions: None,
                        deletions: None,
                    })),
                },
            })),
        })
    }
    let first = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+first\n";
    let latest = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1,2 @@\n-first\n+latest one\n+latest two\n";
    let unrelated = "--- a/docs/app.rs.bak\n+++ b/docs/app.rs.bak\n@@ -1 +1 @@\n-note\n+mentions src/app.rs\n";
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("review-session".into());
            view.handle_backend_event(notification("edit", "src/app.rs", first), cx);
        });
        window.render_frame(cx);
        window.click(super::opaque_path_element_id("change-row", "src/app.rs"), cx);
        let viewer = view.read(cx).change_viewer.as_ref().unwrap().clone();
        viewer.update(cx, |viewer, cx| {
            viewer.set_mode(DiffMode::Split, cx);
            viewer.set_selected_lines(Some(DiffLineRange::new("src/app.rs", DiffSide::Modified, 1, 1)), cx);
        });
        let selection = viewer.read(cx).selected_lines();
        assert!(selection.is_some());
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).review_open);
        view.update(cx, |view, cx| view.handle_backend_event(notification("other", "docs/app.rs.bak", unrelated), cx));
        window.render_frame(cx);
        assert_eq!(
            window
                .find(super::opaque_path_element_id("change-row", "docs/app.rs.bak"))
                .label(),
            Some("Review docs/app.rs.bak")
        );
        assert_eq!(
            window.find(super::opaque_path_element_id("change-row", "src/app.rs")).label(),
            Some("Review src/app.rs"),
            "a newly inserted row must not inherit an existing file's accessibility node"
        );
        assert_eq!(viewer.read(cx).selected_lines(), selection, "unrelated events preserve review selection");
        // Updating the same item id exercises the streaming/upsert path, not only a new change-set.
        view.update(cx, |view, cx| view.handle_backend_event(notification("edit", "src/app.rs", latest), cx));
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).files()[0].additions(), 2, "the retained viewer must receive the accepted update");
        assert_eq!(viewer.read(cx).mode(), DiffMode::Split);
        assert!(viewer.read(cx).selected_lines().is_none(), "new text invalidates an old line selection");
        window.within("diff-review-panel").click("copy-change-patch", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(latest));
        window
            .within("diff-review-panel")
            .click(super::opaque_path_element_id("change-row", "docs/app.rs.bak"), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("docs/app.rs.bak"));
        assert_eq!(view.read(cx).change_viewer.as_ref().unwrap().read(cx).mode(), DiffMode::Split);
        window
            .within("diff-review-panel")
            .click(super::opaque_path_element_id("change-row", "src/app.rs"), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/app.rs"));
        // A malformed replacement invalidates this item's preview rather than leaving a stale viewer.
        view.update(cx, |view, cx| view.handle_backend_event(notification("edit", "src/app.rs", "invalid patch"), cx));
        assert!(view.read(cx).selected_change.is_none());
        assert!(view.read(cx).change_viewer.is_none());
        window.render_frame(cx);
        assert!(!view.read(cx).review_open);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn loaded_session_restores_review_without_replaying_live_run_state(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    let patch = format!(
        "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-{}\n+{}\n",
        "original word ".repeat(30),
        "restored word ".repeat(35)
    );
    let item = ThreadItem {
        context: None,
        id: "saved-edit".into(),
        details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
            changes: vec![FileUpdateChange {
                path: "src/app.rs".into(),
                kind: PatchChangeKind::Update,
            }],
            status: PatchApplyStatus::Completed,
            unified_diff: Some(patch.clone()),
            diff_incomplete: Some(false),
            additions: None,
            deletions: None,
        })),
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                error: None,
                result: Some(json!({
                    "session": {"sessionId": "restored", "title": "Saved work", "workspace": "/workspace", "completed": true, "resumable": true},
                    "messages": [{"role": "user", "text": "Make this change"}],
                    "reviewItems": [item], "reviewHistoryIncomplete": true,
                })),
            }, cx);
            assert!(!view.running);
            assert!(view.active_turn_id.is_none());
            assert_eq!(view.selected_session_id.as_deref(), Some("restored"));
            assert!(view.status.contains("partial"));
            assert!(view.restored_review_history_incomplete);
            assert!(!view.restored_activity_history_incomplete);
            assert_eq!(view.changes.entries().len(), 1);
            assert_eq!(view.changes.patch_for(&view.transcript, "src/app.rs").as_deref(), Some(patch.as_str()));
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Changes;
        });
        window.render_frame(cx);
        let warning = window.find("review-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some(super::PARTIAL_REVIEW_HISTORY_MESSAGE));
        window.click(super::opaque_path_element_id("change-row", "src/app.rs"), cx);
        window.render_frame(cx);
        window.click("copy-change-patch", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(patch.as_str()));
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).review_open);
        window.click("review-split", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        let viewport = window.find("change-diff-view").bounds();
        let original = window.within(("old", 0usize)).find("source").bounds();
        let modified = window.within(("new", 0usize)).find("source").bounds();
        assert!(original.size.width > px(100.));
        assert!(modified.size.width > px(100.));
        assert!(original.right() <= modified.left());
        assert!(modified.right() <= viewport.right() + px(1.), "both wrapped split columns must remain visible");
    }).unwrap();
}

#[gpui_kit::test]
fn loaded_activity_restores_output_and_interrupts_unfinished_history(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    let output = "first line\nsecond line\n";
    let command = ThreadItem {
        id: "saved-command".into(),
        context: None,
        details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "cargo check --locked".into(),
            arguments: None,
            aggregated_output: output.into(),
            exit_code: None,
            status: CommandExecutionStatus::InProgress,
        })),
    };
    let finished = ThreadItem {
        id: "finished-command".into(),
        context: None,
        details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "previous command".into(),
            arguments: None,
            aggregated_output: "previous output".into(),
            exit_code: Some(0),
            status: CommandExecutionStatus::Completed,
        })),
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(), error: None,
                result: Some(json!({
                    "session": {"sessionId": "saved", "title": "Saved work", "workspace": "/workspace", "completed": true, "resumable": true},
                    "messages": [{"role": "tool", "text": "Unstructured archive result"}],
                    "activityItems": [finished, command], "activityHistoryIncomplete": true,
                })),
            }, cx);
            assert!(!view.running);
            assert!(view.active_turn_id.is_none());
            assert!(view.interaction.is_none());
            assert!(view.status.contains("partial"));
            assert!(!view.restored_review_history_incomplete);
            assert!(view.restored_activity_history_incomplete);
            let rows = crate::desktop::inspector::activity_rows(&view.transcript, &view.restored_activity);
            assert_eq!(rows.len(), 2, "archive text is retained without duplicating structured activity");
            assert_eq!(view.transcript.items().filter(|item| item.role == "tool").count(), 1,
                "only the original archive tool card remains in the conversation");
            assert!(!view.transcript.get(0).unwrap().expanded);
            assert!(!view.transcript.get(0).unwrap().has_activity_status);
            assert_eq!(rows[0].state, crate::desktop::inspector::ActivityState::Completed);
            assert_eq!(rows[1].status, "interrupted");
            assert_eq!(rows[1].state, crate::desktop::inspector::ActivityState::Interrupted);
            assert!(view.transcript.items().any(|item| item.text == "Unstructured archive result"));
            let restored_output = crate::desktop::inspector::latest_output(&view.transcript, &view.restored_activity).unwrap();
            assert_eq!(restored_output.text, output);
            assert!(!restored_output.running);
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
        });
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some(super::PARTIAL_ACTIVITY_HISTORY_MESSAGE));
        assert_eq!(window.find("activity-saved-command").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("activity-saved-command").label(), Some("cargo check --locked"));
        view.update(cx, |view, _| view.inspector_tab = crate::desktop::inspector::InspectorTab::Output);
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some(super::PARTIAL_ACTIVITY_HISTORY_MESSAGE));
        assert_eq!(window.find("output-body").label(), Some(output));
        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(output));
        // A new live item still reports running, independent of historical display.
        view.update(cx, |view, _| {
            view.update_runtime_item(&ThreadItem {
                id: "saved-command".into(), context: None,
                details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                    command: "live".into(), arguments: None, aggregated_output: "live output".into(),
                    exit_code: None, status: CommandExecutionStatus::InProgress,
                })),
            });
            assert!(crate::desktop::inspector::latest_output(&view.transcript, &view.restored_activity).unwrap().running);
            assert_eq!(view.restored_activity.len(), 1, "live identity replaces its historical projection");
            assert_eq!(crate::desktop::inspector::activity_rows(&view.transcript, &view.restored_activity).len(), 2);
            view.reset_session_state();
            assert!(view.restored_activity.is_empty());
            assert!(!view.restored_review_history_incomplete);
            assert!(!view.restored_activity_history_incomplete);
            assert!(crate::desktop::inspector::latest_output(&view.transcript, &view.restored_activity).is_none());
        });
    }).unwrap();
}

#[gpui_kit::test]
fn partial_history_warnings_remain_visible_when_no_rows_were_restored(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.inspector_open = true;
        view.restored_review_history_incomplete = true;
        view.restored_activity_history_incomplete = true;
        view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some(super::PARTIAL_ACTIVITY_HISTORY_MESSAGE));

        view.update(cx, |view, _| view.inspector_tab = crate::desktop::inspector::InspectorTab::Output);
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));

        view.update(cx, |view, _| view.inspector_tab = crate::desktop::inspector::InspectorTab::Changes);
        window.render_frame(cx);
        let warning = window.find("review-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some(super::PARTIAL_REVIEW_HISTORY_MESSAGE));

        view.update(cx, |view, _| view.reset_session_state());
        window.render_frame(cx);
        assert!(window.try_find("review-history-warning").is_none());
        assert!(window.try_find("activity-history-warning").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn untracked_review_renders_added_empty_and_spaced_paths_and_reports_omissions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    let patch = "diff --git a/empty.rs b/empty.rs\nnew file mode 100644\ndiff --git a/new file.rs b/new file.rs\nnew file mode 100644\n--- /dev/null\n+++ b/new file.rs\n@@ -0,0 +1 @@\n+new content\n";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(), server_version: "test".into(),
            });
            view.open_workspace_review(window, cx);
        });
        window.render_frame(cx);
        window.click("workspace-review-include-untracked", cx);
        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else { panic!("review request") };
        assert_eq!(params["includeUntracked"], true);
        view.update(cx, |view, cx| {
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::WORKSPACE_REVIEW.into(), error: None,
                result: Some(json!({"requestId": params["requestId"], "workspace":"/workspace", "capturedAt":"now", "includeUntracked":true, "patch":patch, "truncated":false, "note":"Known omissions: 0 untracked files"})),
            }, cx);
            let viewer = view.workspace_review.as_ref().unwrap().viewer.as_ref().unwrap().read(cx);
            assert_eq!(viewer.files().len(), 2);
            assert_eq!(viewer.files()[0].path().as_str(), "empty.rs");
            assert_eq!(viewer.files()[1].path().as_str(), "new file.rs");
            assert_eq!(viewer.files()[1].additions(), 1);
        });
        window.render_frame(cx);
        window.click("copy-workspace-review", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(patch));
        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else { panic!("review request") };
        view.update(cx, |view, cx| view.handle_backend_event(BackendEvent::Response {
            method: vtcode_desktop_protocol::method::WORKSPACE_REVIEW.into(), error: None,
            result: Some(json!({"requestId": params["requestId"], "workspace":"/workspace", "capturedAt":"now", "includeUntracked":true, "patch":"", "truncated":true, "note":"Known omissions: 1 untracked file"})),
        }, cx));
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-clean").label(), Some("No previewable changes"));
        assert_eq!(window.find("copy-workspace-review").disabled(), Some(true));
    }).unwrap();
}

#[gpui_kit::test]
fn working_tree_review_is_independent_refreshable_and_rejects_stale_results(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    let patch = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-before\n+after\n";
    let snapshot = |request_id: &str, workspace: &str| BackendEvent::Response {
        method: vtcode_desktop_protocol::method::WORKSPACE_REVIEW.into(),
        error: None,
        result: Some(
            json!({"requestId": request_id, "workspace": workspace, "capturedAt": "2026-10-24T00:00:00Z", "patch": patch, "truncated": false, "note": "Tracked changes against HEAD"}),
        ),
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
            });
            view.inspector_open = true;
        });
        window.render_frame(cx);
        window.click("review-working-tree", cx);
        window.render_frame(cx);
        assert!(requests.try_recv().is_err(), "opening review must wait for an explicit capture");
        window.click("workspace-review-include-untracked", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.workspace_review
                .as_ref()
                .unwrap()
                .scope_input
                .update(cx, |input, cx| input.set_value("src", window, cx));
        });
        window.click("refresh-workspace-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).review_open);
        assert_eq!(window.find("diff-review-panel").label(), Some("Working tree review"));
        assert_eq!(window.find("refresh-workspace-review").disabled(), Some(true));
        let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
            panic!("review request")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::WORKSPACE_REVIEW);
        assert_eq!(params["scope"], "src");
        assert_eq!(params["includeUntracked"], true);
        assert_eq!(window.find("workspace-review-include-untracked").disabled(), Some(true));
        let request_id = params["requestId"].as_str().unwrap().to_string();
        view.update(cx, |view, cx| {
            view.handle_backend_event(snapshot("stale", "/workspace"), cx);
            assert!(view.workspace_review.as_ref().unwrap().pending);
            view.handle_backend_event(snapshot(&request_id, "/other-workspace"), cx);
            assert!(view.workspace_review.as_ref().unwrap().pending);
            view.handle_backend_event(snapshot(&request_id, "/workspace"), cx);
            let review = view.workspace_review.as_ref().unwrap();
            assert!(!review.pending);
            assert_eq!(review.viewer.as_ref().unwrap().read(cx).files().len(), 2);
            assert!(view.transcript.is_empty());
            assert!(view.changes.is_empty());
            assert!(view.selected_session_id.is_none());
            // Session projection invalidation cannot close a Git review.
            view.change_viewer_dirty = true;
            view.refresh_selected_change(cx);
            assert!(view.review_open);
        });
        window.render_frame(cx);
        let layout_options = window.find("workspace-review-layout-options");
        assert_eq!(layout_options.role(), Some(gpui_kit::Role::Group));
        assert_eq!(layout_options.label(), Some("Diff layout"));
        assert_eq!(window.find("workspace-review-unified").checked(), Some(true));
        assert_eq!(window.find("workspace-review-split").checked(), Some(false));
        assert!(window.find("workspace-review-diff").bounds().size.width > px(900.));
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));
        window.click("workspace-review-previous-file", cx);
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 2 of 2 · b.rs"));
        window.click("workspace-review-next-file", cx);
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));
        window.click("workspace-review-next-file", cx);
        window.click("workspace-review-split", cx);
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-unified").checked(), Some(false));
        assert_eq!(window.find("workspace-review-split").checked(), Some(true));
        assert_eq!(
            view.read(cx)
                .workspace_review
                .as_ref()
                .unwrap()
                .viewer
                .as_ref()
                .unwrap()
                .read(cx)
                .mode(),
            gpui_kit::component::diff::DiffMode::Split
        );
        window.click("copy-workspace-review", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(patch));
        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else {
            panic!("refresh request")
        };
        let refreshed_id = params["requestId"].as_str().unwrap().to_string();
        assert_ne!(refreshed_id, request_id);
        view.update(cx, |view, cx| {
            view.handle_backend_event(snapshot(&request_id, "/workspace"), cx);
            assert!(view.workspace_review.as_ref().unwrap().pending);
            // Reordering must preserve the path, not its old index.
            let (first, second) = patch.split_once("diff --git a/b.rs").unwrap();
            let reordered = format!("diff --git a/b.rs{second}{first}");
            let mut response = snapshot(&refreshed_id, "/workspace");
            if let BackendEvent::Response { result: Some(result), .. } = &mut response {
                result["patch"] = json!(reordered);
            }
            view.handle_backend_event(response, cx);
            assert_eq!(
                view.workspace_review.as_ref().unwrap().viewer.as_ref().unwrap().read(cx).mode(),
                gpui_kit::component::diff::DiffMode::Split
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · b.rs"));
        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else {
            panic!("refresh request")
        };
        view.update(cx, |view, cx| {
            let mut response = snapshot(params["requestId"].as_str().unwrap(), "/workspace");
            if let BackendEvent::Response { result: Some(result), .. } = &mut response {
                result["patch"] = json!(patch.split_once("diff --git a/b.rs").unwrap().0);
            }
            view.handle_backend_event(response, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 1 · a.rs"));
        assert_eq!(window.find("workspace-review-previous-file").disabled(), Some(true));
        assert_eq!(window.find("workspace-review-next-file").disabled(), Some(true));
        for _ in 0..10 {
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).review_focus.contains_focused(window, cx));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).review_open);
        assert!(!view.read(cx).review_focus.contains_focused(window, cx));
        assert!(view.read(cx).workspace_review.is_none());
        view.update(cx, |view, cx| {
            view.handle_backend_event(snapshot(&refreshed_id, "/workspace"), cx);
            assert!(view.workspace_review.is_none());
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn working_tree_review_distinguishes_partial_clean_and_unavailable_snapshots(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
            });
            view.open_workspace_review(window, cx);
            view.refresh_workspace_review(cx);
            let id = view.workspace_review.as_ref().unwrap().request_id.clone();
            view.accept_workspace_review(
                vtcode_desktop_protocol::WorkspaceReviewResult {
                    include_untracked: false,
                    scope: String::new(),
                    request_id: id,
                    workspace: "/workspace".into(),
                    captured_at: "now".into(),
                    patch: Some("partial patch\n".into()),
                    truncated: true,
                    note: "Tracked changes".into(),
                },
                cx,
            );
            assert!(view.workspace_review.as_ref().unwrap().viewer.is_none());
        });
        window.render_frame(cx);
        assert!(window.find("workspace-review-raw").bounds().size.height > px(300.));
        assert_eq!(window.find("workspace-review-split").disabled(), Some(true));
        window.click("copy-workspace-review", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("partial patch\n"));
        for patch in [Some(String::new()), None] {
            view.update(cx, |view, cx| {
                view.refresh_workspace_review(cx);
                let id = view.workspace_review.as_ref().unwrap().request_id.clone();
                view.accept_workspace_review(
                    vtcode_desktop_protocol::WorkspaceReviewResult {
                        include_untracked: false,
                        scope: String::new(),
                        request_id: id,
                        workspace: "/workspace".into(),
                        captured_at: "now".into(),
                        patch: patch.clone(),
                        truncated: false,
                        note: "Scope or unavailable reason".into(),
                    },
                    cx,
                );
                assert_eq!(view.workspace_review.as_ref().unwrap().snapshot.as_ref().unwrap().patch, patch);
            });
            window.render_frame(cx);
            assert_eq!(window.find("copy-workspace-review").disabled(), Some(true));
            if patch.is_some() {
                assert_eq!(window.find("workspace-review-clean").label(), Some("No tracked changes"));
            } else {
                assert_eq!(window.find("workspace-review-unavailable").label(), Some("Git review unavailable"));
            }
        }
        view.update(cx, |view, cx| {
            view.refresh_workspace_review(cx);
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);
            assert!(!view.workspace_review.as_ref().unwrap().pending);
            view.reset_session_state();
            assert!(view.workspace_review.is_none());
            assert!(!view.review_open);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn working_tree_queue_failure_preserves_agent_and_draft_and_allows_retry(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    commands
        .try_send(BackendCommand::Request { method: "occupied".into(), params: json!({}) })
        .unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
            });
            view.running = true;
            view.composer
                .update(cx, |input, cx| input.set_value("Keep this draft", window, cx));
            view.open_workspace_review(window, cx);
            view.refresh_workspace_review(cx);
            assert!(!view.workspace_review.as_ref().unwrap().pending);
            assert!(view.running);
            assert_eq!(view.composer.read(cx).value().to_string(), "Keep this draft");
        });
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-unavailable").label(), Some("Git review unavailable"));
        assert_ne!(window.find("refresh-workspace-review").disabled(), Some(true));
        let _occupied = requests.try_recv().unwrap();
        window.click("refresh-workspace-review", cx);
        assert!(view.read(cx).workspace_review.as_ref().unwrap().pending);
        assert!(requests.try_recv().is_ok());
        assert!(view.read(cx).running);
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_accessibility_exposes_content_and_distinguishes_tool_actions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "answer".into(),
                "assistant".into(),
                "VT Code".into(),
                "Tests passed: **7**. Run `cargo test`.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "check".into(),
                "tool".into(),
                "cargo check".into(),
                "Finished successfully".into(),
            ));
        });
        window.render_frame(cx);
        assert_eq!(window.find("message-body-answer").role(), Some(gpui_kit::Role::Label));
        assert_eq!(window.find("message-body-answer").label(), Some("VT Code: Tests passed: 7. Run cargo test."));
        assert_eq!(window.find("tool-check").label(), Some("Show details for cargo check"));
        window.click("tool-check", cx);
        window.render_frame(cx);
        assert_eq!(window.find("tool-check").label(), Some("Hide details for cargo check"));
        assert_eq!(window.find("tool-output-check").label(), Some("Finished successfully"));
        window.click("tool-check", cx);
        assert!(!view.read(cx).transcript.get(1).unwrap().expanded);
        window.click("copy-message-answer", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("Tests passed: **7**. Run `cargo test`."));
    })
    .unwrap();
}

#[gpui_kit::test]
fn appearance_accessibility_exposes_current_values_after_adjustment(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.settings_open = true);
        window.render_frame(cx);
        assert_eq!(window.find("font-ui-value").label(), Some("UI font size: 16 pixels"));
        assert_eq!(window.find("font-code-value").label(), Some("Code font size: 14 pixels"));
        window.click("font-ui-1", cx);
        window.click("font-code-0", cx);
        window.render_frame(cx);
        assert_eq!(window.find("font-ui-value").label(), Some("UI font size: 17 pixels"));
        assert_eq!(window.find("font-code-value").label(), Some("Code font size: 13 pixels"));
        assert!(window.find("choose-ui-font").label().unwrap().contains("current:"));
        assert!(window.find("choose-code-font").label().unwrap().contains("current:"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn reduced_motion_setting_overrides_and_restores_the_system_value(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.reduced_motion = crate::desktop::preferences::ReducedMotionPreference::System;
            cx.set_reduce_motion(false);
            view.set_reduced_motion(crate::desktop::preferences::ReducedMotionPreference::System, cx);
            assert!(cx.reduce_motion(), "selecting System again refreshes its captured system value");
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-240.))), cx);
        window.render_frame(cx);
        assert_eq!(window.find("reduce-motion-system").label(), Some("Reduce motion: System (selected)"));

        window.click("reduce-motion-off", cx);
        window.render_frame(cx);
        assert!(!cx.reduce_motion());
        assert_eq!(window.find("reduce-motion-off").label(), Some("Reduce motion: Off (selected)"));
        assert_eq!(
            crate::desktop::preferences::Preferences::load().reduced_motion,
            crate::desktop::preferences::ReducedMotionPreference::Off
        );

        window.click("reduce-motion-system", cx);
        window.render_frame(cx);
        assert!(cx.reduce_motion(), "System restores the system value captured before overrides");
        assert_eq!(window.find("reduce-motion-system").label(), Some("Reduce motion: System (selected)"));
        assert_eq!(
            crate::desktop::preferences::Preferences::load().reduced_motion,
            crate::desktop::preferences::ReducedMotionPreference::System
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn navigation_accessibility_distinguishes_current_workspace_and_thread(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let alpha_id = super::opaque_path_element_id("workspace", "/project/alpha");
    let beta_id = super::opaque_path_element_id("workspace", "/project/beta");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.workspace = Some(PathBuf::from("/project/alpha"));
            view.preferences.recent_workspaces = vec!["/project/alpha".into(), "/project/beta".into()];
            view.sessions = ["First", "Second"]
                .into_iter()
                .map(|title| vtcode_desktop_protocol::SessionSummary {
                    session_id: title.into(),
                    title: Some(title.into()),
                    workspace: "/project/alpha".into(),
                    completed: false,
                    resumable: true,
                })
                .collect();
            view.selected_session_id = Some("Second".into());
        });
        window.render_frame(cx);
        assert_eq!(window.find(alpha_id.clone()).label(), Some("alpha (current workspace)"));
        assert_eq!(window.find(beta_id.clone()).label(), Some("beta"));
        assert_eq!(window.find("session-First").label(), Some("First"));
        assert_eq!(window.find("session-Second").label(), Some("Second (current thread)"));
        view.update(cx, |view, _| {
            view.selected_session_id = Some("First".into());
            view.sessions.reverse();
            view.preferences.recent_workspaces.reverse();
        });
        window.render_frame(cx);
        assert_eq!(window.find(alpha_id.clone()).label(), Some("alpha (current workspace)"));
        assert_eq!(window.find(beta_id.clone()).label(), Some("beta"));
        assert_eq!(window.find("session-First").label(), Some("First (current thread)"));
        assert_eq!(window.find("session-Second").label(), Some("Second"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn status_bar_announces_agent_state_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.preferences.show_status_bar = true;
        view.running = false;
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find("agent-status");
        assert_eq!(status.role(), Some(gpui_kit::Role::Status));
        assert_eq!(status.label(), Some("Agent: idle"));
    })
    .unwrap();

    view.update(cx, |view, cx| {
        view.running = true;
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find("agent-status");
        assert_eq!(status.role(), Some(gpui_kit::Role::Status));
        assert_eq!(status.label(), Some("Agent: running"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_errors_are_exposed_as_alerts(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.error = Some("The local service request queue is full. Try again.".into());
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let error = window.find("composer-error");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(error.label(), Some("The local service request queue is full. Try again."));
    })
    .unwrap();
}

#[gpui_kit::test]
fn sidebar_service_status_announces_state_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.status = "Starting local VT Code service…".into();
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find("sidebar-status");
        assert_eq!(status.role(), Some(gpui_kit::Role::Status));
        assert_eq!(status.label(), Some("Starting local VT Code service…"));
    })
    .unwrap();

    view.update(cx, |view, cx| {
        view.status = "Connected to the local VT Code service".into();
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find("sidebar-status");
        assert_eq!(status.role(), Some(gpui_kit::Role::Status));
        assert_eq!(status.label(), Some("Connected to the local VT Code service"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_groups_preserve_commentary_order_and_expand_live_output(cx: &mut TestAppContext) {
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
            view.transcript.set_output("build", Some("compiler error: missing name".into()));
        });
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").label(), Some("Expand 2 tool actions · 1 failed"));
        assert_eq!(window.find("activity-group-read").checked(), Some(false));
        assert_eq!(window.find("message-body-comment").label(), Some("VT Code: The build needs a fix."));
        assert_eq!(window.find("tool-retry").label(), Some("Show details for cargo check"));
        assert!(!view.read(cx).transcript.get(1).unwrap().activity_expanded);
        window.click("activity-group-read", cx);
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").checked(), Some(true));
        assert_eq!(window.find("tool-read").label(), Some("Show details for Read Cargo.toml"));
        window.click("tool-build", cx);
        window.render_frame(cx);
        assert_eq!(window.find("tool-output-build").label(), Some("compiler error: missing name"));
        view.update(cx, |view, _| {
            view.transcript.upsert(TranscriptItem::new(
                "read".into(),
                "tool".into(),
                "Read Cargo.toml".into(),
                "in progress\nupdated".into(),
            ));
            assert!(view.transcript.get(1).unwrap().activity_expanded, "streaming preserves group expansion");
            assert_eq!(view.transcript.activity_group(2), Some(1..3));
            assert_eq!(view.transcript.activity_group(4), None, "commentary breaks a group");
        });
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").label(), Some("Collapse 2 tool actions · 1 running · 1 failed"));
        assert_eq!(window.find("activity-group-read").checked(), Some(true));
        window.click("activity-group-read", cx);
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").checked(), Some(false));
        assert!(!view.read(cx).transcript.get(1).unwrap().activity_expanded);
        assert!(view.read(cx).transcript.get(2).unwrap().expanded, "group collapse preserves detail choice");
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_model_name_uses_the_active_provider_catalog_entry(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.configuration = Some(
                serde_json::from_value(json!({
                    "provider": "provider-b", "model": "shared-id", "authenticated": true,
                    "credentialSetupCommand": "",
                }))
                .unwrap(),
            );
            view.models = [("provider-a", "Wrong provider model"), ("provider-b", "Readable Model")]
                .into_iter()
                .map(|(provider, name)| vtcode_desktop_protocol::ModelInfo {
                    provider: provider.into(),
                    id: "shared-id".into(),
                    display_name: name.into(),
                    context_window: None,
                    current: false,
                })
                .collect();
        });
        window.render_frame(cx);
        assert_eq!(window.find("pick-model").label(), Some("Readable Model"));
        view.update(cx, |view, _| view.models.clear());
        window.render_frame(cx);
        assert_eq!(
            window.find("pick-model").label(),
            Some("shared-id"),
            "custom IDs remain usable without catalog metadata"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn sidebar_changed_file_navigation_wraps_and_copies_the_selected_patch(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let mut item = TranscriptItem::new("edits".into(), "diff".into(), "Edits".into(), String::new());
            item.diff = Some(super::DiffCard {
                files: Vec::new(), additions: Some(2), deletions: Some(2), incomplete: false, failed: false,
                patch: "--- a/first.rs\n+++ b/first.rs\n@@ -1 +1 @@\n-old\n+first\n--- a/second.rs\n+++ b/second.rs\n@@ -1 +1 @@\n-before\n+second\n".into(),
            });
            view.transcript.push(item);
            view.changes = super::ChangesProjection::rebuild(&view.transcript);
            view.select_change("first.rs".into(), cx);
        });
        window.render_frame(cx);
        window.click("next-change-file", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("second.rs"));
        window.render_frame(cx);
        window.click("copy-change-patch", cx);
        let copied = cx.read_from_clipboard().unwrap().text().unwrap();
        assert!(copied.contains("+second"));
        window.click("next-change-file", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("first.rs"));
        window.click("previous-change-file", cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("second.rs"));
        assert_eq!(window.find("open-change-file").label(), Some("Open file"));
    }).unwrap();
}

#[gpui_kit::test]
fn utilities_pane_projects_real_activity_and_routes_existing_workflows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Utilities;
            view.attachments = vec![
                vtcode_desktop_protocol::TurnAttachment {
                    path: "src/main.rs".into(),
                    text: "fn main() {}".into(),
                },
                vtcode_desktop_protocol::TurnAttachment {
                    path: "src/lib.rs".into(),
                    text: "pub fn run() {}".into(),
                },
            ];
            for (id, status) in [
                ("read", "completed"),
                ("build", "failed (exit 1)"),
                ("search", "in progress"),
            ] {
                view.transcript.push(TranscriptItem::new(
                    id.into(),
                    "tool".into(),
                    id.into(),
                    format!("{status}\ndetails"),
                ));
            }
        });
        window.render_frame(cx);
        assert_eq!(window.find("utilities-pane").role(), Some(gpui_kit::Role::Group));
        assert_eq!(
            window.find("utilities-activity").label(),
            Some("Recent activity: 1 running · 1 done · 1 failed · 0 other")
        );
        assert_eq!(
            window.find("utilities-changes").label(),
            Some("Session changes: 0 files, 0 additions, 0 deletions")
        );
        assert_eq!(
            window
                .find(super::opaque_path_element_id("utilities-source", "src/main.rs"))
                .label(),
            Some("Preview attached source: src/main.rs")
        );
        assert_eq!(
            window
                .find(super::opaque_path_element_id("utilities-source", "src/lib.rs"))
                .label(),
            Some("Preview attached source: src/lib.rs")
        );
        view.update(cx, |view, _| view.attachments.reverse());
        window.render_frame(cx);
        assert_eq!(
            window
                .find(super::opaque_path_element_id("utilities-source", "src/main.rs"))
                .label(),
            Some("Preview attached source: src/main.rs"),
            "source identity follows its path after reordering"
        );
        window.click("utilities-activity", cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Activity);
        view.update(cx, |view, _| view.inspector_tab = crate::desktop::inspector::InspectorTab::Utilities);
        window.render_frame(cx);
        window.click("utilities-changes", cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Changes);
        view.update(cx, |view, _| view.inspector_tab = crate::desktop::inspector::InspectorTab::Utilities);
        window.render_frame(cx);
        window.click("utilities-browse-files", cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Files);
        assert_eq!(view.read(cx).attachments.len(), 2, "navigation preserves the draft sources");
    })
    .unwrap();
}

#[gpui_kit::test]
fn startup_collapses_panels_and_navigation_peek_is_transient(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let fresh = cx.new(|cx| DesktopView::new(window, cx, ThemeMode::Dark));
        assert!(!fresh.read(cx).sidebar_open);
        assert!(!fresh.read(cx).inspector_open);
        view.update(cx, |view, cx| {
            view.preferences.inspector_open = Some(true);
            view.load_preferences(window, cx);
            view.sidebar_open = false;
            view.focus_handle.focus(window, cx);
        });
        window.render_frame(cx);
        assert!(!view.read(cx).inspector_open, "launch remains focused even after an earlier expanded inspector");
        window.hover("nav-home", cx);
        window.render_frame(cx);
        assert!(view.read(cx).navigation_peek);
        assert!(!view.read(cx).sidebar_open, "hover does not pin the sidebar");
        window.press("escape", cx);
        assert!(!view.read(cx).navigation_peek);
        window.click("toggle-sidebar", cx);
        assert!(view.read(cx).sidebar_open);
        window.press("cmd-b", cx);
        assert!(!view.read(cx).sidebar_open);
    })
    .unwrap();
}

#[gpui_kit::test]
fn shortcut_settings_reject_conflicts_then_persist_and_reset(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.shortcut_inputs[0].update(cx, |input, cx| input.set_value("cmd-n", window, cx));
        });
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-2500.))), cx);
        window.click("save-shortcuts", cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("already assigned"));
        assert!(crate::desktop::preferences::Preferences::load().shortcuts.is_empty());
        view.update(cx, |view, cx| {
            view.shortcut_inputs[0].update(cx, |input, cx| input.set_value("ctrl-shift-k", window, cx))
        });
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-2500.))), cx);
        window.click("save-shortcuts", cx);
        assert_eq!(
            crate::desktop::preferences::Preferences::load()
                .shortcuts
                .get("commands")
                .map(String::as_str),
            Some("ctrl-shift-k")
        );
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-2500.))), cx);
        window.click("reset-shortcuts", cx);
        assert!(crate::desktop::preferences::Preferences::load().shortcuts.is_empty());
        assert_eq!(view.read(cx).shortcut_inputs[0].read(cx).value(), "cmd-k");
    })
    .unwrap();
}

#[test]
fn native_titlebar_is_transparent_and_kit_owns_dragging() {
    let options = super::desktop_window_options();
    assert!(options.app_owns_titlebar_drag, "avoid duplicate AppKit/Kit double-click handling");
    let titlebar = options.titlebar.unwrap();
    assert!(titlebar.appears_transparent);
    assert_eq!(titlebar.traffic_light_position, Some(point(px(12.), px(17.))));
}

#[gpui_kit::test]
fn chrome_overflow_menus_are_accessible_and_preserve_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.press("cmd-l", cx);
        window.input("Keep this draft", cx);
        window.render_frame(cx);
        assert_eq!(window.find("title-bar").bounds().size.height, px(48.));
        assert!(window.find("toggle-sidebar").bounds().origin.x >= px(80.), "reserve native traffic-light space");
        for (id, label) in [
            ("header-overflow", "Window actions"),
            ("navigation-overflow", "Navigation actions"),
        ] {
            assert_eq!(window.find(id).label(), Some(label));
            window.click(id, cx);
            window.render_frame(cx);
            assert_eq!(
                window.within(format!("dropdown-menu:Name(\"{id}\")")).find("popup-menu").role(),
                Some(gpui_kit::Role::Menu)
            );
            window.press("escape", cx);
            window.render_frame(cx);
            assert_eq!(view.read(cx).composer.read(cx).value(), "Keep this draft");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn compact_composer_caps_width_grows_with_text_and_preserves_controls(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
        });
        window.render_frame(cx);
        let compact = window.find("message-composer").bounds();
        assert!(compact.size.width <= px(960.));
        assert!(compact.size.height <= px(120.), "empty composer height: {:?}", compact.size.height);
        assert_eq!(window.find("send-prompt").disabled(), Some(true));
        assert_eq!(window.find("composer-settings").label(), Some("Composer settings"));
        assert_eq!(window.find("composer-policy").label(), Some("Execution policy: Permissions unavailable"));
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("first\nsecond\nthird\nfourth\nfifth", window, cx));
        });
        window.render_frame(cx);
        let expanded = window.find("message-composer").bounds();
        assert!(expanded.size.height > compact.size.height, "multiline drafts should grow instead of clipping");
        assert!(expanded.size.height <= px(240.));
        assert_eq!(view.read(cx).composer.read(cx).value(), "first\nsecond\nthird\nfourth\nfifth");
        let long_draft = (1..=30).map(|line| format!("Draft line {line}")).collect::<Vec<_>>().join("\n");
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value(long_draft.clone(), window, cx));
        });
        window.render_frame(cx);
        let capped = window.find("message-composer").bounds();
        assert!(capped.size.height <= px(240.), "long drafts must scroll after six rows: {capped:?}");
        assert_eq!(view.read(cx).composer.read(cx).value(), long_draft);
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("first\nsecond\nthird\nfourth\nfifth", window, cx));
        });
        window.render_frame(cx);
        window.click("composer-settings", cx);
        assert!(view.read(cx).settings_open);
        assert_eq!(view.read(cx).composer.read(cx).value(), "first\nsecond\nthird\nfourth\nfifth");
    })
    .unwrap();
}

#[gpui_kit::test]
fn narrow_composer_wraps_actions_and_bounds_long_model_names(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 800.);
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.configuration = Some(serde_json::from_value(json!({
                "provider": "local", "model": "a-very-long-model-name-with-a-large-context-window-and-extra-provider-description",
                "authenticated": true, "credentialSetupCommand": "", "sandbox": "Ask before protected actions",
            })).unwrap());
        });
    }).unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.viewport_size().width, px(600.));
        let panel = window.find("message-composer").bounds();
        for id in ["pick-model", "send-prompt", "attach-files"] {
            let control = window.find(id).bounds();
            assert!(
                control.origin.x >= panel.origin.x
                    && control.origin.x + control.size.width <= panel.origin.x + panel.size.width,
                "{id} must fit horizontally: control={control:?}, panel={panel:?}"
            );
            assert!(
                control.origin.y + control.size.height <= panel.origin.y + panel.size.height,
                "{id} must fit vertically"
            );
        }
        assert_eq!(window.find("composer-policy").label(), Some("Execution policy: Ask before protected actions"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_theme_inheritance_respects_explicit_desktop_override_and_keeps_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let snapshot = |theme: &str| BackendEvent::Response {
        method: vtcode_desktop_protocol::method::CONFIG_DISCOVER.into(),
        error: None,
        result: Some(json!({ "provider": "ollama", "model": "local", "authenticated": true,
            "credentialSetupCommand": "", "sharedUi": {"theme": theme} })),
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(19);
            view.composer
                .update(cx, |input, cx| input.set_value("Preserve this draft", window, cx));
            view.handle_backend_event(snapshot("ciapre-blue"), cx);
            assert_eq!(view.theme_id, "ciapre-blue");
            assert_eq!(view.preferences.follow_cli_theme, Some(true));
            assert_eq!(cx.theme().font_size, px(19.));
            view.apply_theme("github-dark".into(), window, cx);
            view.handle_backend_event(snapshot("solarized-light"), cx);
            assert_eq!(view.theme_id, "github-dark", "CLI discovery cannot overwrite an explicit desktop choice");
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx);
        window.click("use-cli-theme", cx);
        assert_eq!(view.read(cx).theme_id, "solarized-light");
        assert_eq!(view.read(cx).preferences.follow_cli_theme, Some(true));
        assert_eq!(view.read(cx).composer.read(cx).value(), "Preserve this draft");
        assert_eq!(cx.theme().font_size, px(19.));
        assert_eq!(crate::desktop::preferences::Preferences::load().follow_cli_theme, Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_shortcut_import_preserves_unsaved_edits_and_explicit_unbinds(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.configuration = Some(
                serde_json::from_value(json!({
                    "provider": "ollama", "model": "local", "authenticated": true, "credentialSetupCommand": "",
                    "sharedUi": {"theme": "ciapre", "shortcuts": {"cancel": ["ctrl-c"], "model": []}}
                }))
                .unwrap(),
            );
            view.shortcut_inputs[0].update(cx, |input, cx| input.set_value("ctrl-shift-k", window, cx));
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx);
        window.click("import-cli-shortcuts", cx);
        let saved = crate::desktop::preferences::Preferences::load();
        assert_eq!(saved.shortcuts.get("cancel").map(String::as_str), Some("ctrl-c"));
        assert_eq!(saved.shortcuts.get("model").map(String::as_str), Some(""));
        assert_eq!(saved.shortcuts.get("commands").map(String::as_str), Some("ctrl-shift-k"));
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("imported"));
        view.update(cx, |view, _| {
            let _previous = view
                .configuration
                .as_mut()
                .unwrap()
                .shared_ui
                .as_mut()
                .unwrap()
                .shortcuts
                .insert("cancel".into(), vec!["cmd-q".into()]);
        });
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx);
        window.click("import-cli-shortcuts", cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("reserved"));
        assert_eq!(
            crate::desktop::preferences::Preferences::load()
                .shortcuts
                .get("cancel")
                .map(String::as_str),
            Some("ctrl-c")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn status_bar_is_opt_in_and_keyboard_toggle_persists_without_losing_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("status-bar").is_none());
        assert!(window.try_find("thread-actions").is_none(), "thread actions belong in the single header menu");
        assert_eq!(window.find("composer-cli-webmcp").label(), Some("VT Code CLI and WebMCP documentation"));
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("Keep this draft", window, cx))
        });
        window.press("cmd-shift-s", cx);
        window.render_frame(cx);
        assert!(window.try_find("status-bar").is_some());
        assert!(crate::desktop::preferences::Preferences::load().show_status_bar);
        window.press("cmd-,", cx);
        window.render_frame(cx);
        window.click("show-status-bar", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.show_status_bar);
        assert!(!crate::desktop::preferences::Preferences::load().show_status_bar);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep this draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn model_popup_follows_composer_above_growing_input(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 900., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.sidebar_open = false;
            view.inspector_open = false;
            view.open_model_palette(window, cx);
        });
        window.render_frame(cx);
        let initial_popup = window.find("command-palette").bounds();
        let composer = window.find("composer-context").bounds();
        assert!(initial_popup.bottom() <= composer.top());
        assert!((initial_popup.right() - composer.right()).abs() <= px(1.));
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("one\ntwo\nthree\nfour\nfive", window, cx));
        });
        window.render_frame(cx);
        let popup = window.find("command-palette").bounds();
        let composer = window.find("composer-context").bounds();
        assert!(popup.bottom() <= composer.top(), "popup must remain above the growing composer");
        assert!(popup.top() < initial_popup.top());
        assert!(popup.left() >= px(12.) && popup.right() <= window.viewport_size().width - px(12.));
    })
    .unwrap();
}

fn runtime_options_config() -> vtcode_desktop_protocol::ConfigurationSnapshot {
    serde_json::from_value(json!({
        "provider": "openai", "model": "gpt-5.6-sol", "authenticated": true, "credentialSetupCommand": "",
        "sandbox": "Ask before protected actions",
        "sessionOptions": {"permissionMode": "ask", "mode": "build", "reasoningEffort": "medium", "supportedEfforts": ["low", "medium", "high"], "speed": "ultrafast"}
    })).unwrap()
}

#[gpui_kit::test]
fn permission_picker_emits_typed_selection_and_preserves_draft_sources(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.selected_session_id = Some("old-session".into());
            view.attachments
                .push(vtcode_desktop_protocol::TurnAttachment { path: "source.rs".into(), text: "source".into() });
            view.composer
                .update(cx, |input, cx| input.set_value("Preserve my draft", window, cx));
        });
        window.render_frame(cx);
        window.click("composer-policy", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Command permissions"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.within("command-palette").press("down", cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["permissionMode"], "autoApproveCommands");
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            let mut config = runtime_options_config();
            config.session_options.as_mut().unwrap().permission_mode =
                vtcode_desktop_protocol::PermissionMode::AutoApproveCommands;
            view.handle_backend_event(BackendEvent::Response { method, result: Some(json!(config)), error: None }, cx);
        });
        assert_eq!(view.read(cx).composer.read(cx).value(), "Preserve my draft");
        assert_eq!(view.read(cx).attachments[0].path, "source.rs");
        assert!(view.read(cx).selected_session_id.is_none());
        assert!(view.read(cx).session_request.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn quick_model_step_control_sends_supported_effort_and_full_search_remains_available(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Model and reasoning effort"));
        assert_eq!(window.find("model-options-summary").label(), Some("gpt-5.6-sol · medium"));
        window.click("increase-effort", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "high");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.session_request = None;
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("palette-label-0-0").label(), Some("Search all models…"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn add_popup_filters_cli_resources_and_inserts_a_skill_without_submitting(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
        });
        window.render_frame(cx);
        window.click("attach-files", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Add files and more @"));
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::COMPOSER_RESOURCES.into(),
                    error: None,
                    result: Some(json!([
                        {"name":"rust-skills", "description":"Rust implementation guidance", "kind":"skill"},
                        {"name":"web-tools", "description":"Installed web plugin", "kind":"plugin"}
                    ])),
                },
                cx,
            );
            view.focus_palette_state(window, cx);
            cx.notify();
        });
        window.render_frame(cx);
        window.within("command-palette").input("rust-skills", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).composer_resources.len(), 2, "resource response: {:?}", view.read(cx).error);
        assert!(window.try_find("palette-label-2-0").is_some(), "skill row should be rendered");
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).query(cx), "rust-skills");
        assert!(view.read(cx).palette.as_ref().unwrap().read(cx).selected_index().is_some());
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("resource request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::COMPOSER_RESOURCES);
    assert!(requests.try_recv().is_err(), "choosing a mention cannot submit a turn");
    cx.update(|cx| assert_eq!(view.read(cx).composer.read(cx).value(), "$rust-skills "));
}

#[gpui_kit::test]
fn composer_option_changes_are_blocked_during_runs_and_reject_malformed_catalogs(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.running = true;
            view.open_permissions(window, cx);
            assert!(view.palette.is_none());
            view.select_session_options(
                vtcode_desktop_protocol::SessionOptionsSelectParams { mode: Some("plan".into()), ..Default::default() },
                cx,
            );
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::COMPOSER_RESOURCES.into(),
                    error: None,
                    result: Some(json!([{"name":"bad\n/command", "description":"unsafe token", "kind":"skill"}])),
                },
                cx,
            );
            assert!(view.composer_resources.is_empty());
            assert!(view.error.as_ref().unwrap().contains("Invalid CLI resource catalog"));
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn late_resource_discovery_keeps_the_selected_command_identity(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_add_context(window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.focus_palette_state(window, cx);
            view.palette.as_ref().unwrap().update(cx, |state, cx| {
                state.set_selected_index(Some(gpui_kit::component::IndexPath::new(2).section(4)), window, cx)
            });
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::COMPOSER_RESOURCES.into(),
                    error: None,
                    result: Some(json!([{"name":"rust-skills","description":"Rust","kind":"skill"}])),
                },
                cx,
            );
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(
            view.read(cx)
                .palette
                .as_ref()
                .unwrap()
                .read(cx)
                .selected_index()
                .unwrap()
                .section,
            4
        );
        assert_eq!(window.find("palette-label-4-2").label(), Some("Toggle inspector panel"));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!view.read(cx).inspector_open);
        assert!(view.read(cx).composer.read(cx).value().is_empty());
    });
}

#[gpui_kit::test]
fn unsupported_inherited_effort_offers_explicit_supported_choices(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            config.session_options.as_mut().unwrap().reasoning_effort = "max".into();
            view.configuration = Some(config);
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("increase-effort").is_none());
        assert_eq!(window.find("palette-label-0-0").label(), Some("Use low effort"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("option request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "low");
}

#[gpui_kit::test]
fn model_switch_keeps_attached_draft_and_waits_for_pending_file_loading(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "context.rs".into(),
                text: "complete source".into(),
            });
            view.composer
                .update(cx, |input, cx| input.set_value("Review this source", window, cx));
            view.attachments_picker_open = true;
            view.open_model_options(window, cx);
            assert!(view.palette.is_none());
            view.attachments_picker_open = false;
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SELECT.into(),
                    result: Some(json!(runtime_options_config())),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.attachments[0].path, "context.rs");
            assert_eq!(view.composer.read(cx).value(), "Review this source");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn runtime_option_popups_focus_the_current_choice_and_confirming_it_is_a_noop(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            let options = config.session_options.as_mut().unwrap();
            options.mode = "plan".into();
            options.permission_mode = vtcode_desktop_protocol::PermissionMode::AutoApproveCommands;
            view.configuration = Some(config);
        });
        window.render_frame(cx);
        window.click("composer-mode", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).selected_index().unwrap().row, 1);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "confirming the current Plan choice must not restart a session");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("composer-policy", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).selected_index().unwrap().row, 1);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "confirming the current policy must not restart a session");
}

#[gpui_kit::test]
fn cli_settings_reload_waits_for_shutdown_and_preserves_draft_sources(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "notes.md".into(),
                text: "local draft source".into(),
            });
            view.composer
                .update(cx, |state, cx| state.set_value("Keep this unsent draft", window, cx));
            view.reload_cli_settings(cx);
            assert!(matches!(requests.try_recv(), Ok(BackendCommand::Stop)));
            assert_eq!(view.pending_workspace, view.workspace);
            assert!(view.backend.is_some(), "must not replace a still-running service");
            view.reload_cli_settings(cx);
            assert!(requests.try_recv().is_err(), "only one shutdown may be pending");
            let workspace = view.workspace.clone().unwrap();
            view.prepare_workspace(&workspace, cx);
            assert_eq!(view.attachments[0].text, "local draft source");
            assert_eq!(view.composer.read(cx).value().as_str(), "Keep this unsent draft");
            assert!(view.configuration.is_none());
            assert!(view.selected_session_id.is_none());
            view.prepare_workspace(&workspace.join("different-workspace"), cx);
            assert!(view.attachments.is_empty(), "workspace-bound sources cannot cross workspaces");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_settings_reload_rejects_busy_and_closed_transport(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.running = true;
            view.reload_cli_settings(cx);
            assert!(requests.try_recv().is_err());
            assert!(view.pending_workspace.is_none());
            view.running = false;
            view.attachments_picker_open = true;
            view.reload_cli_settings(cx);
            assert!(requests.try_recv().is_err());
            view.attachments_picker_open = false;
            drop(requests);
            view.reload_cli_settings(cx);
            assert!(view.pending_workspace.is_none(), "failed shutdown must not strand the transition gate");
            assert!(view.error.as_deref().unwrap().contains("queue is unavailable"));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_settings_reload_button_and_action_share_the_transition_guard(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.scroll("settings-panel", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-5000.))), cx);
        window.render_frame(cx);
        window.click("reload-cli-settings", cx);
        assert!(matches!(requests.try_recv(), Ok(BackendCommand::Stop)));
        assert!(view.read(cx).pending_workspace.is_some());
        window.dispatch_action(Box::new(super::ReloadCliSettings), cx);
        assert!(requests.try_recv().is_err(), "the keyboard action cannot bypass the pending restart");
    })
    .unwrap();
}
