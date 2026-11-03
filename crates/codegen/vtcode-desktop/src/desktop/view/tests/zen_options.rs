use super::*;

#[gpui_kit::test]
fn zen_model_button_opens_options_and_commits_effort(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.zen_mode = true;
        });
        window.render_frame(cx);
        assert_eq!(window.find("pick-model").label(), Some("gpt-5.6-sol · Medium · Ultrafast"));
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Select model"));
        assert_eq!(window.find("model-options-summary").label(), Some("gpt-5.6-sol · medium"));
        // The rail runs bottom-to-top: the top edge is the maximum level.
        let track = window.find("slider-bar-container").bounds();
        window.click_at("slider-bar-container", point(track.size.width / 2., px(2.)), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("effort request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "high");
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn zen_mode_button_lists_cli_modes_and_sends_selection(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.zen_mode = true;
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-mode").label(), Some("Agent mode: Build"));
        window.click("composer-mode", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Agent mode"));
        for name in ["build", "plan", "auto", "duck", "coordinator", "custom-reviewer"] {
            assert!(
                window
                    .try_find(palette_action_id(&super::super::PaletteAction::SelectSessionOptions(
                        vtcode_desktop_protocol::SessionOptionsSelectParams {
                            mode: Some(name.into()),
                            ..Default::default()
                        }
                    )))
                    .is_some(),
                "CLI mode {name} must be selectable in zen"
            );
        }
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("Plan", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("mode request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["mode"], "plan");
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn zen_policy_button_changes_permission_and_blocks_during_active_work(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.zen_mode = true;
            view.running = true;
        });
        window.render_frame(cx);
        // Active work renders the controls but leaves them inert.
        for button in ["pick-model", "composer-mode", "composer-policy"] {
            assert!(window.try_find(button).is_some(), "{button} stays visible while running");
            window.click(button, cx);
            window.render_frame(cx);
            assert!(view.read(cx).palette.is_none(), "{button} must not open a picker while running");
        }
        assert!(requests.try_recv().is_err(), "no request while running");

        view.update(cx, |view, _| view.running = false);
        window.render_frame(cx);
        assert_eq!(window.find("composer-policy").label(), Some("Execution policy: Ask before protected actions"));
        window.click("composer-policy", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Command permissions"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.within("command-palette").press("down", cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("permission request") else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["permissionMode"], "autoApproveCommands");
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn zen_composer_popups_stay_above_input_in_narrow_window(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop_sized(cx, 520., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.zen_mode = true;
        });
        window.render_frame(cx);
        let viewport = window.viewport_size();
        let cluster = window.find("zen-composer-context").bounds();
        let send = window.find("send-prompt").bounds();
        assert!(
            cluster.right() <= send.left(),
            "the narrow option cluster must end before the send action: {cluster:?} vs {send:?}"
        );
        for button in ["pick-model", "composer-mode", "composer-policy"] {
            window.click(button, cx);
            window.render_frame(cx);
            let popup = window.find("command-palette").bounds();
            let frame = window.find("message-composer-frame").bounds();
            assert!(popup.bottom() <= frame.top(), "{button} popup must stay above the input");
            assert!(popup.left() >= px(0.), "{button} popup left={:?}", popup.left());
            assert!(popup.right() <= viewport.width, "{button} popup right={:?}, viewport={viewport:?}", popup.right());
            window.press("escape", cx);
            window.render_frame(cx);
            assert!(view.read(cx).palette.is_none(), "{button} popup must dismiss on escape");
            // The trigger stays healthy: reopening works after dismissal.
            window.click(button, cx);
            window.render_frame(cx);
            assert!(window.try_find("command-palette").is_some(), "{button} must reopen after escape");
            window.press("escape", cx);
            window.render_frame(cx);
            assert!(view.read(cx).palette.is_none());
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_surface_spans_the_window_edge_to_edge(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // The suite baseline is normal mode; opt into zen for this comparison.
    view.update(cx, |view, cx| {
        view.zen_mode = true;
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Zen drops the toolbar: the drag strip owns the top 40px and the
        // conversation runs flush below it with no chrome rhythm inset and
        // square corners.
        assert!(window.try_find("title-bar").is_none(), "zen drops the toolbar");
        let surface = window.find("workspace-surface").bounds();
        assert_eq!(surface.origin, point(px(0.), px(40.)), "zen content starts below the drag strip");
        // Exiting zen restores the toolbar and the windowed pane rhythm.
        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        let title_bar = window.find("title-bar").bounds();
        let windowed = window.find("workspace-surface").bounds();
        assert_eq!(windowed.origin.y, title_bar.bottom() + px(12.), "windowed mode keeps the top rhythm");
        assert!(windowed.origin.x > surface.origin.x, "windowed mode keeps the side rhythm");
    })
    .unwrap();
    cx.update(|cx| {
        assert!(!view.read(cx).zen_mode, "the toggle must have exited zen for the rhythm comparison");
    });
}

/// Windowed zen hides the toolbar that otherwise owns window dragging: Kit
/// TitleBar moves the window via `start_window_move`, while
/// `app_owns_titlebar_drag` disables native AppKit dragging for the whole
/// window — without a drag region the window cannot be moved at all. The
/// zen drag strip takes the toolbar's place above the content and owns the
/// traffic-light zone, so route pages and the workspace surface start
/// below it. Normal mode keeps the toolbar instead, and fullscreen
/// renders neither (a fullscreen window cannot move).
#[gpui_kit::test]
fn zen_drag_strip_restores_window_dragging_in_windowed_zen(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.zen_mode = true;
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("title-bar").is_none(), "zen drops the toolbar");
        let strip = window.find("zen-drag-strip").bounds();
        let viewport = window.viewport_size();
        assert_eq!(strip.origin, point(px(0.), px(0.)), "strip spans the top edge: {strip:?}");
        assert_eq!(strip.size.width, viewport.width, "strip spans the full width: {strip:?} {viewport:?}");
        assert_eq!(strip.size.height, px(40.), "strip owns the traffic-light zone (lights end at y = 29): {strip:?}");
        let surface = window.find("workspace-surface").bounds();
        assert_eq!(surface.origin.y, strip.bottom(), "workspace starts below the strip: {surface:?}");
        // Route pages keep their light clearance through the strip, not padding.
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        let panel = window.find("settings-panel").bounds();
        assert_eq!(panel.origin.y, strip.bottom(), "settings starts below the strip: {panel:?}");
        // A click arms on press and disarms on release without starting a
        // native drag (headless `start_window_move` would panic): the strip
        // is a safe no-op for clicks, drags only.
        window.click("zen-drag-strip", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(!view.read(cx).zen_drag_armed, "clicking the strip ends disarmed");
    });
    // Fullscreen hides the lights and cannot move: no strip.
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleFullScreen), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.is_fullscreen(), "dispatch must enter full screen");
        assert!(window.try_find("zen-drag-strip").is_none(), "fullscreen has no drag strip");
    })
    .unwrap();
    // Normal mode restores the toolbar instead of the strip.
    view.update(cx, |view, cx| {
        view.zen_mode = false;
        cx.notify();
    });
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(super::super::ToggleFullScreen), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.is_fullscreen(), "dispatch must exit full screen");
        assert!(window.try_find("title-bar").is_some(), "normal mode restores the toolbar");
        assert!(window.try_find("zen-drag-strip").is_none(), "normal mode has no zen strip");
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_speed_suffix_renders_quiet_text_and_restores_badge_on_exit(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.configuration = Some(runtime_options_config());
            view.zen_mode = true;
        });
        window.render_frame(cx);
        // The accessible contract is identical in both modes: the model
        // summary and the speed readout keep their names.
        assert_eq!(window.find("pick-model").label(), Some("gpt-5.6-sol · Medium · Ultrafast"));
        let quiet = window.find("composer-model-speed");
        assert_eq!(quiet.role(), Some(gpui_kit::Role::Label));
        assert_eq!(quiet.label(), Some("Processing speed: Ultrafast"));
        let zen_width = quiet.bounds().size.width;
        let effort_width = window.find("composer-model-effort").bounds().size.width;

        // Exiting zen restores the filled badge: same readout, louder paint.
        // The badge carries pill padding plus an icon around the same word,
        // so it must measure wider than the quiet text.
        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).zen_mode);
        assert_eq!(window.find("pick-model").label(), Some("gpt-5.6-sol · Medium · Ultrafast"));
        let badge = window.find("composer-model-speed");
        assert_eq!(badge.role(), Some(gpui_kit::Role::Label));
        assert_eq!(badge.label(), Some("Processing speed: Ultrafast"));
        let badge_width = badge.bounds().size.width;
        assert!(
            badge_width > zen_width,
            "the filled badge must measure wider than zen quiet text: badge={badge_width:?} quiet={zen_width:?}"
        );
        // Control: the effort suffix is quiet in both modes, so its width
        // must not move across the toggle.
        assert_eq!(
            window.find("composer-model-effort").bounds().size.width,
            effort_width,
            "effort keeps one paint across the toggle"
        );

        // Re-entering zen restores the quiet text exactly.
        window.press("ctrl-cmd-z", cx);
        window.render_frame(cx);
        assert!(view.read(cx).zen_mode);
        assert_eq!(window.find("composer-model-speed").bounds().size.width, zen_width);
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_option_cluster_orders_model_mode_policy_before_send(cx: &mut TestAppContext) {
    use gpui_kit::component::theme::Theme;

    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.configuration = Some({
            let mut configuration = runtime_options_config();
            configuration.provider = "anthropic".into();
            // Asymmetric extreme: a long name forces truncation pressure
            // while the mode/policy siblings keep their short labels.
            configuration.model = "claude-sonnet-5-extended-thinking-marathon".into();
            configuration
        });
        view.zen_mode = true;
    });

    for base_size in [14., 24.] {
        cx.update(|cx| {
            Theme::update(cx, |theme| {
                theme.font_size = px(base_size);
            });
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let model = window.find("pick-model").bounds();
            let mode = window.find("composer-mode").bounds();
            let policy = window.find("composer-policy").bounds();
            let send = window.find("send-prompt").bounds();
            assert!(
                model.right() <= mode.left() && mode.right() <= policy.left(),
                "cluster reads model, mode, policy left to right at {base_size}px: model={model:?} mode={mode:?} policy={policy:?}"
            );
            for (name, button) in [("mode", mode), ("policy", policy), ("send", send)] {
                let drift = (button.center().y - model.center().y).abs();
                assert!(
                    drift <= px(1.),
                    "cluster shares one center line at {base_size}px: {name} drifts {drift:?}"
                );
            }
            let cluster = window.find("zen-composer-context").bounds();
            assert!(
                cluster.right() <= send.left(),
                "the option cluster must end before the send action at {base_size}px: {cluster:?} vs {send:?}"
            );
        })
        .unwrap();
    }

    // Without a processing tier the speed suffix disappears but the cluster
    // order still holds.
    view.update(cx, |view, _| {
        let mut configuration = runtime_options_config();
        configuration.provider = "anthropic".into();
        configuration.model = "claude-sonnet-5".into();
        let options = configuration.session_options.as_mut().unwrap();
        options.speed = None;
        options.fast_mode_enabled = false;
        view.configuration = Some(configuration);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("composer-model-speed").is_none());
        let model = window.find("pick-model").bounds();
        let mode = window.find("composer-mode").bounds();
        let policy = window.find("composer-policy").bounds();
        assert!(
            model.right() <= mode.left() && mode.right() <= policy.left(),
            "cluster order holds without a speed suffix: model={model:?} mode={mode:?} policy={policy:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn zen_toggle_round_trips_through_command_palette(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        assert!(!view.read(cx).zen_mode);
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        let toggle = palette_action_id(&super::super::PaletteAction::ToggleZenMode);
        assert!(window.try_find(toggle).is_some(), "zen toggle row renders in the palette");
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("Toggle Zen", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).zen_mode, "palette entry enters zen");
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Exit Zen mode"));

        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("Toggle Zen", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!view.read(cx).zen_mode, "palette entry exits zen");
        assert_eq!(window.find("zen-mode-toggle").label(), Some("Enter Zen mode"));
    })
    .unwrap();
}
