use super::*;

fn curated_snapshot() -> vtcode_desktop_protocol::ConfigSettingsSnapshot {
    vtcode_desktop_protocol::ConfigSettingsSnapshot {
        settings: vec![
            config_page_setting("sandbox.enabled", json!(true), "Approvals & Security"),
            config_page_setting("sandbox.default_policy", json!("workspace_write"), "Approvals & Security"),
            config_page_setting("sandbox.network.policy", json!("allowlist_only"), "Approvals & Security"),
            config_page_setting("tools.web_search.provider", json!("auto"), "Tools & Integrations"),
            config_page_setting("agent.verbosity", json!("medium"), "Model & Provider"),
            config_page_setting("ui.reasoning_display_mode", json!("toggle"), "Interface & Terminal"),
            config_page_setting("agent.default_model", json!("model-a"), "Model & Provider"),
        ],
        target_versions: Vec::new(),
        omitted_sensitive: 0,
        omitted_large: 0,
        omitted_limit: 0,
    }
}

#[gpui_kit::test]
fn curated_overview_is_the_default_config_landing(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(curated_snapshot());
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("config-category-curated").is_some());
        assert!(window.try_find("open-config-file").is_some());
        assert_ne!(window.find("open-config-file").disabled(), Some(true));
        for id in [
            "curated-sandboxing",
            "curated-approval",
            "curated-sandbox",
            "curated-network",
            "curated-web-search",
            "curated-verbosity",
            "curated-reasoning",
        ] {
            assert!(window.try_find(id).is_some(), "{id} renders on the overview");
        }
        assert!(window.try_find("curated-effort-low").is_some());
        assert_eq!(window.find("curated-network").checked(), Some(false));
        assert!(window.try_find("config-edit-agent.default_model").is_none(), "catalog rows stay behind their group");

        view.update(cx, |view, cx| {
            view.config_page_group = Some("Model & Provider".into());
            cx.notify();
        });
        window.render_frame(cx);
        assert!(window.try_find("config-edit-agent.default_model").is_some());
        assert!(window.try_find("curated-sandbox").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn curated_deep_link_returns_to_the_overview(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(curated_snapshot());
            view.config_page_group = Some("Tools & Integrations".into());
            let action =
                super::super::composer_controls::DesktopSlashAction::Configuration(Some("configuration".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_page_open);
        assert_eq!(view.read(cx).config_page_group.as_deref(), Some("Configuration"));
        assert!(window.try_find("curated-sandbox").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn curated_network_toggle_writes_the_mapped_policy_in_both_directions(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(curated_snapshot());
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.click("curated-network", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("network write request") else {
        panic!("expected config write request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["path"], "sandbox.network.policy");
    assert_eq!(params["value"], "allow_all");

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.config_write_pending = false;
            view.session_request = None;
            let snapshot = view.cli_config_settings.as_mut().expect("snapshot");
            let policy = snapshot
                .settings
                .iter_mut()
                .find(|setting| setting.path == "sandbox.network.policy")
                .expect("network policy");
            policy.value = json!("allow_all");
            cx.notify();
        });
        window.render_frame(cx);
        assert_eq!(window.find("curated-network").checked(), Some(true));
        window.click("curated-network", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("network revert request") else {
        panic!("expected config write request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["path"], "sandbox.network.policy");
    assert_eq!(params["value"], "allowlist_only");
}

#[gpui_kit::test]
fn curated_network_toggle_is_disabled_outside_workspace_write(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            let mut snapshot = curated_snapshot();
            snapshot
                .settings
                .iter_mut()
                .find(|setting| setting.path == "sandbox.default_policy")
                .expect("sandbox policy")
                .value = json!("read_only");
            view.cli_config_settings = Some(snapshot);
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("curated-network").disabled(),
            Some(true),
            "read-only sandboxes decide egress on their own"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn curated_open_file_requests_the_selected_layer(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(curated_snapshot());
            view.config_target = vtcode_desktop_protocol::ConfigTarget::Workspace;
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        window.click("open-config-file", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("open file request") else {
        panic!("expected config open request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_OPEN_FILE);
    assert_eq!(params["target"], "workspace");
}

#[gpui_kit::test]
fn curated_effort_switch_updates_the_saved_selection(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 1280., 1600.);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(curated_snapshot());
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("curated-effort-low").checked(), Some(true));
        window.click("curated-effort-low", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).preferences.available_efforts().contains(&"low".to_string()));
        assert_eq!(window.find("curated-effort-low").checked(), Some(false));
        window.click("curated-effort-low", cx);
        window.render_frame(cx);
        assert!(view.read(cx).preferences.available_efforts().contains(&"low".to_string()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn hidden_efforts_still_offer_supported_choices_for_unsupported_effort(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.preferences.available_efforts = Some(Vec::new());
            let mut config = runtime_options_config();
            config.session_options.as_mut().unwrap().reasoning_effort = "max".into();
            view.configuration = Some(config);
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).effort_slider.is_none(), "no knob without visible levels");
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
    })
    .unwrap();
}

#[gpui_kit::test]
fn reopening_model_options_clears_a_stale_knob(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).effort_slider.is_some());
        view.update(cx, |view, cx| {
            let config = view.configuration.as_mut().expect("configuration");
            config.session_options.as_mut().unwrap().reasoning_effort = "max".into();
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).effort_slider.is_none(), "the fallback must not reuse the previous knob range");
    })
    .unwrap();
}

#[gpui_kit::test]
fn filtered_efforts_keep_the_current_level_in_knob_position(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.preferences.available_efforts = Some(vec!["low".into()]);
            view.configuration = Some(runtime_options_config());
            view.open_model_options(window, cx);
        });
        window.render_frame(cx);
        let slider = view.read(cx).effort_slider.clone().expect("filtered knob");
        assert_eq!(slider.read(cx).value().start(), 1.0, "medium stays at index 1 of [low, medium]");
    })
    .unwrap();
}

#[gpui_kit::test]
fn blocked_network_policy_disables_the_toggle(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            let mut snapshot = curated_snapshot();
            snapshot
                .settings
                .iter_mut()
                .find(|setting| setting.path == "sandbox.network.policy")
                .expect("network policy")
                .value = json!("block_all");
            view.cli_config_settings = Some(snapshot);
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("curated-network").checked(), Some(false));
        assert_eq!(
            window.find("curated-network").disabled(),
            Some(true),
            "the toggle must not silently replace block_all"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn non_editable_network_policy_disables_the_toggle(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            let mut snapshot = curated_snapshot();
            snapshot
                .settings
                .iter_mut()
                .find(|setting| setting.path == "sandbox.network.policy")
                .expect("network policy")
                .editable = false;
            view.cli_config_settings = Some(snapshot);
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("curated-network").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn missing_curated_paths_render_unavailable_rows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![config_page_setting(
                    "agent.default_model",
                    json!("model-a"),
                    "Model & Provider",
                )],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        for id in [
            "curated-sandboxing",
            "curated-sandbox",
            "curated-network",
            "curated-reasoning",
        ] {
            assert!(window.try_find(id).is_some(), "{id} renders without its setting");
            assert_eq!(window.find(id).disabled(), Some(true), "{id} is unavailable");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn open_file_error_surfaces_instead_of_sticking(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::Response {
                method: vtcode_desktop_protocol::method::CONFIG_OPEN_FILE.into(),
                result: None,
                error: Some("Could not launch the editor".into()),
            },
            cx,
        );
        assert_eq!(view.error.as_deref(), Some("Could not launch the editor"));
        assert_eq!(view.status, "Service request failed");
    });
}

#[gpui_kit::test]
fn sandboxing_row_writes_the_toggled_value(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            let mut snapshot = curated_snapshot();
            snapshot
                .settings
                .iter_mut()
                .find(|setting| setting.path == "sandbox.enabled")
                .expect("sandbox enabled")
                .value = json!(false);
            view.cli_config_settings = Some(snapshot);
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("curated-sandboxing").checked(), Some(false));
        window.click("curated-sandboxing", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("sandbox write request") else {
        panic!("expected config write request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["path"], "sandbox.enabled");
    assert_eq!(params["value"], true);
}

#[gpui_kit::test]
fn open_file_is_disabled_while_a_write_is_pending(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(curated_snapshot());
            view.config_write_pending = true;
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("open-config-file").disabled(), Some(true));
        assert!(!view.read(cx).can_open_config_file());
    })
    .unwrap();
}
