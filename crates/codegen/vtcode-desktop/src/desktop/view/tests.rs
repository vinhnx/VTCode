use gpui_kit::Focusable;
use gpui_kit::Styled;
use gpui_kit::component::theme::ActiveTheme;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, CursorStyle, Entity, TestAppContext, WindowBounds, WindowOptions, point, px,
    size,
};
use serde_json::json;

use super::{
    DesktopButtonExt, DesktopView, FixedRowSizeCache, MAX_UI_EVENTS_PER_TICK, ThemeMode, WelcomePeriod,
    scale_row_height_for_ui_font, workspace_picker_options,
};
use crate::desktop::transcript::MAX_TRANSCRIPT_ITEMS;
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
    crate::desktop::localization::initialize(crate::desktop::localization::UiLanguage::English);
    cx.update(gpui_kit::init);
    cx.update(super::super::notifications::initialize);
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

fn connected_webmcp_status() -> vtcode_desktop_protocol::WebmcpBridgeStatus {
    vtcode_desktop_protocol::WebmcpBridgeStatus {
        running: true,
        endpoint: Some("ws://127.0.0.1:4000/webmcp".into()),
        allowed_origins: vec!["https://example.test".into()],
        session_id: Some("session-old".into()),
        turns_available: true,
        workspace: "/workspace-old".into(),
    }
}

fn pending_webmcp_pairing() -> vtcode_desktop_protocol::WebmcpPairResult {
    vtcode_desktop_protocol::WebmcpPairResult {
        endpoint: "ws://127.0.0.1:4000/webmcp".into(),
        origin: "https://example.test".into(),
        pairing_code: "one-time-code".into(),
        expires_in_secs: 60,
    }
}

fn palette_action_id(action: &super::PaletteAction) -> String {
    super::palette_entry_element_id(action)
}

#[test]
fn desktop_buttons_use_pointer_when_enabled_and_arrow_when_disabled() {
    let mut enabled = super::desktop_button("native-cursor-enabled");
    assert_eq!(enabled.style().mouse_cursor, Some(CursorStyle::PointingHand));

    let mut disabled = super::desktop_button("native-cursor-disabled").desktop_disabled(true);
    assert_eq!(disabled.style().mouse_cursor, Some(CursorStyle::Arrow));

    let mut reenabled = super::desktop_button("native-cursor-reenabled").desktop_disabled(false);
    assert_eq!(reenabled.style().mouse_cursor, Some(CursorStyle::PointingHand));
}

#[test]
fn activity_details_omit_tool_output_from_the_metadata_copy() {
    use vtcode_exec_events::{
        CommandExecutionItem, CommandExecutionStatus, ThreadItemDetails, ToolCallStatus, ToolOutputItem,
    };

    let details = ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
        call_id: "call-1".into(),
        tool_call_id: Some("provider-call-1".into()),
        spool_path: Some(".vtcode/tool-outputs/call-1".into()),
        output: "large captured result".repeat(32),
        exit_code: Some(0),
        status: ToolCallStatus::Completed,
    }));
    let payload = super::activity_details_payload(&details);

    assert!(payload.contains("call-1"), "tool identity remains available in metadata");
    assert!(!payload.contains("large captured result"), "the separate output field owns captured text");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
        json!({
            "type": "tool_output",
            "call_id": "call-1",
            "tool_call_id": "provider-call-1",
            "spool_path": ".vtcode/tool-outputs/call-1",
            "exit_code": 0,
            "status": "completed"
        })
    );

    let command = ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
        command: "cargo check".into(),
        arguments: Some(json!({ "cwd": "/workspace" })),
        aggregated_output: "discarded command output".repeat(64),
        exit_code: None,
        status: CommandExecutionStatus::InProgress,
    }));
    let payload = super::activity_details_payload(&command);
    assert!(!payload.contains("discarded command output"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
        json!({
            "type": "command_execution",
            "command": "cargo check",
            "arguments": { "cwd": "/workspace" },
            "status": "in_progress"
        })
    );
}

#[test]
fn palette_entry_ids_follow_action_identity_without_exposing_payloads() {
    let path = "private/workspace/src/main.rs";
    let file_action = super::PaletteAction::PreviewFile(path.into());
    let file_id = palette_action_id(&file_action);
    assert_eq!(file_id, palette_action_id(&file_action));
    assert!(!file_id.contains(path), "workspace paths stay out of element identifiers");
    assert_ne!(
        file_id,
        palette_action_id(&super::PaletteAction::PreviewFile("private/workspace/src/lib.rs".into()))
    );
}

#[gpui_kit::test]
fn inactive_agent_events_post_system_notices_and_click_reopens_the_window(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, TurnFailedEvent, Usage, VersionedThreadEvent};

    let completed_turn = |turn_id: &str| {
        BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
            session_id: "session".into(),
            turn_id: Some(turn_id.into()),
            event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
                completed_at: None,
                usage: Usage {
                    input_tokens: 0,
                    cached_input_tokens: 0,
                    cache_creation_tokens: 0,
                    output_tokens: 0,
                },
                in_progress_exec_sessions: Vec::new(),
            })),
        })
    };
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        assert!(!window.is_window_active());
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.active_turn_id = Some("turn-1".into());
            view.running = true;
            view.event_tx.send(completed_turn("turn-1")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(!view.running);
        });
        window.render_frame(cx);
        assert!(window.notifications(cx).is_empty(), "background completion is system-only");
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.event_tx.send(completed_turn("stale-turn")).unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
    })
    .unwrap();
    assert_eq!(cx.shown_system_notifications().len(), 1, "idle stale events do not post system notices");

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.active_turn_id = Some("turn-2".into());
            view.running = true;
            view.event_tx
                .send(BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
                    session_id: "session".into(),
                    turn_id: Some("turn-2".into()),
                    event: VersionedThreadEvent::new(ThreadEvent::TurnFailed(TurnFailedEvent {
                        completed_at: None,
                        message: "provider detail that should not appear in a system notification".into(),
                        usage: None,
                    })),
                }))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);

            view.active_turn_id = Some("turn-3".into());
            view.running = true;
            view.event_tx
                .send(BackendEvent::Interaction(vtcode_desktop_protocol::InteractionRequest {
                    session_id: "session".into(),
                    turn_id: "turn-3".into(),
                    interaction_id: "interaction-1".into(),
                    kind: vtcode_desktop_protocol::InteractionKind::Question,
                    wizard: false,
                    title: "private prompt detail".into(),
                    details: Vec::new(),
                    choices: Vec::new(),
                    steps: Vec::new(),
                    selected_choice_id: None,
                    selected_choice_ids: Vec::new(),
                }))
                .unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(view.interaction.is_some());
        });
        window.render_frame(cx);
        assert!(window.notifications(cx).is_empty(), "background notices do not duplicate an in-app toast");
    })
    .unwrap();

    let notifications = cx.delivered_system_notifications();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].title, "VT Code");
    assert_eq!(notifications[0].body, "An agent run is waiting for your input.");
    let shown = cx.shown_system_notifications();
    assert_eq!(shown.len(), 3);
    assert_eq!(shown[0].body, "Your agent run has finished.");
    assert_eq!(shown[1].body, "Your agent run needs attention.");
    assert_eq!(shown[2].body, "An agent run is waiting for your input.");
    assert!(
        shown
            .iter()
            .all(|notice| { !notice.body.contains("provider detail") && !notice.body.contains("private prompt") })
    );

    cx.simulate_system_notification_response(gpui_kit::SystemNotificationResponse {
        tag: notifications[0].tag.clone(),
        action_id: None,
    });
    cx.run_until_parked();
    cx.update_window(window, |_, window, _| {
        assert!(window.is_window_active(), "the system notification activates its owning window");
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.active_turn_id = Some("turn-4".into());
            view.running = true;
            view.event_tx.send(completed_turn("turn-4")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(!view.running);
        });
    })
    .unwrap();
    assert_eq!(cx.shown_system_notifications().len(), 3, "active runs do not post system notices");
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

fn focus_element_by_tab(window: &mut gpui_kit::Window, id: &'static str, cx: &mut gpui_kit::App) {
    for _ in 0..128 {
        if window.try_find(id).is_some_and(|element| element.focused() == Some(true)) {
            return;
        }
        window.press("tab", cx);
        window.render_frame(cx);
    }
    panic!("Tab did not reach focusable element {id}");
}

fn select_settings_category(window: &mut gpui_kit::Window, index: usize, cx: &mut gpui_kit::App) {
    window.press("home", cx);
    window.render_frame(cx);
    for _ in 0..index {
        window.press("right", cx);
        window.render_frame(cx);
    }
}

fn scroll_theme_into_view(
    window: &mut gpui_kit::Window,
    view: &Entity<DesktopView>,
    id: &'static str,
    cx: &mut gpui_kit::App,
) {
    for _ in 0..24 {
        let viewport = window.find("theme-list").bounds();
        let item = window.find(id).bounds();
        if item.left() >= viewport.left() - px(1.) && item.right() <= viewport.right() + px(1.) {
            return;
        }
        let delta = if item.left() >= viewport.right() {
            -px(336.)
        } else {
            px(336.)
        };
        view.update(cx, |view, cx| {
            let mut offset = view.theme_scroll.offset();
            offset.x += delta;
            view.theme_scroll.set_offset(offset);
            cx.notify();
        });
        window.render_frame(cx);
    }
    let viewport = window.find("theme-list").bounds();
    let item = window.find(id).bounds();
    panic!("theme swatch {id} did not scroll into view: viewport={viewport:?}, item={item:?}");
}

fn horizontal_scroll_item_is_visible(scroll: &gpui_kit::ScrollHandle, index: usize) -> bool {
    let viewport = scroll.bounds();
    let Some(item) = scroll.bounds_for_item(index) else {
        return false;
    };
    let offset = scroll.offset().x;
    item.left() + offset >= viewport.left() - px(1.) && item.right() + offset <= viewport.right() + px(1.)
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
fn workspace_picker_task_is_owned_by_view_and_clears_pending_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.choose_workspace(window, cx);
            assert!(view.workspace_picker_open);
            assert!(view.workspace_picker_task.is_some());

            // Repeated activation while the native picker is pending must not
            // start another continuation owned by the same view.
            view.choose_workspace(window, cx);
            assert!(view.workspace_picker_open);
        });
    })
    .unwrap();
    assert!(cx.did_prompt_for_paths());

    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories);
        None
    });
    cx.run_until_parked();

    assert!(!cx.did_prompt_for_paths(), "one picker response resolves the only pending prompt");
    cx.update_entity(&view, |view: &mut DesktopView, _| {
        assert!(!view.workspace_picker_open);
        assert!(view.workspace_picker_task.is_some(), "the view retains the completed task until replacement");
    });
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
fn reopening_activates_the_focused_window_when_multiple_are_open(cx: &mut TestAppContext) {
    let (first_window, _first_view) = open_desktop(cx);
    let (second_window, _second_view) = open_desktop(cx);
    assert_ne!(first_window, second_window);
    assert_eq!(cx.windows().len(), 2);

    cx.update_window(second_window, |_, window, _| window.activate_window())
        .unwrap();
    cx.update(|cx| {
        assert_eq!(cx.active_window(), Some(second_window));
        assert_eq!(super::open_or_activate_main_window(cx).unwrap(), second_window);
        assert_eq!(cx.windows().len(), 2, "reopening activates a window instead of duplicating it");
    });

    cx.update_window(first_window, |_, window, _| window.remove_window()).unwrap();
    cx.update(|cx| {
        assert_eq!(cx.windows(), vec![second_window]);
        assert!(first_window.update(cx, |_, _, _| ()).is_err(), "closed handles reject later updates");
        second_window.update(cx, |_, window, cx| window.render_frame(cx)).unwrap();
    });
}

#[gpui_kit::test]
fn event_drain_task_is_owned_by_the_desktop_view(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| view.start_event_drain(cx));
    })
    .unwrap();

    cx.update_entity(&view, |view: &mut DesktopView, _| {
        assert!(view.event_drain_task.is_some());
    });
}

#[gpui_kit::test]
fn event_drain_wakes_on_events_and_reschedules_full_batches(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.start_event_drain(cx);
            for index in 0..MAX_UI_EVENTS_PER_TICK + 3 {
                view.event_tx
                    .try_send(BackendEvent::TurnStatus(
                        json!({ "sessionId": "session", "status": format!("event-{index}") }),
                    ))
                    .unwrap();
            }
        });
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_entity(&view, |view: &mut DesktopView, _| {
        assert_eq!(view.status, format!("event-{}", MAX_UI_EVENTS_PER_TICK + 2));
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
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            for index in 0..MAX_UI_EVENTS_PER_TICK + 3 {
                view.event_tx
                    .try_send(BackendEvent::TurnStatus(
                        json!({ "sessionId": "session", "status": format!("event-{index}") }),
                    ))
                    .unwrap();
            }
            let _batch_full = view.drain_events(window, cx);
            assert_eq!(view.status, format!("event-{}", MAX_UI_EVENTS_PER_TICK - 1));
            let _batch_full = view.drain_events(window, cx);
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
                    recommended_rank: None,
                    capabilities: None,
                },
                vtcode_desktop_protocol::ModelInfo {
                    provider: "anthropic".to_string(),
                    id: "claude-sonnet-5".to_string(),
                    display_name: "Claude Sonnet 5".to_string(),
                    context_window: Some(200_000),
                    current: false,
                    recommended_rank: Some(0),
                    capabilities: None,
                },
            ];
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Select model"));
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
        assert!(
            window
                .find(palette_action_id(&super::PaletteAction::SelectModel {
                    provider: "anthropic".into(),
                    model: "claude-sonnet-5".into(),
                }))
                .label()
                .unwrap()
                .contains("200K context")
        );
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
fn malformed_catalog_responses_preserve_the_last_valid_sessions_and_models(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session = vtcode_desktop_protocol::SessionSummary {
        session_id: "saved-session".to_string(),
        title: Some("Keep this session".to_string()),
        workspace: std::env::temp_dir().display().to_string(),
        completed: true,
        resumable: true,
    };
    let model = vtcode_desktop_protocol::ModelInfo {
        provider: "openai".to_string(),
        id: "gpt-test".to_string(),
        display_name: "Test model".to_string(),
        context_window: None,
        current: true,
        recommended_rank: None,
        capabilities: None,
    };

    view.update(cx, |view, _| {
        view.sessions = vec![session.clone()];
        view.models = vec![model.clone()];
    });

    for result in [
        json!({ "unexpected": "object" }),
        json!([{
            "sessionId": "",
            "title": "Invalid row",
            "workspace": std::env::temp_dir().display().to_string(),
            "completed": false,
            "resumable": true,
        }]),
    ] {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::SESSION_LIST.to_string(),
                    result: Some(result),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.sessions.as_slice(), std::slice::from_ref(&session));
            assert_eq!(view.error.as_deref(), Some("The local service returned an invalid session list."));
        });
    }

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::CONFIG_MODELS.to_string(),
                result: Some(json!({ "unexpected": "object" })),
                error: None,
            },
            cx,
        );
        assert_eq!(view.models.as_slice(), std::slice::from_ref(&model));
        assert_eq!(view.error.as_deref(), Some("The local service returned an invalid model catalog."));
    });
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
}

#[gpui_kit::test]
fn malformed_session_load_preserves_the_current_conversation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.selected_session_id = Some("current-session".to_string());
        view.workspace = Some(PathBuf::from("/current-workspace"));
        view.transcript.push(TranscriptItem::new(
            "current-message".into(),
            "assistant".into(),
            "Assistant".into(),
            "Keep this conversation".into(),
        ));
    });

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.to_string(),
                result: Some(json!({
                    "session": {
                        "sessionId": "   ",
                        "title": "Invalid session",
                        "workspace": "/replacement-workspace",
                        "completed": false,
                        "resumable": true,
                    },
                    "messages": [{ "role": "assistant", "text": "Do not replace history" }],
                })),
                error: None,
            },
            cx,
        );

        assert_eq!(view.selected_session_id.as_deref(), Some("current-session"));
        assert_eq!(view.workspace.as_deref(), Some(Path::new("/current-workspace")));
        assert_eq!(view.transcript.len(), 1);
        assert_eq!(view.transcript.get(0).unwrap().text, "Keep this conversation");
        assert_eq!(view.error.as_deref(), Some("The local service returned an invalid session load result."));
    });
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
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
        select_settings_category(window, 3, cx);
        window.render_frame(cx);
        assert_eq!(window.find("provider-details").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("provider-details").label(), Some("Provider and model configuration"));
        assert_eq!(window.find("provider-settings-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("cli-preferences-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("cli-configuration-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(
            window.find("config-target-workspace").label(),
            Some("Write CLI settings to Workspace configuration (selected)")
        );
        select_settings_category(window, 4, cx);
        window.render_frame(cx);
        assert_eq!(window.find("local-service-settings-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("local-service-details").label(), Some("Local service connection details"));
        select_settings_category(window, 3, cx);
        window.render_frame(cx);
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
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).settings_open);
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
        select_settings_category(window, 3, cx);
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
            let patch = format!(
                "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,80 +1,80 @@\n{}",
                (1..=80)
                    .map(|line| format!("-line {line}\n+{}\n", if line == 40 { "here" } else { "updated" }))
                    .collect::<String>()
            );
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
                            unified_diff: Some(patch.clone()),
                            additions: Some(80),
                            deletions: Some(80),
                        })),
                    },
                })),
            });
            assert_eq!(view.changes.entries().len(), 1);
            assert_eq!(view.changes.entries()[0].path, "src/app.rs");
            assert_eq!(view.changes.totals(), (80, 80));
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
        assert_eq!(window.find("review-split").label(), Some("Split diff view (selected)"));
        assert_eq!(window.find("review-unified").label(), Some("Unified diff view"));
        window.within("diff-review-panel").click("review-unified", cx);
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).mode(), gpui_kit::component::diff::DiffMode::Unified);
        assert_eq!(window.find("review-unified").label(), Some("Unified diff view (selected)"));
        assert_eq!(window.find("review-split").label(), Some("Split diff view"));
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
        let viewport = window.within("diff-review-panel").find("change-diff-view").bounds();
        window.scroll("change-diff-view", gpui_kit::ScrollDelta::Lines(point(0., -100.)), cx);
        window.render_frame(cx);
        let scrolled_line = window.within(("new", 52usize)).find("source").bounds();
        assert!(
            scrolled_line.top() >= viewport.top() && scrolled_line.bottom() <= viewport.bottom(),
            "the Kit Diff's virtualized list should scroll within its own review viewport"
        );
        view.update(cx, |view, cx| {
            view.reset_session_state();
            view.restore_focus_after_review_close(window, cx);
            assert!(!view.review_open);
            assert!(!view.review_focus.contains_focused(window, cx));
            assert!(view.composer.read(cx).focus_handle(cx).is_focused(window));
        });
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
        assert_eq!(window.find(palette_action_id(&super::PaletteAction::NewSession)).label(), Some("New session"));
        window.input("new", cx);
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).query(cx), "new");
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).palette_query.as_ref(), "new");
        assert_eq!(window.find(palette_action_id(&super::PaletteAction::NewSession)).label(), Some("New session"));
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
        assert!(view.read(cx).palette_query.is_empty(), "closing resets the search highlight");
    })
    .unwrap();
    let BackendCommand::Request { method, params: _ } = requests.try_recv().unwrap() else {
        panic!("expected session create request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CREATE);
}

#[gpui_kit::test]
fn command_palette_searches_all_loaded_sessions_and_workspace_files(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.sessions = (0..16)
            .map(|index| vtcode_desktop_protocol::SessionSummary {
                session_id: format!("archived-{index}"),
                title: Some(format!("Archived thread {index}")),
                workspace: std::env::temp_dir().display().to_string(),
                completed: true,
                resumable: true,
            })
            .collect();
        view.files = (0..60)
            .map(|index| super::FileEntry {
                rel_path: format!("src/generated-{index:02}.rs"),
                depth: 1,
                is_dir: false,
            })
            .collect();
    });

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);

        let older_thread = palette_action_id(&super::PaletteAction::LoadSession("archived-15".into()));
        window.within("command-palette").input("Archived thread 15", cx);
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).matched_count(), 1);
        assert_eq!(window.find(older_thread).label(), Some("Open session: Archived thread 15"));

        let later_file = palette_action_id(&super::PaletteAction::PreviewFile("src/generated-59.rs".into()));
        window.within("command-palette").press("cmd-a", cx);
        window.within("command-palette").input("generated-59.rs", cx);
        window.render_frame(cx);
        assert_eq!(palette.read(cx).query(cx), "generated-59.rs");
        assert_eq!(palette.read(cx).matched_count(), 1);
        assert_eq!(window.find(later_file).label(), Some("Preview file: src/generated-59.rs"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn command_palette_keeps_dialog_accessible_with_reduced_motion(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);

        assert_eq!(window.find("command-palette").role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(window.find("command-palette").label(), Some("Command palette"));
        assert!(view.read(cx).palette_focus.contains_focused(window, cx));
    })
    .unwrap();
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
        let initial_sidebar_width = view.read(cx).sidebar_width;

        // Press on the divider, drag right by 60px, release.
        dispatch_mouse(window, start, MousePhase::Down, cx);
        let resize = view.read(cx).resizing.expect("splitter press begins a resize");
        assert_eq!(resize.start_pointer_x_window, start.x, "drag start uses window-space pointer coordinates");
        assert_eq!(resize.start_width, px(initial_sidebar_width), "drag start retains pixel units for pane width");
        dispatch_mouse(window, point(start.x + px(6.), start.y), MousePhase::Move, cx);
        assert!(cx.has_active_drag(), "GPUI starts a typed resize drag");
        dispatch_mouse(window, point(start.x + px(60.), start.y), MousePhase::Move, cx);
        assert!(view.read(cx).resizing.is_some(), "splitter drag remains active");
        dispatch_mouse(window, point(start.x + px(60.), start.y), MousePhase::Up, cx);
        let width = view.read(cx).sidebar_width;
        assert!(width > 300.0 && width < 340.0, "sidebar grew by the drag delta: {width}");

        // Dragging past the bound clamps. Re-find the handle: the first
        // drag moved it.
        let handle = window.find("resize-sidebar").bounds();
        let start = point(handle.center().x, handle.center().y);
        dispatch_mouse(window, start, MousePhase::Down, cx);
        dispatch_mouse(window, point(start.x + px(6.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x + px(600.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x + px(600.), start.y), MousePhase::Up, cx);
        let width = view.read(cx).sidebar_width;
        assert!((width - super::super::preferences::MAX_PANE_WIDTH).abs() < f32::EPSILON, "clamped width: {width}");

        let inspector_start_width = view.read(cx).inspector_width;
        let handle = window.find("resize-inspector").bounds();
        let start = point(handle.center().x, handle.center().y);
        dispatch_mouse(window, start, MousePhase::Down, cx);
        dispatch_mouse(window, point(start.x - px(6.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x - px(40.), start.y), MousePhase::Move, cx);
        dispatch_mouse(window, point(start.x - px(40.), start.y), MousePhase::Up, cx);
        let view_state = view.read(cx);
        assert!(view_state.resizing.is_none(), "splitter release ends the resize");
        assert!(
            view_state.inspector_width > inspector_start_width + 30.
                && view_state.inspector_width < inspector_start_width + 50.,
            "moving the inspector edge left grows it: {}",
            view_state.inspector_width
        );
        assert_eq!(view_state.preferences.inspector_width, Some(view_state.inspector_width));
    })
    .unwrap();
}

#[gpui_kit::test]
fn pane_splitters_expose_accessibility_and_keyboard_resize(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let splitter = window.find("resize-sidebar");
        assert_eq!(splitter.role(), Some(gpui_kit::Role::Splitter));
        assert_eq!(splitter.label(), Some("Resize navigation"));

        for _ in 0..128 {
            if view.read(cx).sidebar_resize_focus.is_focused(window) {
                break;
            }
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).sidebar_resize_focus.is_focused(window), "Tab reaches the sidebar splitter");
        window.press("right", cx);
        assert_eq!(view.read(cx).sidebar_width, 280.);
        window.press("shift-right", cx);
        assert_eq!(view.read(cx).sidebar_width, 330.);
        window.press("left", cx);
        assert_eq!(view.read(cx).sidebar_width, 320.);
        assert_eq!(view.read(cx).preferences.sidebar_width, Some(320.));
        window.click("resize-sidebar", cx);
        assert!(view.read(cx).sidebar_resize_focus.is_focused(window), "pointer interaction focuses the splitter");

        for _ in 0..128 {
            if view.read(cx).inspector_resize_focus.is_focused(window) {
                break;
            }
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).inspector_resize_focus.is_focused(window), "Tab reaches the inspector splitter");
        window.press("left", cx);
        assert_eq!(view.read(cx).inspector_width, 330.);
        window.press("shift-right", cx);
        assert_eq!(view.read(cx).inspector_width, 280.);
        assert_eq!(view.read(cx).preferences.inspector_width, Some(280.));
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
        window.render_frame(cx);
        assert!(window.find_all("toggle-theme").is_empty());
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
        for (mode, id) in [(ThemeMode::Dark, "system-dark"), (ThemeMode::Light, "system-light")] {
            view.update(cx, |view, cx| view.run_palette_action(super::PaletteAction::ToggleTheme, window, cx));
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
    })
    .unwrap();
}

#[gpui_kit::test]
fn separate_theme_profiles_switch_and_restore_each_selected_theme(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.apply_theme("ciapre".into(), window, cx));
        window.click("open-settings", cx);
        window.render_frame(cx);
        window.click("separate-theme-profiles", cx);
        window.render_frame(cx);

        {
            let preferences = &view.read(cx).preferences;
            assert!(preferences.separate_theme_profiles);
            assert_eq!(preferences.dark_theme.as_deref(), Some("ciapre"));
            assert_eq!(preferences.light_theme.as_deref(), Some("catppuccin-latte"));
        }
        assert_eq!(window.find("theme-catppuccin-latte").label(), Some("Catppuccin Latte (light profile)"),);
        assert_eq!(window.find("edit-dark-profile").label(), Some("Edit dark appearance profile (selected)"));
        assert_eq!(window.find("edit-light-profile").label(), Some("Edit light appearance profile"));

        window.click("edit-light-profile", cx);
        window.render_frame(cx);
        assert!(view.read(cx).settings_open);
        assert_eq!(window.find("edit-light-profile").label(), Some("Edit light appearance profile (selected)"));
        assert_eq!(window.find("edit-dark-profile").label(), Some("Edit dark appearance profile"));
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Light);
        assert_eq!(view.read(cx).theme_id, "catppuccin-latte");
        window.within("settings-panel").click("font-ui-1", cx);
        window.render_frame(cx);
        assert_eq!(cx.theme().font_size, px(17.));
        assert_eq!(window.find("font-ui-value").label(), Some("UI font size: 17 pixels"));
        assert_eq!(view.read(cx).preferences.typography_for(ThemeMode::Light).ui_font_size, 17);
        window.click("edit-dark-profile", cx);
        window.render_frame(cx);
        assert!(view.read(cx).settings_open);
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Dark);
        assert_eq!(cx.theme().font_size, px(16.));
        window.click("edit-light-profile", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Light);
        assert_eq!(cx.theme().font_size, px(17.));

        scroll_theme_into_view(window, &view, "theme-solarized-light", cx);
        window.click("theme-solarized-light", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "solarized-light");
        assert_eq!(view.read(cx).preferences.light_theme.as_deref(), Some("solarized-light"));

        window.within("sheet-content").click("close", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.run_palette_action(super::PaletteAction::ToggleTheme, window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Dark);
        assert_eq!(cx.theme().font_size, px(16.));
        assert_eq!(view.read(cx).theme_id, "ciapre");
        view.update(cx, |view, cx| view.run_palette_action(super::PaletteAction::ToggleTheme, window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_mode, ThemeMode::Light);
        assert_eq!(cx.theme().font_size, px(17.));
        assert_eq!(view.read(cx).theme_id, "solarized-light");

        let saved = crate::desktop::preferences::Preferences::load();
        assert!(saved.separate_theme_profiles);
        assert_eq!(saved.light_theme.as_deref(), Some("solarized-light"));
        assert_eq!(saved.dark_theme.as_deref(), Some("ciapre"));
        assert_eq!(saved.typography_for(ThemeMode::Light).ui_font_size, 17);
        assert_eq!(saved.typography_for(ThemeMode::Dark).ui_font_size, 16);
    })
    .unwrap();
}

#[gpui_kit::test]
fn appearance_themes_scroll_horizontally_without_moving_typography(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 760., 700.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.click("open-settings", cx);
        window.render_frame(cx);
        let strip = window.find("theme-list").bounds();
        let first = window.find("theme-system-light").bounds();
        let typography_top = window.find("choose-ui-font").bounds().top();
        assert!(
            ["theme-github-dark", "theme-catppuccin-mocha", "theme-solarized-light"]
                .into_iter()
                .any(|id| window.find(id).bounds().left() > strip.right()),
            "the list must overflow horizontally"
        );
        assert_eq!(first.size.width, px(104.), "swatches must not shrink to fit");
        window.click("next-themes", cx);
        window.render_frame(cx);
        assert!(window.find("theme-system-light").bounds().left() < first.left());
        window.click("previous-themes", cx);
        window.render_frame(cx);
        assert_eq!(window.find("theme-system-light").bounds().left(), first.left());

        scroll_theme_into_view(window, &view, "theme-catppuccin-mocha", cx);
        let mocha = window.find("theme-catppuccin-mocha").bounds();
        assert!(mocha.left() >= strip.left() && mocha.right() <= strip.right());
        assert_eq!(window.find("choose-ui-font").bounds().top(), typography_top);
        window.click("theme-catppuccin-mocha", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "catppuccin-mocha");

        window.scroll("theme-list", gpui_kit::ScrollDelta::Lines(point(0., 200.)), cx);
        window.render_frame(cx);
        assert_eq!(window.find("choose-ui-font").bounds().top(), typography_top);
        window.click("theme-system-light", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_id, "system-light");
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
        scroll_theme_into_view(window, &view, "theme-github-dark", cx);
        window.click("theme-github-dark", cx);
        {
            let view = view.read(cx);
            assert_eq!(view.theme_id, "github-dark");
            assert_eq!(view.theme_mode, ThemeMode::Dark);
        }
        scroll_theme_into_view(window, &view, "theme-solarized-light", cx);
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
    let scroller = cx.update(|cx| {
        view.update(cx, |view, _| {
            for index in 0..20 {
                view.transcript.push(TranscriptItem::new(
                    format!("message-{index}"),
                    "assistant".to_string(),
                    "VT Code".into(),
                    format!("Message {index}\n\n{}", "wrapped words ".repeat(index + 1)),
                ));
            }
            view.transcript.push(TranscriptItem::new(
                "stream".to_string(),
                "assistant".to_string(),
                "VT Code".into(),
                "A tall paragraph.\n\n".repeat(100),
            ));
            view.message_scroller.clone()
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.within("pane-root").find_by_label("Conversation").role(), Some(gpui_kit::Role::Region));
        let last = window.find("transcript-row-stream").bounds();
        let viewport = window.find("transcript-scroll").bounds();
        assert!(last.size.height > viewport.size.height);
        assert!(last.bottom() <= viewport.bottom() + px(1.));
        assert!(last.bottom() >= viewport.bottom() - px(32.), "last={last:?}, viewport={viewport:?}");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.scroll("transcript-scroll", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-120.))), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.executor().advance_clock(Duration::from_millis(250));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let anchor = scroller.read(cx).logical_scroll_top();
        assert!(!scroller.read(cx).is_following_tail());
        assert!(scroller.read(cx).is_scrolled_up());
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
        let after = scroller.read(cx).logical_scroll_top();
        assert_eq!(after.item_ix, anchor.item_ix);
        assert!((after.offset_in_item - anchor.offset_in_item).abs() <= px(1.));
        assert_eq!(
            window
                .within("transcript-scroller")
                .find_by_label("Jump to latest message")
                .role(),
            Some(gpui_kit::Role::Button)
        );
        window.click((gpui_kit::ElementId::from("transcript-scroller"), "jump-to-latest"), cx);
        window.render_frame(cx);
        assert!(scroller.read(cx).is_following_tail());
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_eviction_preserves_reader_anchor(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let scroller = cx.update(|cx| {
        view.update(cx, |view, _| {
            for index in 0..MAX_TRANSCRIPT_ITEMS {
                view.transcript.push(TranscriptItem::new(
                    index.to_string(),
                    "assistant".to_string(),
                    "VT Code".into(),
                    format!("Message {index}"),
                ));
            }
            view.message_scroller.clone()
        })
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        scroller.update(cx, |state, cx| assert!(state.scroll_to_item(12, cx)));
        window.render_frame(cx);
        assert_eq!(scroller.read(cx).logical_scroll_top().item_ix, 12);

        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "newest".to_string(),
                "assistant".to_string(),
                "VT Code".into(),
                "Newest message".to_string(),
            ));
        });
        window.render_frame(cx);

        let anchor = scroller.read(cx).logical_scroll_top();
        assert_eq!(view.read(cx).transcript.get(anchor.item_ix).unwrap().id, "12");
        assert_eq!(scroller.read(cx).item_count(), MAX_TRANSCRIPT_ITEMS);
        assert!(!scroller.read(cx).is_following_tail());
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
                "VT Code".into(),
                source.to_string(),
            ));
        });
        window.render_frame(cx);
        window.click("copy-message-copy-test", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(source));
        assert_eq!(window.notifications(cx).len(), 1);
        focus_element_by_tab(window, "copy-code", cx);
        window.press("space", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap().trim_end(), "fn sample() -> u8 { 7 }");
        assert_eq!(window.notifications(cx).len(), 1);
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
                    "VT Code".into(),
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
        let scroller = view.read(cx).message_scroller.clone();
        assert!(scroller.read(cx).is_following_tail());
        assert!(row.bottom() <= window.find("transcript-scroll").bounds().bottom() + px(1.));
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
fn webmcp_connect_command_issues_a_pair_request_without_starting_an_agent_turn(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer.update(cx, |state, cx| {
                state.set_value("/webmcp connect https://vtcode.vinhnx.chatgpt.site", window, cx)
            });
            view.submit_prompt(window, cx);
            assert!(view.settings_open);
            assert!(!view.running);
            assert!(view.composer.read(cx).value().is_empty());
        });
        window.render_frame(cx);
        assert!(window.try_find("connections-settings").is_some());
    })
    .unwrap();

    let Some(BackendCommand::Request { method, params }) = requests.try_recv().ok() else {
        panic!("connect command should request a browser pairing");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_PAIR);
    assert_eq!(params["origin"], "https://vtcode.vinhnx.chatgpt.site");
    assert_eq!(params["replace"], false);
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn composer_cli_webmcp_button_opens_connections_settings(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.backend = Some(BackendHandle { commands }));
        window.render_frame(cx);
        window.click("composer-cli-webmcp", cx);
        window.render_frame(cx);
        assert!(window.try_find("connections-settings").is_some());
    })
    .unwrap();

    let Some(BackendCommand::Request { method, .. }) = requests.try_recv().ok() else {
        panic!("WebMCP button should refresh connection status");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WEBMCP_STATUS);
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn webmcp_tools_and_roots_commands_show_shared_guidance_without_backend_requests(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.composer
                .update(cx, |state, cx| state.set_value("/webmcp tools", window, cx));
            view.submit_prompt(window, cx);
            assert!(view.settings_open);
            assert!(!view.running);
            assert_eq!(view.webmcp_notice.as_deref(), Some(vtcode_desktop_protocol::slash_command::WEBMCP_TOOLS_HELP));

            view.composer
                .update(cx, |state, cx| state.set_value("/webmcp roots", window, cx));
            view.submit_prompt(window, cx);
            assert_eq!(view.webmcp_notice.as_deref(), Some(vtcode_desktop_protocol::slash_command::WEBMCP_ROOTS_HELP));
        });
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
            assert!(
                view.error
                    .as_deref()
                    .is_some_and(|error| error.contains("Restore or dismiss the retained message"))
            );
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
fn service_launch_failure_exposes_a_retry_path(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.first_submission = Some(super::SubmissionDraft {
                prompt: "Keep this request".into(),
                attachments: Vec::new(),
                echo_id: "first-request".into(),
                delivery_uncertain: false,
            });
            view.webmcp_status = Some(connected_webmcp_status());
            view.webmcp_pairing = Some(pending_webmcp_pairing());
            view.webmcp_request_pending = Some(vtcode_desktop_protocol::method::WEBMCP_STATUS);
            view.handle_backend_event(BackendEvent::LaunchFailed("Helper executable is unavailable.".into()), cx);
            assert_eq!(view.status, "Could not connect to the local VT Code service");
            assert_eq!(view.retained_submission.as_ref().map(|draft| draft.prompt.as_str()), Some("Keep this request"));
            assert!(view.webmcp_status.is_none());
            assert!(view.webmcp_pairing.is_none());
            assert!(view.webmcp_request_pending.is_none());
            assert!(!view.can_reload_cli_settings(), "resolve retained content before a service restart");
            assert!(view.error.as_deref().is_some_and(|error| {
                error.contains("Helper executable is unavailable")
                    && error.contains("Restore or dismiss the retained message")
                    && error.contains("Reload CLI settings")
            }));
        });

        window.render_frame(cx);
        let error = window.find("composer-error");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert!(error.label().unwrap().contains("Reload CLI settings"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn service_exit_clears_pending_interaction_and_answer_before_retry(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{InteractionKind, InteractionRequest};

    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.interaction = Some(InteractionRequest {
                session_id: "session".into(),
                turn_id: "turn".into(),
                interaction_id: "approval".into(),
                kind: InteractionKind::Question,
                wizard: false,
                title: "Question".into(),
                details: Vec::new(),
                choices: Vec::new(),
                steps: Vec::new(),
                selected_choice_id: None,
                selected_choice_ids: Vec::new(),
            });
            view.interaction_choice_ids = vec![Some("approve".into())];
            view.interaction_answer_texts = vec![Some("private answer".into())];
            view.question_answer
                .update(cx, |state, cx| state.set_value("private answer", window, cx));
            view.event_tx
                .send(BackendEvent::Exited { status: Some(1), authentication_required: false })
                .unwrap();

            let _batch_full = view.drain_events(window, cx);

            assert!(view.interaction.is_none());
            assert!(view.interaction_choice_ids.is_empty());
            assert!(view.interaction_answer_texts.is_empty());
            assert!(view.question_answer.read(cx).value().is_empty());
            assert!(view.can_reload_cli_settings(), "an exited interaction cannot block reconnection");
        });
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn service_exit_discards_stale_webmcp_state_and_refreshes_after_reconnect(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.webmcp_status = Some(connected_webmcp_status());
            view.webmcp_pairing = Some(pending_webmcp_pairing());
            view.webmcp_request_pending = Some(vtcode_desktop_protocol::method::WEBMCP_STATUS);
            view.event_tx
                .send(BackendEvent::Exited { status: Some(1), authentication_required: false })
                .unwrap();

            let _batch_full = view.drain_events(window, cx);

            assert!(view.webmcp_status.is_none());
            assert!(view.webmcp_pairing.is_none(), "pairing codes belong to the exited service process");
            assert!(view.webmcp_request_pending.is_none(), "an abandoned request cannot keep the panel busy");
            assert!(
                view.webmcp_notice
                    .as_deref()
                    .is_some_and(|notice| notice.contains("disconnected"))
            );

            let (commands, mut requests) = tokio::sync::mpsc::channel(8);
            view.backend = Some(BackendHandle { commands });
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::INITIALIZE.into(),
                    result: Some(json!({
                        "protocol": vtcode_desktop_protocol::PROTOCOL_VERSION,
                        "workspace": "/workspace-new",
                        "serverVersion": "test",
                    })),
                    error: None,
                },
                cx,
            );
            let mut methods = Vec::new();
            for _ in 0..5 {
                let Some(BackendCommand::Request { method, .. }) = requests.try_recv().ok() else {
                    panic!("initialization should refresh WebMCP state");
                };
                methods.push(method);
            }
            assert_eq!(methods.last().map(String::as_str), Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
            assert_eq!(view.webmcp_request_pending, Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
            assert!(view.webmcp_status.is_none(), "the new status appears only after its response arrives");

            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::WEBMCP_STATUS.into(),
                    result: Some(serde_json::to_value(connected_webmcp_status()).unwrap()),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.webmcp_status, Some(connected_webmcp_status()));
            assert!(view.webmcp_request_pending.is_none());
            assert!(view.webmcp_notice.is_none(), "a successful refresh clears the disconnect notice");
        });
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn unexpected_service_exit_replaces_connected_status_and_exposes_recovery(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.status = "Connected to the local VT Code service".into();
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);

            assert!(view.backend.is_none());
            assert_eq!(view.status, "Local service exited with code 1");
            assert!(view.can_reload_cli_settings(), "disconnected workspaces can restart the service");
            assert!(view.error.as_deref().is_some_and(|error| error.contains("Reload CLI settings")));
        });

        window.render_frame(cx);
        let error = window.find("composer-error");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert!(error.label().unwrap().contains("Reload CLI settings"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn authentication_failure_explains_how_to_reconnect_without_restarting_desktop(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.retained_submission = Some(super::SubmissionDraft {
                prompt: "Uncertain request".into(),
                attachments: Vec::new(),
                echo_id: "uncertain-request".into(),
                delivery_uncertain: true,
            });
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: true }, cx);
            assert_eq!(view.status, "Provider credentials required");
            let error = view.error.as_deref().unwrap();
            assert!(error.contains("vtcode auth"));
            assert!(error.contains("Restore or dismiss the retained message"));
            assert!(error.contains("Reload CLI settings"));
            assert!(!error.contains("restart the desktop app"));
            assert!(!view.can_reload_cli_settings(), "the retained submission must be resolved before restart");
        });
        window.render_frame(cx);
        let error = window.find("composer-error");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert!(error.label().unwrap().contains("Reload CLI settings"));
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
fn workspace_shortcuts_match_navigation_and_review_controls(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
            });
        });
        window.render_frame(cx);

        window.press("cmd-shift-u", cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Utilities);

        window.press("cmd-shift-e", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Files);

        window.press("cmd-shift-g", cx);
        window.render_frame(cx);
        assert!(view.read(cx).review_open);
        assert!(view.read(cx).workspace_review.is_some());

        window.press("cmd-r", cx);
        assert!(view.read(cx).workspace_review.as_ref().unwrap().pending);
    })
    .unwrap();

    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("workspace review request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::WORKSPACE_REVIEW);
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
        assert_eq!(view.read(cx).transcript.len(), 1);
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
fn multi_step_question_progress_tracks_the_runtime_step_order(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{InteractionChoice, InteractionKind, InteractionRequest, InteractionStep};

    let (window, view) = open_desktop(cx);
    let make_choice = |id: &str| InteractionChoice {
        id: id.into(),
        label: format!("Choose {id}"),
        description: None,
        recommended: false,
    };
    let steps = [
        ("Workspace", "Choose a workspace", "workspace"),
        ("Task", "Describe the task", "task"),
        ("Review", "Review the plan", "review"),
    ]
    .into_iter()
    .map(|(title, question, choice_id)| InteractionStep {
        title: title.into(),
        question: question.into(),
        choices: vec![make_choice(choice_id)],
        selected_choice_id: None,
        allow_freeform: false,
    })
    .collect();
    view.update(cx, |view, cx| {
        view.interaction = Some(InteractionRequest {
            session_id: "session".into(),
            turn_id: "turn".into(),
            interaction_id: "question-wizard".into(),
            kind: InteractionKind::Question,
            wizard: true,
            title: "Set up the task".into(),
            details: Vec::new(),
            choices: Vec::new(),
            steps,
            selected_choice_id: None,
            selected_choice_ids: Vec::new(),
        });
        view.interaction_step_index = 0;
        view.interaction_choice_ids = vec![None; 3];
        view.interaction_answer_texts = vec![None; 3];
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("interaction-question-progress").label(), Some("Question progress"));
        let stepper = window
            .within("interaction-question-progress")
            .find("interaction-question-stepper");
        assert!(stepper.visible());
        assert_eq!(stepper.role(), Some(gpui_kit::Role::List));
        assert_eq!(stepper.label(), Some("Question steps"));
        assert_eq!(window.find("question-step-counter").label(), Some("Question 1 of 3"));
        window.click("choice-workspace", cx);
        window.click("continue-question", cx);
        window.render_frame(cx);
        assert_eq!(window.find("question-step-counter").label(), Some("Question 2 of 3"));
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
    let (commands, mut requests) = tokio::sync::mpsc::channel(5);
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
                vtcode_desktop_protocol::method::WEBMCP_STATUS,
            ] {
                let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else { panic!("discovery") };
                assert_eq!(method, expected);
            }
            assert_eq!(view.webmcp_request_pending, Some(vtcode_desktop_protocol::method::WEBMCP_STATUS));
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
        assert_eq!(window.find(palette_action_id(&super::PaletteAction::CloseSession)).label(), Some("Close session"));
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
    let (window, view) = open_desktop_sized(cx, 600., 900.);
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
            if key == "end" || key == "left" {
                let scroll = &view.read(cx).inspector_tabs_scroll;
                let visible = horizontal_scroll_item_is_visible(scroll, 4);
                let offset = scroll.offset().x;
                assert!(visible, "keyboard selection reveals the final inspector tab; scroll offset={offset:?}");
                assert!(offset < px(0.), "narrow inspector tab row must scroll to its final tab");
            }
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
fn hiding_a_focused_inspector_restores_the_desktop_focus_path(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).inspector_focus.is_focused(window));

        view.update(cx, |view, cx| view.toggle_inspector(cx));
        window.render_frame(cx);

        assert!(!view.read(cx).inspector_open);
        assert!(view.read(cx).focus_handle.is_focused(window));
        window.press("cmd-l", cx);
        window.render_frame(cx);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_programmatic_selection_reveals_selected_tab(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 900.);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_utilities(cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Utilities);
        assert!(
            horizontal_scroll_item_is_visible(&view.read(cx).inspector_tabs_scroll, 4),
            "workflow selection reveals the selected inspector tab"
        );
        assert!(
            view.read(cx).inspector_tabs_scroll.offset().x < px(0.),
            "workflow selection must scroll the narrow inspector tab row"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_model_picker_keeps_current_model_without_resetting_a_session(cx: &mut TestAppContext) {
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
                recommended_rank: None,
                capabilities: None,
            }];
            view.composer
                .update(cx, |state, cx| state.set_value("Keep my draft", window, cx));
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Select model"));
        window.click("model-search-all", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
        let current_model_item = window.find(palette_action_id(&super::PaletteAction::SelectModel {
            provider: "provider-a".into(),
            model: "custom-current-id".into(),
        }));
        let current_model_label = current_model_item.label().unwrap();
        assert!(current_model_label.contains("123.45K context"));
        assert!(current_model_label.contains("Current"));
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
                recommended_rank: None,
                capabilities: None,
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
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
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
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-320.))), cx);
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
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_sheet_close_button_restores_the_previous_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-l", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        assert!(window.within("sheet-content").find("close").visible());
        window.within("sheet-content").click("close", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        assert!(!window.has_active_sheet(cx), "sheet close button did not close the Kit sheet");
        assert!(!view.read(cx).settings_open);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn welcome_workspace_prompt_wraps_long_names_and_preserves_workspace_guards(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 700.);
    let workspace_name = "A workspace with a deliberately long name for responsive welcome typography";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.sidebar_open = false;
            view.inspector_open = false;
            view.welcome_period = WelcomePeriod::Morning;
            view.welcome_copy_index = 0;
            view.workspace = Some(PathBuf::from("/projects").join(workspace_name));
        });
        window.render_frame(cx);
        let welcome = window.find("welcome").bounds();
        let link = window.find("welcome-workspace");
        assert_eq!(link.role(), Some(gpui_kit::Role::Button));
        assert_eq!(link.label(), Some(format!("Change workspace, current: {workspace_name}").as_str()));
        assert!(link.bounds().left() >= welcome.left() && link.bounds().right() <= welcome.right());
        assert!(link.bounds().size.height >= px(28.), "workspace link must retain headline-sized text");
        assert_eq!(window.find("welcome-prompt").role(), Some(gpui_kit::Role::Heading));
        assert_eq!(
            window.find("welcome-prompt").label(),
            Some(format!("Good morning. What should we build in {workspace_name}?").as_str())
        );
        view.update(cx, |view, _| view.workspace_picker_open = true);
        window.render_frame(cx);
        assert_eq!(window.find("welcome-workspace").disabled(), Some(true));
        view.update(cx, |view, _| {
            view.workspace_picker_open = false;
            view.welcome_period = WelcomePeriod::Evening;
            view.welcome_copy_index = 2;
            view.workspace = None;
        });
        window.render_frame(cx);
        assert!(window.find_all("welcome-workspace").is_empty());
        assert_eq!(window.find("welcome-prompt").label(), Some("Good evening. What are we exploring?"));
        assert_eq!(window.find("open-first-workspace").label(), Some("Select a folder"));
        window.within("welcome").click("open-first-workspace", cx);
        assert!(view.read(cx).workspace_picker_open, "the welcome action opens the native workspace picker");
    })
    .unwrap();

    assert!(cx.did_prompt_for_paths());
    cx.simulate_path_prompt_response(|options| {
        assert!(options.directories);
        None
    });
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert!(!view.read(cx).workspace_picker_open, "canceling the picker restores the welcome state");
    })
    .unwrap();
}

#[test]
fn welcome_greeting_uses_local_day_period_boundaries() {
    use super::WelcomePeriod::{Afternoon, Evening, Morning};

    assert_eq!(super::welcome_period_for_hour(5), Morning);
    assert_eq!(super::welcome_period_for_hour(11), Morning);
    assert_eq!(super::welcome_period_for_hour(12), Afternoon);
    assert_eq!(super::welcome_period_for_hour(16), Afternoon);
    assert_eq!(super::welcome_period_for_hour(17), Evening);
    assert_eq!(super::welcome_period_for_hour(4), Evening);
}

#[gpui_kit::test]
fn quiet_welcome_keeps_workspace_utilities_in_navigation_without_creating_a_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("welcome").label(), Some("VT Code welcome"));
        assert!(window.find_all("thread-header").is_empty());
        for id in ["welcome-review-changes", "welcome-browse-files", "welcome-choose-model"] {
            assert!(window.find_all(id).is_empty(), "welcome should not duplicate workspace controls");
        }
        window.click("nav-utilities", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Utilities);
        assert!(view.read(cx).selected_session_id.is_none());
        assert_eq!(window.find("send-prompt").label(), Some("Send message"));
        view.update(cx, |view, _| {
            view.workspace = None;
            view.inspector_open = false;
            view.welcome_copy_index = 0;
        });
        window.render_frame(cx);
        assert_eq!(window.within("welcome").find("open-first-workspace").label(), Some("Choose a workspace"));
        view.update(cx, |view, _| {
            view.workspace = Some(std::env::temp_dir());
            view.transcript.push(TranscriptItem::new(
                "first-message".into(),
                "user".into(),
                "You".into(),
                "Inspect this workspace".into(),
            ));
        });
        window.render_frame(cx);
        assert!(window.find_all("welcome").is_empty());
        assert_eq!(window.find_all("thread-header").len(), 1);
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
            Some("Review src/app.rs (selected)"),
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
        view.update(cx, |view, cx| {
            view.event_tx.send(notification("edit", "src/app.rs", "invalid patch")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(view.selected_change.is_none());
            assert!(view.change_viewer.is_none());
            assert!(!view.review_open);
            assert!(!view.review_focus.contains_focused(window, cx));
            assert!(view.composer.read(cx).focus_handle(cx).is_focused(window));
        });
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
            assert!(rows[0].has_output);
            assert!(rows[1].has_output);
            assert!(view.transcript.items().any(|item| item.text == "Unstructured archive result"));
            let restored_output =
                crate::desktop::inspector::output_for(&view.transcript, &view.restored_activity, None).unwrap();
            assert_eq!(restored_output.text, output);
            assert!(!restored_output.running);
            let restored_command = view
                .restored_activity
                .iter()
                .find(|item| item.id == "saved-command")
                .expect("historical command activity");
            assert!(!restored_command.text.contains(output), "output is not duplicated in activity metadata");
            assert_eq!(restored_command.output.as_deref(), Some(output), "full output remains recoverable");
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
        });
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some(super::PARTIAL_ACTIVITY_HISTORY_MESSAGE));
        assert_eq!(window.find("activity-saved-command").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("activity-saved-command").label(), Some("cargo check --locked"));
        let output_action = window.find("activity-output-finished-command");
        assert_eq!(output_action.role(), Some(gpui_kit::Role::Button));
        assert_eq!(output_action.label(), Some("View output for previous command"));
        window.click("activity-output-finished-command", cx);
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some(super::PARTIAL_ACTIVITY_HISTORY_MESSAGE));
        assert_eq!(window.find("output-body").label(), Some("previous output"));
        assert_eq!(view.read(cx).selected_output_id.as_deref(), Some("finished-command"));
        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("previous output"));
        assert_eq!(window.find("output-follow-latest").label(), Some("Follow latest output"));
        window.click("output-follow-latest", cx);
        window.render_frame(cx);
        assert!(view.read(cx).selected_output_id.is_none());
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
            assert!(crate::desktop::inspector::output_for(&view.transcript, &view.restored_activity, None)
                .unwrap()
                .running);
            assert_eq!(view.restored_activity.len(), 1, "live identity replaces its historical projection");
            assert_eq!(crate::desktop::inspector::activity_rows(&view.transcript, &view.restored_activity).len(), 2);
            view.reset_session_state();
            assert!(view.restored_activity.is_empty());
            assert!(view.selected_output_id.is_none());
            assert!(!view.restored_review_history_incomplete);
            assert!(!view.restored_activity_history_incomplete);
            assert!(crate::desktop::inspector::output_for(&view.transcript, &view.restored_activity, None).is_none());
        });
    }).unwrap();
}

#[gpui_kit::test]
fn output_tab_bounds_rendered_large_output_but_copies_the_full_capture(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let output = format!("command output\n{}", "界".repeat(90_000));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item =
                TranscriptItem::new("large-output".into(), "tool".into(), "Large command".into(), "completed".into());
            item.output = Some(output.clone().into());
            item.expanded = true;
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);

        let output_body = window.find("output-body");
        let rendered = output_body.label().unwrap();
        assert!(rendered.starts_with("… Earlier output truncated"));
        assert!(rendered.len() <= 135_000, "the accessible output excerpt is byte-bounded");
        assert!(rendered.ends_with("界"));
        let tool_body = window.find("tool-output-large-output");
        let tool_excerpt = tool_body.label().unwrap();
        assert!(tool_excerpt.starts_with("… Earlier output truncated"));
        assert!(tool_excerpt.len() <= 135_000, "expanded tool details use the same byte bound");

        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(output.as_str()));
    })
    .unwrap();
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
    cx.update(|cx| cx.set_reduce_motion(true));
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
        let loading = window.find("workspace-review-loading");
        assert_eq!(loading.role(), Some(gpui_kit::Role::Status));
        assert_eq!(loading.label(), Some("Reading Git changes"));
        assert!(window.find("workspace-review-placeholder-file").visible());
        cx.set_reduce_motion(false);
        window.render_frame(cx);
        assert!(window.find("workspace-review-placeholder-file").visible());
        cx.set_reduce_motion(true);
        window.render_frame(cx);
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
        assert_eq!(window.find("transcript-row-answer").role(), None);
        assert_eq!(window.find("transcript-row-check").role(), Some(gpui_kit::Role::ListItem));
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
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("font-ui-value").label(), Some("UI font size: 16 pixels"));
        assert_eq!(window.find("font-code-value").label(), Some("Code font size: 14 pixels"));
        let preview_height = window.find("appearance-code-preview").bounds().size.height;
        assert!(preview_height > px(0.), "the highlighted code sample should have rendered layout");
        window.click("font-ui-1", cx);
        window.click("font-code-1", cx);
        window.render_frame(cx);
        assert_eq!(window.find("font-ui-value").label(), Some("UI font size: 17 pixels"));
        assert_eq!(window.find("font-code-value").label(), Some("Code font size: 15 pixels"));
        assert!(
            window.find("appearance-code-preview").bounds().size.height > preview_height,
            "changing code size should re-layout the syntax-highlighted preview"
        );
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
        select_settings_category(window, 1, cx);
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-240.))), cx);
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
        let navigation = window.find("left-navigation");
        assert_eq!(navigation.role(), Some(gpui_kit::Role::Navigation));
        assert_eq!(navigation.label(), Some("VT Code navigation"));
        assert_eq!(window.find("project-thread-sidebar").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("project-thread-sidebar").label(), Some("Workspace, projects, and recent threads"));
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
fn recent_threads_virtualize_rows_and_scroll_to_older_sessions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let sessions = (0..120)
        .map(|index| vtcode_desktop_protocol::SessionSummary {
            session_id: format!("thread-{index}"),
            title: Some(format!("Thread {index}")),
            workspace: "/project/vtcode".into(),
            completed: false,
            resumable: true,
        })
        .collect();
    view.update(cx, |view, _| view.sessions = sessions);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("session-thread-0").is_some(), "the first thread is visible at the top");
        assert!(window.try_find("session-thread-119").is_none(), "offscreen threads are not built into the UI tree");

        view.read(cx).session_scroll.scroll_to_item(119, gpui_kit::ScrollStrategy::Top);
        window.render_frame(cx);

        assert!(window.try_find("session-thread-119").is_some(), "scrolling renders the requested thread");
        assert!(window.try_find("session-thread-0").is_none(), "the previous top rows leave the rendered range");
    })
    .unwrap();
}

#[test]
fn fixed_virtual_list_geometry_reuses_rows_until_count_or_font_size_changes() {
    let mut cache = FixedRowSizeCache::default();
    let default_session_height = scale_row_height_for_ui_font(36., px(16.));
    let large_session_height = scale_row_height_for_ui_font(36., px(24.));
    assert_eq!(default_session_height, 36.);
    assert_eq!(large_session_height, 54.);
    assert_eq!(scale_row_height_for_ui_font(72., px(24.)), 108.);

    let first = cache.get(120, default_session_height);
    let same_geometry = cache.get(120, default_session_height);
    assert!(std::rc::Rc::ptr_eq(&first, &same_geometry));
    assert_eq!(first.len(), 120);
    assert_eq!(first[0].height, px(36.));

    let changed_count = cache.get(121, default_session_height);
    assert!(!std::rc::Rc::ptr_eq(&same_geometry, &changed_count));
    assert_eq!(changed_count.len(), 121);

    let changed_font_size = cache.get(121, large_session_height);
    assert!(!std::rc::Rc::ptr_eq(&changed_count, &changed_font_size));
    assert_eq!(changed_font_size[0].height, px(54.));
}

#[gpui_kit::test]
fn recent_session_rows_scale_with_the_accessibility_font_size(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.preferences.ui_font_size = Some(24);
        view.sessions.push(vtcode_desktop_protocol::SessionSummary {
            session_id: "font-scale".into(),
            title: Some("Readable thread title".into()),
            workspace: std::env::temp_dir().display().to_string(),
            completed: false,
            resumable: true,
        });
        view.apply_typography(cx);
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("session-row-font-scale").bounds().size.height, px(54.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_rows_scale_with_the_accessibility_font_size(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.preferences.ui_font_size = Some(24);
        view.apply_typography(cx);
        view.inspector_open = true;
        view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
        view.transcript.push(TranscriptItem::new(
            "tool-font-scale".into(),
            "tool".into(),
            "Read source".into(),
            "completed".into(),
        ));
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("activity-tool-font-scale").bounds().size.height, px(108.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_workspace_popover_exposes_recent_projects_and_folder_picker(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.workspace = Some(PathBuf::from("/project/alpha"));
        view.preferences.recent_workspaces = vec!["/project/alpha".into(), "/project/beta".into()];
        cx.notify();
    });

    let alpha_id = super::opaque_path_element_id("recent-workspace-popover", "/project/alpha");
    let beta_id = super::opaque_path_element_id("recent-workspace-popover", "/project/beta");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("composer-workspace", cx);
        window.render_frame(cx);

        assert_eq!(window.find("open-workspace-folder").label(), Some("Choose folder…"));
        assert_eq!(window.find(alpha_id.clone()).label(), Some("alpha (current workspace)"));
        assert_eq!(window.find(beta_id.clone()).label(), Some("beta"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn selecting_a_recent_workspace_uses_the_existing_shutdown_transition(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let target = tempfile::tempdir().unwrap();
    let target_path = target.path().to_path_buf();
    let canonical_target = vtcode_commons::paths::canonicalize(&target_path).unwrap();
    let target_display = target_path.display().to_string();
    let (commands, mut received) = tokio::sync::mpsc::channel(1);
    view.update(cx, |view, cx| {
        view.preferences.recent_workspaces = vec![target_display];
        view.backend = Some(BackendHandle { commands });
        cx.notify();
    });

    let target_id = super::opaque_path_element_id("recent-workspace-popover", &target_path.display().to_string());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("composer-workspace", cx);
        window.render_frame(cx);
        window.click(target_id, cx);
        window.render_frame(cx);
    })
    .unwrap();

    cx.update(|cx| {
        assert_eq!(view.read(cx).pending_workspace.as_deref(), Some(canonical_target.as_path()));
    });
    assert!(matches!(received.try_recv(), Ok(BackendCommand::Stop)));
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
        let status_bar = window.find("status-bar");
        assert_eq!(status_bar.role(), Some(gpui_kit::Role::Group));
        assert_eq!(status_bar.label(), Some("Status bar"));
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
fn restored_history_accessibility_ids_follow_message_content_across_reload(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::SessionHistoryMessage;

    let (window, view) = open_desktop(cx);
    let session_id = "archive-session";
    let prompt = SessionHistoryMessage { role: "user".into(), text: "Question".into() };
    let answer = SessionHistoryMessage { role: "assistant".into(), text: "Answer".into() };
    let answer_id = super::restored_history_element_id(session_id, &answer.role, &answer.text, 0);
    let repeated_answer_id = super::restored_history_element_id(session_id, &answer.role, &answer.text, 1);
    assert_ne!(answer_id, repeated_answer_id, "repeated archive messages need distinct row identities");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.load_history(session_id, vec![prompt.clone(), answer.clone()]));
        window.render_frame(cx);
        assert_eq!(window.find(format!("message-body-{answer_id}")).label(), Some("VT Code: Answer"));

        let inserted = SessionHistoryMessage {
            role: "user".into(),
            text: "A newer question".into(),
        };
        // The snapshot can advance while the original row remains in its projection.
        view.update(cx, |view, _| {
            view.load_history(
                session_id,
                vec![
                    inserted,
                    prompt,
                    answer,
                    SessionHistoryMessage { role: "assistant".into(), text: "Answer".into() },
                ],
            )
        });
        window.render_frame(cx);
        assert_eq!(window.find(format!("message-body-{answer_id}")).label(), Some("VT Code: Answer"));
        assert_eq!(window.find(format!("message-body-{repeated_answer_id}")).label(), Some("VT Code: Answer"));
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
        window.click("copy-tool-output-build", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("compiler error: missing name"));
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
                    recommended_rank: None,
                    capabilities: None,
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
        assert_eq!(window.find("utilities-workspace-group").role(), Some(gpui_kit::Role::Group));
        assert!(window.find("utilities-workspace-group").label().is_some());
        assert_eq!(
            window.find("utilities-activity").label(),
            Some("Recent activity: 1 running · 1 done · 1 failed · 0 interrupted · 0 other")
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
        window.render_frame(cx);
        assert_eq!(
            window.find("activity-status-chart").label(),
            Some("Activity statuses: 1 running · 1 done · 1 failed · 0 interrupted · 0 other")
        );
        assert!(window.try_find("activity-read").is_some(), "the virtual list renders visible activity rows");
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
fn activity_tab_virtualizes_the_bounded_tool_history(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
            for index in 0..300 {
                view.transcript.push(TranscriptItem::new(
                    format!("tool-{index}"),
                    "tool".into(),
                    format!("tool {index}").into(),
                    "completed".into(),
                ));
            }
        });
        window.render_frame(cx);

        let rendered_row_indexes = (0..300)
            .filter(|index| window.try_find(format!("activity-tool-{index}")).is_some())
            .collect::<Vec<_>>();
        assert!(!rendered_row_indexes.is_empty(), "visible tool activity is rendered");
        assert!(rendered_row_indexes.len() < 300, "off-screen tool activity stays virtualized");
        let first_visible = rendered_row_indexes[0];
        let expected_label = format!("tool {first_visible}");
        assert_eq!(
            window.find(format!("activity-tool-{first_visible}")).label(),
            Some(expected_label.as_str()),
            "borrowing the row projection preserves its accessible label"
        );
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
        });
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        window.click("edit-shortcuts", cx);
        view.update(cx, |view, cx| {
            view.shortcut_inputs[0].update(cx, |input, cx| input.set_value("cmd-n", window, cx));
        });
        window.render_frame(cx);
        focus_element_by_tab(window, "save-shortcuts", cx);
        window.press("enter", cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("already assigned"));
        assert!(crate::desktop::preferences::Preferences::load().shortcuts.is_empty());
        view.update(cx, |view, cx| {
            view.shortcut_inputs[0].update(cx, |input, cx| input.set_value("ctrl-shift-k", window, cx))
        });
        focus_element_by_tab(window, "save-shortcuts", cx);
        window.press("enter", cx);
        assert_eq!(
            crate::desktop::preferences::Preferences::load()
                .shortcuts
                .get("commands")
                .map(String::as_str),
            Some("ctrl-shift-k")
        );
        window.render_frame(cx);
        window.click("edit-shortcuts", cx);
        focus_element_by_tab(window, "reset-shortcuts", cx);
        window.press("enter", cx);
        assert!(crate::desktop::preferences::Preferences::load().shortcuts.is_empty());
        assert_eq!(view.read(cx).shortcut_inputs[0].read(cx).value(), "cmd-k");
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_categories_are_keyboard_navigable_and_shortcuts_use_kit_kbd(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 450., 900.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("settings-panel").role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(window.find("settings-navigation").label(), Some("Settings categories"));
        assert!(window.try_find("appearance-settings").is_some());
        assert_eq!(window.find("appearance-visual-style-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("appearance-visual-style-group").label(), Some("Visual style"));
        assert_eq!(window.find("appearance-typography-group").role(), Some(gpui_kit::Role::Group));

        window.press("right", cx);
        window.render_frame(cx);
        assert!(window.try_find("accessibility-settings").is_some());
        assert_eq!(window.find("accessibility-display-group").role(), Some(gpui_kit::Role::Group));
        window.press("end", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find("connections-settings").is_some());
        assert_eq!(window.within("settings-tabs").find(4usize).selected(), Some(true));
        let scroll = &view.read(cx).settings_tabs_scroll;
        let offset = scroll.offset().x;
        assert!(
            horizontal_scroll_item_is_visible(scroll, 4),
            "End reveals the selected settings category; scroll offset={offset:?}"
        );
        assert!(offset < px(0.), "narrow settings tabs must scroll to the selected category");

        select_settings_category(window, 2, cx);
        assert!(window.try_find("keyboard-settings").is_some());
        assert_eq!(window.find("keyboard-shortcuts-group").role(), Some(gpui_kit::Role::Group));
        let context = gpui_kit::KeyBindingContextPredicate::parse("VTCodeDesktop").unwrap();
        let binding = gpui_kit::KeyBinding::load(
            "ctrl-shift-k",
            Box::new(super::OpenCommandPalette),
            Some(context.into()),
            false,
            None,
            &gpui_kit::DummyKeyboardMapper,
        )
        .unwrap();
        cx.bind_keys([binding]);
        window.render_frame(cx);
        window.render_frame(cx);
        let expected_shortcut_label = format!(
            "Shortcut for Search commands: {}",
            gpui_kit::component::kbd::Kbd::format(&gpui_kit::Keystroke::parse("ctrl-shift-k").unwrap())
        );
        let shortcut = window.find("shortcut-badge-commands");
        assert_eq!(shortcut.role(), Some(gpui_kit::Role::Label));
        assert_eq!(shortcut.label(), Some(expected_shortcut_label.as_str()));
        assert!(window.try_find("shortcut-input-commands").is_none(), "inputs stay hidden until edit mode");

        window.click("edit-shortcuts", cx);
        window.render_frame(cx);
        assert!(window.try_find("shortcut-input-commands").is_some());
        view.update(cx, |view, cx| {
            view.shortcut_inputs[0].update(cx, |input, cx| input.set_value("ctrl-shift-k", window, cx));
        });
        window.click("cancel-shortcuts", cx);
        window.render_frame(cx);
        assert!(window.try_find("shortcut-input-commands").is_none());
        assert_eq!(
            crate::desktop::preferences::Preferences::load().shortcuts.get("commands"),
            None,
            "cancelling discards unsaved bindings"
        );
        assert_eq!(window.find("shortcut-badge-commands").label(), Some(expected_shortcut_label.as_str()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_sheet_and_category_tabs_fit_a_narrow_window(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 700.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let sheet = window.find("sheet-content").bounds();
        let panel = window.find("settings-panel").bounds();
        let viewport = window.viewport_size();
        assert!(
            sheet.origin.x >= px(0.) && sheet.origin.y >= px(0.),
            "settings sheet starts outside viewport: sheet={sheet:?}, viewport={viewport:?}"
        );
        assert!(
            sheet.origin.x + sheet.size.width <= viewport.width,
            "settings sheet exceeds viewport width: sheet={sheet:?}, viewport={viewport:?}"
        );
        assert!(
            sheet.origin.y + sheet.size.height <= viewport.height,
            "settings sheet exceeds viewport height: sheet={sheet:?}, viewport={viewport:?}"
        );
        assert!(panel.size.width <= sheet.size.width);

        select_settings_category(window, 2, cx);
        window.render_frame(cx);
        let navigation = window.find("settings-navigation").bounds();
        let content = window.find("settings-content").bounds();
        assert!(navigation.origin.x >= panel.origin.x);
        assert!(navigation.origin.x + navigation.size.width <= panel.origin.x + panel.size.width);
        assert!(content.origin.x >= panel.origin.x);
        assert!(content.origin.x + content.size.width <= panel.origin.x + panel.size.width);
        assert!(window.try_find("keyboard-settings").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_tab_bar_stays_fixed_while_settings_content_scrolls(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 560.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.set_ui_font_size(view.theme_mode, 24);
            view.apply_typography(cx);
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        window.render_frame(cx);

        let sheet = window.find("sheet-content").bounds();
        let panel = window.find("settings-panel").bounds();
        assert!(
            panel.origin.y + panel.size.height <= sheet.origin.y + sheet.size.height,
            "large UI typography should keep settings within the sheet: panel={panel:?}, sheet={sheet:?}"
        );
        let navigation_before = window.find("settings-navigation").bounds();
        window.scroll("settings-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-240.))), cx);
        window.render_frame(cx);

        let offset = view.read(cx).settings_scroll.offset().y;
        assert!(offset != px(0.), "settings content should scroll independently");
        assert_eq!(window.find("settings-navigation").bounds(), navigation_before);
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_sheet_tracks_window_width_after_open(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1_100., 900.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
    })
    .unwrap();

    cx.simulate_window_resize(window, size(px(600.), px(700.)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let sheet = window.find("sheet-content").bounds();
        let viewport = window.viewport_size();
        assert_eq!(sheet.size.width, viewport.width - px(24.), "sheet width follows the resized viewport");
    })
    .unwrap();
}

#[test]
fn native_titlebar_is_transparent_and_kit_owns_dragging() {
    let options = super::desktop_window_options();
    assert!(options.app_owns_titlebar_drag, "avoid duplicate AppKit/Kit double-click handling");
    assert_eq!(options.window_background, gpui_kit::WindowBackgroundAppearance::Opaque);
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
        assert_eq!(window.find("toggle-inspector").label(), Some("Hide panel"));
        assert_eq!(window.find("search-commands").role(), Some(gpui_kit::Role::Button));
        assert!(window.find_all("toggle-theme").is_empty());
        focus_element_by_tab(window, "search-commands", cx);
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(window.find("toggle-inspector").focused(), Some(true));
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(window.find("open-settings").focused(), Some(true));
        for (id, label) in [
            ("header-overflow", "Window actions"),
            ("navigation-overflow", "Navigation actions"),
        ] {
            assert_eq!(window.find_all(id).len(), 1, "each chrome area has one overflow trigger");
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
        select_settings_category(window, 3, cx);
        focus_element_by_tab(window, "use-cli-theme", cx);
        window.press("enter", cx);
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
        select_settings_category(window, 2, cx);
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
        assert_eq!(window.find("composer-cli-webmcp").label(), Some("Open VT Code CLI and WebMCP connection settings"));
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
        select_settings_category(window, 1, cx);
        window.click("show-status-bar", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.show_status_bar);
        assert!(!crate::desktop::preferences::Preferences::load().show_status_bar);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep this draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_popups_align_above_the_input_and_preserve_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 900., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(16);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.sidebar_open = false;
            view.inspector_open = false;
            view.composer
                .update(cx, |input, cx| input.set_value("Keep\nthis\ndraft", window, cx));
        });
        window.render_frame(cx);
        for button in ["attach-files", "composer-policy", "composer-mode", "pick-model"] {
            window.click(button, cx);
            window.render_frame(cx);
            let popup = window.find("command-palette").bounds();
            let composer = window.find("composer-context").bounds();
            assert!(popup.bottom() <= composer.top(), "{button} popup must stay above the input");
            if button == "pick-model" {
                assert!((popup.right() - composer.right()).abs() <= px(1.));
            } else {
                assert!((popup.left() - composer.left()).abs() <= px(1.));
            }
            assert!(popup.left() >= px(12.) && popup.right() <= window.viewport_size().width - px(12.));
            window.press("escape", cx);
            window.render_frame(cx);
            assert!(view.read(cx).palette.is_none());
            assert_eq!(view.read(cx).composer.read(cx).value().as_str(), "Keep\nthis\ndraft");
        }
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
            view.configuration = Some(runtime_options_config());
            view.sidebar_open = false;
            view.inspector_open = false;
            view.open_model_palette(window, cx);
        });
        window.render_frame(cx);
        let initial_popup = window.find("command-palette").bounds();
        assert_eq!(initial_popup.size.width, px(392.));
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

        window.press("escape", cx);
        view.update(cx, |view, cx| {
            view.preferences.set_ui_font_size(ThemeMode::Light, 24);
            view.apply_typography(cx);
            view.open_model_palette(window, cx);
        });
        window.render_frame(cx);
        let enlarged_popup = window.find("command-palette").bounds();
        assert_eq!(enlarged_popup.size.width, px(24.5 * 24.));
        assert!(enlarged_popup.left() >= px(12.));
        assert!(enlarged_popup.right() <= window.viewport_size().width - px(12.));

        window.press("escape", cx);
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        let quick_popup = window.find("command-palette").bounds();
        assert_eq!(quick_popup.size.width, px(22. * 24.));
        assert!(quick_popup.left() >= px(12.));
        assert!(quick_popup.right() <= window.viewport_size().width - px(12.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn enlarged_model_popup_stays_inside_a_narrow_window(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 450., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.preferences.set_ui_font_size(ThemeMode::Light, 24);
            view.apply_typography(cx);
            view.configuration = Some(runtime_options_config());
            view.sidebar_open = false;
            view.inspector_open = false;
            view.open_model_palette(window, cx);
        });
        window.render_frame(cx);
        let popup = window.find("command-palette").bounds();
        let viewport = window.viewport_size();
        assert!(popup.left() >= px(12.), "popup left={:?}", popup.left());
        assert!(popup.right() <= viewport.width - px(12.), "popup right={:?}, viewport={viewport:?}", popup.right());
    })
    .unwrap();
}

fn runtime_options_config() -> vtcode_desktop_protocol::ConfigurationSnapshot {
    serde_json::from_value(json!({
        "provider": "openai", "model": "gpt-5.6-sol", "authenticated": true, "credentialSetupCommand": "",
        "sandbox": "Ask before protected actions",
        "sessionOptions": {"permissionMode": "ask", "mode": "build", "reasoningEffort": "medium", "supportedEfforts": ["low", "medium", "high"], "speed": "ultrafast", "fastModeSupported": true, "fastModeEnabled": true, "fastModeLabel": "Ultrafast"}
    })).unwrap()
}

#[gpui_kit::test]
fn reasoning_effort_slider_accessible_label_tracks_the_cli_level(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);

        let slider_id = view.read(cx).effort_slider.as_ref().unwrap().entity_id();
        let slider = window.find(("slider", slider_id));
        assert_eq!(slider.role(), Some(gpui_kit::Role::Slider));
        assert_eq!(slider.label(), Some("Reasoning effort: medium"));
        assert_eq!(window.find("model-effort-value").label(), Some("Current reasoning effort: medium"));

        view.update(cx, |view, cx| {
            let slider = view.effort_slider.clone().unwrap();
            slider.update(cx, |state, cx| state.set_value(2., window, cx));
        });
        window.render_frame(cx);

        let slider_id = view.read(cx).effort_slider.as_ref().unwrap().entity_id();
        assert_eq!(window.find(("slider", slider_id)).label(), Some("Reasoning effort: high"));
        assert_eq!(window.find("model-effort-value").label(), Some("Current reasoning effort: high"));
        assert_eq!(window.find("model-options-summary").label(), Some("gpt-5.6-sol · high"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn compact_model_picker_wraps_long_model_names_and_labels_the_current_effort(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    let model_name = "DeepSeek V4.1 Flash 1017 (Merge Gateway Compatible Route)";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            config.provider = "deepseek".into();
            config.model = "deepseek-v4.1-flash-1017".into();
            view.configuration = Some(config);
            view.models = vec![vtcode_desktop_protocol::ModelInfo {
                provider: "deepseek".into(),
                id: "deepseek-v4.1-flash-1017".into(),
                display_name: model_name.into(),
                context_window: None,
                current: true,
                recommended_rank: Some(0),
                capabilities: None,
            }];
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);

        let model_label = window.find("model-selected-name");
        assert_eq!(model_label.label(), Some(model_name));
        assert!(model_label.bounds().size.height > px(20.), "long names should wrap instead of truncate");
        assert_eq!(window.find("model-effort-value").label(), Some("Current reasoning effort: medium"));
        let expected_summary = format!("{model_name} · medium");
        assert_eq!(window.find("model-options-summary").label(), Some(expected_summary.as_str()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn reasoning_effort_slider_subscriptions_follow_palette_lifecycle(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);

        view.update(cx, |view, cx| {
            assert!(view.effort_slider.is_some());
            assert_eq!(view._effort_slider_subscriptions.len(), 2);
            view.close_palette(window, cx);
            assert!(view.effort_slider.is_none());
            assert!(view._effort_slider_subscriptions.is_empty());
            assert!(view.palette.is_none());
        });
    })
    .unwrap();
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
fn quick_model_picker_omits_recommendations_and_keeps_full_search_visible(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 450., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.models = (0..12)
                .map(|rank| vtcode_desktop_protocol::ModelInfo {
                    provider: "recommended".into(),
                    id: format!("model-{rank}"),
                    display_name: format!("Recommended model {rank}"),
                    context_window: None,
                    current: false,
                    recommended_rank: Some(rank),
                    capabilities: None,
                })
                .collect();
            view.sidebar_open = false;
            view.inspector_open = false;
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);

        let popup = window.find("command-palette").bounds();
        let search_all = window.find("model-search-all");
        let bounds = search_all.bounds();
        assert_eq!(search_all.label(), Some("Search all models…"));
        assert!(bounds.top() >= popup.top(), "search CTA starts above the popup: {bounds:?} {popup:?}");
        assert!(bounds.bottom() <= popup.bottom(), "search CTA is clipped by the popup: {bounds:?} {popup:?}");
        let first_recommendation = palette_action_id(&super::PaletteAction::SelectModel {
            provider: "recommended".into(),
            model: "model-0".into(),
        });
        assert!(window.try_find(first_recommendation).is_none(), "quick selection does not list recommendations");
        assert!(view.read(cx).palette_groups.is_empty(), "model browsing is reserved for full search");

        window.click("model-search-all", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
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
            view.models = vec![vtcode_desktop_protocol::ModelInfo {
                provider: "custom".into(),
                id: "custom-route".into(),
                display_name: "Custom route".into(),
                context_window: None,
                current: false,
                recommended_rank: None,
                capabilities: None,
            }];
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Select model"));
        assert_eq!(window.find("command-palette").bounds().size.width, px(352.));
        assert_eq!(window.find("model-options-summary").label(), Some("gpt-5.6-sol · medium"));
        assert!(
            !view
                .read(cx)
                .palette_groups
                .iter()
                .flat_map(|(_, entries)| entries)
                .any(|entry| entry.label == "Custom route")
        );
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
        view.update(cx, |view, _| view.session_request = None);
        window.click("pick-model", cx);
        window.render_frame(cx);
        let search_all = window.find("model-search-all");
        let search_all_bounds = search_all.bounds();
        let popup_bounds = window.find("command-palette").bounds();
        assert_eq!(search_all.label(), Some("Search all models…"));
        assert!(search_all_bounds.top() >= popup_bounds.top());
        assert!(search_all_bounds.bottom() <= popup_bounds.bottom());
        assert!(
            view.read(cx)
                .palette_groups
                .iter()
                .flat_map(|(_, entries)| entries)
                .all(|entry| !matches!(entry.action, super::PaletteAction::ChooseModel))
        );
        window.click("model-search-all", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
        assert!(
            view.read(cx)
                .palette_groups
                .iter()
                .flat_map(|(_, entries)| entries)
                .any(|entry| entry.label == "Custom route")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn compact_model_picker_shows_current_options_and_full_search_selects_the_real_route(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.models = vec![
                vtcode_desktop_protocol::ModelInfo {
                    provider: "anthropic".into(),
                    id: "claude-sonnet-5".into(),
                    display_name: "Claude Sonnet 5".into(),
                    context_window: Some(200_000),
                    current: false,
                    recommended_rank: Some(0),
                    capabilities: None,
                },
                vtcode_desktop_protocol::ModelInfo {
                    provider: "google".into(),
                    id: "gemini-3.8-flash".into(),
                    display_name: "Gemini 3.8 Flash".into(),
                    context_window: Some(1_000_000),
                    current: false,
                    recommended_rank: Some(3),
                    capabilities: Some(vtcode_desktop_protocol::ModelCapabilities {
                        reasoning: true,
                        vision: true,
                        tool_call: true,
                        structured_output: true,
                        is_pro: true,
                        input_modalities: vec![
                            "text".into(),
                            "image".into(),
                            "audio".into(),
                            "video".into(),
                            "pdf".into(),
                        ],
                    }),
                },
                vtcode_desktop_protocol::ModelInfo {
                    provider: "custom".into(),
                    id: "not-recommended".into(),
                    display_name: "Not recommended".into(),
                    context_window: None,
                    current: false,
                    recommended_rank: None,
                    capabilities: None,
                },
            ];
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        let claude_id = palette_action_id(&super::PaletteAction::SelectModel {
            provider: "anthropic".into(),
            model: "claude-sonnet-5".into(),
        });
        assert!(window.try_find(claude_id.clone()).is_none(), "model results stay out of the compact panel");
        let gemini_id = palette_action_id(&super::PaletteAction::SelectModel {
            provider: "google".into(),
            model: "gemini-3.8-flash".into(),
        });
        assert!(window.try_find(gemini_id.clone()).is_none(), "catalog choices appear in full search only");
        assert_eq!(window.find("model-selected-name").label(), Some("gpt-5.6-sol"));
        assert_eq!(window.find("model-effort-value").label(), Some("Current reasoning effort: medium"));
        assert!(view.read(cx).palette_groups.is_empty());
        let search_all = window.find("model-search-all");
        assert_eq!(search_all.label(), Some("Search all models…"));
        let search_all_bounds = search_all.bounds();
        let popup_bounds = window.find("command-palette").bounds();
        assert!(search_all_bounds.top() >= popup_bounds.top());
        assert!(search_all_bounds.bottom() <= popup_bounds.bottom());
        let uncatalogued_id = palette_action_id(&super::PaletteAction::SelectModel {
            provider: "custom".into(),
            model: "not-recommended".into(),
        });
        assert!(window.try_find(uncatalogued_id).is_none(), "uncurated catalog entries stay in full search");
        assert_eq!(window.find("fast-mode-toggle").label(), Some("Disable Ultrafast processing"));
        window.click("model-search-all", cx);
        window.render_frame(cx);
        assert!(
            window
                .find(claude_id)
                .label()
                .unwrap()
                .starts_with("Claude Sonnet 5 · anthropic · 200K context")
        );
        let gemini_item = window.find(gemini_id.clone());
        let gemini_row = gemini_item.label().unwrap().to_owned();
        assert!(gemini_row.starts_with("Gemini 3.8 Flash · google · Reasoning · Vision · Tools"));
        assert!(gemini_row.contains("Structured output · Pro · Audio · Video · PDF · 1M context"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("gemini", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).query(cx), "gemini");
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).matched_count(), 1);
        assert_eq!(window.find(gemini_id).label(), Some(gemini_row.as_str()));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected quick model selection request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SELECT);
    assert_eq!(params, json!({ "provider": "google", "model": "gemini-3.8-flash" }));
}

#[gpui_kit::test]
fn fast_mode_toggle_sends_cli_option_and_is_exposed_as_a_toggle(cx: &mut TestAppContext) {
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
        let toggle = window.find("fast-mode-toggle");
        assert_eq!(toggle.label(), Some("Disable Ultrafast processing"));
        assert_eq!(toggle.checked(), Some(true));
        assert_eq!(toggle.role(), Some(gpui_kit::Role::Button));
        window.click("fast-mode-toggle", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("fast-mode request") else {
        panic!("expected runtime option request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["fastMode"], false);
}

#[test]
fn model_picker_includes_catalog_capabilities_and_search_keywords() {
    let model = vtcode_desktop_protocol::ModelInfo {
        provider: "google".into(),
        id: "gemini-special".into(),
        display_name: "Gemini Special".into(),
        context_window: Some(1_000_000),
        current: false,
        recommended_rank: None,
        capabilities: Some(vtcode_desktop_protocol::ModelCapabilities {
            reasoning: true,
            vision: true,
            tool_call: true,
            structured_output: true,
            is_pro: true,
            input_modalities: vec!["text".into(), "audio".into(), "video".into(), "pdf".into()],
        }),
    };
    let command = super::model_palette_command(&model, false);
    let description = command.description.as_deref().unwrap();
    assert!(
        description.contains("Reasoning · Vision · Tools · Structured output · Pro · Audio · Video · PDF · 1M context")
    );
    assert!(command.keywords.contains(&"gemini-special".into()));
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
        assert!(
            window
                .try_find(palette_action_id(&super::PaletteAction::InsertDraft("$rust-skills".into())))
                .is_some(),
            "skill row should be rendered"
        );
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
        assert_eq!(
            window.find(palette_action_id(&super::PaletteAction::ToggleInspector)).label(),
            Some("Toggle inspector panel")
        );
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
        assert_eq!(
            window
                .find(palette_action_id(&super::PaletteAction::SelectSessionOptions(
                    vtcode_desktop_protocol::SessionOptionsSelectParams {
                        reasoning_effort: Some("low".into()),
                        ..Default::default()
                    }
                )))
                .label(),
            Some("Use low effort")
        );
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
fn composer_tracks_the_active_agent_without_changing_next_session_mode(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.configuration = Some(runtime_options_config());
            view.selected_session_id = Some("session-a".into());
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Build"));

        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::PrimaryAgentChanged(vtcode_desktop_protocol::PrimaryAgentChanged {
                    session_id: "session-a".into(),
                    display_name: Some("Plan".into()),
                }),
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Plan"));
        assert_eq!(
            view.read(cx)
                .configuration
                .as_ref()
                .unwrap()
                .session_options
                .as_ref()
                .unwrap()
                .mode,
            "build"
        );

        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::PrimaryAgentChanged(vtcode_desktop_protocol::PrimaryAgentChanged {
                    session_id: "session-b".into(),
                    display_name: Some("Stale agent".into()),
                }),
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Plan"));

        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::PrimaryAgentChanged(vtcode_desktop_protocol::PrimaryAgentChanged {
                    session_id: "session-a".into(),
                    display_name: None,
                }),
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Build"));
    })
    .unwrap();
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
        select_settings_category(window, 3, cx);
        window.click("reload-cli-settings", cx);
        assert!(matches!(requests.try_recv(), Ok(BackendCommand::Stop)));
        assert!(view.read(cx).pending_workspace.is_some());
        window.dispatch_action(Box::new(super::ReloadCliSettings), cx);
        assert!(requests.try_recv().is_err(), "the keyboard action cannot bypass the pending restart");
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_workspace_is_inset_below_the_toolbar_and_after_navigation(cx: &mut TestAppContext) {
    let (window, _) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let navigation = window.find("left-navigation").bounds();
        let toolbar = window.find("title-bar").bounds();
        let workspace = window.find("workspace-surface");
        let bounds = workspace.bounds();

        assert_eq!(workspace.role(), Some(gpui_kit::Role::Group));
        assert_eq!(workspace.label(), Some("Conversation workspace"));
        assert!(bounds.left() > navigation.right(), "workspace must have a visible left inset: {bounds:?}");
        assert!(bounds.top() > toolbar.bottom(), "workspace must have a visible top inset: {bounds:?}");
        assert!(bounds.size.width > px(500.));
        assert!(bounds.size.height > px(500.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_shortcut_opens_webmcp_sections_with_unique_accessibility_ids(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.webmcp_pairing = Some(vtcode_desktop_protocol::WebmcpPairResult {
                endpoint: "ws://127.0.0.1:43719/mcp".into(),
                origin: "https://example.test".into(),
                pairing_code: "0123456789abcdef".into(),
                expires_in_secs: 300,
            });
        });
        window.press("cmd-,", cx);
        window.render_frame(cx);
        select_settings_category(window, 4, cx);

        assert!(view.read(cx).settings_open, "the Settings shortcut opens the dialog");
        assert_eq!(window.find_all("webmcp-status").len(), 1);
        assert_eq!(window.find_all("webmcp-pairing").len(), 1);
        assert_eq!(window.find_all("refresh-webmcp-status").len(), 1);
        assert_eq!(window.find_all("webmcp-pairing-details").len(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_utilities_can_return_to_changes_without_creating_a_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.workspace = Some(std::env::temp_dir()));
        window.render_frame(cx);
        window.click("nav-utilities", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        window.press("home", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Changes);
        assert!(view.read(cx).selected_session_id.is_none());
        assert_eq!(window.find("send-prompt").label(), Some("Send message"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn failed_working_tree_refresh_keeps_the_last_successful_snapshot(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    let patch = "diff --git a/src/app.rs b/src/app.rs\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+new\n";
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
        });
        let BackendCommand::Request { params, .. } = requests.try_recv().unwrap() else {
            panic!("initial review request")
        };
        view.update(cx, |view, cx| {
            view.accept_workspace_review(
                vtcode_desktop_protocol::WorkspaceReviewResult {
                    include_untracked: false,
                    scope: String::new(),
                    request_id: params["requestId"].as_str().unwrap().to_owned(),
                    workspace: "/workspace".into(),
                    captured_at: "first capture".into(),
                    patch: Some(patch.into()),
                    truncated: false,
                    note: "Tracked changes against HEAD".into(),
                },
                cx,
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("workspace-review-diff").is_some());

        window.click("refresh-workspace-review", cx);
        let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
            panic!("refresh request")
        };
        assert_eq!(method, vtcode_desktop_protocol::method::WORKSPACE_REVIEW);
        window.render_frame(cx);
        assert!(window.try_find("workspace-review-refreshing").is_some());
        assert!(window.try_find("workspace-review-diff").is_some());

        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::WORKSPACE_REVIEW.into(),
                    result: None,
                    error: Some("Git capture timed out".into()),
                },
                cx,
            );
            let review = view.workspace_review.as_ref().unwrap();
            assert!(!review.pending);
            assert_eq!(review.snapshot.as_ref().unwrap().patch.as_deref(), Some(patch));
            assert!(review.viewer.is_some());
        });
        window.render_frame(cx);
        assert!(window.try_find("workspace-review-refreshing").is_none());
        assert!(window.try_find("workspace-review-refresh-warning").is_some());
        assert!(window.try_find("workspace-review-diff").is_some());
        window.click("copy-workspace-review", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(patch));
    })
    .unwrap();
}

#[gpui_kit::test]
fn effort_knob_track_click_commits_the_supported_level(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        window.render_frame(cx);
        window.click(("effort-stop", 2usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("effort request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "high");
    assert!(requests.try_recv().is_err());
}
