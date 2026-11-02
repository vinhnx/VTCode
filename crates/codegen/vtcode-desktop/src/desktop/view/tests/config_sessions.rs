use super::*;

#[gpui_kit::test]
fn session_load_defers_startup_interaction_until_history_is_ready(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{InteractionKind, InteractionRequest};
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.start_event_drain(cx);
            view.selected_session_id = Some("old".into());
            view.session_request = Some(vtcode_desktop_protocol::method::SESSION_LOAD);
            view.event_tx.send(BackendEvent::Response { method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({ "session": { "sessionId": "restored", "title": null, "workspace": "/workspace", "completed": false, "resumable": true }, "messages": [] })), error: None }).unwrap();
            view.event_tx.send(BackendEvent::Interaction(InteractionRequest {
                session_id: "restored".into(), turn_id: "session-start".into(), interaction_id: "startup".into(),
                kind: InteractionKind::Question, wizard: false, title: "Question".into(), details: Vec::new(),
                choices: Vec::new(), steps: Vec::new(), selected_choice_id: None, selected_choice_ids: Vec::new(),
            })).unwrap();
            assert!(!view.drain_events(window, cx));
            assert!(view.session_load_task.is_some());
            assert!(view.interaction.is_none());
            assert!(!view.drain_events(window, cx), "the pending load must not consume later events");
        });
    }).unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, _cx| {
            assert_eq!(view.selected_session_id.as_deref(), Some("restored"));
            assert!(view.session_load_task.is_none());
            assert_eq!(
                view.interaction.as_ref().map(|interaction| interaction.interaction_id.as_str()),
                Some("startup")
            );
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn malformed_session_load_preserves_the_current_conversation(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.selected_session_id = Some("current-session".to_string());
        view.workspace = Some(PathBuf::from("/current-workspace"));
        view.session_request = Some(vtcode_desktop_protocol::method::SESSION_LOAD);
        view.transcript.push(TranscriptItem::new(
            "current-message".into(),
            "assistant".into(),
            "Assistant".into(),
            "Keep this conversation".into(),
        ));
    });

    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.to_string(),
                result: Some(json!({
                    "session": {
                        "sessionId": "   ",
                        "title": "Invalid session",
                        "workspace": "/replacement-workspace",
                        "completed": false,
                        "resumable": true,
                    },
                    "messages": [{ "role": "assistant", "text": "Do not replace history" }],
                })),
                error: None,
            },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let view = view.read(cx);
        assert_eq!(view.selected_session_id.as_deref(), Some("current-session"));
        assert_eq!(view.workspace.as_deref(), Some(Path::new("/current-workspace")));
        assert_eq!(view.transcript.len(), 1);
        assert_eq!(view.transcript.get(0).unwrap().text, "Keep this conversation");
        assert_eq!(view.error.as_deref(), Some("The local service returned an invalid session load result."));
    });
    cx.update_window(window, |_, window, cx| window.render_frame(cx)).unwrap();
}

#[gpui_kit::test]
fn disconnected_session_load_cannot_restore_stale_history(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.session_request = Some(vtcode_desktop_protocol::method::SESSION_LOAD);
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                result: Some(json!({
                    "session": { "sessionId": "stale", "title": "Stale", "workspace": "/workspace", "completed": true, "resumable": true },
                    "messages": [{ "role": "assistant", "text": "Must not be restored after disconnect" }],
                })),
                error: None,
            },
            cx,
        );
        view.handle_backend_event(
            BackendEvent::Exited { status: None, authentication_required: false },
            cx,
        );
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let view = view.read(cx);
        assert!(view.transcript.is_empty());
        assert!(view.selected_session_id.is_none());
        assert!(view.session_request.is_none());
    });
}

#[gpui_kit::test]
fn settings_dialog_copies_the_credential_setup_command(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_DISCOVER.to_string(),
                    result: Some(json!({
                        "provider": "ollama",
                        "model": "gpt-oss:20b",
                        "authenticated": true,
                        "credentialSetupCommand": "vtcode login ollama",
                    })),
                    error: None,
                },
                cx,
            );
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::INITIALIZE.to_string(),
                    result: Some(json!({
                        "protocol": vtcode_desktop_protocol::PROTOCOL_VERSION,
                        "workspace": std::env::temp_dir().display().to_string(),
                        "serverVersion": "0.180.5",
                    })),
                    error: None,
                },
                cx,
            );
        });
        window.render_frame(cx);
        window.click("open-settings", cx);
        window.render_frame(cx);
        select_settings_category(window, 3, cx);
        window.render_frame(cx);
        assert_eq!(window.find("provider-details").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("provider-details").label(), Some("Provider and model configuration"));
        assert_eq!(window.find("provider-settings-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("cli-preferences-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("cli-configuration-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(
            window.find("config-target-workspace").label(),
            Some("Write CLI settings to Workspace configuration (selected)")
        );
        select_settings_category(window, 4, cx);
        window.render_frame(cx);
        assert_eq!(window.find("local-service-settings-group").role(), Some(gpui_kit::Role::Group));
        assert_eq!(window.find("local-service-details").label(), Some("Local service connection details"));
        select_settings_category(window, 3, cx);
        window.render_frame(cx);
        window.click("copy-credential-command", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("vtcode login ollama"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn escape_closes_cli_config_editor_before_the_settings_page(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![vtcode_desktop_protocol::ConfigSetting {
                    path: "agent.default_model".into(),
                    value: json!("model-a"),
                    origin: Some("workspace".into()),
                    label: Some("Default model".into()),
                    type_name: Some("string".into()),
                    default_value: None,
                    description: Some("Model used for new conversations.".into()),
                    options: Vec::new(),
                    group: Some("Model & Provider".into()),
                    group_description: None,
                    editable: true,
                }],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
            view.open_settings(window, cx);
            view.open_cli_config_editor("agent.default_model".into(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.simulate_next_frame(cx) > 0);
        assert!(view.read(cx).config_editor.is_some());
        assert!(
            view.read(cx).config_value_input.read(cx).focus_handle(cx).is_focused(window),
            "free-form values receive initial focus"
        );
        view.update(cx, |view, cx| {
            view.config_write_pending = true;
            view.handle_backend_event(BackendEvent::Exited { status: Some(1), authentication_required: false }, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("cli-config-save-error").role(), Some(gpui_kit::Role::Alert));
        assert!(
            window
                .find("cli-config-save-error")
                .label()
                .unwrap()
                .contains("outcome is unknown")
        );
        assert_ne!(window.find("cancel-cli-config-edit").disabled(), Some(true));
        assert_eq!(window.find("save-cli-config-edit").disabled(), Some(true));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_none());
        assert!(view.read(cx).current_route() == Route::Settings);
        assert!(view.read(cx).settings_focus.contains_focused(window, cx));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Settings);
    })
    .unwrap();
}

#[gpui_kit::test]
fn settings_surface_close_clears_the_editor_first_and_never_during_a_write(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![vtcode_desktop_protocol::ConfigSetting {
                    path: "agent.default_model".into(),
                    value: json!("model-a"),
                    origin: Some("workspace".into()),
                    label: Some("Default model".into()),
                    type_name: Some("string".into()),
                    default_value: None,
                    description: Some("Model used for new conversations.".into()),
                    options: Vec::new(),
                    group: Some("Model & Provider".into()),
                    group_description: None,
                    editable: true,
                }],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
            view.open_settings(window, cx);
            view.open_cli_config_editor("agent.default_model".into(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.simulate_next_frame(cx) > 0);
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_some(), "editor state must be set");
        assert!(window.try_find("cancel-cli-config-edit").is_some());

        // A write in flight pins the editor: backdrop/dismiss must wait.
        view.update(cx, |view, cx| {
            view.config_write_pending = true;
            assert!(view.go_route_back(window, cx));
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_some());
        assert!(view.read(cx).current_route() == Route::Settings);

        // Idle editor closes first, settings stays for the next decision.
        view.update(cx, |view, cx| {
            view.config_write_pending = false;
            assert!(view.go_route_back(window, cx));
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_none());
        assert!(window.try_find("cancel-cli-config-edit").is_none());
        assert!(view.read(cx).current_route() == Route::Settings);

        // With no editor, the same path backs out of the page.
        view.update(cx, |view, cx| assert!(view.go_route_back(window, cx)));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Settings);
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_config_enum_editor_uses_kit_select_and_writes_the_selected_option(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![vtcode_desktop_protocol::ConfigSetting {
                    path: "provider.openai.service_tier".into(),
                    value: json!("auto"),
                    origin: Some("user".into()),
                    label: Some("Service tier".into()),
                    type_name: Some("string".into()),
                    default_value: None,
                    description: Some("Choose the request service tier.".into()),
                    options: vec!["auto".into(), "fast".into()],
                    group: Some("Model & Provider".into()),
                    group_description: None,
                    editable: true,
                }],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
            view.open_settings(window, cx);
            view.open_cli_config_editor("provider.openai.service_tier".into(), window, cx);
        });
        window.render_frame(cx);
        assert!(window.simulate_next_frame(cx) > 0);
        assert_eq!(window.find("cli-config-option-select").label(), Some("Value for provider.openai.service_tier"));
        let select = view
            .read(cx)
            .config_editor
            .as_ref()
            .and_then(|editor| editor.option_select.clone())
            .expect("enum settings use a Kit select");
        assert!(select.read(cx).focus_handle(cx).is_focused(window), "the enum picker receives initial focus");
        window.within("cli-config-option-select").click("input", cx);
        window.render_frame(cx);
        assert_eq!(window.find("config-option-auto").label(), Some("auto"));
        assert_eq!(window.find("config-option-fast").label(), Some("fast"));
        window.press("down", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        let select = view
            .read(cx)
            .config_editor
            .as_ref()
            .and_then(|editor| editor.option_select.clone())
            .expect("enum settings retain their Kit select state");
        assert_eq!(select.read(cx).selected_value().map(String::as_str), Some("fast"));
        window.click("save-cli-config-edit", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("config write request") else {
        panic!("expected config write request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["path"], "provider.openai.service_tier");
    assert_eq!(params["value"], "fast");
}

#[gpui_kit::test]
fn sync_cli_theme_persists_through_the_protocol_and_preserves_composer_draft(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(directory.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("session-live".to_string());
            view.theme_id = "ciapre-blue".into();
            view.configuration = serde_json::from_value(json!({
                "provider": "ollama",
                "model": "gpt-oss:20b",
                "authenticated": true,
                "credentialSetupCommand": "vtcode login ollama",
                "sharedUi": { "theme": "ciapre", "shortcuts": {}, "shortcutWarnings": [] }
            }))
            .ok();
            view.composer
                .update(cx, |state, cx| state.set_value("Keep this draft", window, cx));
            assert!(view.can_sync_theme_to_cli(), "ordinary composer text remains safe during theme sync");
            let submission = || super::super::SubmissionDraft {
                prompt: "Do not lose this message".into(),
                attachments: Vec::new(),
                echo_id: "pending-echo".into(),
                delivery_uncertain: false,
            };
            view.pending_submission = Some(submission());
            assert!(!view.can_sync_theme_to_cli(), "a queued send must be resolved first");
            view.pending_submission = None;
            view.first_submission = Some(submission());
            assert!(!view.can_sync_theme_to_cli(), "a first-session send must be resolved first");
            view.first_submission = None;
            view.retained_submission = Some(submission());
            assert!(!view.can_sync_theme_to_cli(), "an uncertain send must be restored or dismissed first");
            view.retained_submission = None;
            view.attachments_picker_open = true;
            assert!(!view.can_sync_theme_to_cli(), "an active attachment picker must finish first");
            view.attachments_picker_open = false;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 3, cx);
        window.click("sync-cli-theme", cx);
        let BackendCommand::Request { method, params } = requests.try_recv().expect("theme request") else {
            panic!("expected a theme request");
        };
        assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_THEME);
        assert_eq!(params, json!({ "theme": "ciapre-blue" }));
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_THEME.into(),
                    result: Some(json!({
                        "provider": "ollama", "model": "gpt-oss:20b", "authenticated": true,
                        "credentialSetupCommand": "vtcode login ollama",
                        "sharedUi": { "theme": "ciapre-blue", "shortcuts": {}, "shortcutWarnings": [] }
                    })),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.theme_id, "ciapre-blue");
            assert_eq!(view.preferences.follow_cli_theme, Some(true));
            assert_eq!(view.composer.read(cx).value().as_str(), "Keep this draft");
            assert_eq!(view.selected_session_id.as_deref(), Some("session-live"), "theme sync preserves the thread");
            assert_eq!(view.status, "CLI theme saved.");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn sync_cli_theme_maps_system_sentinels_to_builtin_defaults(cx: &mut TestAppContext) {
    use super::super::themes;
    let directory = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let configuration = |theme: &str| {
        serde_json::from_value(json!({
            "provider": "ollama",
            "model": "gpt-oss:20b",
            "authenticated": true,
            "credentialSetupCommand": "vtcode login ollama",
            "sharedUi": { "theme": theme, "shortcuts": {}, "shortcutWarnings": [] }
        }))
        .ok()
    };
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(directory.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.configuration = configuration("vitesse-dark");
            for (system_id, expected) in [
                (themes::SYSTEM_LIGHT_ID, themes::default_id(ThemeMode::Light)),
                (themes::SYSTEM_DARK_ID, themes::default_id(ThemeMode::Dark)),
            ] {
                view.theme_id = system_id.into();
                assert!(view.can_sync_theme_to_cli(), "{system_id} must sync as its per-mode default");
                view.sync_theme_to_cli(cx);
                let BackendCommand::Request { method, params } = requests.try_recv().expect("theme request") else {
                    panic!("expected a theme request");
                };
                assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_THEME);
                assert_eq!(params, json!({ "theme": expected }));
                view.session_request = None;
            }
            view.theme_id = themes::SYSTEM_DARK_ID.into();
            view.configuration = configuration(themes::default_id(ThemeMode::Dark));
            assert!(!view.can_sync_theme_to_cli(), "a CLI already on the mapped default needs no sync");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn superseded_palette_confirmation_does_not_execute_or_dismiss_the_new_popup(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.open_command_palette(window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        let focus = window.focused(cx).expect("palette input focus");
        focus.dispatch_action(&gpui_kit::base::actions::Confirm { secondary: false }, window, cx);
        view.update(cx, |view, cx| view.open_modes(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "a stale confirmation cannot close the replacement palette");
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::Modes));
        assert_eq!(window.find("command-palette").label(), Some("Agent mode"));
        assert!(requests.try_recv().is_err(), "the old New session action must not execute");
    })
    .unwrap();
}

#[gpui_kit::test]
fn command_palette_confirms_the_highlighted_command(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(std::env::temp_dir());
            view.backend = Some(BackendHandle { commands });
            view.open_command_palette(window, cx);
            assert!(view.palette.is_some(), "palette state exists");
            assert_eq!(view.palette_groups[0].0, "Session");
            assert_eq!(view.palette_groups[0].1[0].label, "New session");
        });
        window.render_frame(cx);
        // Focus after the first render, matching the kit's draw-focus-draw
        // pattern (the palette installs its input state during render).
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(window.find("command-palette").label(), Some("Command palette"));
        assert_eq!(
            window.find(palette_action_id(&super::super::PaletteAction::NewSession)).label(),
            Some("New session")
        );
        window.input("new", cx);
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).query(cx), "new");
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).palette_query.as_ref(), "new");
        assert_eq!(
            window.find(palette_action_id(&super::super::PaletteAction::NewSession)).label(),
            Some("New session")
        );
        for key in ["tab", "shift-tab", "tab"] {
            window.press(key, cx);
            assert!(view.read(cx).palette_focus.contains_focused(window, cx), "{key} stays inside the palette");
        }
        // Enter matches the palette's Command-context Confirm binding; the
        // confirm callback itself is deferred by the kit.
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert!(view.read(cx).palette.is_none(), "confirm closes the palette");
        assert!(view.read(cx).palette_query.is_empty(), "closing resets the search highlight");
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "a blank thread does not start a runtime");
}

#[gpui_kit::test]
fn command_and_add_context_palettes_follow_the_selected_language(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let language = crate::desktop::localization::UiLanguage::Spanish;
    crate::desktop::localization::set_language(language);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.set_ui_language(language);
            view.open_command_palette(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Paleta de comandos"));
        assert_eq!(view.read(cx).palette_groups[0].0, "Sesión");
        assert_eq!(view.read(cx).palette_groups[0].1[0].label, "Nueva sesión");

        view.update(cx, |view, cx| view.close_palette(window, cx));
        window.render_frame(cx);
        // The attach button opens the tabbed card picker.
        window.click("attach-files", cx);
        window.render_frame(cx);
        assert!(window.try_find("picker-tab-workspace").is_some(), "attach opens the picker");
        view.update(cx, |view, cx| view.close_attachment_picker(window, cx));
        window.render_frame(cx);
        // The AddContext palette remains reachable through its action (@ / ⌘⇧A).
        window.dispatch_action(Box::new(super::super::OpenAddContext), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Añadir archivos y más @"));
        let palette_groups = view.read(cx).composer_resource_groups();
        assert_eq!(palette_groups[0].0, "Añadir");
        assert_eq!(palette_groups[0].1[0].label, "Archivos…");
    })
    .unwrap();
    crate::desktop::localization::set_language(crate::desktop::localization::UiLanguage::English);
}

#[gpui_kit::test]
fn unauthenticated_discover_keeps_service_connected_with_reload_guidance(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(std::env::temp_dir());
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_DISCOVER.to_string(),
                    result: Some(json!({
                        "provider": "meta",
                        "model": "muse-spark",
                        "authenticated": false,
                        "credentialSetupCommand": "vtcode secret add meta",
                        "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
                    })),
                    error: None,
                },
                cx,
            );
            assert!(
                view.configuration
                    .as_ref()
                    .is_some_and(|configuration| !configuration.authenticated),
                "the snapshot still lands while unauthenticated"
            );
            let error = view.error.as_deref().expect("auth guidance");
            assert!(error.contains("vtcode secret add meta"));
            assert!(error.contains("Reload CLI settings"));
            assert!(!error.contains("restart the desktop app"));
            assert!(view.can_reload_cli_settings(), "reload recovers without an app restart");
        });
        window.render_frame(cx);
        let error = window.find("composer-error");
        assert_eq!(error.role(), Some(gpui_kit::Role::Alert));
        assert!(error.label().unwrap().contains("Reload CLI settings"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn submit_without_credentials_keeps_draft_and_points_to_reload(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(std::env::temp_dir());
            view.configuration = Some(
                serde_json::from_value(json!({
                    "provider": "meta",
                    "model": "muse-spark",
                    "authenticated": false,
                    "credentialSetupCommand": "vtcode secret add meta",
                    "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
                }))
                .unwrap(),
            );
            view.composer
                .update(cx, |state, cx| state.set_value("Drafted while logged out", window, cx));
            view.submit_prompt(window, cx);
            assert!(!view.running);
            assert!(view.session_request.is_none(), "no session is created for a doomed turn");
            assert!(view.first_submission.is_none());
            assert!(view.transcript.is_empty());
            assert_eq!(view.composer.read(cx).value(), "Drafted while logged out");
            let error = view.error.as_deref().expect("auth guidance");
            assert!(error.contains("vtcode secret add meta"));
            assert!(error.contains("Reload CLI settings"));
        });
        window.render_frame(cx);
        assert!(window.find("composer-error").label().unwrap().contains("Reload CLI settings"));
    })
    .unwrap();
    assert!(requests.try_recv().is_err(), "no request leaves the client without credentials");
}

#[gpui_kit::test]
fn deferred_submission_rechecks_credentials_before_sending(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(1);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some("session".into());
        view.configuration = Some(
            serde_json::from_value(json!({
                "provider": "meta",
                "model": "muse-spark",
                "authenticated": false,
                "credentialSetupCommand": "vtcode secret add meta",
                "secretStore": {"provider": "meta", "envVar": "META_API_KEY"},
            }))
            .unwrap(),
        );
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let draft = super::super::SubmissionDraft {
                prompt: "Deferred past a credential revocation".into(),
                attachments: Vec::new(),
                echo_id: "deferred-echo".into(),
                delivery_uncertain: false,
            };
            assert!(view.enqueue_submission(draft, cx).is_err());
            assert!(!view.running);
            assert!(view.transcript.is_empty());
            assert!(view.error.as_deref().is_some_and(|error| error.contains("Reload CLI settings")));
        });
        window.render_frame(cx);
    })
    .unwrap();
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn model_switch_response_preserves_the_thread_and_applies_the_snapshot(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.selected_session_id = Some("session-live".to_string());
            view.open_tabs = vec!["session-live".to_string()];
            view.transcript.push(TranscriptItem::new(
                "kept-message".into(),
                "assistant".into(),
                "VT Code".into(),
                "Conversation remains visible.".into(),
            ));
            let mut configuration = runtime_options_config();
            configuration.provider = "ollama".into();
            configuration.model = "gpt-oss:20b".into();
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SELECT.into(),
                    result: Some(json!(configuration)),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.selected_session_id.as_deref(), Some("session-live"), "model switch preserves the thread");
            assert_eq!(view.open_tabs, vec!["session-live".to_string()], "tabs survive the change");
            assert_eq!(view.transcript.get(0).unwrap().text, "Conversation remains visible.");
            assert!(view.transcript.get(1).is_none(), "no transcript row is added or dropped");
            let applied = view.configuration.as_ref().unwrap();
            assert_eq!(applied.provider, "ollama");
            assert_eq!(applied.model, "gpt-oss:20b");
            assert_eq!(view.status, "Switched to gpt-oss:20b.");
            assert!(view.error.is_none());
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn model_switch_invalid_snapshot_keeps_configuration_and_reports_error(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.selected_session_id = Some("session-live".to_string());
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SELECT.into(),
                    result: Some(json!({"bogus": true})),
                    error: None,
                },
                cx,
            );
            // A malformed snapshot must never silently clear the running
            // configuration: keep the old snapshot, keep the thread, and say
            // what went wrong (CONFIG_OPTIONS parity).
            assert_eq!(view.configuration.as_ref().unwrap().model, "gpt-5.6-sol");
            assert_eq!(view.selected_session_id.as_deref(), Some("session-live"));
            assert!(view.error.is_some());
            assert!(view.backend.is_none(), "protocol desync disconnects like the options arm");
        });
    })
    .unwrap();
}
