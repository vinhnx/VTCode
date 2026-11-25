//! The event drain notifies only when a handler reports a real view mutation.
//! These tests drive the production drain path (not `handle_backend_event`
//! directly), so they pin the whole-window invalidation contract.

use super::*;
use std::cell::Cell;
use std::rc::Rc;
use vtcode_desktop_protocol::ThreadEventNotification;
use vtcode_exec_events::{
    ItemUpdatedEvent, ThreadEvent, ThreadItem, ThreadItemDetails, ToolCallStatus, ToolOutputItem, VersionedThreadEvent,
};

fn drain_one(window: AnyWindowHandle, view: &Entity<DesktopView>, event: BackendEvent, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.event_tx.send(event).unwrap();
            let _batch_full = view.drain_events(window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|_| {});
}

#[gpui_kit::test]
fn stale_backend_events_do_not_notify_the_drain(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.run_until_parked();
    cx.update(|_| {});
    let notified = Rc::new(Cell::new(0_usize));
    let probe = notified.clone();
    let _subscription = view.update(cx, |_, cx| {
        let handle = cx.entity();
        cx.observe(&handle, move |_, _, _| probe.set(probe.get() + 1))
    });

    // A history response with no matching pending page is dropped by the
    // router without touching view state. Receipt alone must not invalidate.
    drain_one(
        window,
        &view,
        BackendEvent::SessionHistoryResponse {
            request_id: "stale".into(),
            result: Some(json!({"events": []})),
            error: None,
        },
        cx,
    );
    assert_eq!(notified.get(), 0, "a no-op event must not invalidate the window from the drain");

    // A turn-queue status with an unparseable payload is another proven no-op.
    drain_one(window, &view, BackendEvent::TurnQueueStatus(json!("not-a-snapshot")), cx);
    assert_eq!(notified.get(), 0, "a malformed no-op payload must not invalidate either");
}

#[gpui_kit::test]
fn changed_backend_events_still_notify_the_drain(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| view.selected_session_id = Some("session".into()));
    cx.run_until_parked();
    cx.update(|_| {});
    let notified = Rc::new(Cell::new(0_usize));
    let probe = notified.clone();
    let _subscription = view.update(cx, |_, cx| {
        let handle = cx.entity();
        cx.observe(&handle, move |_, _, _| probe.set(probe.get() + 1))
    });

    // A real transition must still repaint the status row.
    drain_one(window, &view, BackendEvent::UiProgress(json!({"sessionId": "session", "active": true})), cx);
    assert!(notified.get() >= 1, "a genuine UI change must notify from the drain");
}

#[gpui_kit::test]
fn terminal_monitor_output_still_notifies_the_drain(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("session".into());
            view.transcript.push(TranscriptItem::new(
                "live".into(),
                "tool".into(),
                "Ran cargo".into(),
                "running".into(),
            ));
            view.open_terminal_monitor("live".into(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|_| {});
    let notified = Rc::new(Cell::new(0_usize));
    let probe = notified.clone();
    let _subscription = view.update(cx, |_, cx| {
        let handle = cx.entity();
        cx.observe(&handle, move |_, _, _| probe.set(probe.get() + 1))
    });

    // A tool-output delta feeds the read-only terminal monitor. That grid
    // mutation has no shell pump of its own, so the drain must still notify
    // even if the transcript projection itself changes nothing.
    let item = ThreadItem {
        id: "live".into(),
        context: None,
        details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
            call_id: "call".into(),
            tool_call_id: None,
            spool_path: None,
            output: "from the monitor\n".into(),
            is_delta: Some(true),
            exit_code: None,
            status: ToolCallStatus::InProgress,
            app_previews: Vec::new(),
        })),
    };
    drain_one(
        window,
        &view,
        BackendEvent::ThreadEvent(ThreadEventNotification {
            session_id: "session".into(),
            turn_id: None,
            event: VersionedThreadEvent::new(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item })),
        }),
        cx,
    );
    assert!(notified.get() >= 1, "the terminal monitor grid must repaint from the drain");
}

#[gpui_kit::test]
fn late_turn_ack_keeps_the_webmcp_invalidation_signal(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.webmcp_status = Some(connected_webmcp_status());
        view.selected_session_id = Some("session".into());
        view.active_turn_id = Some("turn".into());
    });
    cx.run_until_parked();
    cx.update(|_| {});
    let notified = Rc::new(Cell::new(0_usize));
    let probe = notified.clone();
    let _subscription = view.update(cx, |_, cx| {
        let handle = cx.entity();
        cx.observe(&handle, move |_, _, _| probe.set(probe.get() + 1))
    });

    // A duplicate ack for the active turn hits the early-return guard. The
    // pre-dispatch WebMCP invalidation still dirtied the status row, so the
    // drain must not treat this event as a no-op.
    drain_one(
        window,
        &view,
        BackendEvent::Response {
            method: vtcode_desktop_protocol::method::TURN_SUBMIT.into(),
            result: Some(json!({"sessionId": "session", "turnId": "turn"})),
            error: None,
        },
        cx,
    );
    assert!(notified.get() >= 1, "a late turn ack must keep the WebMCP invalidation");
}
