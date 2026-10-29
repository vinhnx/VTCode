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
fn escape_closes_cli_config_editor_before_the_settings_sheet(cx: &mut TestAppContext) {
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
        assert!(view.read(cx).settings_open);
        assert!(view.read(cx).settings_focus.contains_focused(window, cx));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).settings_open);
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
            assert!(view.selected_session_id.is_none());
            assert_eq!(view.status, "CLI theme saved. The next message starts a new session.");
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
fn workspace_files_open_a_read_only_preview(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("README.md"), "# Notes\n\nhello from the workspace").expect("write file");
    std::fs::create_dir_all(workspace.path().join("src/nested")).expect("create nested sources");
    std::fs::write(workspace.path().join("src/nested/mod.rs"), "pub fn nested() {}").expect("write nested source");
    std::fs::create_dir_all(workspace.path().join("target/debug")).expect("skip dir");
    std::fs::write(workspace.path().join("target/debug/artifact"), b"binary").expect("write artifact");

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.refresh_files(cx);
            assert!(view.files.is_empty(), "scan does not block the UI callback");
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        {
            let view = view.read(cx);
            assert!(!view.files.iter().any(|entry| entry.rel_path.starts_with("target")));
            assert!(view.files.iter().any(|entry| entry.rel_path == "README.md"));
        };
        window.render_frame(cx);
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        assert_eq!(file_tree_paths(&view, cx), vec!["src", "README.md"]);
        let directory_index = view.read(cx).file_directory_paths.clone();
        assert!(directory_index.contains("src"), "the retained tree index records directories");
        window.render_frame(cx);
        assert!(
            std::sync::Arc::ptr_eq(&directory_index, &view.read(cx).file_directory_paths),
            "redrawing the inspector should reuse the directory index"
        );
        let tree_id = format!("tree-{}", view.read(cx).file_tree.entity_id());
        let tree = window
            .find_all(tree_id)
            .into_iter()
            .find(|tree| tree.role() == Some(gpui_kit::Role::Tree))
            .expect("file tree exposes its semantic role");
        assert_eq!(tree.role(), Some(gpui_kit::Role::Tree));
        assert_eq!(window.find("focus-files").label(), Some("Focus workspace file tree"));
        let root_item = file_tree_item_snapshot(window, 0);
        assert_eq!(root_item.role(), Some(gpui_kit::Role::TreeItem));
        assert_eq!(root_item.label(), Some("src"));
        assert_eq!(window.find("workspace-file-src").label(), Some("Folder: src"));
        assert_eq!(root_item.expanded(), Some(false));

        window.click("workspace-file-src", cx);
        window.render_frame(cx);
        assert_eq!(file_tree_item_snapshot(window, 0).expanded(), Some(true));
        assert!(file_tree_paths(&view, cx).contains(&"src/nested".to_string()));
        assert!(view.read(cx).file_preview.is_none(), "folder activation expands instead of previewing");
        window.click("workspace-file-src", cx);
        window.render_frame(cx);
        assert_eq!(file_tree_item_snapshot(window, 0).expanded(), Some(false));

        let mut reached_file_focus = false;
        for _ in 0..32 {
            window.press("tab", cx);
            window.render_frame(cx);
            if window.find("focus-files").focused() == Some(true) {
                reached_file_focus = true;
                break;
            }
        }
        assert!(reached_file_focus, "Tab reaches the file-tree focus control");
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        let tree = view.read(cx).file_tree.clone();
        let selected_path = tree.read(cx).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected_path.as_deref(), Some("src"));
        assert_eq!(file_tree_item_snapshot(window, 0).selected(), Some(true));
        window.press("right", cx);
        assert_eq!(file_tree_item_snapshot(window, 0).expanded(), Some(true));
        assert!(file_tree_paths(&view, cx).contains(&"src/nested".to_string()));
        window.press("down", cx);
        window.press("right", cx);
        assert_eq!(file_tree_item_snapshot(window, 1).expanded(), Some(true));
        assert!(file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));
        window.press("left", cx);
        assert!(!file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));
        window.press("right", cx);
        assert!(file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));

        view.update(cx, |view, cx| view.refresh_files(cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));
        let tree = view.read(cx).file_tree.clone();
        let selected_path = tree.read(cx).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected_path.as_deref(), Some("src/nested"));
        window.press("down", cx);
        let tree = view.read(cx).file_tree.clone();
        let selected_path = tree.read(cx).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected_path.as_deref(), Some("src/nested/mod.rs"));
        window.press("enter", cx);
        assert!(view.read(cx).file_preview.is_none(), "read does not block the UI callback");
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("file-preview").label(), Some("File preview: src/nested/mod.rs"));
        assert_eq!(window.find("open-preview-editor").label(), Some("Open in editor"));
        assert_eq!(window.find("copy-preview").label(), Some("Copy"));
        assert_eq!(window.find("close-preview").label(), Some("Close"));
        assert_eq!(window.find("open-preview-editor").role(), Some(gpui_kit::Role::Button));
        assert_eq!(window.find("open-preview-vscode").role(), Some(gpui_kit::Role::Button));
        window.click("copy-preview", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("pub fn nested() {}"));
        window.click("close-preview", cx);
        assert!(view.read(cx).file_preview.is_none(), "close dismisses the preview");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.click("workspace-file-README.md", cx);
        assert!(view.read(cx).file_preview.is_none(), "mouse activation also reads asynchronously");
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        let preview = view.read(cx).file_preview.as_ref().map(|preview| preview.text.clone());
        assert_eq!(preview.as_deref(), Some("# Notes\n\nhello from the workspace"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_files_show_loading_and_empty_states(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.inspector_tab = super::super::InspectorTab::Files;
            view.files_scan_cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));
        });
        window.render_frame(cx);
        let loading = window.find("file-list-loading");
        assert_eq!(loading.role(), Some(gpui_kit::Role::Status));
        assert_eq!(loading.label(), Some("Scanning workspace files"));
        assert!(window.find("file-scan-skeleton-1").bounds().size.height > px(0.));

        view.update(cx, |view, cx| {
            view.files_scan_cancel = None;
            cx.notify();
        });
        window.render_frame(cx);
        let empty = window.find("file-list-empty");
        assert_eq!(empty.role(), Some(gpui_kit::Role::Status));
        assert_eq!(empty.label(), Some("No workspace files"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_changes_list_files_and_open_a_diff_viewer(cx: &mut TestAppContext) {
    use vtcode_exec_events::{
        FileChangeItem, FileUpdateChange, ItemCompletedEvent, PatchApplyStatus, PatchChangeKind, ThreadEvent,
        ThreadItem, ThreadItemDetails,
    };

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.workspace = Some(std::env::temp_dir());
            let patch = format!(
                "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,80 +1,80 @@\n{}",
                (1..=80)
                    .map(|line| format!("-line {line}\n+{}\n", if line == 40 { "here" } else { "updated" }))
                    .collect::<String>()
            );
            view.handle_thread_event(vtcode_desktop_protocol::ThreadEventNotification {
                session_id: "session-f".to_string(),
                turn_id: Some("turn-1".to_string()),
                event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: ThreadItem {
                        context: None,
                        id: "change-9".to_string(),
                        details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                            diff_incomplete: Some(false),
                            changes: vec![FileUpdateChange {
                                path: "src/app.rs".to_string(),
                                kind: PatchChangeKind::Update,
                            }],
                            status: PatchApplyStatus::Completed,
                            unified_diff: Some(patch.clone()),
                            additions: Some(80),
                            deletions: Some(80),
                        })),
                    },
                })),
            });
            assert_eq!(view.changes.entries().len(), 1);
            assert_eq!(view.changes.entries()[0].path, "src/app.rs");
            assert_eq!(view.changes.totals(), Some((80, 80)));
        });
        window.render_frame(cx);
        window.click(super::super::opaque_path_element_id("change-row", "src/app.rs"), cx);
        {
            let view = view.read(cx);
            assert_eq!(view.selected_change.as_deref(), Some("src/app.rs"));
            assert!(view.change_viewer.is_some(), "selecting a file builds the diff viewer");
        }
        window.click("copy-change-patch", cx);
        assert!(cx.read_from_clipboard().unwrap().text().unwrap().contains("+here"));
        let viewer = view.read(cx).change_viewer.as_ref().unwrap().clone();
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).review_open);
        assert_eq!(window.find("diff-review-panel").role(), Some(gpui_kit::Role::Dialog));
        assert!(view.read(cx).review_focus.contains_focused(window, cx));
        window.within("diff-review-panel").click("review-split", cx);
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).mode(), gpui_kit::component::diff::DiffMode::Split);
        assert_eq!(window.find("review-split").label(), Some("Split diff view (selected)"));
        assert_eq!(window.find("review-unified").label(), Some("Unified diff view"));
        window.within("diff-review-panel").click("review-unified", cx);
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).mode(), gpui_kit::component::diff::DiffMode::Unified);
        assert_eq!(window.find("review-unified").label(), Some("Unified diff view (selected)"));
        assert_eq!(window.find("review-split").label(), Some("Split diff view"));
        for _ in 0..8 {
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).review_focus.contains_focused(window, cx));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).review_open);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/app.rs"));
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        window.click("close-diff-review", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).review_open);
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(window.within("diff-review-panel").find("change-diff-view").bounds().size.width > px(900.));
        assert!(window.within("diff-review-panel").find("change-diff-view").bounds().size.height > px(400.));
        let viewport = window.within("diff-review-panel").find("change-diff-view").bounds();
        window.scroll("change-diff-view", gpui_kit::ScrollDelta::Lines(point(0., -100.)), cx);
        window.render_frame(cx);
        let scrolled_line = window.within(("new", 52usize)).find("source").bounds();
        assert!(
            scrolled_line.top() >= viewport.top() && scrolled_line.bottom() <= viewport.bottom(),
            "the Kit Diff's virtualized list should scroll within its own review viewport"
        );
        view.update(cx, |view, cx| {
            view.reset_session_state();
            view.restore_focus_after_review_close(window, cx);
            assert!(!view.review_open);
            assert!(!view.review_focus.contains_focused(window, cx));
            assert!(view.composer.read(cx).focus_handle(cx).is_focused(window));
        });
        window.render_frame(cx);
        assert!(!view.read(cx).review_open);
        assert!(view.read(cx).change_viewer.is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
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
        window.click("attach-files", cx);
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
