//! Ephemeral computer-use screenshot cards: fetch-on-expand and body states.

use super::super::computer_preview::{
    ComputerPreviewBody, ComputerPreviewBodyKey, PendingComputerPreviewRead, is_computer_preview_uri,
    valid_computer_preview_read,
};
use crate::desktop::transcript::TranscriptItem;
use vtcode_desktop_protocol::ComputerPreviewReadResult;
use vtcode_exec_events::AppPreviewItem;

use super::*;

fn preview_item() -> TranscriptItem {
    let mut item = TranscriptItem::new(
        "cua-1".into(),
        "tool".into(),
        "Computer use: take a screenshot".into(),
        "completed\nturn finished".into(),
    );
    item.output = Some("turn finished".into());
    item.expanded = true;
    item.app_previews = vec![AppPreviewItem {
        uri: "computer-preview://cua-1".into(),
        mime_type: Some("image/png".into()),
        bytes: 412,
        blocks: 1,
        ui: None,
    }];
    item
}

fn preview_body() -> ComputerPreviewReadResult {
    ComputerPreviewReadResult {
        uri: "computer-preview://cua-1".into(),
        request_id: "preview-read-1".into(),
        mime_type: Some("image/png".into()),
        data: Some("iVBORw0KGgpwcm9iZQ==".into()),
        total_bytes: 13,
        downscaled: false,
        expired: false,
    }
}

fn decoded_body() -> ComputerPreviewBody {
    ComputerPreviewBody {
        image: std::sync::Arc::new(gpui_kit::Image::from_bytes(
            gpui_kit::ImageFormat::Png,
            b"\x89PNG\r\n\x1a\nprobe".to_vec(),
        )),
        bytes: 13,
        downscaled: false,
    }
}

#[test]
fn computer_preview_uri_partition_keeps_mcp_reads_clean() {
    assert!(is_computer_preview_uri("computer-preview://cua-1"));
    assert!(!is_computer_preview_uri("ui://demo/app.html"));
    assert!(!is_computer_preview_uri(""));
}

#[test]
fn computer_preview_read_validation_matches_pending_identity() {
    let pending = PendingComputerPreviewRead {
        request_id: "preview-read-1".into(),
        item_id: "cua-1".into(),
        uri: "computer-preview://cua-1".into(),
    };
    assert!(valid_computer_preview_read(&preview_body(), &pending));
    let mut stale = preview_body();
    stale.request_id = "preview-read-0".into();
    assert!(!valid_computer_preview_read(&stale, &pending));
    let mut swapped = preview_body();
    swapped.uri = "computer-preview://cua-2".into();
    assert!(!valid_computer_preview_read(&swapped, &pending));
}

#[gpui_kit::test]
fn computer_preview_expand_fetches_once_and_skips_settled_uris(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.transcript.push(preview_item());
            view.request_computer_previews("cua-1", cx);
            assert_eq!(view.computer_preview_pending.len(), 1);
            // A second expand while the read is in flight queues nothing new.
            view.request_computer_previews("cua-1", cx);
            assert_eq!(view.computer_preview_pending.len(), 1);
        });
        window.render_frame(cx);
        let BackendCommand::Request { method, params } = requests.try_recv().expect("one preview read") else {
            panic!("expected a request command");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::COMPUTER_PREVIEW_READ);
        assert_eq!(params["uri"], json!("computer-preview://cua-1"));
        assert!(requests.try_recv().is_err(), "no duplicate read queued");

        view.update(cx, |view, cx| {
            let key = ComputerPreviewBodyKey {
                item_id: "cua-1".into(),
                uri: "computer-preview://cua-1".into(),
            };
            assert_eq!(view.computer_preview_errors.insert(key.clone(), "boom".into()), None);
            view.computer_preview_pending.clear();
            // Errors wait for an explicit retry; bodies and expiry never refetch.
            view.request_computer_previews("cua-1", cx);
            assert!(view.computer_preview_pending.is_empty());
            assert!(view.computer_preview_errors.remove(&key).is_some());
            assert_eq!(view.computer_preview_bodies.insert(key.clone(), decoded_body()), None);
            view.request_computer_previews("cua-1", cx);
            assert!(view.computer_preview_pending.is_empty());
            assert!(view.computer_preview_bodies.remove(&key).is_some());
            assert!(view.computer_preview_expired.insert(key));
            view.request_computer_previews("cua-1", cx);
            assert!(view.computer_preview_pending.is_empty(), "expired previews never refetch");
        });
        assert!(requests.try_recv().is_err(), "settled URIs stay quiet");
    })
    .unwrap();
}

#[gpui_kit::test]
fn computer_preview_error_row_retries_through_the_button(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.transcript.push(preview_item());
            assert_eq!(
                view.computer_preview_errors.insert(
                    ComputerPreviewBodyKey {
                        item_id: "cua-1".into(),
                        uri: "computer-preview://cua-1".into()
                    },
                    "Could not load the screenshot. Try again.".into(),
                ),
                None
            );
        });
        window.render_frame(cx);
        let error = window.find("computer-preview-error-cua-1");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(error.label(), Some("Could not load the screenshot. Try again."));
        assert!(window.try_find("retry-computer-preview-cua-1").is_some());
        window.click("retry-computer-preview-cua-1", cx);
        window.render_frame(cx);

        view.update(cx, |view, _| {
            let key = ComputerPreviewBodyKey {
                item_id: "cua-1".into(),
                uri: "computer-preview://cua-1".into(),
            };
            assert!(!view.computer_preview_errors.contains_key(&key), "retry clears the error first");
            assert_eq!(view.computer_preview_pending.len(), 1);
        });
        let BackendCommand::Request { method, .. } = requests.try_recv().expect("retry read") else {
            panic!("expected a request command");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::COMPUTER_PREVIEW_READ);
    })
    .unwrap();
}

#[gpui_kit::test]
fn computer_preview_response_resolves_bodies_and_expiry(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.transcript.push(preview_item());
            view.request_computer_previews("cua-1", cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("computer-preview-loading-cua-1").is_some());
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { params, .. } = requests.try_recv().expect("preview read") else {
        panic!("expected a request command");
    };
    let request_id = params["requestId"].as_str().unwrap().to_string();
    let mut body = preview_body();
    body.request_id.clone_from(&request_id);
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::ComputerPreviewResponse { request_id, result: Some(json!(body)), error: None },
            cx,
        );
    });
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, _| {
            let key = ComputerPreviewBodyKey {
                item_id: "cua-1".into(),
                uri: "computer-preview://cua-1".into(),
            };
            assert!(view.computer_preview_bodies.contains_key(&key));
            assert!(view.computer_preview_pending.is_empty());
        });
        assert!(window.try_find("computer-preview-body-cua-1").is_some());
        assert!(window.try_find("computer-preview-loading-cua-1").is_none());
        assert_eq!(window.find("computer-preview-body-cua-1").label(), Some("Screenshot preview (image/png · 13B)"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn computer_preview_scaled_body_marks_the_caption(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(preview_item());
            let key = ComputerPreviewBodyKey {
                item_id: "cua-1".into(),
                uri: "computer-preview://cua-1".into(),
            };
            let mut body = decoded_body();
            body.downscaled = true;
            assert_eq!(view.computer_preview_bodies.insert(key, body), None);
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("computer-preview-body-cua-1").label(),
            Some("Screenshot preview (image/png · 13B · scaled)")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn computer_preview_expired_renders_without_retry_or_refetch(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(8);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.transcript.push(preview_item());
            view.request_computer_previews("cua-1", cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { params, .. } = requests.try_recv().expect("preview read") else {
        panic!("expected a request command");
    };
    let request_id = params["requestId"].as_str().unwrap().to_string();
    let expired = ComputerPreviewReadResult {
        uri: "computer-preview://cua-1".into(),
        request_id: request_id.clone(),
        mime_type: None,
        data: None,
        total_bytes: 0,
        downscaled: false,
        expired: true,
    };
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::ComputerPreviewResponse {
                request_id,
                result: Some(json!(expired)),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // The expired note renders with no retry control and no body panel.
        assert!(window.try_find("retry-computer-preview-cua-1").is_none());
        assert!(window.try_find("computer-preview-body-cua-1").is_none());
        view.update(cx, |view, cx| {
            let key = ComputerPreviewBodyKey {
                item_id: "cua-1".into(),
                uri: "computer-preview://cua-1".into(),
            };
            assert!(view.computer_preview_expired.contains(&key));
            assert!(view.computer_preview_pending.is_empty());
            view.request_computer_previews("cua-1", cx);
            assert!(view.computer_preview_pending.is_empty(), "expired previews never refetch");
        });
        assert!(requests.try_recv().is_err(), "no refetch queued");
    })
    .unwrap();
}

#[gpui_kit::test]
fn session_reset_clears_the_computer_preview_cache(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let key = ComputerPreviewBodyKey {
        item_id: "cua-1".into(),
        uri: "computer-preview://cua-1".into(),
    };
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, _| {
            assert_eq!(view.computer_preview_bodies.insert(key.clone(), decoded_body()), None);
            assert_eq!(view.computer_preview_errors.insert(key.clone(), "stale".into()), None);
            assert!(view.computer_preview_expired.insert(key.clone()));
            assert!(
                view.computer_preview_pending
                    .insert(
                        "preview-read-1".into(),
                        PendingComputerPreviewRead {
                            request_id: "preview-read-1".into(),
                            item_id: "cua-1".into(),
                            uri: "computer-preview://cua-1".into(),
                        },
                    )
                    .is_none()
            );
            view.reset_session_state();
            assert!(view.computer_preview_bodies.is_empty(), "preview pixels must not survive the switch");
            assert!(view.computer_preview_errors.is_empty());
            assert!(view.computer_preview_expired.is_empty());
            assert!(view.computer_preview_pending.is_empty());
            assert!(view.computer_preview_task.is_none());
        });
    })
    .unwrap();
}
