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

#[gpui_kit::test]
fn exec_buttons_hide_while_idle(cx: &mut TestAppContext) {
    let (window, view, _) = open_running_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.running = false);
        window.render_frame(cx);
        assert!(window.try_find("background-exec").is_none());
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
