use super::super::shortcuts::{COMMANDS, CommandId};
use super::*;
use vtcode_desktop_protocol::SessionSummary;

fn summary(session_id: &str, title: &str) -> SessionSummary {
    SessionSummary {
        session_id: session_id.into(),
        title: Some(title.into()),
        workspace: std::env::temp_dir().display().to_string(),
        completed: false,
        resumable: true,
        size_bytes: 0,
        updated_at: None,
    }
}

fn open_running_desktop(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<DesktopView>, tokio::sync::mpsc::Receiver<BackendCommand>) {
    let (window, view) = open_desktop(cx);
    let (commands, requests) = tokio::sync::mpsc::channel(8);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.workspace = Some(std::env::temp_dir());
        view.sessions = vec![summary("thread-a", "First thread")];
        view.selected_session_id = Some("thread-a".into());
        view.open_tabs = vec!["thread-a".into()];
        view.running = true;
        view.active_turn_id = Some("turn-1".into());
    });
    (window, view, requests)
}

fn take_request(requests: &mut tokio::sync::mpsc::Receiver<BackendCommand>) -> (String, serde_json::Value) {
    let BackendCommand::Request { method, params } = requests.try_recv().expect("a backend request was sent") else {
        panic!("expected a session request");
    };
    (method, params)
}

#[test]
fn background_shortcut_registers_with_default_binding() {
    let command = COMMANDS
        .iter()
        .find(|command| command.id == CommandId::BackgroundOperation)
        .expect("background command is registered");
    assert_eq!(command.default_binding, "cmd-shift-d");
}

#[gpui_kit::test]
fn background_button_sends_session_exec_background(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("background-exec", cx);
    })
    .unwrap();
    let (method, params) = take_request(&mut requests);
    assert_eq!(method, vtcode_desktop_protocol::method::EXEC_BACKGROUND);
    assert_eq!(params["sessionId"], "thread-a");
    cx.update(|cx| assert_eq!(view.read(cx).error, None));
}

#[gpui_kit::test]
fn force_cancel_button_sends_session_exec_force_cancel(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("force-cancel-exec", cx);
    })
    .unwrap();
    let (method, params) = take_request(&mut requests);
    assert_eq!(method, vtcode_desktop_protocol::method::EXEC_FORCE_CANCEL);
    assert_eq!(params["sessionId"], "thread-a");
    cx.update(|cx| assert_eq!(view.read(cx).error, None));
}

/// The background action leaves the composer's run cluster: it lives in the
/// thread header instead, beside the marker naming the running state.
#[gpui_kit::test]
fn background_exec_lives_in_thread_header(cx: &mut TestAppContext) {
    let (window, _, mut requests) = open_running_desktop(cx);
    let (row, composer) = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            (
                window.try_find("background-exec").map(|handle| handle.bounds()),
                window.try_find("thread-header").map(|handle| handle.bounds()),
            )
        })
        .unwrap();
    let row = row.expect("background button renders in the thread header");
    let composer = composer.expect("thread header renders");
    let composer_top = composer.origin.y + composer.size.height;
    assert!(
        row.origin.y < composer_top,
        "background button sits in the header ({row:?}), not the composer run cluster (top {composer_top:?})"
    );
    cx.update_window(window, |_, window, cx| {
        window.click("background-exec", cx);
    })
    .unwrap();
    let (method, params) = take_request(&mut requests);
    assert_eq!(method, vtcode_desktop_protocol::method::EXEC_BACKGROUND);
    assert_eq!(params["sessionId"], "thread-a");
}

#[gpui_kit::test]
fn exec_buttons_hide_while_idle(cx: &mut TestAppContext) {
    let (window, view, _) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.running = false);
        window.render_frame(cx);
        assert!(window.try_find("force-cancel-exec").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn exec_accepted_response_updates_status(cx: &mut TestAppContext) {
    let (window, view, _) = open_running_desktop(cx);
    for (method, expected) in [
        (vtcode_desktop_protocol::method::EXEC_BACKGROUND, "Background requested"),
        (vtcode_desktop_protocol::method::EXEC_FORCE_CANCEL, "Force-cancel requested"),
    ] {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: method.into(),
                    result: Some(serde_json::json!({ "accepted": true })),
                    error: None,
                },
                cx,
            );
        });
        cx.update(|cx| assert_eq!(view.read(cx).status.as_str(), expected, "{method} status"));
    }
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
}

#[gpui_kit::test]
fn run_cluster_matches_idle_send_size_and_swaps_exclusively(cx: &mut TestAppContext) {
    let (window, view, _) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("send-prompt").is_none(), "send hides while a turn runs");
        let stop = window.find("cancel-turn").bounds();
        let bounds = window.find("force-cancel-exec").bounds();
        // 1px tolerates subpixel rounding; a size-class drift (medium vs
        // large) differs by an order of magnitude more, so it still fails.
        assert!(
            (bounds.size.width - stop.size.width).abs() <= px(1.)
                && (bounds.size.height - stop.size.height).abs() <= px(1.),
            "force-cancel-exec matches cancel-turn size: {bounds:?} vs {stop:?}"
        );
        // Wrapping stays the designed narrow-viewport behavior, but the
        // common case holds one row: a future size bump that wraps the
        // pair at the default viewport must fail loudly here.
        assert!(
            (bounds.origin.y - stop.origin.y).abs() <= px(1.),
            "force-cancel-exec stays on the cluster row: {bounds:?} vs {stop:?}"
        );
        view.update(cx, |view, _| view.running = false);
        window.render_frame(cx);
        for id in ["force-cancel-exec", "cancel-turn"] {
            assert!(window.try_find(id).is_none(), "{id} hides while idle");
        }
        let send = window.find("send-prompt").bounds();
        assert!(
            (send.size.width - stop.size.width).abs() <= px(1.)
                && (send.size.height - stop.size.height).abs() <= px(1.),
            "idle send matches running pair size: {send:?} vs {stop:?}"
        );
    })
    .unwrap();
}

/// The send control follows draft state: an empty draft renders a disabled
/// send, a submittable draft enables it, and submitting (which clears the
/// draft and ends the turn here) disables it again.
#[gpui_kit::test]
fn run_control_tracks_send_actionable_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session-run-control".to_string());
        });
        window.render_frame(cx);
        assert_eq!(window.find("send-prompt").disabled(), Some(true));

        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |state, cx| state.set_value("Send this draft", window, cx));
        });
        window.render_frame(cx);
        assert_ne!(window.find("send-prompt").disabled(), Some(true));
        window.click("send-prompt", cx);
        assert!(view.read(cx).running, "clicking the send control submits");

        view.update(cx, |view, _| view.running = false);
        window.render_frame(cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        assert_eq!(window.find("send-prompt").disabled(), Some(true));
    })
    .unwrap();
}

/// While a turn runs, the cancel control replaces the send control; ending
/// the turn swaps the pair back.
#[gpui_kit::test]
fn run_control_swaps_to_cancel_while_running(cx: &mut TestAppContext) {
    let (window, view, mut requests) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("send-prompt").is_none());
        window.click("cancel-turn", cx);
    })
    .unwrap();
    let (method, _) = take_request(&mut requests);
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.running = false);
        window.render_frame(cx);
        assert!(window.try_find("cancel-turn").is_none(), "idle unmounts the cancel control");
        assert!(window.try_find("send-prompt").is_some());
    })
    .unwrap();
}

/// Under reduced motion the run control stays fully operable: the button must
/// remain present, sized, and clickable with motion disabled.
#[gpui_kit::test]
fn run_control_stays_operable_under_reduced_motion(cx: &mut TestAppContext) {
    let (window, _, mut requests) = open_running_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let stop = window.find("cancel-turn").bounds();
        assert!(stop.size.width > px(0.) && stop.size.height > px(0.));
        window.click("cancel-turn", cx);
    })
    .unwrap();
    let (method, _) = take_request(&mut requests);
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
}
