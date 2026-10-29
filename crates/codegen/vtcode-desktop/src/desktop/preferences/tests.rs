use super::*;

#[test]
fn test_preferences_paths_are_fresh_and_isolated_per_thread() {
    let first = install_test_preferences_path();
    let current = install_test_preferences_path();
    let other = std::thread::spawn(|| {
        let _path = install_test_preferences_path();
        TEST_PREFERENCES_PATH.with(|slot| slot.borrow().clone().expect("child path installed"))
    })
    .join()
    .expect("child thread returns its preference path");

    assert_ne!(first, current, "each test setup receives a fresh file");
    assert_ne!(current, other, "different test threads do not share preferences");
    assert_eq!(TEST_PREFERENCES_PATH.with(|slot| slot.borrow().clone()), Some(current));
}

#[test]
fn ui_language_round_trips_and_invalid_saved_values_preserve_other_preferences() {
    let old: Preferences = serde_json::from_str(r#"{"theme":"ciapre"}"#).unwrap();
    assert_eq!(old.ui_language(), UiLanguage::English);

    let mut preferences = old;
    preferences.set_ui_language(UiLanguage::Vietnamese);
    let serialized = serde_json::to_string(&preferences).unwrap();
    let loaded: Preferences = serde_json::from_str(&serialized).unwrap();
    assert_eq!(loaded.ui_language(), UiLanguage::Vietnamese);
    assert_eq!(loaded.theme.as_deref(), Some("ciapre"));

    for language in UiLanguage::ALL {
        preferences.set_ui_language(language);
        let serialized = serde_json::to_string(&preferences).unwrap();
        let loaded: Preferences = serde_json::from_str(&serialized).unwrap();
        assert_eq!(loaded.ui_language(), language);
    }

    let invalid: Preferences = serde_json::from_str(r#"{"theme":"ciapre","uiLanguage":"unsupported"}"#).unwrap();
    assert_eq!(invalid.ui_language(), UiLanguage::English);
    assert_eq!(invalid.theme.as_deref(), Some("ciapre"));
}

#[test]
fn file_open_destination_round_trips_and_invalid_values_fall_back_to_viewer() {
    assert_eq!(FileOpenDestination::default(), FileOpenDestination::Viewer);
    let legacy: Preferences = serde_json::from_str(r#"{"theme":"ciapre"}"#).unwrap();
    assert_eq!(legacy.file_open_destination(), FileOpenDestination::Viewer);

    for destination in FileOpenDestination::ALL {
        let mut preferences = Preferences::default();
        preferences.set_file_open_destination(destination);
        let serialized = serde_json::to_string(&preferences).unwrap();
        assert!(
            serialized.contains(&format!("\"fileOpenDestination\":\"{}\"", destination.id())),
            "saved id is stable for {destination:?}: {serialized}"
        );
        let loaded: Preferences = serde_json::from_str(&serialized).unwrap();
        assert_eq!(loaded.file_open_destination(), destination);
        assert_eq!(loaded.file_open_destination, Some(destination.id().to_owned()));
    }

    // Parsing is case-insensitive and trims whitespace, like UI language ids.
    let padded: Preferences = serde_json::from_str(r#"{"fileOpenDestination":"  VSCode "}"#).unwrap();
    assert_eq!(padded.file_open_destination(), FileOpenDestination::VsCode);
    for (saved, expected) in [
        ("VSCode", FileOpenDestination::VsCode),
        ("  system", FileOpenDestination::System),
        ("viewer\t", FileOpenDestination::Viewer),
    ] {
        assert_eq!(FileOpenDestination::from_saved(Some(saved)), expected, "parse {saved:?}");
    }

    // Unknown or empty ids keep every other preference intact.
    for saved in ["emacs", "", "   "] {
        let content = format!(r#"{{"theme":"ciapre","fileOpenDestination":{saved:?}}}"#);
        let loaded: Preferences = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.file_open_destination(), FileOpenDestination::Viewer, "saved {saved:?}");
        assert_eq!(loaded.theme.as_deref(), Some("ciapre"));
    }
}

#[test]
fn file_open_destination_maps_each_id_to_its_external_editor() {
    assert_eq!(FileOpenDestination::Viewer.external_editor(), None);
    for (destination, expected) in [
        (FileOpenDestination::System, ExternalEditor::System),
        (FileOpenDestination::VsCode, ExternalEditor::VsCode),
        (FileOpenDestination::Sublime, ExternalEditor::Sublime),
        (FileOpenDestination::Xcode, ExternalEditor::Xcode),
        (FileOpenDestination::AndroidStudio, ExternalEditor::AndroidStudio),
        (FileOpenDestination::Finder, ExternalEditor::Finder),
        (FileOpenDestination::Terminal, ExternalEditor::Terminal),
        (FileOpenDestination::ITerm2, ExternalEditor::ITerm2),
        (FileOpenDestination::Ghostty, ExternalEditor::Ghostty),
    ] {
        assert_eq!(destination.external_editor(), Some(expected), "map {destination:?}");
        assert!(FileOpenDestination::ALL.contains(&destination));
    }
    assert_eq!(FileOpenDestination::ALL.len(), 10, "extend this table when adding a destination");
}

#[test]
fn offered_destinations_combine_always_available_detected_and_saved() {
    use FileOpenDestination as Destination;

    let base = Destination::offered(Destination::Viewer, &[]);
    assert_eq!(
        base,
        vec![
            Destination::Viewer,
            Destination::System,
            Destination::Finder,
            Destination::Terminal
        ]
    );

    // Detected editors join in canonical order, not detection order.
    let detected = Destination::offered(
        Destination::Viewer,
        &[
            ExternalEditor::Ghostty,
            ExternalEditor::VsCode,
            ExternalEditor::AndroidStudio,
        ],
    );
    assert_eq!(
        detected,
        vec![
            Destination::Viewer,
            Destination::System,
            Destination::VsCode,
            Destination::AndroidStudio,
            Destination::Finder,
            Destination::Terminal,
            Destination::Ghostty,
        ]
    );

    // The saved choice stays selectable even when its app is missing, so
    // the control never renders blank and reinstalls restore behavior.
    let missing = Destination::offered(Destination::Xcode, &[ExternalEditor::VsCode]);
    assert_eq!(
        missing,
        vec![
            Destination::Viewer,
            Destination::System,
            Destination::VsCode,
            Destination::Xcode,
            Destination::Finder,
            Destination::Terminal,
        ]
    );

    // A saved choice that is also detected appears exactly once.
    let present = Destination::offered(Destination::Sublime, &[ExternalEditor::Sublime]);
    assert_eq!(present.iter().filter(|d| **d == Destination::Sublime).count(), 1);
    assert_eq!(Destination::offered(Destination::Viewer, &ExternalEditor::DETECTABLE).len(), 10);
}

#[test]
fn destination_availability_matches_editor_availability() {
    // The viewer has no editor; every other always-available destination
    // maps to an always-available editor, and vice versa.
    for destination in FileOpenDestination::ALWAYS_AVAILABLE {
        match destination.external_editor() {
            None => assert_eq!(destination, FileOpenDestination::Viewer),
            Some(editor) => assert!(
                ExternalEditor::ALWAYS_AVAILABLE.contains(&editor),
                "{destination:?} maps to detected-only {editor:?}"
            ),
        }
    }
    assert_eq!(
        FileOpenDestination::ALWAYS_AVAILABLE.len(),
        ExternalEditor::ALWAYS_AVAILABLE.len() + 1,
        "the viewer is the only destination without an editor"
    );
}

#[gpui_kit::test]
fn system_motion_baseline_initializes_once_without_redundant_global_notifications(cx: &mut gpui_kit::TestAppContext) {
    let notifications = std::rc::Rc::new(std::cell::Cell::new(0));
    let notifications_for_observer = notifications.clone();
    let observer = cx.update(|cx| {
        cx.observe_global::<SystemMotionBaseline>(move |_| {
            notifications_for_observer.set(notifications_for_observer.get() + 1);
        })
    });

    cx.update(|cx| {
        cx.set_reduce_motion(true);
        assert!(system_motion_baseline(cx));
    });
    assert_eq!(notifications.get(), 1);

    cx.update(|cx| {
        cx.set_reduce_motion(false);
        assert!(system_motion_baseline(cx), "the original system setting remains authoritative");
    });
    assert_eq!(notifications.get(), 1, "reading the existing baseline does not notify observers");
    drop(observer);
}

#[test]
fn font_families_round_trip_independently_and_legacy_defaults_remain_optional() {
    let old: Preferences = serde_json::from_str("{}").unwrap();
    assert!(old.ui_font_family.is_none() && old.code_font_family.is_none());
    let selected = Preferences {
        ui_font_family: Some("Helvetica Neue".into()),
        code_font_family: Some("Menlo".into()),
        ..old
    };
    let loaded: Preferences = serde_json::from_str(&serde_json::to_string(&selected).unwrap()).unwrap();
    assert_eq!(loaded.ui_font_family.as_deref(), Some("Helvetica Neue"));
    assert_eq!(loaded.code_font_family.as_deref(), Some("Menlo"));
}

#[test]
fn font_sizes_default_clamp_and_round_trip_independently() {
    let old: Preferences = serde_json::from_str("{\"theme\":\"ciapre\"}").unwrap();
    assert_eq!((old.ui_font_size(), old.code_font_size()), (16, 14));
    let settings = Preferences {
        ui_font_size: Some(0),
        code_font_size: Some(255),
        ..old
    };
    assert_eq!((settings.ui_font_size(), settings.code_font_size()), (14, 24));
    let parsed: Preferences = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
    assert_eq!((parsed.ui_font_size(), parsed.code_font_size()), (14, 24));
}

#[test]
fn reduced_motion_defaults_to_system_and_persists_explicit_choices() {
    let defaults: Preferences = serde_json::from_str("{}").unwrap();
    assert_eq!(defaults.reduced_motion, ReducedMotionPreference::System);

    let selected = Preferences {
        reduced_motion: ReducedMotionPreference::On,
        ..defaults
    };
    let loaded: Preferences = serde_json::from_str(&serde_json::to_string(&selected).unwrap()).unwrap();
    assert_eq!(loaded.reduced_motion, ReducedMotionPreference::On);
}

#[test]
fn separate_theme_profiles_default_off_and_round_trip_independently() {
    let defaults: Preferences = serde_json::from_str("{}").unwrap();
    assert!(!defaults.separate_theme_profiles);
    assert!(defaults.light_theme.is_none() && defaults.dark_theme.is_none());
    assert!(defaults.light_typography.is_none() && defaults.dark_typography.is_none());

    let selected = Preferences {
        separate_theme_profiles: true,
        light_theme: Some("solarized-light".into()),
        dark_theme: Some("ciapre".into()),
        light_typography: Some(TypographyProfile {
            ui_font_size: Some(19),
            ..TypographyProfile::default()
        }),
        dark_typography: Some(TypographyProfile {
            ui_font_size: Some(16),
            ..TypographyProfile::default()
        }),
        ..defaults
    };
    let loaded: Preferences = serde_json::from_str(&serde_json::to_string(&selected).unwrap()).unwrap();
    assert!(loaded.separate_theme_profiles);
    assert_eq!(loaded.light_theme.as_deref(), Some("solarized-light"));
    assert_eq!(loaded.dark_theme.as_deref(), Some("ciapre"));
    assert_eq!(loaded.typography_for(ThemeMode::Light).ui_font_size, 19);
    assert_eq!(loaded.typography_for(ThemeMode::Dark).ui_font_size, 16);
}

#[test]
fn theme_profiles_keep_typography_independent_and_merge_active_profile_on_disable() {
    let mut preferences = Preferences {
        ui_font_family: Some("Helvetica Neue".into()),
        code_font_family: Some("Monaco".into()),
        ui_font_size: Some(16),
        code_font_size: Some(14),
        ..Preferences::default()
    };
    preferences.set_separate_theme_profiles(true, ThemeMode::Dark);
    preferences.set_ui_font_size(ThemeMode::Light, 19);
    preferences.set_code_font_family(ThemeMode::Dark, Some("Menlo".into()));
    preferences.set_ui_font_family(ThemeMode::Light, None);

    let light = preferences.typography_for(ThemeMode::Light);
    let dark = preferences.typography_for(ThemeMode::Dark);
    assert_eq!(light.ui_font_size, 19);
    assert_eq!(dark.ui_font_size, 16);
    assert_eq!(light.ui_font_family, None);
    assert_eq!(dark.ui_font_family, Some("Helvetica Neue"));
    assert_eq!(light.code_font_family, Some("Monaco"));
    assert_eq!(dark.code_font_family, Some("Menlo"));

    preferences.set_separate_theme_profiles(false, ThemeMode::Light);
    assert!(!preferences.separate_theme_profiles);
    assert_eq!(preferences.ui_font_size(), 19);
    assert_eq!(preferences.ui_font_family, None);
    assert_eq!(preferences.code_font_family.as_deref(), Some("Monaco"));
    assert!(preferences.light_typography.is_none() && preferences.dark_typography.is_none());
}

#[test]
fn profile_sizes_are_clamped_and_legacy_shared_typography_is_inherited() {
    let mut preferences: Preferences =
        serde_json::from_str(r#"{"separateThemeProfiles":true,"uiFontSize":18,"codeFontSize":13}"#).unwrap();
    assert_eq!(preferences.typography_for(ThemeMode::Light).ui_font_size, 18);
    assert_eq!(preferences.typography_for(ThemeMode::Dark).code_font_size, 13);
    preferences.set_ui_font_size(ThemeMode::Dark, 0);
    preferences.set_code_font_size(ThemeMode::Dark, 255);
    let dark = preferences.typography_for(ThemeMode::Dark);
    assert_eq!((dark.ui_font_size, dark.code_font_size), (14, 24));
    let light = preferences.typography_for(ThemeMode::Light);
    assert_eq!((light.ui_font_size, light.code_font_size), (18, 13));
}

#[test]
fn touch_workspace_moves_to_front_and_bounds_the_list() {
    let mut preferences = Preferences::default();
    for index in 0..(MAX_RECENT_WORKSPACES + 4) {
        preferences.touch_workspace(&format!("/ws-{index}"));
    }
    assert_eq!(preferences.recent_workspaces.len(), MAX_RECENT_WORKSPACES);
    assert_eq!(preferences.recent_workspaces[0], format!("/ws-{}", MAX_RECENT_WORKSPACES + 3));

    preferences.touch_workspace("/ws-5");
    assert_eq!(preferences.recent_workspaces[0], "/ws-5");
    assert_eq!(
        preferences.recent_workspaces.iter().filter(|path| **path == "/ws-5").count(),
        1,
        "re-touching moves the entry instead of duplicating it"
    );
}

#[test]
fn widths_are_clamped_to_usable_bounds() {
    let mut preferences = Preferences::default();
    assert!((preferences.sidebar_width() - DEFAULT_SIDEBAR_WIDTH).abs() < f32::EPSILON);
    assert!((preferences.inspector_width() - DEFAULT_INSPECTOR_WIDTH).abs() < f32::EPSILON);

    preferences.set_widths(40.0, 2_000.0);
    assert!((preferences.sidebar_width() - MIN_PANE_WIDTH).abs() < f32::EPSILON);
    assert!((preferences.inspector_width() - MAX_PANE_WIDTH).abs() < f32::EPSILON);
}

#[test]
fn window_maximized_defaults_to_true_and_round_trips() {
    let legacy: Preferences = serde_json::from_str("{}").unwrap();
    assert!(legacy.window_maximized(), "fresh installs launch maximized");

    let mut preferences = Preferences::default();
    preferences.set_window_maximized(false);
    let serialized = serde_json::to_string(&preferences).unwrap();
    assert!(serialized.contains("\"windowMaximized\":false"), "saved key is stable: {serialized}");
    let loaded: Preferences = serde_json::from_str(&serialized).unwrap();
    assert!(!loaded.window_maximized());

    preferences.set_window_maximized(true);
    let loaded: Preferences = serde_json::from_str(&serde_json::to_string(&preferences).unwrap()).unwrap();
    assert!(loaded.window_maximized());
}

#[test]
fn round_trips_through_json() {
    let mut preferences = Preferences {
        theme: Some("ciapre".to_string()),
        inspector_open: Some(false),
        navigation_on_right: Some(false),
        navigation_visible: Some(false),
        ..Preferences::default()
    };
    preferences.touch_workspace("/tmp/ws");
    preferences.set_widths(240.0, 380.0);

    let content = serde_json::to_string(&preferences).expect("serialize");
    let parsed: Preferences = serde_json::from_str(&content).expect("deserialize");
    assert_eq!(parsed.theme.as_deref(), Some("ciapre"));
    assert_eq!(parsed.recent_workspaces, vec!["/tmp/ws"]);
    assert_eq!(parsed.inspector_open, Some(false));
    assert!(!parsed.navigation_on_right());
    assert!(!parsed.navigation_visible());
    assert!((parsed.sidebar_width() - 240.0).abs() < f32::EPSILON);
    assert!((parsed.inspector_width() - 380.0).abs() < f32::EPSILON);

    let empty: Preferences = serde_json::from_str("{}").expect("defaults");
    assert_eq!(empty.theme, None);
    assert!(empty.recent_workspaces.is_empty());
    assert!(empty.navigation_on_right(), "new and legacy preferences default navigation to the right");
    assert!(empty.navigation_visible(), "new and legacy preferences show navigation by default");
}

#[test]
fn notification_preferences_default_on_and_round_trip() {
    // Legacy files without the block keep every category enabled and
    // turn-terminal notifications background-only.
    let legacy: Preferences = serde_json::from_str(r#"{"theme":"ciapre"}"#).unwrap();
    let notifications = &legacy.notifications;
    assert!(notifications.enabled());
    assert_eq!(notifications.turn_completion(), TurnCompletionMode::Unfocused);
    assert!(notifications.permission_prompts());
    assert!(notifications.questions());
    assert!(notifications.updates());
    assert_eq!(notifications.last_notified_update, None);

    // Asymmetric fixtures: explicit choices survive a round trip while
    // untouched toggles keep their defaults.
    let mut preferences = Preferences::default();
    preferences.notifications.enabled = Some(false);
    preferences.notifications.set_turn_completion(TurnCompletionMode::Always);
    preferences.notifications.questions = Some(false);
    preferences.notifications.last_notified_update = Some("0.181.0".to_string());
    let serialized = serde_json::to_string(&preferences).unwrap();
    assert!(serialized.contains(r#""turnCompletion":"always""#), "{serialized}");
    let loaded: Preferences = serde_json::from_str(&serialized).unwrap();
    assert!(!loaded.notifications.enabled());
    assert_eq!(loaded.notifications.turn_completion(), TurnCompletionMode::Always);
    assert!(loaded.notifications.permission_prompts());
    assert!(!loaded.notifications.questions());
    assert!(loaded.notifications.updates());
    assert_eq!(loaded.notifications.last_notified_update.as_deref(), Some("0.181.0"));
}

#[test]
fn turn_completion_mode_parsing_is_case_insensitive_with_unfocused_fallback() {
    assert_eq!(TurnCompletionMode::default(), TurnCompletionMode::Unfocused);
    for mode in TurnCompletionMode::ALL {
        assert_eq!(TurnCompletionMode::from_saved(Some(mode.id())), mode);
    }
    assert_eq!(TurnCompletionMode::from_saved(Some("  ALWAYS ")), TurnCompletionMode::Always);
    assert_eq!(TurnCompletionMode::from_saved(Some("Never")), TurnCompletionMode::Never);
    for saved in [None, Some(""), Some("sometimes")] {
        assert_eq!(TurnCompletionMode::from_saved(saved), TurnCompletionMode::Unfocused, "{saved:?}");
    }
}
