//! Static MCP Apps preview cards: tier gating, fetch-on-expand, and body states.

use super::super::mcp_app::{
    McpAppBodyKey, PendingMcpAppRead, head_with_truncation, preview_element_id, valid_mcp_app_read,
};
use crate::desktop::transcript::TranscriptItem;
use vtcode_desktop_protocol::{ConfigSetting, ConfigSettingsSnapshot, McpAppReadResult};
use vtcode_exec_events::AppPreviewItem;

use super::*;

fn preview_id(prefix: &str, uri: &str) -> String {
    preview_element_id(prefix, "app-1", uri)
}

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

#[test]
fn preview_element_id_keys_the_domain_and_hides_the_uri() {
    let id = preview_id("mcp-app-preview", "ui://demo/app.html");
    assert!(id.starts_with("mcp-app-preview-"));
    assert!(!id.contains("app-1"));
    assert!(!id.contains("ui://demo/app.html"));
    assert_ne!(id, preview_element_id("mcp-app-preview", "app-2", "ui://demo/app.html"));
    assert_ne!(id, preview_id("mcp-app-preview", "ui://demo/second.html"));
    assert_ne!(id, preview_id("mcp-app-body", "ui://demo/app.html"));
}

#[gpui_kit::test]
fn preview_ids_survive_a_slot_change(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let app_uri = "ui://demo/app.html";
    let second_uri = "ui://demo/second.html";
    let app_id = preview_id("mcp-app-preview", app_uri);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.cli_config_settings = Some(std::rc::Rc::new(tier_snapshot("static")));
            view.transcript.push(preview_item());
        });
        window.render_frame(cx);
        // The app preview starts in slot 0; the sibling is not present yet.
        assert!(window.try_find(app_id.clone()).is_some());
        assert!(window.try_find(preview_id("mcp-app-preview", second_uri)).is_none());

        view.update(cx, |view, _| {
            let original = preview_item().app_previews[0].clone();
            let mut sibling = original.clone();
            sibling.uri = second_uri.into();
            view.transcript.set_app_previews("app-1", vec![sibling, original]);
        });
        window.render_frame(cx);
        // The same preview now sits in slot 1 but keeps its domain-derived id.
        // A positional id would have moved it to `-1`, losing this lookup.
        assert!(window.try_find(app_id).is_some());
        assert!(window.try_find(preview_id("mcp-app-preview", second_uri)).is_some());
        assert!(window.try_find("mcp-app-preview-app-1-0").is_none());
        assert!(window.try_find("mcp-app-preview-app-1-1").is_none());
    })
    .unwrap();
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
        assert!(window.try_find(preview_id("mcp-app-preview", "ui://demo/app.html")).is_none());

        view.update(cx, |view, _| {
            view.cli_config_settings = Some(std::rc::Rc::new(tier_snapshot("off")));
            assert!(!view.mcp_apps_preview_enabled());
        });
        window.render_frame(cx);
        assert!(window.try_find(preview_id("mcp-app-preview", "ui://demo/app.html")).is_none());

        view.update(cx, |view, _| {
            view.cli_config_settings = Some(std::rc::Rc::new(tier_snapshot("interactive")));
            assert!(!view.mcp_apps_preview_enabled(), "no engine renders interactive yet");
            view.cli_config_settings = Some(std::rc::Rc::new(tier_snapshot("static")));
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
        assert!(window.try_find(preview_id("mcp-app-preview", "ui://demo/app.html")).is_some());
        assert!(window.try_find(preview_id("mcp-app-body", "ui://demo/app.html")).is_some());
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
            view.cli_config_settings = Some(std::rc::Rc::new(tier_snapshot("static")));
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
fn multiple_failed_previews_get_distinct_alert_ids(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(std::rc::Rc::new(tier_snapshot("static")));
            let mut item = preview_item();
            let mut second = item.app_previews[0].clone();
            second.uri = "ui://demo/second.html".into();
            item.app_previews.push(second);
            view.transcript.push(item);
            for (index, uri) in ["ui://demo/app.html", "ui://demo/second.html"].into_iter().enumerate() {
                assert_eq!(
                    view.mcp_app_errors.insert(
                        McpAppBodyKey { item_id: "app-1".into(), uri: uri.into() },
                        format!("Failed {index}").into(),
                    ),
                    None
                );
            }
        });
        window.render_frame(cx);
        assert_eq!(window.find(preview_id("mcp-app-error", "ui://demo/app.html")).label(), Some("Failed 0"));
        assert_eq!(window.find(preview_id("mcp-app-error", "ui://demo/second.html")).label(), Some("Failed 1"));
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
            view.cli_config_settings = Some(std::rc::Rc::new(tier_snapshot("static")));
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
        let error = window.find(preview_id("mcp-app-error", "ui://demo/app.html"));
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(error.label(), Some("Could not load the app preview. Try again."));
        assert!(window.try_find(preview_id("retry-mcp-app", "ui://demo/app.html")).is_some());
        window.click(preview_id("retry-mcp-app", "ui://demo/app.html"), cx);
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

#[gpui_kit::test]
fn mcp_app_loading_row_shows_while_read_pending(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(std::rc::Rc::new(tier_snapshot("static")));
            view.transcript.push(preview_item());
            view.request_mcp_app_previews("app-1", cx);
            assert_eq!(view.mcp_app_pending.len(), 1);
        });
        window.render_frame(cx);
        assert!(
            window
                .find(preview_id("mcp-app-loading-label", "ui://demo/app.html"))
                .bounds()
                .size
                .height
                > px(0.)
        );

        view.update(cx, |view, _| {
            let key = McpAppBodyKey {
                item_id: "app-1".into(),
                uri: "ui://demo/app.html".into(),
            };
            assert_eq!(view.mcp_app_bodies.insert(key, app_body()), None);
            view.mcp_app_pending.clear();
        });
        window.render_frame(cx);
        assert!(
            window
                .try_find(preview_id("mcp-app-loading-label", "ui://demo/app.html"))
                .is_none()
        );
    })
    .unwrap();
}

/// Two previews on one row are requested and answered back-to-back. A single
/// retained decode task slot would drop the first handle before its completion
/// runs, leaving that pending read unresolved forever. Each read owns its task.
#[gpui_kit::test]
fn concurrent_mcp_app_reads_both_resolve(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    let first = app_body();
    let mut second = first.clone();
    second.request_id = "app-read-2".into();
    second.uri = "ui://demo/second.html".into();
    view.update(cx, |view, cx| {
        for (request_id, uri) in [
            ("app-read-1", "ui://demo/app.html"),
            ("app-read-2", "ui://demo/second.html"),
        ] {
            let _previous = view.mcp_app_pending.insert(
                request_id.into(),
                PendingMcpAppRead {
                    request_id: request_id.into(),
                    item_id: "app-1".into(),
                    uri: uri.into(),
                },
            );
        }
        view.begin_mcp_app_preview(json!(first), cx);
        view.begin_mcp_app_preview(json!(second), cx);
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert!(view.mcp_app_pending.is_empty(), "both in-flight reads must resolve");
        assert!(view.mcp_app_bodies.contains_key(&McpAppBodyKey {
            item_id: "app-1".into(),
            uri: "ui://demo/app.html".into(),
        }));
        assert!(view.mcp_app_bodies.contains_key(&McpAppBodyKey {
            item_id: "app-1".into(),
            uri: "ui://demo/second.html".into(),
        }));
    });
}

/// A decode detached by `begin_mcp_app_preview` outlives an intervening
/// session reset. The cleared pending map must make its late result a no-op.
#[gpui_kit::test]
fn stale_mcp_app_decode_does_not_publish_after_session_reset(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        let _previous = view.mcp_app_pending.insert(
            "app-read-1".into(),
            PendingMcpAppRead {
                request_id: "app-read-1".into(),
                item_id: "app-1".into(),
                uri: "ui://demo/app.html".into(),
            },
        );
        view.begin_mcp_app_preview(json!(app_body()), cx);
        view.reset_session_state();
    });
    cx.run_until_parked();
    view.update(cx, |view, _| {
        assert!(view.mcp_app_bodies.is_empty(), "a late decode must not repopulate a reset session");
        assert!(view.mcp_app_errors.is_empty());
    });
}

#[gpui_kit::test]
fn session_reset_clears_the_mcp_app_preview_cache(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let key = McpAppBodyKey {
        item_id: "app-1".into(),
        uri: "ui://demo/app.html".into(),
    };
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            assert_eq!(view.mcp_app_bodies.insert(key.clone(), app_body()), None);
            assert_eq!(view.mcp_app_errors.insert(key, "stale".into()), None);
            assert!(
                view.mcp_app_pending
                    .insert(
                        "app-read-1".into(),
                        PendingMcpAppRead {
                            request_id: "app-read-1".into(),
                            item_id: "app-1".into(),
                            uri: "ui://demo/app.html".into(),
                        },
                    )
                    .is_none()
            );
            view.reset_session_state();
            assert!(
                view.mcp_app_bodies.is_empty(),
                "preview bodies are session-scoped and must not survive the switch"
            );
            assert!(view.mcp_app_errors.is_empty());
            assert!(view.mcp_app_pending.is_empty());
        });
    })
    .unwrap();
}
