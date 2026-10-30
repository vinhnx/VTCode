pub(super) use gpui_kit::Focusable;
pub(super) use gpui_kit::Styled;
pub(super) use gpui_kit::component::theme::ActiveTheme;
pub(super) use std::path::{Path, PathBuf};
pub(super) use std::time::Duration;

pub(super) use gpui_kit::component::WindowExt as _;
pub(super) use gpui_kit::component::select::SelectEvent;
pub(super) use gpui_kit::test::{ClickOptions, TestWindowExt};
pub(super) use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, CursorStyle, Entity, MouseButton, NavigationDirection, TestAppContext,
    WindowBounds, WindowOptions, point, px, size,
};
pub(super) use serde_json::json;

pub(super) use super::navigation_history::{NavigationLocation, PendingNavigation};
pub(super) use super::{
    ActivityState, DesktopButtonExt, DesktopView, ExternalLaunchCall, FixedRowSizeCache, MAX_UI_EVENTS_PER_TICK,
    TestLaunchIntercept, ThemeMode, WelcomePeriod, append_restored_output_page, restored_output_for_copy,
    scale_row_height_for_ui_font, workspace_picker_options,
};
pub(super) use crate::desktop::files::ExternalEditor;
pub(super) use crate::desktop::preferences::FileOpenDestination;
pub(super) use crate::desktop::transcript::MAX_TRANSCRIPT_ITEMS;
pub(super) use crate::desktop::transcript::TranscriptItem;
pub(super) use crate::desktop::transport::{BackendCommand, BackendEvent, BackendHandle};

/// Redirect preference writes away from the real user home for GUI tests.
fn isolate_preferences() -> PathBuf {
    crate::desktop::preferences::install_test_preferences_path()
}

fn open_desktop(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<DesktopView>) {
    open_desktop_sized(cx, 1280., 900.)
}

fn open_desktop_sized(cx: &mut TestAppContext, width: f32, height: f32) -> (AnyWindowHandle, Entity<DesktopView>) {
    open_desktop_sized_with_preferences_path(cx, width, height, isolate_preferences(), true)
}

fn open_desktop_with_preferences_path(
    cx: &mut TestAppContext,
    preferences_path: PathBuf,
) -> (AnyWindowHandle, Entity<DesktopView>) {
    open_desktop_sized_with_preferences_path(cx, 1280., 900., preferences_path, true)
}

fn open_desktop_sized_with_preferences_path(
    cx: &mut TestAppContext,
    width: f32,
    height: f32,
    preferences_path: PathBuf,
    normal_baseline: bool,
) -> (AnyWindowHandle, Entity<DesktopView>) {
    crate::desktop::preferences::set_test_preferences_path(preferences_path);
    crate::desktop::localization::initialize(crate::desktop::localization::UiLanguage::English);
    cx.update(gpui_kit::init);
    cx.update(super::super::notifications::initialize);
    let preferences = crate::desktop::preferences::Preferences::load();
    cx.update(|cx| super::init_keybindings(cx, &preferences));
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
                    if normal_baseline {
                        view.workspace = Some(std::env::temp_dir());
                        view.sidebar_open = true;
                        view.inspector_open = true;
                        // Suite baseline is normal mode; zen-by-default is pinned
                        // by zen_mode_is_active_on_fresh_launch below.
                        view.zen_mode = false;
                    }
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

fn agent_message_update_event(session_id: &str, turn_id: &str, item_id: &str, text: &str) -> BackendEvent {
    use vtcode_exec_events::{AgentMessageItem, ItemUpdatedEvent, ThreadEvent, ThreadItem, ThreadItemDetails};

    BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
        session_id: session_id.into(),
        turn_id: Some(turn_id.into()),
        event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemUpdated(ItemUpdatedEvent {
            item: ThreadItem {
                context: None,
                id: item_id.into(),
                details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: text.into() }),
            },
        })),
    })
}

fn plan_delta_notification(
    session_id: &str,
    turn_id: &str,
    item_id: &str,
    delta: &str,
    include_checkpoint: bool,
) -> vtcode_desktop_protocol::ThreadEventNotification {
    use vtcode_exec_events::{PlanDeltaEvent, ThreadEvent};

    vtcode_desktop_protocol::ThreadEventNotification {
        session_id: session_id.into(),
        turn_id: Some(turn_id.into()),
        event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::PlanDelta(Box::new(PlanDeltaEvent {
            thread_id: session_id.into(),
            turn_id: turn_id.into(),
            item_id: item_id.into(),
            delta: delta.into(),
            recovery: include_checkpoint.then(Box::default),
        }))),
    }
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
    for _ in 0..120 {
        let viewport = window.find("theme-list").bounds();
        let item = window.find(id).bounds();
        if item.left() >= viewport.left() - px(1.) && item.right() <= viewport.right() + px(1.) {
            return;
        }
        let delta = if item.right() > viewport.right() {
            -px(112.)
        } else {
            px(112.)
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

fn config_page_setting(path: &str, value: serde_json::Value, group: &str) -> vtcode_desktop_protocol::ConfigSetting {
    vtcode_desktop_protocol::ConfigSetting {
        path: path.into(),
        value,
        origin: Some("workspace".into()),
        label: None,
        type_name: None,
        default_value: None,
        description: None,
        options: Vec::new(),
        group: Some(group.into()),
        group_description: None,
        editable: true,
    }
}

fn swipe_history_fixture(
    cx: &mut TestAppContext,
    view: &Entity<DesktopView>,
    commands: tokio::sync::mpsc::Sender<BackendCommand>,
) -> Vec<vtcode_desktop_protocol::SessionSummary> {
    let workspace = std::env::temp_dir();
    let summaries = ["swipe-first", "swipe-second", "swipe-third"]
        .into_iter()
        .map(|id| vtcode_desktop_protocol::SessionSummary {
            session_id: id.into(),
            title: Some(format!("Thread {id}")),
            workspace: workspace.display().to_string(),
            completed: true,
            resumable: true,
        })
        .collect::<Vec<_>>();
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.workspace = Some(workspace.clone());
        view.sessions.clone_from(&summaries);
        view.selected_session_id = Some(summaries[1].session_id.clone());
        for summary in &summaries {
            view.navigation_history.record(NavigationLocation {
                workspace: workspace.clone(),
                session_id: Some(summary.session_id.clone()),
            });
        }
        assert!(view.navigation_history.navigate_to(1));
    });
    summaries
}

fn swipe_navigate(window: AnyWindowHandle, direction: NavigationDirection, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click_with_options("pane-root", ClickOptions::new().with_button(MouseButton::Navigate(direction)), cx);
    })
    .unwrap();
}

fn runtime_options_config() -> vtcode_desktop_protocol::ConfigurationSnapshot {
    serde_json::from_value(json!({
        "provider": "openai", "model": "gpt-5.6-sol", "authenticated": true, "credentialSetupCommand": "",
        "sandbox": "Ask before protected actions",
        "sessionOptions": {"permissionMode": "ask", "mode": "build", "availableModes": [
            {"name":"build", "displayName":"build", "description":"Implementation"},
            {"name":"plan", "displayName":"plan", "description":"Planning"},
            {"name":"auto", "displayName":"auto", "description":"Autonomous implementation"},
            {"name":"duck", "displayName":"duck", "description":"Read-only discussion"},
            {"name":"coordinator", "displayName":"coordinator", "description":"Orchestration"},
            {"name":"custom-reviewer", "displayName":"Custom reviewer", "description":"Workspace primary agent"}
        ], "reasoningEffort": "medium", "supportedEfforts": ["low", "medium", "high"], "speed": "ultrafast", "fastModeSupported": true, "fastModeEnabled": true, "fastModeLabel": "Ultrafast"}
    })).unwrap()
}

enum MousePhase {
    Down,
    Move,
    Up,
}

mod activity_settings;
mod config_sessions;
mod curated_config;
mod files_webmcp;
mod inspector_appearance;
mod mcp_app;
mod notifications;
mod option_popups;
mod options_misc;
mod review_output;
mod search_config;
mod secret_store;
mod service_navigation;
mod sessions_startup;
mod shortcuts_editor;
mod slash_options;
mod startup_interactions;
mod themes_transcript;
mod turns_transcript;
mod worktree_a11y;
mod zen_workspace;
