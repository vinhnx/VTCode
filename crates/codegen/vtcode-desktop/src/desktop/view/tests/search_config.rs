use super::*;

#[gpui_kit::test]
fn settings_search_filters_categories_and_cli_config_paths(cx: &mut TestAppContext) {
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
        });
        window.render_frame(cx);
        window.press("cmd-f", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Search settings and CLI config"));
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::SettingsSearch));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("font", cx);
        window.render_frame(cx);
        assert_eq!(
            window
                .find(palette_action_id(&super::super::PaletteAction::SelectSettingsTab(
                    super::super::settings::SettingsTab::Appearance
                )))
                .label(),
            Some("Appearance")
        );
        window.within("command-palette").click(
            palette_action_id(&super::super::PaletteAction::SelectSettingsTab(
                super::super::settings::SettingsTab::Appearance,
            )),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).settings_open);
        assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::Appearance);

        window.click("settings-filter-search", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.within("command-palette").input("agent.default_model", cx);
        window.render_frame(cx);
        let config_action = super::super::PaletteAction::EditConfigSetting("agent.default_model".into());
        assert!(
            window
                .find(palette_action_id(&config_action))
                .label()
                .is_some_and(|label| label.starts_with("Default model"))
        );
        window.within("command-palette").click(palette_action_id(&config_action), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).config_editor.is_some(), "selecting a CLI setting opens its editor");
        assert!(view.read(cx).settings_open);
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_config_browser_uses_its_own_search_and_returns_to_settings(cx: &mut TestAppContext) {
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
            let snapshot = view.cli_config_settings.as_mut().unwrap();
            let mut reference = snapshot.settings[0].clone();
            reference.path = "tools.example".into();
            reference.group = Some("Tools".into());
            reference.label = Some("Tool reference".into());
            reference.editable = false;
            snapshot.settings.push(reference);
            view.slash_commands = vec![vtcode_desktop_protocol::SlashCommandInfo {
                name: "config".into(),
                description: "Open CLI configuration".into(),
            }];
            view.composer.update(cx, |input, cx| {
                input.set_value("/config", window, cx);
                input.set_cursor_position(gpui_kit::component::input::Position::new(0, 7), window, cx);
            });
            view.update_composer_suggestions(window, cx);
            let action =
                super::super::composer_controls::desktop_slash_action("/config").expect("native /config route");
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(view.read(cx).settings_open);
        assert!(view.read(cx).config_page_open);
        assert!(window.try_find("cli-config-page").is_some());
        assert!(window.try_find("config-category-curated").is_some());
        window.click("config-category-Model & Provider", cx);
        window.render_frame(cx);
        assert!(window.try_find("config-edit-agent.default_model").is_some());
        assert!(window.try_find("config-category-Model & Provider").is_some());
        assert!(view.read(cx).palette.is_none(), "/config opens the configuration page first");
        window.click("config-category-Tools", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_page_group.as_deref(), Some("Tools"));
        assert!(window.try_find("config-edit-agent.default_model").is_none());
        assert!(window.try_find("config-edit-tools.example").is_none());
        window.click("config-category-Model & Provider", cx);
        window.render_frame(cx);
        assert!(window.try_find("config-edit-agent.default_model").is_some());
        window.click("browse-cli-config", cx);
        window.render_frame(cx);
        assert!(matches!(view.read(cx).palette_kind, super::super::PaletteKind::ConfigSettings));
        assert_eq!(window.find("command-palette").label(), Some("VT Code configuration"));
        let action = super::super::PaletteAction::EditConfigSetting("agent.default_model".into());
        assert!(window.try_find(palette_action_id(&action)).is_some());
        window.press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).settings_open, "closing the settings browser returns to its parent panel");
        assert!(view.read(cx).palette.is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn cli_config_page_keeps_header_fixed_while_catalog_scrolls_in_short_windows(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 600., 400.);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.preferences.set_ui_font_size(view.theme_mode, 24);
            view.apply_typography(cx);
            let settings = (0..6)
                .map(|index| {
                    config_page_setting(
                        &format!("agent.setting_{index}"),
                        json!(format!("value-{index}")),
                        "Model & Provider",
                    )
                })
                .collect();
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings,
                target_versions: Vec::new(),
                omitted_sensitive: 1,
                omitted_large: 1,
                omitted_limit: 0,
            });
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        for id in [
            "cli-config-header",
            "config-search",
            "browse-cli-config",
            "reload-cli-config",
            "config-rail",
            "cli-config-page-content",
            "cli-config-footer",
        ] {
            assert!(window.try_find(id).is_some(), "{id} stays visible in short windows");
        }
        let header = window.find("cli-config-header").bounds();
        let content = window.find("cli-config-page-content").bounds();
        let footer = window.find("cli-config-footer").bounds();
        assert!(
            header.size.height > px(0.) && content.size.height > px(0.) && footer.size.height > px(0.),
            "header={header:?} content={content:?} footer={footer:?}"
        );
        assert!(
            header.origin.y + header.size.height <= content.origin.y + px(1.),
            "the header sits above the catalog: {header:?} {content:?}"
        );
        assert!(
            footer.origin.y >= content.origin.y,
            "the notes trail inside the catalog scroll region: {content:?} {footer:?}"
        );
        let before = view.read(cx).settings_scroll.offset().y;
        window.scroll("cli-config-page-content", gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-300.))), cx);
        window.render_frame(cx);
        assert!(view.read(cx).settings_scroll.offset().y < before, "the catalog scrolls");
        assert_eq!(
            window.find("cli-config-header").bounds(),
            header,
            "the header stays fixed while the catalog scrolls"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_deep_link_selects_group_setting_or_search(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![
                    config_page_setting("agent.default_model", json!("model-a"), "Model & Provider"),
                    config_page_setting("agent.thinking_budget", json!(8_000), "Model & Provider"),
                    config_page_setting("tools.auto_approve", json!(true), "Tools"),
                ],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
            let action = super::super::composer_controls::DesktopSlashAction::Configuration(Some("Tools".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_page_open);
        assert_eq!(view.read(cx).config_page_group.as_deref(), Some("Tools"));
        assert!(view.read(cx).config_search_input.read(cx).value().is_empty());
        assert!(window.try_find("config-category-Tools").is_some());
        assert!(window.try_find("config-category-Model & Provider").is_some());

        view.update(cx, |view, cx| {
            let action =
                super::super::composer_controls::DesktopSlashAction::Configuration(Some("agent.default_model".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_page_group.as_deref(), Some("Model & Provider"));
        assert!(window.try_find("config-edit-agent.default_model").is_some());
        assert!(
            window.try_find("config-edit-agent.thinking_budget").is_none(),
            "the exact path narrows past its shared prefix"
        );

        view.update(cx, |view, cx| {
            view.config_page_group = Some("Tools".into());
            let action = super::super::composer_controls::DesktopSlashAction::Configuration(Some("budget".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("config-category-Tools").is_none(), "groups without matches leave the rail");
        assert!(window.try_find("config-category-Model & Provider").is_some());
        assert!(window.try_find("config-edit-agent.thinking_budget").is_some());

        view.update(cx, |view, cx| {
            let action =
                super::super::composer_controls::DesktopSlashAction::Configuration(Some("zzz-no-such-setting".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("config-rail").is_none(), "no match leaves no group rail");
        assert!(window.try_find("clear-config-search").is_some());
        window.click("clear-config-search", cx);
        window.render_frame(cx);
        assert!(window.try_find("config-category-Tools").is_some(), "clearing restores every group");
        assert!(window.try_find("cli-config-page-content").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_deep_link_before_catalog_load_filters_once_settings_arrive(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            assert!(view.cli_config_settings.is_none());
            let action = super::super::composer_controls::DesktopSlashAction::Configuration(Some("budget".into()));
            assert!(view.handle_desktop_slash_action(action, window, cx));
        });
        window.render_frame(cx);
        assert!(view.read(cx).config_page_open);
        assert_eq!(view.read(cx).config_search_input.read(cx).value().as_str(), "budget");
        assert!(window.try_find("config-loading-line-one").is_some());

        view.update(cx, |view, _| {
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![
                    config_page_setting("agent.default_model", json!("model-a"), "Model & Provider"),
                    config_page_setting("agent.thinking_budget", json!(8_000), "Model & Provider"),
                    config_page_setting("tools.auto_approve", json!(true), "Tools"),
                ],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
        });
        window.render_frame(cx);
        assert!(window.try_find("config-edit-agent.thinking_budget").is_some());
        assert!(window.try_find("config-edit-agent.default_model").is_none());
        assert!(window.try_find("config-category-Tools").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn config_boolean_switch_writes_the_toggled_value_in_both_directions(cx: &mut TestAppContext) {
    let workspace = tempfile::tempdir().expect("workspace");
    let (window, view) = open_desktop(cx);
    // The sheet slides in on a wall-clock animation; reduce motion so the
    // trailing row controls land inside the viewport on the first frame.
    cx.update(|cx| cx.set_reduce_motion(true));
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.backend = Some(BackendHandle { commands });
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![
                    config_page_setting("agent.verbose", json!(true), "Model & Provider"),
                    config_page_setting("agent.quiet", json!(false), "Model & Provider"),
                ],
                target_versions: Vec::new(),
                omitted_sensitive: 0,
                omitted_large: 0,
                omitted_limit: 0,
            });
            view.config_page_group = Some("Model & Provider".into());
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("config-toggle-agent.verbose").checked(), Some(true));
        assert_eq!(window.find("config-toggle-agent.quiet").checked(), Some(false));
        window.click("config-toggle-agent.verbose", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("verbose write request") else {
        panic!("expected config write request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["path"], "agent.verbose");
    assert_eq!(params["value"], false);

    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).config_write_pending);
        assert_eq!(
            window.find("config-toggle-agent.quiet").disabled(),
            Some(true),
            "the in-flight write disables every row control"
        );
        view.update(cx, |view, cx| {
            // Simulate the completed write; the reload cycle it triggers is covered elsewhere.
            view.config_write_pending = false;
            view.session_request = None;
            cx.notify();
        });
        window.render_frame(cx);
        window.click("config-toggle-agent.quiet", cx);
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().expect("quiet write request") else {
        panic!("expected config write request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::CONFIG_SETTINGS_WRITE);
    assert_eq!(params["path"], "agent.quiet");
    assert_eq!(params["value"], true);
}

#[gpui_kit::test]
fn config_page_footer_shows_reset_hint_and_write_notice(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.cli_config_settings = Some(vtcode_desktop_protocol::ConfigSettingsSnapshot {
                settings: vec![config_page_setting(
                    "agent.default_model",
                    json!("model-a"),
                    "Model & Provider",
                )],
                target_versions: Vec::new(),
                omitted_sensitive: 1,
                omitted_large: 0,
                omitted_limit: 0,
            });
            view.config_write_notice = Some("Saved agent.default_model to workspace configuration.".into());
            view.config_page_open = true;
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("config-write-notice").role(), Some(gpui_kit::Role::Status));
        assert!(window.try_find("config-reset-hint").is_some());
        assert!(window.try_find("cli-config-footer").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn command_palette_refreshes_recent_threads_on_demand(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_command_palette(window, cx);
            assert!(
                view.palette_groups[0]
                    .1
                    .iter()
                    .any(|entry| entry.label == "Refresh recent threads")
            );
            view.run_palette_action(super::super::PaletteAction::RefreshSessions, window, cx);
        });
    })
    .unwrap();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("expected recent thread refresh request");
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_LIST);
    assert_eq!(params, json!({}));
}

#[gpui_kit::test]
fn conversation_search_filters_loaded_messages_and_jumps_to_the_selected_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "search-thread".to_string();
    let target_id = "message-08".to_string();
    view.update(cx, |view, _| {
        view.selected_session_id = Some(session_id.clone());
        for index in 0..40 {
            let id = format!("message-{index:02}");
            let (role, text) = if index == 8 {
                ("user", "Needle phrase in a user message")
            } else if index == 9 {
                ("tool", "A separate tool activity row")
            } else {
                ("assistant", "An ordinary conversation message")
            };
            view.transcript
                .push(TranscriptItem::new(id, role.into(), "VT Code".into(), text.into()));
        }
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);

        let open_search = super::super::PaletteAction::OpenConversationSearch { session_id: session_id.clone() };
        window.within("command-palette").input("Find in conversation", cx);
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).matched_count(), 1);
        assert!(window.try_find(palette_action_id(&open_search)).is_some());
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Find in conversation"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);

        let target_action = super::super::PaletteAction::JumpToTranscriptItem {
            session_id: session_id.clone(),
            item_id: target_id.clone(),
        };
        window.within("command-palette").input("Needle phrase", cx);
        window.render_frame(cx);
        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(palette.read(cx).matched_count(), 1, "unmatched tool activity does not pollute the message results");
        assert!(window.try_find(palette_action_id(&target_action)).is_some());
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert_eq!(view.read(cx).message_scroller.read(cx).logical_scroll_top().item_ix, 8);
        assert!(window.try_find(format!("transcript-row-{target_id}")).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_search_includes_tool_activity_and_reveals_the_matching_row(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "tool-search-thread".to_string();
    view.update(cx, |view, _| {
        view.selected_session_id = Some(session_id.clone());
        for index in 0..40 {
            view.transcript.push(TranscriptItem::new(
                format!("search-context-{index}"),
                "assistant".into(),
                "VT Code".into(),
                "Intermediate conversation message".into(),
            ));
        }
        view.transcript.push(TranscriptItem::new(
            "tool-before".into(),
            "tool".into(),
            "Read file".into(),
            "completed: read notes.md".into(),
        ));
        view.transcript.push(TranscriptItem::new(
            "tool-target".into(),
            "tool".into(),
            "Run command".into(),
            "completed: archived output needle-marker".into(),
        ));
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_conversation_search(session_id.clone(), window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("needle-marker", cx);
        window.render_frame(cx);

        let target_action = view
            .read(cx)
            .palette_groups
            .iter()
            .flat_map(|(_, entries)| entries)
            .find(|entry| entry.label.contains("needle-marker"))
            .unwrap()
            .action
            .clone();
        assert!(matches!(target_action, super::super::PaletteAction::JumpToTranscriptItem { .. }));
        assert_eq!(view.read(cx).palette.as_ref().unwrap().read(cx).matched_count(), 1);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let view_state = view.read(cx);
        assert!(view_state.palette.is_none());
        assert!(view_state.transcript.get(40).unwrap().activity_expanded);
        assert!(view_state.transcript.get(41).unwrap().activity_expanded);
        assert!(view_state.transcript.get(41).unwrap().expanded);
        let scroll_top = view_state.message_scroller.read(cx).logical_scroll_top().item_ix;
        assert!(scroll_top > 0 && scroll_top <= 41, "tool result should move into the viewport: {scroll_top}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_search_finds_restored_tool_output_and_opens_it_in_the_inspector(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let session_id = "output-search-thread".to_string();
    view.update(cx, |view, _| {
        view.selected_session_id = Some(session_id.clone());
        let mut item = TranscriptItem::new(
            "archived-command".into(),
            "tool".into(),
            "Run command".into(),
            "completed\n\ncargo check".into(),
        );
        item.output = Some("archived-only-output-marker\ncompiler finished".into());
        view.restored_activity.push(item);
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_conversation_search(session_id.clone(), window, cx));
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("archived-only-output-marker", cx);
        window.render_frame(cx);

        let search = view.read(cx).palette.as_ref().unwrap().clone();
        assert_eq!(search.read(cx).matched_count(), 1);
        let target_action = view
            .read(cx)
            .palette_groups
            .iter()
            .flat_map(|(_, entries)| entries)
            .find(|entry| entry.label.contains("archived-only-output-marker"))
            .unwrap()
            .action
            .clone();
        assert!(matches!(target_action, super::super::PaletteAction::ShowActivityOutput { .. }));
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let view_state = view.read(cx);
        assert!(view_state.palette.is_none());
        assert!(view_state.inspector_open);
        assert_eq!(view_state.inspector_tab, super::super::inspector::InspectorTab::Output);
        assert_eq!(view_state.selected_output_id.as_deref(), Some("archived-command"));
        let output = super::super::inspector::output_for(
            &view_state.transcript,
            &view_state.restored_activity,
            view_state.selected_output_id.as_deref(),
        )
        .unwrap();
        assert!(output.text.contains("archived-only-output-marker"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_search_loads_earlier_history_and_keeps_the_search_open(cx: &mut TestAppContext) {
    use vtcode_desktop_protocol::{SessionHistoryMessage, SessionHistoryPage};

    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(2);
    let session_id = "search-archive-thread".to_string();
    view.update(cx, |view, _| {
        view.backend = Some(BackendHandle { commands });
        view.selected_session_id = Some(session_id.clone());
        view.history_start_index = 2;
        view.history_has_earlier = true;
        view.transcript.push(TranscriptItem::new(
            "current-message".into(),
            "assistant".into(),
            "VT Code".into(),
            "Current page only".into(),
        ));
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_conversation_search(session_id.clone(), window, cx));
        window.render_frame(cx);
        assert_eq!(window.find("conversation-search-load-earlier").role(), Some(gpui_kit::Role::Button));
        window.click("conversation-search-load-earlier", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "loading older messages keeps search open");
        assert!(view.read(cx).history_page_loading);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("earlier history request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_HISTORY);
    assert_eq!(params["sessionId"], session_id);
    assert_eq!(params["beforeIndex"], 2);
    let request_id = params["requestId"].as_str().unwrap().to_string();
    let page = SessionHistoryPage {
        session_id: session_id.clone(),
        request_id,
        start_index: 1,
        messages: vec![SessionHistoryMessage {
            hidden: false,
            archive_index: Some(1),
            role: "tool".into(),
            text: "Archived tool entry contains the needle".into(),
        }],
        has_earlier: true,
    };
    view.update(cx, |view, cx| view.begin_session_history_page(json!(page), cx));
    cx.run_until_parked();

    let mut target_row_id = None;
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "loading an archive page keeps the search palette open");
        assert_eq!(view.read(cx).history_start_index, 1);
        assert!(window.try_find("conversation-search-load-earlier").is_some());
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("needle", cx);
        window.render_frame(cx);

        let palette = view.read(cx).palette.as_ref().unwrap().clone();
        let target_action = view
            .read(cx)
            .palette_groups
            .iter()
            .flat_map(|(_, entries)| entries)
            .find(|entry| {
                entry.label.contains("Archived tool entry")
                    && matches!(&entry.action, super::super::PaletteAction::JumpToTranscriptItem { .. })
            })
            .unwrap()
            .action
            .clone();
        let super::super::PaletteAction::JumpToTranscriptItem { item_id, .. } = &target_action else {
            panic!("message result action")
        };
        target_row_id = Some(format!("transcript-row-{item_id}"));
        assert_eq!(palette.read(cx).matched_count(), 1);
        assert!(window.try_find(palette_action_id(&target_action)).is_some());
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();

    cx.run_until_parked();
    let target_row_id = target_row_id.unwrap();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let view_state = view.read(cx);
        assert!(view_state.palette.is_none());
        assert_eq!(view_state.message_scroller.read(cx).logical_scroll_top().item_ix, 0);
        assert!(window.try_find(target_row_id.clone()).is_some());
        let tool = view_state.transcript.get(0).unwrap();
        assert!(tool.expanded);
        assert!(window.try_find(format!("tool-output-{}", tool.id)).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn conversation_search_shortcut_opens_the_loaded_message_palette(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.selected_session_id = Some("search-shortcut-thread".into());
        view.transcript.push(TranscriptItem::new(
            "message-shortcut".into(),
            "user".into(),
            "You".into(),
            "Searchable message".into(),
        ));
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("cmd-shift-f", cx);
        window.render_frame(cx);
        assert_eq!(window.find("command-palette").label(), Some("Find in conversation"));
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).palette_focus.contains_focused(window, cx));
    })
    .unwrap();
}

#[test]
fn conversation_search_snippet_is_single_line_and_bounded() {
    assert_eq!(super::super::transcript_search_snippet("\n  First line\nsecond line"), "First line");
    assert_eq!(super::super::transcript_search_snippet(&"x".repeat(98)), format!("{}…", "x".repeat(96)));
}

#[gpui_kit::test]
fn config_target_switch_flips_selected_accessibility_state(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
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
        assert_eq!(window.find("config-target-user").checked(), Some(false));
        assert_eq!(window.find("config-target-workspace").checked(), Some(true));
        assert_eq!(window.find("config-target-user").label(), Some("Write CLI settings to User configuration"));
        window.click("config-target-user", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).config_target, vtcode_desktop_protocol::ConfigTarget::User);
        assert_eq!(window.find("config-target-user").checked(), Some(true));
        assert_eq!(
            window.find("config-target-user").label(),
            Some("Write CLI settings to User configuration (selected)")
        );
        assert_eq!(window.find("config-target-workspace").checked(), Some(false));
    })
    .unwrap();
}
