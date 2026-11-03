use super::*;

#[gpui_kit::test]
fn reasoning_effort_rail_labels_every_stop_and_home_end_jump(cx: &mut TestAppContext) {
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
        assert_eq!(window.find(("effort-label", 0usize)).label(), Some("Low"));
        assert_eq!(window.find(("effort-label", 1usize)).label(), Some("Medium"));
        assert_eq!(window.find(("effort-label", 2usize)).label(), Some("High"));
        // Home jumps straight to the minimum without stepping through medium.
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.press("home", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("home request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "low");
    cx.update_window(window, |_, window, cx| {
        // CLI acknowledgement re-enables the rail; End jumps to the maximum.
        let mut ack = runtime_options_config();
        ack.session_options.as_mut().unwrap().reasoning_effort = "low".into();
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                    result: Some(json!(ack)),
                    error: None,
                },
                cx,
            );
            view.focus_palette_state(window, cx);
        });
        window.render_frame(cx);
        window.press("end", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("end request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "high");
    assert!(requests.try_recv().is_err());
}
