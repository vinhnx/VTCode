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
    ActivityState, DesktopButtonExt, DesktopView, ExternalLaunchCall, FixedRowSizeCache, MAX_UI_EVENTS_PER_TICK, Route,
    TestLaunchIntercept, ThemeMode, WelcomePeriod, append_restored_output_page, content_page_title, eyebrow_label,
    group_box_title, opaque_path_element_id, restored_output_for_copy, row_title, scale_row_height_for_ui_font,
    section_label, workspace_picker_options,
};
pub(super) use crate::desktop::files::{ExternalEditor, FileEntry};
pub(super) use crate::desktop::preferences::FileOpenDestination;
pub(super) use crate::desktop::transcript::MAX_TRANSCRIPT_ITEMS;
pub(super) use crate::desktop::transcript::TranscriptItem;
pub(super) use crate::desktop::transport::{BackendCommand, BackendEvent, BackendHandle};

/// Redirect preference writes away from the real user home for GUI tests.
fn isolate_preferences() -> PathBuf {
    crate::desktop::preferences::install_test_preferences_path()
}

/// A real 1x1 PNG so the renderer can decode image fixtures and thumbnails.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xFC, 0xFF, 0xFF, 0x3F, 0x03, 0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x2D, 0x1E,
    0x48, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// A decoded-ready thumbnail for attachment-map fixtures that seed the cache
/// directly instead of driving the workspace drop flow.
fn test_thumbnail_image() -> std::sync::Arc<gpui_kit::Image> {
    std::sync::Arc::new(gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, TINY_PNG.to_vec()))
}

/// Assert one chord resolves to the expected action in the given context.
/// Keymaps append, so a reinstalled table leaves identical duplicates behind;
/// dispatch tries the highest-precedence hit first, which is what this pins.
pub(super) fn assert_chord_resolves_to(
    cx: &gpui_kit::App,
    chord: &str,
    context: &str,
    expected: &dyn gpui_kit::Action,
) {
    let resolved = live_chord_bindings(cx, chord, context);
    assert!(!resolved.is_empty(), "chord {chord} must stay live in {context}");
    assert!(
        gpui_kit::Action::partial_eq(resolved[0].action(), expected),
        "chord {chord} resolved to the wrong action in {context}"
    );
}

/// Assert one chord resolves exactly once to the expected action: the fresh
/// binding with no lingering stale copy beneath it.
pub(super) fn assert_chord_resolves_once_to(
    cx: &gpui_kit::App,
    chord: &str,
    context: &str,
    expected: &dyn gpui_kit::Action,
) {
    let resolved = live_chord_bindings(cx, chord, context);
    assert_eq!(resolved.len(), 1, "chord {chord} must resolve exactly once in {context}");
    assert!(
        gpui_kit::Action::partial_eq(resolved[0].action(), expected),
        "chord {chord} resolved to the wrong action in {context}"
    );
}

/// Assert one chord resolves to nothing in the given context: unbound, or
/// retired by a `NoAction` shadow after a rebind.
pub(super) fn assert_chord_dead(cx: &gpui_kit::App, chord: &str, context: &str) {
    let resolved = live_chord_bindings(cx, chord, context);
    assert!(resolved.is_empty(), "chord {chord} must stay dead in {context}");
}

fn live_chord_bindings(cx: &gpui_kit::App, chord: &str, context: &str) -> Vec<gpui_kit::KeyBinding> {
    // `parse` needs a `'static` source; none of these chords use `secondary-`,
    // so the host-OS parse resolves them identically.
    let keystroke = gpui_kit::Keystroke::parse_with_os(chord, super::shortcuts::host_operating_system()).unwrap();
    let context = gpui_kit::KeyContext::parse(context).unwrap();
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let (resolved, _) = keymap.bindings_for_input(&[keystroke], &[context]);
    resolved.into_iter().collect()
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

/// Tab until focus leaves `scope`, then keep tabbing until it cycles back in.
/// Route panels do not trap focus, so a fixed tab count breaks whenever any
/// window control is added, removed, or reordered. Bounded; panics naming the
/// scope when the cycle never leaves or never returns.
fn tab_out_and_back_into_scope(
    window: &mut gpui_kit::Window,
    cx: &mut gpui_kit::App,
    scope: &'static str,
    mut in_scope: impl FnMut(&mut gpui_kit::Window, &mut gpui_kit::App) -> bool,
) {
    for _ in 0..128 {
        if !in_scope(window, cx) {
            break;
        }
        window.press("tab", cx);
        window.render_frame(cx);
    }
    assert!(!in_scope(window, cx), "tab never left {scope}");
    for _ in 0..128 {
        if in_scope(window, cx) {
            return;
        }
        window.press("tab", cx);
        window.render_frame(cx);
    }
    panic!("tab never cycled back into {scope}");
}

/// Embedded-panel contract: the toolbar persists above every panel, visible
/// navigation sits beside it, and the panel stays inside the viewport.
fn assert_panel_embeds(window: &mut gpui_kit::Window, panel_id: &'static str) {
    let panel = window.find(panel_id).bounds();
    let viewport = window.viewport_size();
    let toolbar = window.find("title-bar").bounds();
    assert!(
        panel.origin.y >= toolbar.bottom(),
        "{panel_id} sits below the persistent toolbar: panel={panel:?}, toolbar={toolbar:?}"
    );
    if let Some(navigation) = window.try_find("primary-navigation") {
        let rail = navigation.bounds();
        assert!(
            panel.origin.x >= rail.right() || panel.right() <= rail.left(),
            "{panel_id} sits beside the persistent navigation: panel={panel:?}, rail={rail:?}"
        );
    }
    assert!(
        panel.origin.x + panel.size.width <= viewport.width && panel.origin.y + panel.size.height <= viewport.height,
        "{panel_id} stays inside the viewport: panel={panel:?}, viewport={viewport:?}"
    );
    let chrome = window
        .try_find("primary-navigation")
        .map(|navigation| navigation.bounds().size.width)
        .unwrap_or(px(0.));
    assert!(
        panel.size.width >= viewport.width - chrome - px(100.),
        "{panel_id} fills the content slot: panel={panel:?}, viewport={viewport:?}"
    );
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
            size_bytes: 0,
            updated_at: None,
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

fn scroll_swipe(window: AnyWindowHandle, dx_px: f32, dy_px: f32, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.scroll("pane-root", gpui_kit::ScrollDelta::Pixels(point(px(dx_px), px(dy_px))), cx);
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

/// Mirror of `submit()`: user echo present, turn running, no content yet.
fn running_turn_without_content(cx: &mut TestAppContext, view: &Entity<DesktopView>) {
    use crate::desktop::transcript::TranscriptRole;

    view.update(cx, |view, _| {
        view.selected_session_id = Some("session".into());
        view.transcript.push(TranscriptItem::new(
            "echo-1".into(),
            TranscriptRole::User.as_str().into(),
            "You".into(),
            "hello".into(),
        ));
        view.running = true;
        view.turn_content_seen = false;
        view.active_turn_id = None;
        view.status = rust_i18n::t!("app.composer.submitting_turn").to_string();
    });
}

fn turn_started_event(session_id: &str, turn_id: &str) -> BackendEvent {
    use vtcode_exec_events::{ThreadEvent, TurnStartedEvent, VersionedThreadEvent};

    BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
        session_id: session_id.into(),
        turn_id: Some(turn_id.into()),
        event: VersionedThreadEvent::new(ThreadEvent::TurnStarted(TurnStartedEvent::default())),
    })
}

fn turn_completed_event(session_id: &str, turn_id: &str) -> BackendEvent {
    use vtcode_exec_events::{ThreadEvent, TurnCompletedEvent, Usage, VersionedThreadEvent};

    BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
        session_id: session_id.into(),
        turn_id: Some(turn_id.into()),
        event: VersionedThreadEvent::new(ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
                cached_input_tokens: 0,
                cache_creation_tokens: 0,
            },
            in_progress_exec_sessions: Vec::new(),
        })),
    })
}

mod about_dialog;
mod activity_settings;
mod activity_summary;
mod attachment_picker;
mod cli_handoff;
mod composer_alignment;
mod composer_font_scale;
mod composer_keys;
mod config_catalog;
mod config_deep_links;
mod config_sessions;
mod curated_config;
mod deeplinks;
mod exec_controls;
mod file_drop;
mod files_destination;
mod files_webmcp;
mod full_access_confirm;
mod heading_tiers;
mod inspector_appearance;
mod inspector_files;
mod inspector_layout;
mod interaction_choices;
mod licenses_viewer;
mod loading_lifecycle;
mod mcp_app;
mod menu_bar_actions;
mod model_palette;
mod notifications;
mod option_popups;
mod options_misc;
mod output_surfaces;
mod overlay_toggles;
mod review_changes;
mod review_diff_mode;
mod review_output;
mod running_confirm;
mod search_config;
mod secret_store;
mod service_navigation;
mod session_link;
mod session_navigation;
mod sessions_startup;
mod settings_panes;
mod settings_spacing;
mod shortcuts_editor;
mod slash_options;
mod startup_interactions;
mod storage_settings;
mod themes_transcript;
mod thread_tabs;
mod transcript_inspector_ctas;
mod transcript_navigation;
mod turn_queue;
mod turns_transcript;
mod worktree_a11y;
mod zen_options;
mod zen_workspace;
