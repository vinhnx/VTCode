use super::*;

#[test]
fn deeplink_paths_cover_every_route_tab_palette_and_command() {
    use settings::SettingsTab;
    // Routes, exact forms only.
    assert_eq!(parse("workspace"), Some(DeepLink::Route(Route::Workspace)));
    assert_eq!(parse("settings"), Some(DeepLink::Route(Route::Settings)));
    assert_eq!(parse("browser"), Some(DeepLink::Route(Route::Browser)));
    assert_eq!(parse("review"), Some(DeepLink::Route(Route::Review)));
    // Every settings tab by canonical name.
    for (name, tab) in [
        ("appearance", SettingsTab::Appearance),
        ("accessibility", SettingsTab::Accessibility),
        ("shortcuts", SettingsTab::Shortcuts),
        ("vtcode", SettingsTab::VtCode),
        ("connections", SettingsTab::Connections),
        ("notifications", SettingsTab::Notifications),
        ("storage", SettingsTab::Storage),
        ("about", SettingsTab::About),
    ] {
        assert_eq!(parse(&format!("settings/{name}")), Some(DeepLink::SettingsTab(tab)), "{name}");
    }
    // Aliases share the slash table, including storage coverage.
    assert_eq!(parse("settings/keybindings"), Some(DeepLink::SettingsTab(SettingsTab::Shortcuts)));
    assert_eq!(parse("settings/disk"), Some(DeepLink::SettingsTab(SettingsTab::Storage)));
    // Config page and paths.
    assert_eq!(parse("config"), Some(DeepLink::ConfigPage));
    assert_eq!(parse("config/agent.default_model"), Some(DeepLink::ConfigPath("agent.default_model".into())));
    // Every palette kind.
    for (name, kind) in [
        ("commands", PaletteLink::Commands),
        ("conversation-search", PaletteLink::ConversationSearch),
        ("models", PaletteLink::Models),
        ("model-options", PaletteLink::ModelOptions),
        ("add-context", PaletteLink::AddContext),
        ("permissions", PaletteLink::Permissions),
        ("modes", PaletteLink::Modes),
        ("fonts/ui", PaletteLink::Fonts(FontTarget::Ui)),
        ("fonts/code", PaletteLink::Fonts(FontTarget::Code)),
        ("settings-search", PaletteLink::SettingsSearch),
        ("config-settings", PaletteLink::ConfigSettings),
        ("prompt-history", PaletteLink::PromptHistory),
    ] {
        assert_eq!(parse(&format!("palette/{name}")), Some(DeepLink::Palette(kind)), "{name}");
    }
    assert_eq!(PaletteLink::ALL.len(), 12);
    // Every shortcut command id round-trips.
    for id in shortcuts::CommandId::ALL {
        assert_eq!(parse(&format!("command/{}", id.as_str())), Some(DeepLink::Command(id)), "{}", id.as_str());
    }
    // Representative actions, parameterless and parameterized.
    assert_eq!(parse("action/toggle-zen"), Some(DeepLink::Action(PaletteAction::ToggleZenMode)));
    assert_eq!(parse("action/toggle-computer-use"), Some(DeepLink::Action(PaletteAction::ToggleComputerUse)));
    assert_eq!(parse("settings/computer-use"), Some(DeepLink::SettingsTab(SettingsTab::ComputerUse)));
    assert_eq!(parse("action/open-in-cli"), Some(DeepLink::Action(PaletteAction::OpenInCli)));
    assert_eq!(parse("action/load-session/abc"), Some(DeepLink::Action(PaletteAction::LoadSession("abc".into()))));
    assert_eq!(
        parse("action/preview-file/src/main.rs"),
        Some(DeepLink::Action(PaletteAction::PreviewFile("src/main.rs".into())))
    );
    assert_eq!(
        parse("action/select-model/openai/gpt-5"),
        Some(DeepLink::Action(PaletteAction::SelectModel { provider: "openai".into(), model: "gpt-5".into() }))
    );
    assert_eq!(
        parse("action/font/code"),
        Some(DeepLink::Action(PaletteAction::SelectFont { target: FontTarget::Code, family: None }))
    );
    assert_eq!(
        parse("action/jump/sess/item-1"),
        Some(DeepLink::Action(PaletteAction::JumpToTranscriptItem {
            session_id: "sess".into(),
            item_id: "item-1".into(),
            from_search: false,
        }))
    );
}

#[test]
fn deeplink_parse_is_lenient_on_form_and_strict_on_content() {
    use settings::SettingsTab;
    // Lenient: scheme prefix (any case), surrounding whitespace and
    // slashes, verb casing. Payloads keep their case.
    assert_eq!(parse("vtcode://settings/shortcuts"), parse("settings/shortcuts"));
    assert_eq!(parse("VTCODE://settings/shortcuts"), parse("settings/shortcuts"));
    assert_eq!(parse("  settings/shortcuts  "), parse("settings/shortcuts"));
    assert_eq!(parse("/settings/shortcuts/"), parse("settings/shortcuts"));
    assert_eq!(parse("settings/"), parse("settings"));
    assert_eq!(parse("action/font/ui/"), parse("action/font/ui"));
    assert_eq!(parse("SETTINGS/SHORTCUTS"), Some(DeepLink::SettingsTab(SettingsTab::Shortcuts)));
    assert_eq!(
        parse("action/preview-file/SRC/App.TS"),
        Some(DeepLink::Action(PaletteAction::PreviewFile("SRC/App.TS".into())))
    );
    assert_eq!(
        parse("action/select-model/OpenAI/GPT-5"),
        Some(DeepLink::Action(PaletteAction::SelectModel { provider: "OpenAI".into(), model: "GPT-5".into() }))
    );
    // Strict: unknown verbs and names, missing or empty payloads,
    // trailing segments on fixed forms, and control characters.
    for bad in [
        "",
        "   ",
        "nope",
        "workspace/extra",
        "settings/nope",
        "settings/shortcuts/extra",
        "command/",
        "command/nope",
        "command/commands/extra",
        "palette/",
        "palette/nope",
        "palette/commands/extra",
        "palette/fonts",
        "palette/fonts/nope",
        "action/",
        "action/nope",
        "action/toggle-zen/extra",
        "action/load-session",
        "action/load-session/",
        "action/load-session/a/b",
        "action/conversation-search/a/b",
        "action/slash/a/b",
        "action/preview-file",
        "action/preview-file//x",
        "action/select-model/only-provider",
        "action/select-model/a/b/c",
        "action/jump/only-session",
        "action/font",
        "action/font/nope",
        "action/font/ui//x",
        "config//x",
        "set\x07tings",
    ] {
        assert_eq!(parse(bad), None, "{bad:?}");
    }
}

#[test]
fn deeplink_paths_round_trip_and_exclude_non_addressable_actions() {
    use settings::SettingsTab;
    use shortcuts::CommandId;
    let links = [
        DeepLink::Route(Route::Review),
        DeepLink::SettingsTab(SettingsTab::Storage),
        DeepLink::ConfigPage,
        DeepLink::ConfigPath("agent.default_model".into()),
        DeepLink::Palette(PaletteLink::Fonts(FontTarget::Code)),
        DeepLink::Command(CommandId::ZenMode),
        DeepLink::Action(PaletteAction::ToggleInspector),
        DeepLink::Action(PaletteAction::PreviewFile("src/main.rs".into())),
        DeepLink::Action(PaletteAction::CopyThreadTitle),
        DeepLink::Action(PaletteAction::CopyFilePath),
        DeepLink::Action(PaletteAction::CloseOtherTabs),
        DeepLink::Changelog,
    ];
    for link in links {
        let path = link.path().expect("addressable links have paths");
        assert_eq!(parse(&path), Some(link), "{path}");
    }
    for id in CommandId::ALL {
        let link = DeepLink::Command(id);
        assert_eq!(parse(&link.path().unwrap()), Some(link));
    }
    // Non-addressable actions have no link form.
    assert_eq!(DeepLink::Action(PaletteAction::InsertDraft("hi".into())).path(), None);
    assert_eq!(DeepLink::Action(PaletteAction::InsertHistoryPrompt("hi".into())).path(), None);
    assert_eq!(DeepLink::Action(PaletteAction::SelectSessionOptions(Default::default())).path(), None);
    assert_eq!(
        DeepLink::Action(PaletteAction::OpenDeepLink(Box::new(DeepLink::Route(Route::Workspace)))).path(),
        None
    );
    assert_eq!(DeepLink::Action(PaletteAction::OpenGoTo).path(), None);
    // Overlapping actions canonicalize to their settings/config forms.
    assert_eq!(
        DeepLink::Action(PaletteAction::SelectSettingsTab(SettingsTab::Shortcuts))
            .path()
            .as_deref(),
        Some("settings/shortcuts")
    );
    assert_eq!(
        DeepLink::Action(PaletteAction::EditConfigSetting("agent.x".into()))
            .path()
            .as_deref(),
        Some("config/agent.x")
    );
}

#[test]
fn notification_target_paths_round_trip_through_parse() {
    let targets = [
        NotificationTarget::Thread { session_id: "thread-a".into() },
        NotificationTarget::Interaction {
            session_id: "thread-a".into(),
            interaction_id: "prompt-1".into(),
        },
        NotificationTarget::Workspace,
        NotificationTarget::UpdateSettings,
    ];
    for target in targets {
        let path = target.path().expect("every target has a link form");
        let link = parse(&path).expect("target paths parse as deeplinks");
        assert_eq!(link.path().as_deref(), Some(path.as_str()), "re-parsed link keeps its spelling");
    }
    // Asymmetric by design: an interaction shares its thread link (the
    // waiting-prompt focus has no link form), while workspace and update
    // settings address distinctly.
    assert_eq!(
        NotificationTarget::Thread { session_id: "s".into() }.path(),
        NotificationTarget::Interaction { session_id: "s".into(), interaction_id: "i".into() }.path()
    );
    assert_eq!(
        NotificationTarget::Thread { session_id: "s".into() }.path().as_deref(),
        Some("action/load-session/s")
    );
    assert_eq!(NotificationTarget::Workspace.path().as_deref(), Some("workspace"));
    // The update toast lands on About → Update status, never on the
    // changelog viewer for a release that is not installed yet.
    assert_eq!(NotificationTarget::UpdateSettings.path().as_deref(), Some("settings/about"));
}
