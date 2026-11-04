//! Unit tests for composer controls (split from `composer_controls.rs`).
use gpui_kit::assets::IconName;

use super::settings::SettingsTab;
use super::{
    ComposerTriggerKind, DesktopSlashAction, capitalize_agent_label, composer_trigger, desktop_slash_action, effort_at,
    file_mention_token, permission_indicator, settings_tab_for_name,
};

#[test]
fn composer_triggers_match_cli_file_and_slash_boundaries() {
    let input = "Inspect @src/main.rs";
    let mention = composer_trigger(input, input.len()).expect("file mention trigger");
    assert_eq!(mention.kind, ComposerTriggerKind::FileMention);
    assert_eq!(input.get(mention.range).expect("valid file mention range"), "@src/main.rs");
    assert_eq!(mention.query, "src/main.rs");

    let input = "/rev";
    let command = composer_trigger(input, input.len()).expect("slash command trigger");
    assert_eq!(command.kind, ComposerTriggerKind::SlashCommand);
    assert_eq!(input.get(command.range).expect("valid slash command range"), "/rev");
    assert_eq!(command.query, "rev");

    let input = "/review --last-diff";
    assert!(composer_trigger(input, input.len()).is_none(), "arguments end slash completion");
}

#[test]
fn composer_triggers_ignore_package_scopes_and_inline_email_addresses() {
    for input in [
        "npm install @types/node",
        "npm install @vitejs/plugin-react",
        "pnpm add @fontsource/inter",
        "Send to dev@example.com",
        "prefix@src/main.rs",
    ] {
        assert!(composer_trigger(input, input.len()).is_none(), "unexpected trigger for {input:?}");
    }

    for input in [
        "npm install && inspect @src/main.rs",
        "Package npm is installed; inspect @src/main.rs",
        "Use pnpm for this workspace, then inspect @src/main.rs",
    ] {
        assert!(
            composer_trigger(input, input.len())
                .is_some_and(|trigger| trigger.kind == ComposerTriggerKind::FileMention),
            "package-manager words outside the active command must not suppress {input:?}"
        );
    }
}

#[test]
fn settings_deep_links_cover_every_settings_tab() {
    for (name, expected) in [
        ("appearance", SettingsTab::Appearance),
        ("theme", SettingsTab::Appearance),
        ("accessibility", SettingsTab::Accessibility),
        ("a11y", SettingsTab::Accessibility),
        ("shortcuts", SettingsTab::Shortcuts),
        ("keybindings", SettingsTab::Shortcuts),
        ("vtcode", SettingsTab::VtCode),
        ("general", SettingsTab::VtCode),
        ("connections", SettingsTab::Connections),
        ("mcp", SettingsTab::Connections),
        ("webmcp", SettingsTab::Connections),
        ("notifications", SettingsTab::Notifications),
        ("notification", SettingsTab::Notifications),
        ("alerts", SettingsTab::Notifications),
        ("updates", SettingsTab::Notifications),
    ] {
        assert_eq!(settings_tab_for_name(name), Some(expected), "alias {name:?}");
    }
    assert_eq!(settings_tab_for_name("unknown"), None);
}

#[test]
fn agent_labels_capitalize_cli_names_without_changing_their_identity() {
    for (name, label) in [("auto", "Auto"), ("build", "Build"), ("plan", "Plan"), ("duck", "Duck")] {
        assert_eq!(capitalize_agent_label(name), label);
    }
    assert_eq!(capitalize_agent_label("custom reviewer"), "Custom reviewer");
    assert_eq!(capitalize_agent_label(""), "");
}

#[test]
fn permission_indicators_distinguish_restrictive_and_auto_approval_states() {
    for palette in super::super::themes::all_themes() {
        let colors = gpui_kit::component::theme::ThemeColor {
            foreground: palette.foreground,
            popover: palette.background,
            success: palette.success,
            warning: palette.warning,
            ..Default::default()
        };
        let (ask_icon, ask_color) = permission_indicator(Some(super::protocol::PermissionMode::Ask), colors);
        let (auto_icon, auto_color) =
            permission_indicator(Some(super::protocol::PermissionMode::AutoApproveCommands), colors);

        assert_eq!(ask_icon, IconName::ShieldCheck, "{} uses a protected state icon", palette.id);
        assert_eq!(auto_icon, IconName::ShieldAlert, "{} marks auto approval as elevated", palette.id);
        assert!(
            super::super::themes::contrast_ratio(ask_color, palette.background) >= 4.5,
            "{} Ask indicator must meet AA contrast",
            palette.id
        );
        assert!(
            super::super::themes::contrast_ratio(auto_color, palette.background) >= 4.5,
            "{} Auto indicator must meet AA contrast",
            palette.id
        );
        assert_ne!(ask_color, auto_color, "{} permission states need distinct colors", palette.id);
    }

    let unavailable_colors = gpui_kit::component::theme::ThemeColor::default();
    let (unavailable_icon, unavailable_color) = permission_indicator(None, unavailable_colors);
    assert_eq!(unavailable_icon, IconName::Shield);
    assert_eq!(unavailable_color, unavailable_colors.muted_foreground);
}

#[test]
fn composer_trigger_handles_unicode_text_and_rejects_non_boundary_cursors() {
    let input = "检查 @src/main.rs";
    let trigger = composer_trigger(input, input.len()).expect("unicode prefix does not break file trigger");
    assert_eq!(trigger.query, "src/main.rs");
    assert!(composer_trigger("é/review", 1).is_none(), "a cursor inside a UTF-8 code point is invalid");
}

#[test]
fn desktop_slash_actions_route_native_equivalents_and_config_deep_links() {
    assert_eq!(desktop_slash_action(" /new "), Some(DesktopSlashAction::NewSession));
    assert_eq!(desktop_slash_action("/exit"), Some(DesktopSlashAction::CloseSession));
    assert_eq!(desktop_slash_action("/stop"), Some(DesktopSlashAction::StopTurn));
    assert_eq!(desktop_slash_action("/models"), Some(DesktopSlashAction::ModelPicker));
    assert_eq!(desktop_slash_action("/history"), Some(DesktopSlashAction::History));
    assert_eq!(desktop_slash_action("/splash"), Some(DesktopSlashAction::Splash));
    assert_eq!(desktop_slash_action("/config"), Some(DesktopSlashAction::Configuration(None)));
    assert_eq!(desktop_slash_action("/settings"), Some(DesktopSlashAction::Settings(None)));
    assert_eq!(desktop_slash_action("/config permissions"), Some(DesktopSlashAction::Permissions));
    assert_eq!(
        desktop_slash_action("/config appearance"),
        Some(DesktopSlashAction::Settings(Some(SettingsTab::Appearance,)))
    );
    assert_eq!(
        desktop_slash_action("/config agent.default_model"),
        Some(DesktopSlashAction::Configuration(Some("agent.default_model".into())))
    );
    assert_eq!(
        desktop_slash_action("/config   Tools  "),
        Some(DesktopSlashAction::Configuration(Some("Tools".into())))
    );
    assert_eq!(desktop_slash_action("/files src/ui"), Some(DesktopSlashAction::Files("src/ui".into())));
    assert_eq!(desktop_slash_action("/fast"), Some(DesktopSlashAction::FastMode(None)));
    assert_eq!(desktop_slash_action("/fast off"), Some(DesktopSlashAction::FastMode(Some(false))));
    assert_eq!(desktop_slash_action("/theme ciapre"), Some(DesktopSlashAction::Theme(Some("ciapre".into()))));
    assert_eq!(desktop_slash_action("/theme"), Some(DesktopSlashAction::Theme(None)));
    // Whitespace-only arguments are "no theme", and exactly one token survives
    // the extra separator runs.
    assert_eq!(desktop_slash_action("/theme   "), Some(DesktopSlashAction::Theme(None)));
    assert_eq!(desktop_slash_action("/theme   ciapre   "), Some(DesktopSlashAction::Theme(Some("ciapre".into()))));
    // Two or more tokens fall through to the service instead of routing.
    assert_eq!(desktop_slash_action("/theme one two"), None);
    assert_eq!(desktop_slash_action("/cli"), Some(DesktopSlashAction::Cli));
    assert_eq!(desktop_slash_action("/desktop"), Some(DesktopSlashAction::Desktop));

    for input in [
        "/fast maybe",
        "/config reset",
        "/config reset --global",
        "/config Reset",
        "/config clear",
        "/config checkup",
        "/config checkup --quick",
        "/config CHECKUP",
        "/config doctor",
        "/config memory",
        "/config Memory",
        "/config agent.persistent_memory",
        "/config tasks",
        "/config jobs",
        "/config log",
        "/config notify",
        "/config subprocess",
        "/config subprocesses",
        "/settings reset",
        "/settings agent.default_model",
        "/splash now",
        "/files\nother",
    ] {
        assert_eq!(desktop_slash_action(input), None, "{input:?} stays with the CLI runtime");
    }
}

#[test]
fn desktop_go_routes_deeplinks_and_storage_alias() {
    assert_eq!(
        desktop_slash_action("/go settings/shortcuts"),
        Some(DesktopSlashAction::Go("settings/shortcuts".into()))
    );
    assert_eq!(
        desktop_slash_action("/GO COMMAND/ZEN_MODE"),
        Some(DesktopSlashAction::Go("COMMAND/ZEN_MODE".into()))
    );
    assert_eq!(desktop_slash_action("/go"), Some(DesktopSlashAction::Go(String::new())));
    // /go always routes natively (even unknown links, which report a
    // desktop status error) so garbage never falls through headless.
    assert_eq!(desktop_slash_action("/go nope"), Some(DesktopSlashAction::Go("nope".into())));
    // Storage joins the known settings tabs for every deep-link surface.
    assert_eq!(
        desktop_slash_action("/settings storage"),
        Some(DesktopSlashAction::Settings(Some(SettingsTab::Storage)))
    );
    assert_eq!(desktop_slash_action("/config disk"), Some(DesktopSlashAction::Settings(Some(SettingsTab::Storage))));
}

#[test]
fn desktop_slash_actions_match_cli_case_insensitivity() {
    // The CLI lowercases the command token before dispatch; a capitalized
    // native command must route natively instead of falling through to the
    // headless service (where `/NEW` would end the service-side session
    // while this client keeps showing it as selected).
    for (input, expected) in [
        ("/NEW", DesktopSlashAction::NewSession),
        ("/New", DesktopSlashAction::NewSession),
        ("/EXIT", DesktopSlashAction::CloseSession),
        ("/Stop", DesktopSlashAction::StopTurn),
        ("/MODEL", DesktopSlashAction::ModelPicker),
        ("/Models", DesktopSlashAction::ModelPicker),
        ("/HISTORY", DesktopSlashAction::History),
        ("/Splash", DesktopSlashAction::Splash),
        ("/HELP", DesktopSlashAction::Help),
        ("/Mode", DesktopSlashAction::Modes),
        ("/CLI", DesktopSlashAction::Cli),
        ("/Desktop", DesktopSlashAction::Desktop),
    ] {
        assert_eq!(desktop_slash_action(input), Some(expected), "{input:?} routes natively");
    }
    // Keyword arguments follow the CLI's case-insensitive matching, while
    // free-form values keep their original case for downstream resolvers.
    assert_eq!(desktop_slash_action("/FAST ON"), Some(DesktopSlashAction::FastMode(Some(true))));
    assert_eq!(desktop_slash_action("/Fast Off"), Some(DesktopSlashAction::FastMode(Some(false))));
    assert_eq!(desktop_slash_action("/CONFIG PERMISSIONS"), Some(DesktopSlashAction::Permissions));
    assert_eq!(
        desktop_slash_action("/Config Appearance"),
        Some(DesktopSlashAction::Settings(Some(SettingsTab::Appearance,)))
    );
    assert_eq!(desktop_slash_action("/SETTINGS Model"), Some(DesktopSlashAction::ModelPicker));
    assert_eq!(desktop_slash_action("/THEME Ciapre"), Some(DesktopSlashAction::Theme(Some("Ciapre".into()))));
    assert_eq!(desktop_slash_action("/FILES SRC/UI"), Some(DesktopSlashAction::Files("SRC/UI".into())));
    // Commands whose CLI backend ignores arguments route natively regardless
    // of trailing text, in any case: falling through would hit
    // headless-unsafe handlers (session teardown, picker parking).
    for (input, expected) in [
        ("/new please", DesktopSlashAction::NewSession),
        ("/NEW please", DesktopSlashAction::NewSession),
        ("/exit now", DesktopSlashAction::CloseSession),
        ("/EXIT now", DesktopSlashAction::CloseSession),
        ("/stop now", DesktopSlashAction::StopTurn),
        ("/STOP now", DesktopSlashAction::StopTurn),
        ("/model foo", DesktopSlashAction::ModelPicker),
        ("/MODEL foo bar", DesktopSlashAction::ModelPicker),
        ("/cli please", DesktopSlashAction::Cli),
        ("/CLI PLEASE", DesktopSlashAction::Cli),
        ("/desktop now", DesktopSlashAction::Desktop),
        ("/DESKTOP now", DesktopSlashAction::Desktop),
    ] {
        assert_eq!(desktop_slash_action(input), Some(expected), "{input:?} routes natively");
    }
    // Guards still apply to capitalized input: unknown commands and
    // runtime-owned `/config` sections stay with the CLI.
    for input in [
        "/NEWS",
        "/FAST maybe",
        "/CONFIG RESET",
        "/Settings reset",
        "/history please",
    ] {
        assert_eq!(desktop_slash_action(input), None, "{input:?} stays with the CLI runtime");
    }
}

#[test]
fn file_mentions_quote_paths_for_the_cli_reference_parser() {
    assert_eq!(file_mention_token("src/main.rs").as_deref(), Some("@src/main.rs"));
    assert_eq!(file_mention_token("docs/guide notes.md").as_deref(), Some("@\"docs/guide notes.md\""));
    assert_eq!(file_mention_token("docs/guide's notes.md").as_deref(), Some("@\"docs/guide's notes.md\""));
    assert_eq!(file_mention_token("docs/guide\"s notes.md").as_deref(), Some("@'docs/guide\"s notes.md'"));
    assert!(file_mention_token("docs/a'\"b notes.md").is_none());
    assert!(file_mention_token("docs/line\nbreak.md").is_none());

    for path in [
        "src/main.rs",
        "docs/guide notes.md",
        "docs/guide's notes.md",
        "docs/guide\"s notes.md",
    ] {
        let mention = file_mention_token(path).expect("the path has representable CLI mention syntax");
        let parsed = vtcode_commons::at_pattern::find_at_patterns(&mention);
        assert_eq!(parsed.first().map(|reference| reference.path), Some(path));
    }
}

#[test]
fn effort_knob_respects_discrete_capability_boundaries() {
    let levels = ["low".into(), "medium".into(), "high".into()];
    assert_eq!(effort_at(&levels, 0.49).map(String::as_str), Some("low"));
    assert_eq!(effort_at(&levels, 0.51).map(String::as_str), Some("medium"));
    assert_eq!(effort_at(&levels, 2.).map(String::as_str), Some("high"));
    for value in [-1., 3., f32::NAN, f32::INFINITY] {
        assert!(effort_at(&levels, value).is_none());
    }
}
