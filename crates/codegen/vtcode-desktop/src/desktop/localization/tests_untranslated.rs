//! Resolution pins for English-literal copy: keys that stay English in
//! every locale until full translations ship. Translated keys belong in
//! the per-namespace `assert_ne!` lists instead; move these there (and
//! delete this file) once translators cover them.

use super::UiLanguage;

#[test]
fn discard_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: every locale
    // carries the English text, so this pins resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.discard_title",
            "app.settings.discard_detail",
            "app.configuration.discard_title",
            "app.files.discard_title",
            "app.files.discard_detail",
            "app.navigation.discard",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
        let detail = rust_i18n::t!("app.configuration.discard_detail", locale = locale, path = "demo.path").to_string();
        assert!(detail.contains("demo.path"), "{locale} dropped the path");
    }
    assert_eq!(rust_i18n::t!("app.navigation.discard", locale = "en").to_string(), "Discard");
}

#[test]
fn computer_use_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.tab.computer_use",
            "app.settings.computer_use.group",
            "app.settings.computer_use.enable",
            "app.settings.computer_use.enable_description",
            "app.settings.computer_use.decisions",
            "app.settings.computer_use.decisions_description",
            "app.settings.computer_use.target",
            "app.settings.computer_use.image",
            "app.settings.computer_use.limits",
            "app.settings.computer_use.not_configured",
            "app.settings.computer_use.configure",
            "app.settings.computer_use.status_on",
            "app.settings.computer_use.status_off",
            "app.settings.computer_use.guide_title",
            "app.settings.computer_use.guide_setup",
            "app.settings.computer_use.guide_run",
            "app.settings.computer_use.guide_safety",
            "app.settings.computer_use.guide_models",
            "app.settings.computer_use.unavailable",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
    }
    assert_eq!(rust_i18n::t!("app.settings.tab.computer_use", locale = "en").to_string(), "Computer use");
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let text = rust_i18n::t!("app.settings.computer_use.status_aria", locale = locale, state = "On").to_string();
        assert!(!text.is_empty(), "{locale}/status_aria");
        assert!(text.contains("On"), "{locale}/status_aria keeps its interpolation: {text}");
    }
}

#[test]
fn computer_use_guide_models_names_every_supported_provider() {
    // The guide note is static locale prose; native route admission is the
    // source of truth. Pin them together so adding a route fails here until
    // the guide names its provider.
    let routes = vtcode_computer_use::gate::NATIVE_ROUTES;
    assert!(!routes.is_empty(), "a supported-list note needs at least one route");
    let text = rust_i18n::t!("app.settings.computer_use.guide_models", locale = "en").to_string();
    let lowered = text.to_lowercase();
    for (provider, _protocol) in routes {
        let provider = provider.to_string().to_lowercase();
        assert!(lowered.contains(&provider), "guide note must name provider {provider}: {text}");
    }
}

#[test]
fn login_items_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.login_items.title",
            "app.settings.login_items.description",
            "app.settings.login_items.approval_title",
            "app.settings.login_items.approval_hint",
            "app.settings.login_items.open_settings",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
        let failure =
            rust_i18n::t!("app.settings.login_items.unavailable", locale = locale, error = "EACCES").to_string();
        assert!(failure.contains("EACCES"), "{locale} dropped the error");
    }
    assert_eq!(rust_i18n::t!("app.settings.login_items.title", locale = "en").to_string(), "Launch at login");
}

#[test]
fn swipe_navigation_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in ["app.navigation.swipe_back_to", "app.navigation.swipe_forward_to"] {
            let text = rust_i18n::t!(key, locale = locale, thread = "Demo thread").to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
            assert!(text.contains("Demo thread"), "{locale}/{key} dropped the thread name");
        }
    }
    assert_eq!(
        rust_i18n::t!("app.navigation.swipe_back_to", locale = "en", thread = "Demo").to_string(),
        "Back to Demo"
    );
    assert_eq!(
        rust_i18n::t!("app.navigation.swipe_forward_to", locale = "en", thread = "Demo").to_string(),
        "Forward to Demo"
    );
}

#[test]
fn update_install_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: every locale
    // carries the English text, so this pins resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.about.install_update",
            "app.settings.about.checking",
            "app.settings.about.verifying",
            "app.settings.about.extracting",
            "app.settings.about.replacing",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
        let progress = rust_i18n::t!(
            "app.settings.about.download_progress",
            locale = locale,
            downloaded = "1.0 MB",
            total = "2.0 MB"
        )
        .to_string();
        assert!(progress.contains("1.0 MB") && progress.contains("2.0 MB"), "{locale} dropped byte counts");
        let unknown =
            rust_i18n::t!("app.settings.about.download_progress_unknown_total", locale = locale, downloaded = "1.0 MB")
                .to_string();
        assert!(unknown.contains("1.0 MB"), "{locale} dropped the byte count");
        let installed = rust_i18n::t!("app.settings.about.installed", locale = locale).to_string();
        assert!(!installed.is_empty(), "{locale}/installed");
        let gated = rust_i18n::t!(
            "app.settings.about.installed_gatekeeper",
            locale = locale,
            done = "Update installed (0.181.8)"
        )
        .to_string();
        assert!(gated.contains("Open Anyway"), "{locale} dropped the approval step");
        let failed = rust_i18n::t!("app.settings.about.install_failed", locale = locale, message = "boom").to_string();
        assert!(failed.contains("boom"), "{locale} dropped the failure message");
    }
    assert_eq!(rust_i18n::t!("app.settings.about.install_update", locale = "en").to_string(), "Install update");
}

#[test]
fn terminal_colors_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.appearance.terminal_colors",
            "app.settings.appearance.terminal_colors_description",
            "app.settings.appearance.terminal_colors_auto",
            "app.settings.appearance.terminal_colors_ciapre",
            "app.settings.appearance.terminal_colors_latte",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
    }
    assert_eq!(
        rust_i18n::t!("app.settings.appearance.terminal_colors", locale = "en").to_string(),
        "Terminal colors"
    );
}

#[test]
fn navigation_terminal_copy_resolves_in_every_language() {
    // English literal by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let text = rust_i18n::t!("app.navigation.terminal", locale = locale).to_string();
        assert!(!text.is_empty(), "{locale}/app.navigation.terminal");
        assert_ne!(text, "app.navigation.terminal", "{locale} echoed the key instead of resolving");
    }
    assert_eq!(rust_i18n::t!("app.navigation.terminal", locale = "en").to_string(), "Terminal");
}

#[test]
fn navigation_terminal_hide_copy_resolves_in_every_language() {
    // English literal by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        let text = rust_i18n::t!("app.navigation.terminal_hide", locale = locale).to_string();
        assert!(!text.is_empty(), "{locale}/app.navigation.terminal_hide");
        assert_ne!(text, "app.navigation.terminal_hide", "{locale} echoed the key instead of resolving");
    }
    assert_eq!(rust_i18n::t!("app.navigation.terminal_hide", locale = "en").to_string(), "Hide terminal");
}

#[test]
fn connections_builtin_terminal_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.connections.terminal_builtin",
            "app.settings.connections.terminal_builtin_run",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
    }
    assert_eq!(
        rust_i18n::t!("app.settings.connections.terminal_builtin", locale = "en").to_string(),
        "Built-in terminal"
    );
    assert_eq!(rust_i18n::t!("app.settings.connections.terminal_builtin_run", locale = "en").to_string(), "Run");
}

#[test]
fn terminal_position_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.appearance.terminal_position",
            "app.settings.appearance.terminal_position_description",
            "app.settings.appearance.terminal_position_bottom",
            "app.settings.appearance.terminal_position_left",
            "app.settings.appearance.terminal_position_right",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
    }
    assert_eq!(
        rust_i18n::t!("app.settings.appearance.terminal_position", locale = "en").to_string(),
        "Terminal position"
    );
}

#[test]
fn chat_width_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.settings.appearance.chat_width",
            "app.settings.appearance.chat_width_description",
            "app.settings.appearance.chat_width_comfortable",
            "app.settings.appearance.chat_width_wide",
            "app.settings.appearance.chat_width_full",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
    }
    assert_eq!(rust_i18n::t!("app.settings.appearance.chat_width", locale = "en").to_string(), "Chat width");
}

#[test]
fn queue_button_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in ["app.composer.queue", "app.composer.queue_tooltip"] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
    }
    assert_eq!(rust_i18n::t!("app.composer.queue", locale = "en").to_string(), "Queue message");
    assert_eq!(rust_i18n::t!("app.composer.queue_tooltip", locale = "en").to_string(), "Queue message (Tab)");
}

#[test]
fn welcome_context_labels_resolve_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.composer.context_workspace",
            "app.composer.context_webmcp",
            "app.composer.context_cli",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
    }
    assert_eq!(rust_i18n::t!("app.composer.context_workspace", locale = "en").to_string(), "Workspace");
    assert_eq!(rust_i18n::t!("app.composer.context_webmcp", locale = "en").to_string(), "WebMCP");
    assert_eq!(rust_i18n::t!("app.composer.context_cli", locale = "en").to_string(), "VT Code CLI");
}

#[test]
fn onboarding_copy_resolves_in_every_language() {
    // English literals by design until full translations ship: this pins
    // resolution (no assert_ne).
    for language in UiLanguage::ALL {
        let locale = language.locale();
        for key in [
            "app.welcome.onboarding_title",
            "app.welcome.onboarding_permissions_label",
            "app.welcome.onboarding_permissions_detail",
            "app.welcome.onboarding_agent_label",
            "app.welcome.onboarding_agent_detail",
            "app.welcome.onboarding_auth_setup_prefix",
            "app.welcome.onboarding_trust_note",
        ] {
            let text = rust_i18n::t!(key, locale = locale).to_string();
            assert!(!text.is_empty(), "{locale}/{key}");
            assert_ne!(text, key, "{locale}/{key} echoed the key instead of resolving");
        }
        let ready = rust_i18n::t!(
            "app.welcome.onboarding_auth_ready",
            locale = locale,
            provider = "openai",
            model = "gpt-6.2-astra"
        )
        .to_string();
        assert!(ready.contains("openai"), "{locale} dropped the provider");
        assert!(ready.contains("gpt-6.2-astra"), "{locale} dropped the model");
    }
    assert_eq!(rust_i18n::t!("app.welcome.onboarding_title", locale = "en").to_string(), "Set up this workspace");
}
