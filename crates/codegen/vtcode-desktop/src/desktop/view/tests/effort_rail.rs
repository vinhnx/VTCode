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
        window.click("composer-effort", cx);
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
        // A commit dismisses the panel, so the CLI acknowledgement has to
        // reopen it before End can jump to the maximum.
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
            view.open_model_options(window, cx);
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
    cx.update(|cx| assert!(view.read(cx).palette.is_none(), "the second commit dismisses the panel too"));
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
        window.click("composer-effort", cx);
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
        window.click("composer-effort", cx);
        window.render_frame(cx);
        // Levels render highest-first; index 2 is high in the low/medium/high fixture.
        assert_eq!(window.find(("effort-label", 1usize)).selected(), Some(true));
        let top = window.find(("effort-label", 2usize)).bounds();
        let bottom = window.find(("effort-label", 0usize)).bounds();
        assert!(top.origin.y < bottom.origin.y, "highest effort renders first: top={top:?} bottom={bottom:?}");
        window.click(("effort-label", 2usize), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("mouse selection request") else {
        panic!("request expected");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "high");
    assert!(requests.try_recv().is_err());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "clicking an effort level dismisses the panel");
        assert!(window.try_find(("effort-label", 2usize)).is_none());
    })
    .unwrap();
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
        window.click("composer-effort", cx);
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
fn compact_model_panel_disables_effort_rows_with_the_effort_button(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    settle_fade_animations(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
        });
        window.render_frame(cx);
        window.click("composer-effort", cx);
        window.render_frame(cx);

        assert_ne!(window.find("composer-effort").disabled(), Some(true));
        for index in 0..3usize {
            assert_ne!(
                window.find(("effort-label", index)).disabled(),
                Some(true),
                "effort row {index} starts enabled"
            );
        }

        // A pending session request darkens the effort button. The compact
        // panel must darken with it instead of leaving lit, clickable-looking
        // effort rows behind a disabled effort button.
        view.update(cx, |view, _| {
            view.session_request = Some(vtcode_desktop_protocol::method::CONFIG_OPTIONS);
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-effort").disabled(), Some(true));
        for index in 0..3usize {
            assert_eq!(
                window.find(("effort-label", index)).disabled(),
                Some(true),
                "effort row {index} must darken with the model button"
            );
        }

        // A native disabled flag is not proof that activation is dropped.
        // Exercise the row: the effort stays put and the panel stays open.
        window.click(("effort-label", 2usize), cx);
        window.render_frame(cx);
        assert_eq!(window.find("effort-options").label(), Some("Reasoning effort: medium"));
        assert!(window.try_find(("effort-label", 2usize)).is_some(), "a rejected click keeps the panel open");
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "a disabled effort row must not commit");
}

#[gpui_kit::test]
fn composer_clicks_stay_at_logical_coordinates_after_display_scale_change(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
    settle_fade_animations(cx);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.configuration = Some(runtime_options_config());
    });
    let at_1x = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find("composer-effort").bounds()
        })
        .unwrap();

    cx.simulate_window_scale_factor_change(window, 2.0);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("composer-effort").bounds(), at_1x, "scale changes device pixels, not logical layout");
        window.click("composer-effort", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "logical hit testing opens the panel at 2x");
        assert!(window.try_find(("effort-label", 2usize)).is_some());
    })
    .unwrap();

    cx.simulate_window_scale_factor_change(window, 1.0);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("composer-effort").bounds(), at_1x, "1x restores the same logical frame");
        window.click("composer-effort", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "the same logical click closes the panel at 1x");
    })
    .unwrap();
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
        window.click("composer-effort", cx);
        window.render_frame(cx);

        view.update(cx, |view, cx| {
            let slider = view.effort_slider.clone().expect("filtered knob");
            assert_eq!(view._effort_slider_subscriptions.len(), 2);
            // The guard seeds from the live slider, so a repeated drag tick at
            // the initial step cannot trigger a redundant palette repaint.
            assert_eq!(view.observed_effort_slider_value, Some(slider.read(cx).value()));
            view.close_palette(window, cx);
            assert!(view.effort_slider.is_none());
            assert!(view._effort_slider_subscriptions.is_empty());
            assert!(view.observed_effort_slider_value.is_none());
            assert!(view.palette.is_none());
        });
    })
    .unwrap();
}
