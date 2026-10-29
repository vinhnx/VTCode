//! Static MCP Apps preview cards: tier gating, fetch-on-expand, and body states.

use super::super::mcp_app::{McpAppBodyKey, PendingMcpAppRead, head_with_truncation, valid_mcp_app_read};
use crate::desktop::transcript::TranscriptItem;
use vtcode_desktop_protocol::{ConfigSetting, ConfigSettingsSnapshot, McpAppReadResult};
use vtcode_exec_events::AppPreviewItem;

use super::*;

fn tier_snapshot(tier: &str) -> ConfigSettingsSnapshot {
    ConfigSettingsSnapshot {
        settings: vec![ConfigSetting {
            path: "mcp.ui.apps".into(),
            value: json!(tier),
            origin: Some("workspace".into()),
            label: Some("MCP Apps".into()),
            type_name: Some("string".into()),
            default_value: None,
            description: None,
            options: Vec::new(),
            group: Some("Connections".into()),
            group_description: None,
            editable: true,
        }],
        target_versions: Vec::new(),
        omitted_sensitive: 0,
        omitted_large: 0,
        omitted_limit: 0,
    }
}

fn preview_item() -> TranscriptItem {
    let mut item = TranscriptItem::new(
        "app-1".into(),
        "tool".into(),
        "mcp_example_lookup".into(),
        "completed\nlookup finished".into(),
    );
    item.output = Some("lookup finished".into());
    item.expanded = true;
    item.app_previews = vec![AppPreviewItem {
        uri: "ui://demo/app.html".into(),
        mime_type: Some("text/html;profile=mcp-app".into()),
        bytes: 4312,
        blocks: 1,
        ui: None,
    }];
    item
}

fn app_body() -> McpAppReadResult {
    McpAppReadResult {
        uri: "ui://demo/app.html".into(),
        request_id: "app-read-1".into(),
        mime_type: Some("text/html;profile=mcp-app".into()),
        body: "<h1>Hi</h1>".into(),
        total_bytes: 12,
        truncated: false,
    }
}

#[test]
fn mcp_app_head_truncation_cuts_on_char_boundaries() {
    assert_eq!(head_with_truncation("abc", 8), ("abc", false));
    assert_eq!(head_with_truncation("abcdefgh", 8), ("abcdefgh", false));
    assert_eq!(head_with_truncation("abcdefghi", 8), ("abcdefgh", true));
    // Multibyte tail: the cut keeps the ellipsis out rather than splitting it.
    assert_eq!(head_with_truncation("ab…cd", 3), ("ab…", true));
    assert_eq!(head_with_truncation("", 8), ("", false));
}

#[test]
fn mcp_app_read_validation_matches_pending_identity() {
    let pending = PendingMcpAppRead {
        request_id: "app-read-1".into(),
        item_id: "app-1".into(),
        uri: "ui://demo/app.html".into(),
    };
    assert!(valid_mcp_app_read(&app_body(), &pending));
    let mut stale = app_body();
    stale.request_id = "app-read-0".into();
    assert!(!valid_mcp_app_read(&stale, &pending));
    let mut swapped = app_body();
    swapped.uri = "ui://demo/other.html".into();
    assert!(!valid_mcp_app_read(&swapped, &pending));
}

#[gpui_kit::test]
fn mcp_app_cards_render_only_under_the_static_tier(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(preview_item());
            assert!(!view.mcp_apps_preview_enabled(), "missing settings default to off");
        });
        window.render_frame(cx);
        assert!(window.try_find("mcp-app-preview-app-1-0").is_none());

        view.update(cx, |view, _| {
            view.cli_config_settings = Some(tier_snapshot("off"));
            assert!(!view.mcp_apps_preview_enabled());
        });
        window.render_frame(cx);
        assert!(window.try_find("mcp-app-preview-app-1-0").is_none());

        view.update(cx, |view, _| {
            view.cli_config_settings = Some(tier_snapshot("interactive"));
            assert!(!view.mcp_apps_preview_enabled(), "no engine renders interactive yet");
            view.cli_config_settings = Some(tier_snapshot("static"));
            assert!(view.mcp_apps_preview_enabled());
            assert_eq!(
                view.mcp_app_bodies.insert(
                    McpAppBodyKey {
                        item_id: "app-1".into(),
                        uri: "ui://demo/app.html".into()
                    },
                    app_body(),
                ),
                None
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("mcp-app-preview-app-1-0").is_some());
        assert!(window.try_find("mcp-app-body-app-1-0").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn mcp_app_expand_fetches_once_and_skips_settled_uris(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(tier_snapshot("static"));
            view.transcript.push(preview_item());
            view.request_mcp_app_previews("app-1", cx);
            assert_eq!(view.mcp_app_pending.len(), 1);
            // A second expand while the read is in flight queues nothing new.
            view.request_mcp_app_previews("app-1", cx);
            assert_eq!(view.mcp_app_pending.len(), 1);
        });
        window.render_frame(cx);
        let BackendCommand::Request { method, params } = requests.try_recv().expect("one app read") else {
            panic!("expected a request command");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::MCP_APP_READ);
        assert_eq!(params["uri"], json!("ui://demo/app.html"));
        assert!(requests.try_recv().is_err(), "no duplicate read queued");

        view.update(cx, |view, cx| {
            let key = McpAppBodyKey {
                item_id: "app-1".into(),
                uri: "ui://demo/app.html".into(),
            };
            assert_eq!(view.mcp_app_errors.insert(key.clone(), "boom".into()), None);
            view.mcp_app_pending.clear();
            // Errors wait for an explicit retry; bodies never refetch.
            view.request_mcp_app_previews("app-1", cx);
            assert!(view.mcp_app_pending.is_empty());
            assert!(view.mcp_app_errors.remove(&key).is_some());
            assert_eq!(view.mcp_app_bodies.insert(key, app_body()), None);
            view.request_mcp_app_previews("app-1", cx);
            assert!(view.mcp_app_pending.is_empty());
        });
        assert!(requests.try_recv().is_err(), "settled URIs stay quiet");
    })
    .unwrap();
}

#[gpui_kit::test]
fn mcp_app_error_row_retries_through_the_button(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(tier_snapshot("static"));
            view.transcript.push(preview_item());
            assert_eq!(
                view.mcp_app_errors.insert(
                    McpAppBodyKey {
                        item_id: "app-1".into(),
                        uri: "ui://demo/app.html".into()
                    },
                    "Could not load the app preview. Try again.".into(),
                ),
                None
            );
        });
        window.render_frame(cx);
        assert!(window.try_find("retry-mcp-app-app-1-0").is_some());
        window.click("retry-mcp-app-app-1-0", cx);
        window.render_frame(cx);

        view.update(cx, |view, _| {
            let key = McpAppBodyKey {
                item_id: "app-1".into(),
                uri: "ui://demo/app.html".into(),
            };
            assert!(!view.mcp_app_errors.contains_key(&key), "retry clears the error first");
            assert_eq!(view.mcp_app_pending.len(), 1);
        });
        let BackendCommand::Request { method, .. } = requests.try_recv().expect("retry read") else {
            panic!("expected a request command");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::MCP_APP_READ);
    })
    .unwrap();
}
