use super::*;

#[gpui_kit::test]
fn cli_theme_inheritance_respects_explicit_desktop_override_and_keeps_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let snapshot = |theme: &str| BackendEvent::Response {
        method: vtcode_desktop_protocol::method::CONFIG_DISCOVER.into(),
        error: None,
        result: Some(json!({ "provider": "ollama", "model": "local", "authenticated": true,
            "credentialSetupCommand": "", "sharedUi": {"theme": theme} })),
    };
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.ui_font_size = Some(19);
            view.composer
                .update(cx, |input, cx| input.set_value("Preserve this draft", window, cx));
            view.handle_backend_event(snapshot("ciapre-blue"), cx);
            assert_eq!(view.theme_id, "ciapre-blue");
            assert_eq!(view.preferences.follow_cli_theme, Some(true));
            assert_eq!(cx.theme().font_size, px(19.));
            view.apply_theme("github-dark".into(), window, cx);
            view.handle_backend_event(snapshot("solarized-light"), cx);
            assert_eq!(view.theme_id, "github-dark", "CLI discovery cannot overwrite an explicit desktop choice");
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 3, cx);
        focus_element_by_tab(window, "use-cli-theme", cx);
        window.press("enter", cx);
        assert_eq!(view.read(cx).theme_id, "solarized-light");
        assert_eq!(view.read(cx).preferences.follow_cli_theme, Some(true));
        assert_eq!(view.read(cx).composer.read(cx).value(), "Preserve this draft");
        assert_eq!(cx.theme().font_size, px(19.));
        assert_eq!(crate::desktop::preferences::Preferences::load().follow_cli_theme, Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_shortcut_import_preserves_unsaved_edits_and_explicit_unbinds(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.configuration = Some(
                serde_json::from_value(json!({
                    "provider": "ollama", "model": "local", "authenticated": true, "credentialSetupCommand": "",
                    "sharedUi": {"theme": "ciapre", "shortcuts": {"cancel": ["ctrl-c"], "model": []}}
                }))
                .unwrap(),
            );
            let _previous = view.shortcut_draft.insert("commands".into(), "ctrl-shift-k".into());
            view.shortcuts_editing = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        window.click("import-cli-shortcuts", cx);
        let saved = crate::desktop::preferences::Preferences::load();
        assert_eq!(saved.shortcuts.get("cancel").map(String::as_str), Some("ctrl-c"));
        assert_eq!(saved.shortcuts.get("model").map(String::as_str), Some(""));
        assert_eq!(saved.shortcuts.get("commands").map(String::as_str), Some("ctrl-shift-k"));
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("imported"));
        view.update(cx, |view, _| {
            let _previous = view
                .configuration
                .as_mut()
                .unwrap()
                .shared_ui
                .as_mut()
                .unwrap()
                .shortcuts
                .insert("cancel".into(), vec!["cmd-q".into()]);
        });
        window.render_frame(cx);
        window.click("import-cli-shortcuts", cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("reserved"));
        assert_eq!(
            view.read(cx).shortcut_draft.get("cancel").map(String::as_str),
            Some("ctrl-c"),
            "a rejected import keeps the staged draft"
        );
        assert_eq!(
            crate::desktop::preferences::Preferences::load()
                .shortcuts
                .get("cancel")
                .map(String::as_str),
            Some("ctrl-c")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn status_bar_is_opt_in_and_keyboard_toggle_persists_without_losing_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("status-bar").is_none());
        assert!(window.try_find("thread-actions").is_none(), "thread actions belong in the single header menu");
        assert!(window.try_find("attach-files").is_some(), "composer keeps its attach action");
        assert!(window.try_find("composer-mode").is_some(), "mode lives in the composer button row");
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("Keep this draft", window, cx))
        });
        window.press("cmd-shift-s", cx);
        window.render_frame(cx);
        assert!(window.try_find("status-bar").is_some());
        assert!(crate::desktop::preferences::Preferences::load().show_status_bar);
        window.press("cmd-,", cx);
        window.render_frame(cx);
        select_settings_category(window, 1, cx);
        window.click("show-status-bar", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.show_status_bar);
        assert!(!crate::desktop::preferences::Preferences::load().show_status_bar);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Keep this draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn composer_popups_align_above_the_input_and_preserve_the_draft(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 900., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(16);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.sidebar_open = false;
            view.inspector_open = false;
            view.composer
                .update(cx, |input, cx| input.set_value("Keep\nthis\ndraft", window, cx));
        });
        window.render_frame(cx);
        for button in ["composer-policy", "composer-mode", "pick-model"] {
            window.click(button, cx);
            window.render_frame(cx);
            let popup = window.find("command-palette").bounds();
            let composer = window.find("message-composer-frame").bounds();
            assert!(popup.bottom() <= composer.top(), "{button} popup must stay above the input");
            if button == "pick-model" {
                assert!((popup.right() - composer.right()).abs() <= px(1.));
            } else {
                assert!((popup.left() - composer.left()).abs() <= px(1.));
            }
            assert!(popup.left() >= px(12.) && popup.right() <= window.viewport_size().width - px(12.));
            window.press("escape", cx);
            window.render_frame(cx);
            assert!(view.read(cx).palette.is_none());
            assert_eq!(view.read(cx).composer.read(cx).value().as_str(), "Keep\nthis\ndraft");
        }
        // The attach button opens the tabbed picker; it aligns above the input
        // exactly like the palette popups.
        window.click("attach-files", cx);
        window.render_frame(cx);
        let picker = window.find("attachment-picker-trap").bounds();
        let composer = window.find("message-composer-frame").bounds();
        assert!(picker.bottom() <= composer.top(), "attach picker must stay above the input");
        assert!((picker.left() - composer.left()).abs() <= px(1.));
        assert!(picker.left() >= px(12.) && picker.right() <= window.viewport_size().width - px(12.));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_none());
        assert_eq!(view.read(cx).composer.read(cx).value().as_str(), "Keep\nthis\ndraft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn model_popup_follows_composer_above_growing_input(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 900., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.sidebar_open = false;
            view.inspector_open = false;
            view.open_model_palette(window, cx);
        });
        window.render_frame(cx);
        let initial_popup = window.find("command-palette").bounds();
        assert_eq!(initial_popup.size.width, px(392.));
        let composer = window.find("message-composer-frame").bounds();
        assert!(initial_popup.bottom() <= composer.top());
        assert!((initial_popup.right() - composer.right()).abs() <= px(1.));
        view.update(cx, |view, cx| {
            view.composer
                .update(cx, |input, cx| input.set_value("one\ntwo\nthree\nfour\nfive", window, cx));
        });
        window.render_frame(cx);
        let popup = window.find("command-palette").bounds();
        let composer = window.find("message-composer-frame").bounds();
        assert!(popup.bottom() <= composer.top(), "popup must remain above the growing composer");
        assert!(popup.top() < initial_popup.top());
        assert!(popup.left() >= px(12.) && popup.right() <= window.viewport_size().width - px(12.));

        window.press("escape", cx);
        view.update(cx, |view, cx| {
            view.preferences.set_ui_font_size(ThemeMode::Light, 24);
            view.apply_typography(cx);
            view.open_model_palette(window, cx);
        });
        window.render_frame(cx);
        let enlarged_popup = window.find("command-palette").bounds();
        assert_eq!(enlarged_popup.size.width, px(24.5 * 24.));
        assert!(enlarged_popup.left() >= px(12.));
        assert!(enlarged_popup.right() <= window.viewport_size().width - px(12.));

        window.press("escape", cx);
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        let quick_popup = window.find("command-palette").bounds();
        assert_eq!(quick_popup.size.width, px(22. * 24.));
        assert!(quick_popup.left() >= px(12.));
        assert!(quick_popup.right() <= window.viewport_size().width - px(12.));
    })
    .unwrap();
}

#[gpui_kit::test]
fn enlarged_model_popup_stays_inside_a_narrow_window(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 450., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.preferences.set_ui_font_size(ThemeMode::Light, 24);
            view.apply_typography(cx);
            view.configuration = Some(runtime_options_config());
            view.sidebar_open = false;
            view.inspector_open = false;
            view.open_model_palette(window, cx);
        });
        window.render_frame(cx);
        let popup = window.find("command-palette").bounds();
        let viewport = window.viewport_size();
        assert!(popup.left() >= px(12.), "popup left={:?}", popup.left());
        assert!(popup.right() <= viewport.width - px(12.), "popup right={:?}, viewport={viewport:?}", popup.right());
    })
    .unwrap();
}

#[gpui_kit::test]
fn mode_picker_uses_the_cli_catalog_and_sends_custom_primary_agents(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
        });
        window.render_frame(cx);
        window.click("composer-mode", cx);
        window.render_frame(cx);
        for (name, expected_label) in [
            ("auto", "Auto"),
            ("build", "Build"),
            ("plan", "Plan"),
            ("duck", "Duck"),
            ("coordinator", "Coordinator"),
        ] {
            let label = view
                .read(cx)
                .palette_groups
                .iter()
                .flat_map(|(_, entries)| entries)
                .find(|entry| {
                    matches!(
                        &entry.action,
                        super::super::PaletteAction::SelectSessionOptions(options)
                            if options.mode.as_deref() == Some(name)
                    )
                })
                .map(|entry| entry.label.as_str());
            assert_eq!(label, Some(expected_label), "CLI mode {name} should have a readable label");
        }
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
                "CLI mode {name} must be selectable"
            );
        }
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("Custom reviewer", cx);
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
        panic!("request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["mode"], "custom-reviewer");
}

#[gpui_kit::test]
fn shift_tab_cycles_primary_agent_and_defers_to_open_suggestions(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.selected_session_id = Some("session".to_string());
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("shift-tab", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("mode request") else {
        panic!("request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["mode"], "plan", "cycle moves build -> plan");
    cx.update_window(window, |_, window, cx| {
        // Second press while the select is in flight is dropped (mode-switch lock).
        window.press("shift-tab", cx);
        assert!(requests.try_recv().is_err(), "locked cycle sends nothing");
        // With suggestions open, Shift+Tab navigates instead of cycling.
        view.update(cx, |view, cx| {
            // Release the first request: without this the mode-switch lock
            // would mask a spurious cycle on the channel below.
            view.session_request = None;
            for (id, text) in [
                ("history-user-1", "Earlier prompt"),
                ("history-user-2", "Latest prompt"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "user".into(), "You".into(), text.into()));
            }
            view.focus_composer(window, cx);
        });
        window.render_frame(cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some());
        let before = view.read(cx).palette.as_ref().and_then(|state| state.read(cx).selected_index());
        window.press("shift-tab", cx);
        assert!(view.read(cx).palette.is_some(), "suggestion nav keeps the palette open");
        let after = view.read(cx).palette.as_ref().and_then(|state| state.read(cx).selected_index());
        assert_ne!(before, after, "shift-tab moved the suggestion highlight");
        assert!(requests.try_recv().is_err(), "suggestion nav never cycles the agent");
    })
    .unwrap();
}

#[gpui_kit::test]
fn shift_tab_on_multiline_draft_never_cycles_the_agent(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.selected_session_id = Some("session".to_string());
            view.composer.update(cx, |state, cx| {
                state.set_value("  indented\n  line", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("shift-tab", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "multiline shift-tab keeps Kit's native outdent");
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn agent_cycle_from_unknown_mode_starts_at_first_mode(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            let mut configuration = runtime_options_config();
            configuration.session_options.as_mut().expect("session options").mode = "zzz-unknown".to_string();
            view.configuration = Some(configuration);
            view.selected_session_id = Some("session".to_string());
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("shift-tab", cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("mode request") else {
        panic!("request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["mode"], "build", "unknown mode restarts at the first available mode");
}

#[gpui_kit::test]
fn locked_agent_cycle_surfaces_busy_status(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.selected_session_id = Some("session".to_string());
            view.composer.update(cx, |state, cx| {
                state.set_value("", window, cx);
                state.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("shift-tab", cx);
    })
    .unwrap();
    let _first = requests.try_recv().expect("first cycle requests");
    cx.update_window(window, |_, window, cx| {
        // Second press while the select is in flight: no request, busy notice instead.
        window.press("shift-tab", cx);
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "locked cycle sends nothing");
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).status, rust_i18n::t!("app.composer.mode_switch_busy").to_string());
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_permission_choices_persist_after_acknowledgement_and_restore_on_reconnect(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::PermissionMode;
    let workspace = tempfile::tempdir().expect("workspace");
    let key = workspace.path().to_string_lossy().into_owned();
    let preferences_path = isolate_preferences();
    let (window, view) = open_desktop_with_preferences_path(cx, preferences_path.clone());
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            let selected = view.select_session_options(
                vtcode_desktop_protocol::SessionOptionsSelectParams {
                    permission_mode: Some(PermissionMode::AutoApproveCommands),
                    ..Default::default()
                },
                cx,
            );
            assert!(selected);
            assert!(
                !crate::desktop::preferences::Preferences::load()
                    .workspace_permissions
                    .contains_key(&key),
                "unacknowledged choices are not persisted"
            );
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                    result: None,
                    error: Some("Rejected".into()),
                },
                cx,
            );
            assert!(view.pending_permission_choice.is_none());
            assert!(
                !crate::desktop::preferences::Preferences::load()
                    .workspace_permissions
                    .contains_key(&key)
            );
            assert!(view.select_session_options(
                vtcode_desktop_protocol::SessionOptionsSelectParams {
                    permission_mode: Some(PermissionMode::AutoApproveCommands),
                    ..Default::default()
                },
                cx
            ));
            let mut snapshot = runtime_options_config();
            snapshot.session_options.as_mut().unwrap().permission_mode = PermissionMode::AutoApproveCommands;
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                    result: Some(json!(snapshot)),
                    error: None,
                },
                cx,
            );
            assert_eq!(
                crate::desktop::preferences::Preferences::load().workspace_permissions.get(&key),
                Some(&PermissionMode::AutoApproveCommands)
            );
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_ok());
    assert!(requests.try_recv().is_ok());
    let (window, view) = open_desktop_with_preferences_path(cx, preferences_path);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.composer
                .update(cx, |input, cx| input.set_value("Keep this draft", window, cx));
            assert!(!view.workspace_permission_ready(), "saved policy is not ready before discovery");
            view.submit_prompt(window, cx);
            assert_eq!(view.composer.read(cx).value(), "Keep this draft");
            assert!(requests.try_recv().is_err(), "no session or turn starts before restoring saved permissions");
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_DISCOVER.into(),
                    result: Some(json!(runtime_options_config())),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.composer.read(cx).value(), "Keep this draft");
            assert_eq!(view.pending_permission_choice, Some(PermissionMode::AutoApproveCommands));
            assert!(!view.workspace_permission_ready());
        });
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().expect("restored policy request") else {
        panic!("request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["permissionMode"], "autoApproveCommands");
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.session_request = None;
            view.workspace = Some(workspace.path().join("other-workspace"));
            view.pending_permission_choice = None;
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_DISCOVER.into(),
                    result: Some(json!(runtime_options_config())),
                    error: None,
                },
                cx,
            );
            assert!(view.pending_permission_choice.is_none());
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "new workspaces keep CLI defaults");
}

#[gpui_kit::test]
fn reasoning_effort_options_accessible_label_tracks_the_cli_level(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
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
fn compact_model_picker_wraps_long_model_names_and_labels_the_current_effort(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    let model_name = "DeepSeek V4.1 Flash 1017 (Merge Gateway Compatible Route)";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            config.provider = "deepseek".into();
            config.model = "deepseek-v4.1-flash-1017".into();
            view.configuration = Some(config);
            view.models = vec![vtcode_desktop_protocol::ModelInfo {
                provider: "deepseek".into(),
                id: "deepseek-v4.1-flash-1017".into(),
                display_name: model_name.into(),
                context_window: None,
                current: true,
                recommended_rank: Some(0),
                capabilities: None,
            }];
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);

        let model_label = window.find("model-selected-name");
        assert_eq!(model_label.label(), Some(model_name));
        assert!(model_label.bounds().size.height > px(20.), "long names should wrap instead of truncate");
        assert_eq!(window.find("model-options-summary").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("model-effort-value").label(), Some("Current reasoning effort: medium"));
        let expected_summary = format!("{model_name} · medium");
        assert_eq!(window.find("model-options-summary").label(), Some(expected_summary.as_str()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn reasoning_effort_slider_subscriptions_follow_palette_lifecycle(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(1);
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

#[gpui_kit::test]
fn permission_picker_emits_typed_selection_and_preserves_draft_sources(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.selected_session_id = Some("old-session".into());
            view.attachments
                .push(vtcode_desktop_protocol::TurnAttachment { path: "source.rs".into(), text: "source".into() });
            view.composer
                .update(cx, |input, cx| input.set_value("Preserve my draft", window, cx));
        });
        window.render_frame(cx);
        window.click("composer-policy", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Command permissions"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.within("command-palette").press("down", cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["permissionMode"], "autoApproveCommands");
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            let mut config = runtime_options_config();
            config.session_options.as_mut().unwrap().permission_mode =
                vtcode_desktop_protocol::PermissionMode::AutoApproveCommands;
            view.handle_backend_event(BackendEvent::Response { method, result: Some(json!(config)), error: None }, cx);
        });
        assert_eq!(view.read(cx).composer.read(cx).value(), "Preserve my draft");
        assert_eq!(view.read(cx).attachments[0].path, "source.rs");
        assert_eq!(
            view.read(cx).selected_session_id.as_deref(),
            Some("old-session"),
            "runtime option change preserves the thread"
        );
        assert!(view.read(cx).session_request.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn quick_model_picker_omits_recommendations_and_keeps_full_search_visible(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 450., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.models = (0..12)
                .map(|rank| vtcode_desktop_protocol::ModelInfo {
                    provider: "recommended".into(),
                    id: format!("model-{rank}"),
                    display_name: format!("Recommended model {rank}"),
                    context_window: None,
                    current: false,
                    recommended_rank: Some(rank),
                    capabilities: None,
                })
                .collect();
            view.sidebar_open = false;
            view.inspector_open = false;
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);

        let popup = window.find("command-palette").bounds();
        let search_all = window.find("model-search-all");
        let bounds = search_all.bounds();
        assert_eq!(search_all.label(), Some("Search all models…"));
        assert!(bounds.top() >= popup.top(), "search CTA starts above the popup: {bounds:?} {popup:?}");
        assert!(bounds.bottom() <= popup.bottom(), "search CTA is clipped by the popup: {bounds:?} {popup:?}");
        let first_recommendation = palette_action_id(&super::super::PaletteAction::SelectModel {
            provider: "recommended".into(),
            model: "model-0".into(),
        });
        assert!(window.try_find(first_recommendation).is_none(), "quick selection does not list recommendations");
        assert!(view.read(cx).palette_groups.is_empty(), "model browsing is reserved for full search");

        window.click("model-search-all", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn quick_model_step_control_sends_supported_effort_and_full_search_remains_available(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.models = vec![vtcode_desktop_protocol::ModelInfo {
                provider: "custom".into(),
                id: "custom-route".into(),
                display_name: "Custom route".into(),
                context_window: None,
                current: false,
                recommended_rank: None,
                capabilities: None,
            }];
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Select model"));
        assert_eq!(window.find("command-palette").bounds().size.width, px(352.));
        assert_eq!(window.find("model-options-summary").label(), Some("gpt-5.6-sol · medium"));
        assert!(
            !view
                .read(cx)
                .palette_groups
                .iter()
                .flat_map(|(_, entries)| entries)
                .any(|entry| entry.label == "Custom route")
        );
        view.update(cx, |view, cx| view.effort_focus.focus(window, cx));
        window.press("right", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("request expected")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "high");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| view.session_request = None);
        window.click("pick-model", cx);
        window.render_frame(cx);
        let search_all = window.find("model-search-all");
        let search_all_bounds = search_all.bounds();
        let popup_bounds = window.find("command-palette").bounds();
        assert_eq!(search_all.label(), Some("Search all models…"));
        assert!(search_all_bounds.top() >= popup_bounds.top());
        assert!(search_all_bounds.bottom() <= popup_bounds.bottom());
        assert!(
            view.read(cx)
                .palette_groups
                .iter()
                .flat_map(|(_, entries)| entries)
                .all(|entry| !matches!(entry.action, super::super::PaletteAction::ChooseModel))
        );
        window.click("model-search-all", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Choose a model"));
        assert!(
            view.read(cx)
                .palette_groups
                .iter()
                .flat_map(|(_, entries)| entries)
                .any(|entry| entry.label == "Custom route")
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn compact_model_picker_shows_current_options_and_full_search_selects_the_real_route(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.models = vec![
                vtcode_desktop_protocol::ModelInfo {
                    provider: "anthropic".into(),
                    id: "claude-sonnet-5".into(),
                    display_name: "Claude Sonnet 5".into(),
                    context_window: Some(200_000),
                    current: false,
                    recommended_rank: Some(0),
                    capabilities: None,
                },
                vtcode_desktop_protocol::ModelInfo {
                    provider: "google".into(),
                    id: "gemini-3.8-flash".into(),
                    display_name: "Gemini 3.8 Flash".into(),
                    context_window: Some(1_000_000),
                    current: false,
                    recommended_rank: Some(3),
                    capabilities: Some(vtcode_desktop_protocol::ModelCapabilities {
                        reasoning: true,
                        vision: true,
                        tool_call: true,
                        structured_output: true,
                        is_pro: true,
                        input_modalities: vec![
                            "text".into(),
                            "image".into(),
                            "audio".into(),
                            "video".into(),
                            "pdf".into(),
                        ],
                    }),
                },
                vtcode_desktop_protocol::ModelInfo {
                    provider: "custom".into(),
                    id: "not-recommended".into(),
                    display_name: "Not recommended".into(),
                    context_window: None,
                    current: false,
                    recommended_rank: None,
                    capabilities: None,
                },
            ];
        });
        window.render_frame(cx);
        window.click("pick-model", cx);
        window.render_frame(cx);
        let claude_id = palette_action_id(&super::super::PaletteAction::SelectModel {
            provider: "anthropic".into(),
            model: "claude-sonnet-5".into(),
        });
        assert!(window.try_find(claude_id.clone()).is_none(), "model results stay out of the compact panel");
        let gemini_id = palette_action_id(&super::super::PaletteAction::SelectModel {
            provider: "google".into(),
            model: "gemini-3.8-flash".into(),
        });
        assert!(window.try_find(gemini_id.clone()).is_none(), "catalog choices appear in full search only");
        assert_eq!(window.find("model-selected-name").label(), Some("gpt-5.6-sol"));
        assert_eq!(window.find("model-effort-value").label(), Some("Current reasoning effort: medium"));
        assert!(view.read(cx).palette_groups.is_empty());
        let search_all = window.find("model-search-all");
        assert_eq!(search_all.label(), Some("Search all models…"));
        let search_all_bounds = search_all.bounds();
        let popup_bounds = window.find("command-palette").bounds();
        assert!(search_all_bounds.top() >= popup_bounds.top());
        assert!(search_all_bounds.bottom() <= popup_bounds.bottom());
        let uncatalogued_id = palette_action_id(&super::super::PaletteAction::SelectModel {
            provider: "custom".into(),
            model: "not-recommended".into(),
        });
        assert!(window.try_find(uncatalogued_id).is_none(), "uncurated catalog entries stay in full search");
        assert_eq!(window.find("fast-mode-toggle").label(), Some("Disable Ultrafast processing"));
        window.click("model-search-all", cx);
        window.render_frame(cx);
        assert!(
            window
                .find(claude_id)
                .label()
                .unwrap()
                .starts_with("Claude Sonnet 5 · Anthropic · 200K context")
        );
        let gemini_item = window.find(gemini_id.clone());
        let gemini_row = gemini_item.label().unwrap().to_owned();
        assert!(gemini_row.starts_with("Gemini 3.8 Flash · Gemini · Reasoning · Vision · Tools"));
        assert!(gemini_row.contains("Structured output · Pro · Audio · Video · PDF · 1M context"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("gemini", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).query(cx), "gemini");
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).matched_count(), 1);
        assert_eq!(window.find(gemini_id).label(), Some(gemini_row.as_str()));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected quick model selection request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SELECT);
    assert_eq!(params, json!({ "provider": "google", "model": "gemini-3.8-flash" }));
}
