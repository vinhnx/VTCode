use super::*;

#[gpui_kit::test]
fn reasoning_effort_rail_labels_every_stop_and_home_end_jump(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    settle_fade_animations(cx);
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

#[gpui_kit::test]
fn reasoning_effort_options_accessible_label_tracks_the_cli_level(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    settle_fade_animations(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);

        let options = window.find("effort-options");
        assert_eq!(options.role(), Some(gpui_kit::Role::RadioGroup));
        assert_eq!(options.label(), Some("Reasoning effort: medium"));
        assert_eq!(window.find("model-effort-value").label(), Some("Current reasoning effort: medium"));

        view.update(cx, |view, cx| {
            let slider = view.effort_slider.clone().unwrap();
            slider.update(cx, |state, cx| state.set_value(2., window, cx));
        });
        window.render_frame(cx);

        assert_eq!(window.find("effort-options").label(), Some("Reasoning effort: high"));
        assert_eq!(window.find("model-effort-value").label(), Some("Current reasoning effort: high"));
        assert_eq!(window.find("model-options-summary").label(), Some("gpt-5.6-sol · high"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn reasoning_effort_option_click_commits_the_selected_level(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    settle_fade_animations(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        // Levels render highest-first; index 2 is high in the low/medium/high fixture.
        assert_eq!(window.find(("effort-label", 1usize)).selected(), Some(true));
        let top = window.find(("effort-label", 2usize)).bounds();
        let bottom = window.find(("effort-label", 0usize)).bounds();
        assert!(top.origin.y < bottom.origin.y, "highest effort renders first: top={top:?} bottom={bottom:?}");
        window.click(("effort-label", 2usize), cx);
        window.render_frame(cx);
        assert_eq!(window.find(("effort-label", 2usize)).selected(), Some(true));
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("mouse selection request") else {
        panic!("request expected");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "high");
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn reasoning_effort_option_does_not_submit_at_its_boundary(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    settle_fade_animations(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            config.session_options.as_mut().unwrap().reasoning_effort = "high".into();
            view.configuration = Some(config);
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        assert!(view.read(cx).effort_focus.is_focused(window));
        window.press("right", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "the maximum value is already selected");
}

#[gpui_kit::test]
fn reasoning_effort_slider_subscriptions_follow_palette_lifecycle(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    settle_fade_animations(cx);
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
