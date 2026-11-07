use super::*;

#[gpui_kit::test]
fn gutter_press_dismisses_a_clean_settings_page(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        // Inner press stays: the panel stops the press before the gutter.
        window.click("settings-title", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings, "inner press keeps the page");
        // Gutter press: 4px into the route container's left padding.
        let panel = window.find("settings-panel").bounds();
        let root = window.find("pane-root").bounds();
        let gutter = point(panel.origin.x - root.origin.x - px(4.), panel.origin.y - root.origin.y + px(40.));
        window.click_at("pane-root", gutter, cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "gutter press dismisses a clean page");
    })
    .unwrap();
}

#[gpui_kit::test]
fn dismiss_buttons_and_gutter_hold_dirty_settings_for_the_prompt(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    // Stage a shortcut edit directly, the same shape a capture commit writes.
    view.update(cx, |view, _| {
        let _previous = view.shortcut_draft.insert("model".into(), "ctrl-shift-m".into());
        view.shortcuts_editing = true;
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        // The close button holds for the prompt: headless it never answers.
        window.click("settings-dismiss", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings, "dirty dismiss waits for the prompt");
        // The gutter holds the same way.
        let panel = window.find("settings-panel").bounds();
        let root = window.find("pane-root").bounds();
        let gutter = point(panel.origin.x - root.origin.x - px(4.), panel.origin.y - root.origin.y + px(40.));
        window.click_at("pane-root", gutter, cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings, "dirty gutter press waits for the prompt");
        // A stale confirm is a no-op; the matched one closes.
        view.update(cx, |view, cx| {
            view.discard_after_confirm(DiscardIntent::CloseRoute(Route::Browser), window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings, "stale confirm is a no-op");
        view.update(cx, |view, cx| {
            view.discard_after_confirm(DiscardIntent::CloseRoute(Route::Settings), window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace, "confirmed discard closes the page");
        assert!(!view.read(cx).shortcuts_editing, "teardown clears the staged flag");
    })
    .unwrap();
}

#[gpui_kit::test]
fn gutter_press_unwinds_one_layer_and_theme_draft_holds_for_the_prompt(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
            settings: vec![vtcode_desktop_protocol::ConfigSetting {
                path: "agent.default_model".into(),
                value: serde_json::json!("model-a"),
                origin: Some("workspace".into()),
                label: Some("Default model".into()),
                type_name: Some("string".into()),
                default_value: None,
                description: None,
                options: Vec::new(),
                group: None,
                group_description: None,
                editable: true,
            }],
            target_versions: Vec::new(),
            omitted_sensitive: 0,
            omitted_large: 0,
            omitted_limit: 0,
        });
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.open_cli_config_editor("agent.default_model".into(), window, cx);
            view.config_write_pending = true;
        });
        window.render_frame(cx);
        // A write in flight pins everything, like the Back path.
        click_editor_scrim(window, cx);
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_some(), "write pins the editor");
        assert!(view.read(cx).current_route() == Route::Settings);
        // Idle editor closes first as one interior step, staying on Settings.
        view.update(cx, |view, _| view.config_write_pending = false);
        click_editor_scrim(window, cx);
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_none());
        assert!(view.read(cx).current_route() == Route::Settings);
        // An armed capture cancels the same way, without asking.
        view.update(cx, |view, _| {
            view.shortcut_capture = Some(super::super::shortcuts::CommandId::ZenMode);
        });
        let panel = window.find("settings-panel").bounds();
        let root = window.find("pane-root").bounds();
        let gutter = point(panel.origin.x - root.origin.x - px(4.), panel.origin.y - root.origin.y + px(40.));
        window.click_at("pane-root", gutter, cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_capture.is_none(), "gutter cancels the capture");
        assert!(view.read(cx).current_route() == Route::Settings);
        // A staged theme holds the route for the prompt instead.
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).custom_theme_editor_open());
        window.click_at("pane-root", gutter, cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings, "staged theme waits for the prompt");
        view.update(cx, |view, cx| {
            view.discard_after_confirm(DiscardIntent::CloseRoute(Route::Settings), window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Workspace);
        assert!(!view.read(cx).custom_theme_editor_open(), "teardown clears the draft");
    })
    .unwrap();
}

#[gpui_kit::test]
fn enum_editor_selection_dirt_holds_for_the_prompt(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.backend = Some(BackendHandle { commands });
        view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
            settings: vec![vtcode_desktop_protocol::ConfigSetting {
                path: "provider.openai.service_tier".into(),
                value: serde_json::json!("auto"),
                origin: Some("user".into()),
                label: Some("Service tier".into()),
                type_name: Some("string".into()),
                default_value: None,
                description: None,
                options: vec!["auto".into(), "fast".into()],
                group: None,
                group_description: None,
                editable: true,
            }],
            target_versions: Vec::new(),
            omitted_sensitive: 0,
            omitted_large: 0,
            omitted_limit: 0,
        });
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
            view.open_cli_config_editor("provider.openai.service_tier".into(), window, cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("cli-config-option-select").is_some());
        // Untouched selection: the scrim press closes the editor.
        click_editor_scrim(window, cx);
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_none(), "clean selection dismisses");
        // Changed selection: holds for the prompt.
        view.update(cx, |view, cx| {
            view.open_cli_config_editor("provider.openai.service_tier".into(), window, cx);
        });
        window.render_frame(cx);
        window.within("cli-config-option-select").click("input", cx);
        window.render_frame(cx);
        window.press("down", cx);
        window.press("enter", cx);
        window.render_frame(cx);
        click_editor_scrim(window, cx);
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_some(), "changed selection waits for the prompt");
        view.update(cx, |view, cx| {
            view.discard_after_confirm(
                DiscardIntent::CloseConfigEditor("provider.openai.service_tier".into()),
                window,
                cx,
            );
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_none(), "confirmed discard closes the editor");
    })
    .unwrap();
}
