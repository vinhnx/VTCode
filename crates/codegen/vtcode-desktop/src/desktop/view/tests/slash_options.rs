use super::*;

#[gpui_kit::test]
fn composer_model_summary_refreshes_effort_and_processing_tier_after_cli_acknowledgement(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 450., 800.);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.sidebar_open = false;
            view.inspector_open = false;
            let mut configuration = runtime_options_config();
            let options = configuration.session_options.as_mut().unwrap();
            options.speed = Some("ULTRAFAST".into());
            options.fast_mode_label = Some("ULTRAFAST".into());
            view.configuration = Some(configuration);
        });
        window.render_frame(cx);
        assert_eq!(window.find("pick-model").label(), Some("gpt-5.6-sol · Medium · Ultrafast"));
        assert_eq!(window.find("composer-model-speed").label(), Some("Processing speed: Ultrafast"));
        let button_bounds = window.find("pick-model").bounds();
        for id in ["composer-model-effort", "composer-model-speed"] {
            let bounds = window.find(id).bounds();
            assert!(
                bounds.left() >= button_bounds.left() && bounds.right() <= button_bounds.right(),
                "{id} must remain visible in a narrow composer"
            );
        }
        window.click("pick-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("fast-mode-toggle").label(), Some("Disable Ultrafast processing"));

        let mut configuration = runtime_options_config();
        let options = configuration.session_options.as_mut().unwrap();
        options.reasoning_effort = "high".into();
        options.speed = Some("fast".into());
        options.fast_mode_label = Some("FAST".into());
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                    result: Some(json!(configuration)),
                    error: None,
                },
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("pick-model").label(), Some("gpt-5.6-sol · High · Fast"));
        assert_eq!(window.find("composer-model-effort").label(), Some("Reasoning effort: High"));
        assert_eq!(window.find("composer-model-speed").label(), Some("Processing speed: Fast"));

        let options = configuration.session_options.as_mut().unwrap();
        options.speed = None;
        options.fast_mode_enabled = false;
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_OPTIONS.into(),
                    result: Some(json!(configuration)),
                    error: None,
                },
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("pick-model").label(), Some("gpt-5.6-sol · High"));
        assert!(window.try_find("composer-model-speed").is_none(), "disabled processing tiers must disappear");
    })
    .unwrap();
}

#[gpui_kit::test]
fn fast_mode_toggle_sends_cli_option_and_is_exposed_as_a_toggle(cx: &mut TestAppContext) {
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
        let toggle = window.find("fast-mode-toggle");
        assert_eq!(toggle.label(), Some("Disable Ultrafast processing"));
        assert_eq!(toggle.checked(), Some(true));
        assert_eq!(toggle.role(), Some(gpui_kit::Role::Button));
        window.click("fast-mode-toggle", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("fast-mode request") else {
        panic!("expected runtime option request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["fastMode"], false);
}

#[test]
fn model_picker_includes_catalog_capabilities_and_search_keywords() {
    let model = vtcode_desktop_protocol::ModelInfo {
        provider: "google".into(),
        id: "gemini-special".into(),
        display_name: "Gemini Special".into(),
        context_window: Some(1_000_000),
        current: false,
        recommended_rank: None,
        capabilities: Some(vtcode_desktop_protocol::ModelCapabilities {
            reasoning: true,
            vision: true,
            tool_call: true,
            structured_output: true,
            is_pro: true,
            input_modalities: vec!["text".into(), "audio".into(), "video".into(), "pdf".into()],
        }),
    };
    let command = super::super::model_palette_command(&model, false);
    let description = command.description.as_deref().unwrap();
    assert!(
        description.contains("Reasoning · Vision · Tools · Structured output · Pro · Audio · Video · PDF · 1M context")
    );
    assert!(command.keywords.contains(&"gemini-special".into()));
}

#[gpui_kit::test]
fn add_popup_filters_cli_resources_and_inserts_a_skill_without_submitting(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
        });
        window.render_frame(cx);
        // The AddContext palette opens through its action; the attach button
        // opens the tabbed card picker (covered in attachment_picker tests).
        window.dispatch_action(Box::new(super::super::OpenAddContext), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Add files and more @"));
        view.update(cx, |view, cx| {
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::COMPOSER_RESOURCES.into(),
                    error: None,
                    result: Some(json!([
                        {"name":"rust-skills", "description":"Rust implementation guidance", "kind":"skill"},
                        {"name":"web-tools", "description":"Installed web plugin", "kind":"plugin"}
                    ])),
                },
                cx,
            );
            view.focus_palette_state(window, cx);
            cx.notify();
        });
        window.render_frame(cx);
        window.within("command-palette").input("rust-skills", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).composer_resources.len(), 2, "resource response: {:?}", view.read(cx).error);
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::InsertDraft("$rust-skills".into())))
                .is_some(),
            "skill row should be rendered"
        );
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).query(cx), "rust-skills");
        assert!(view.read(cx).palette.as_ref().unwrap().read(cx).selected_index().is_some());
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, .. } = requests.try_recv().unwrap() else {
        panic!("resource request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::COMPOSER_RESOURCES);
    assert!(requests.try_recv().is_err(), "choosing a mention cannot submit a turn");
    cx.update(|cx| assert_eq!(view.read(cx).composer.read(cx).value(), "$rust-skills "));
}

#[gpui_kit::test]
fn composer_supports_cli_file_mentions_and_slash_command_completion(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("composer-session".into());
            view.files = vec![
                FileEntry {
                    rel_path: "src/main.rs".into(),
                    depth: 1,
                    is_dir: false,
                },
                FileEntry {
                    rel_path: "docs/guide.md".into(),
                    depth: 1,
                    is_dir: false,
                },
                FileEntry {
                    rel_path: "docs/guide notes.md".into(),
                    depth: 1,
                    is_dir: false,
                },
                FileEntry { rel_path: "src".into(), depth: 1, is_dir: true },
            ];
            view.slash_commands = vec![
                vtcode_desktop_protocol::SlashCommandInfo {
                    name: "review".into(),
                    description: "Review current changes".into(),
                },
                vtcode_desktop_protocol::SlashCommandInfo {
                    name: "redo".into(),
                    description: "Restore the last checkpoint".into(),
                },
            ];
            view.focus_composer(window, cx);
            view.composer.update(cx, |input, cx| {
                input.set_value("Inspect @src/", window, cx);
                input.set_cursor_position(gpui_kit::component::input::Position::new(0, 13), window, cx);
            });
            view.update_composer_suggestions(window, cx);
        });
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::FileMentions));
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::InsertFileMention("src/main.rs".into())))
                .is_some()
        );
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::InsertFileMention("docs/guide.md".into())))
                .is_none()
        );
        window.press("enter", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "Inspect @src/main.rs ");
        assert!(view.read(cx).palette.is_none(), "selecting a file reference only completes the draft");
        assert!(requests.try_recv().is_err(), "mention completion never submits a turn");

        view.update(cx, |view, cx| {
            view.composer.update(cx, |input, cx| {
                input.set_value("context\n@notes", window, cx);
                input.set_cursor_position(gpui_kit::component::input::Position::new(1, 6), window, cx);
            });
            view.update_composer_suggestions(window, cx);
        });
        window.render_frame(cx);
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::InsertFileMention(
                    "docs/guide notes.md".into()
                )))
                .is_some()
        );
        window.press("enter", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "context\n@\"docs/guide notes.md\" ");
        assert!(requests.try_recv().is_err(), "a quoted mention remains a draft edit");

        // Moving vertically to an existing mention updates autocomplete without changing the text.
        view.update(cx, |view, cx| {
            view.composer.update(cx, |input, cx| {
                input.set_value("context\n@src/", window, cx);
                input.set_cursor_position(gpui_kit::component::input::Position::new(0, 7), window, cx);
            });
            view.update_composer_suggestions(window, cx);
        });
        window.render_frame(cx);
        window.press("down", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::FileMentions));

        view.update(cx, |view, cx| {
            view.composer.update(cx, |input, cx| {
                input.set_value("/", window, cx);
                input.set_cursor_position(gpui_kit::component::input::Position::new(0, 1), window, cx);
            });
            view.update_composer_suggestions(window, cx);
        });
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::SlashCommands));
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::CompleteSlashCommand("review".into())))
                .is_some()
        );
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::CompleteSlashCommand("redo".into())))
                .is_some()
        );

        // Arrow selection stays in the composer and Tab completes without running.
        window.press("down", cx);
        window.render_frame(cx);
        window.press("tab", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "/redo ");
        assert!(view.read(cx).palette.is_none());
        assert!(requests.try_recv().is_err(), "Tab completion is an edit, not a submission");

        // Plain Enter follows CLI behavior: complete the selected slash command and submit it.
        view.update(cx, |view, cx| {
            view.composer.update(cx, |input, cx| {
                input.set_value("/rev", window, cx);
                input.set_cursor_position(gpui_kit::component::input::Position::new(0, 4), window, cx);
            });
            view.update_composer_suggestions(window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            assert!(view.move_composer_suggestion(false, window, cx));
        });
        window.press("enter", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        assert!(view.read(cx).running);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("slash command should submit through the existing turn path");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_SUBMIT);
    assert_eq!(params, json!({ "sessionId": "composer-session", "prompt": "/review ", "attachments": [] }));
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn unavailable_theme_slash_command_is_reported_without_submitting_prompt(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("theme-session".into());
            view.composer
                .update(cx, |input, cx| input.set_value("/theme not-a-real-theme", window, cx));
            view.submit_prompt(window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();

    assert!(requests.try_recv().is_err(), "invalid theme identifiers stay out of the model prompt");
    cx.update(|cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "");
        assert!(
            view.read(cx)
                .error
                .as_deref()
                .is_some_and(|message| message.contains("not-a-real-theme"))
        );
    });
}

#[gpui_kit::test]
fn slash_files_opens_native_file_mentions_without_submitting_a_turn(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.files = vec![FileEntry {
                rel_path: "src/main.rs".into(),
                depth: 1,
                is_dir: false,
            }];
            view.composer.update(cx, |input, cx| input.set_value("/files src/", window, cx));
            view.submit_prompt(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).composer.read(cx).value(), "@src/");
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::FileMentions));
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::InsertFileMention("src/main.rs".into())))
                .is_some()
        );
        assert!(requests.try_recv().is_err(), "file browsing must not enqueue a model turn");
    })
    .unwrap();
}

#[gpui_kit::test]
fn slash_files_refreshes_suggestions_after_workspace_scan(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("workspace");
    std::fs::create_dir_all(workspace.path().join("src")).expect("create source directory");
    std::fs::write(workspace.path().join("src/main.rs"), "fn main() {}").expect("write source file");

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.workspace = Some(workspace.path().to_path_buf());
            view.composer.update(cx, |input, cx| input.set_value("/files src/", window, cx));
            view.submit_prompt(window, cx);
            assert!(view.files.is_empty(), "workspace scan stays off the UI callback");
            assert_eq!(view.composer.read(cx).value(), "@src/");
        });
        window.render_frame(cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).files.iter().any(|entry| entry.rel_path == "src/main.rs"));
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::FileMentions));
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::InsertFileMention("src/main.rs".into())))
                .is_some(),
            "scan completion refreshes the active file mention results"
        );
        assert!(requests.try_recv().is_err(), "opening /files never submits a model turn");
    })
    .unwrap();
}

#[gpui_kit::test]
fn slash_history_opens_recent_thread_prompts_without_submitting(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    let previous_prompt = "Review stale diffs\nUse the current branch.";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("history-session".into());
            view.transcript.push(TranscriptItem::new(
                "history-user-1".into(),
                "user".into(),
                "You".into(),
                previous_prompt.into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "history-assistant".into(),
                "assistant".into(),
                "VT Code".into(),
                "I reviewed the changes.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "history-user-2".into(),
                "user".into(),
                "You".into(),
                previous_prompt.into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "history-user-3".into(),
                "user".into(),
                "You".into(),
                "Implement current changes".into(),
            ));
            view.focus_composer(window, cx);
            view.composer.update(cx, |input, cx| input.set_value("/history", window, cx));
            view.submit_prompt(window, cx);
        });
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::PromptHistory));
        assert_eq!(view.read(cx).palette_groups[0].1.len(), 2, "duplicate prompts are collapsed");
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::InsertHistoryPrompt(previous_prompt.into())))
                .is_some()
        );
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::InsertHistoryPrompt(
                    "I reviewed the changes.".into()
                )))
                .is_none(),
            "assistant replies are not shown as prompt history"
        );
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.press("down", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), previous_prompt);
        assert!(view.read(cx).palette.is_none());
        assert!(requests.try_recv().is_err(), "choosing history only restores a draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_r_opens_prefiltered_prompt_history_with_toggle_and_restore(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("history-session".into());
            for (id, text) in [
                ("history-user-1", "Review stale diffs"),
                ("history-user-2", "Implement current changes"),
            ] {
                view.transcript
                    .push(TranscriptItem::new(id.into(), "user".into(), "You".into(), text.into()));
            }
            view.composer.update(cx, |input, cx| {
                input.set_value("stale", window, cx);
                input.focus(window, cx);
            });
        });
        window.render_frame(cx);
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::PromptHistory));
        assert!(view.read(cx).palette.is_some());
        // Second press toggles closed; the draft is untouched.
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert_eq!(view.read(cx).composer.read(cx).value(), "stale");
        // Reopen and accept: the draft-prefiltered match wins over recency order.
        window.press("ctrl-r", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value(), "Review stale diffs");
        assert!(view.read(cx).palette.is_none());
        assert!(requests.try_recv().is_err(), "choosing history only restores a draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn slash_new_and_stop_use_desktop_session_lifecycle(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("existing-session".into());
            view.composer.update(cx, |input, cx| input.set_value("/new", window, cx));
            view.submit_prompt(window, cx);
            assert_eq!(view.composer.read(cx).value(), "");
        });
        window.render_frame(cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("/new should close the current desktop session");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_CLOSE);
    assert_eq!(params, json!({ "sessionId": "existing-session" }));

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("existing-session".into());
            view.active_turn_id = Some("active-turn".into());
            view.running = true;
            view.composer.update(cx, |input, cx| input.set_value("/stop", window, cx));
            view.submit_prompt(window, cx);
            assert_eq!(view.composer.read(cx).value(), "");
        });
        window.render_frame(cx);
    })
    .unwrap();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("/stop should cancel through the desktop service");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::TURN_CANCEL);
    assert_eq!(params["sessionId"], "existing-session");
    assert_eq!(params["turnId"], "active-turn");
    assert!(requests.try_recv().is_err());
}

#[test]
fn initialize_result_remains_compatible_with_older_cli_helpers() {
    let result = serde_json::from_value::<vtcode_desktop_protocol::InitializeResult>(json!({
        "protocol": vtcode_desktop_protocol::PROTOCOL_VERSION,
        "workspace": "/workspace",
        "serverVersion": "older-helper"
    }))
    .unwrap();
    assert!(result.slash_commands.is_empty());
}

#[test]
fn invalid_slash_command_catalogs_are_rejected() {
    let valid = vec![vtcode_desktop_protocol::SlashCommandInfo {
        name: "review-template".into(),
        description: "Review a workspace template".into(),
    }];
    assert!(super::super::valid_slash_command_catalog(&valid));
    for name in ["", "bad command", "bad/command", "bad\ncommand"] {
        assert!(!super::super::valid_slash_command_catalog(&[vtcode_desktop_protocol::SlashCommandInfo {
            name: name.into(),
            description: "Safe description".into(),
        }]));
    }
}

#[gpui_kit::test]
fn composer_option_changes_are_blocked_during_runs_and_reject_malformed_catalogs(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.running = true;
            view.open_permissions(window, cx);
            assert!(view.palette.is_none());
            let _selected = view.select_session_options(
                vtcode_desktop_protocol::SessionOptionsSelectParams { mode: Some("plan".into()), ..Default::default() },
                cx,
            );
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::COMPOSER_RESOURCES.into(),
                    error: None,
                    result: Some(json!([{"name":"bad\n/command", "description":"unsafe token", "kind":"skill"}])),
                },
                cx,
            );
            assert!(view.composer_resources.is_empty());
            assert!(view.error.as_ref().unwrap().contains("Invalid CLI resource catalog"));
        });
    })
    .unwrap();
    assert!(requests.try_recv().is_err());
}

#[gpui_kit::test]
fn late_resource_discovery_keeps_the_selected_command_identity(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_add_context(window, cx);
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| {
            view.focus_palette_state(window, cx);
            view.palette.as_ref().unwrap().update(cx, |state, cx| {
                state.set_selected_index(Some(gpui_kit::component::IndexPath::new(2).section(4)), window, cx)
            });
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::COMPOSER_RESOURCES.into(),
                    error: None,
                    result: Some(json!([{"name":"rust-skills","description":"Rust","kind":"skill"}])),
                },
                cx,
            );
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(
            view.read(cx)
                .palette
                .as_ref()
                .unwrap()
                .read(cx)
                .selected_index()
                .unwrap()
                .section,
            4
        );
        assert_eq!(
            window
                .find(palette_action_id(&super::super::PaletteAction::ToggleInspector))
                .label(),
            Some("Toggle inspector panel")
        );
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!view.read(cx).inspector_open);
        assert!(view.read(cx).composer.read(cx).value().is_empty());
    });
}

#[gpui_kit::test]
fn unsupported_inherited_effort_offers_explicit_supported_choices(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            config.session_options.as_mut().unwrap().reasoning_effort = "max".into();
            view.configuration = Some(config);
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("increase-effort").is_none());
        assert_eq!(
            window
                .find(palette_action_id(&super::super::PaletteAction::SelectSessionOptions(
                    vtcode_desktop_protocol::SessionOptionsSelectParams {
                        reasoning_effort: Some("low".into()),
                        ..Default::default()
                    }
                )))
                .label(),
            Some("Use low effort")
        );
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("option request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPTIONS);
    assert_eq!(params["reasoningEffort"], "low");
}

#[gpui_kit::test]
fn model_switch_keeps_attached_draft_and_waits_for_pending_file_loading(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "context.rs".into(),
                text: "complete source".into(),
            });
            view.composer
                .update(cx, |input, cx| input.set_value("Review this source", window, cx));
            view.attachments_picker_open = true;
            view.open_model_options(window, cx);
            assert!(view.palette.is_none());
            view.attachments_picker_open = false;
            view.handle_backend_event(
                BackendEvent::Response {
                    method: vtcode_desktop_protocol::method::CONFIG_SELECT.into(),
                    result: Some(json!(runtime_options_config())),
                    error: None,
                },
                cx,
            );
            assert_eq!(view.attachments[0].path, "context.rs");
            assert_eq!(view.composer.read(cx).value(), "Review this source");
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn runtime_option_popups_focus_the_current_choice_and_confirming_it_is_a_noop(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            let options = config.session_options.as_mut().unwrap();
            options.mode = "plan".into();
            options.permission_mode = vtcode_desktop_protocol::PermissionMode::AutoApproveCommands;
            view.configuration = Some(config);
        });
        window.render_frame(cx);
        window.click("composer-mode", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).selected_index().unwrap().row, 1);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "confirming the current Plan choice must not restart a session");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("composer-policy", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).selected_index().unwrap().row, 1);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err(), "confirming the current policy must not restart a session");
    let workspace = std::env::temp_dir().to_string_lossy().into_owned();
    assert_eq!(
        crate::desktop::preferences::Preferences::load()
            .workspace_permissions
            .get(&workspace),
        Some(&vtcode_desktop_protocol::PermissionMode::AutoApproveCommands),
        "confirming the current Auto policy persists it without restarting"
    );
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.configuration
                .as_mut()
                .unwrap()
                .session_options
                .as_mut()
                .unwrap()
                .permission_mode = vtcode_desktop_protocol::PermissionMode::Ask;
        });
        window.render_frame(cx);
        window.click("composer-policy", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(requests.try_recv().is_err());
    assert_eq!(
        crate::desktop::preferences::Preferences::load()
            .workspace_permissions
            .get(&workspace),
        Some(&vtcode_desktop_protocol::PermissionMode::Ask),
        "explicitly choosing the current Ask policy still persists that preference"
    );
}

#[gpui_kit::test]
fn composer_policy_button_tracks_permission_mode_and_availability(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(16);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("composer-policy").label(), Some("Execution policy: Permissions unavailable"));
        assert_eq!(window.find("composer-policy").disabled(), Some(true));

        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-policy").label(), Some("Execution policy: Ask before protected actions"));
        assert_ne!(window.find("composer-policy").disabled(), Some(true));

        view.update(cx, |view, _| {
            view.configuration
                .as_mut()
                .unwrap()
                .session_options
                .as_mut()
                .unwrap()
                .permission_mode = vtcode_desktop_protocol::PermissionMode::AutoApproveCommands;
        });
        window.render_frame(cx);
        assert_eq!(window.find("composer-policy").label(), Some("Execution policy: Auto-approve commands"));
        assert_ne!(window.find("composer-policy").disabled(), Some(true));
    })
    .unwrap();
}
