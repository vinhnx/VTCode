//! Custom desktop themes: creation, rename/validation, deletion, profiles,
//! persistence, and the desktop-only CLI boundary.

use super::*;

fn open_appearance(window: &mut gpui_kit::Window, view: &Entity<DesktopView>, cx: &mut gpui_kit::App) {
    view.update(cx, |view, cx| view.open_settings(window, cx));
    window.render_frame(cx);
    assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::Appearance);
}

fn set_name(view: &Entity<DesktopView>, name: &str, window: &mut gpui_kit::Window, cx: &mut gpui_kit::App) {
    view.update(cx, |view, cx| {
        view.theme_editor
            .name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
    });
}

#[gpui_kit::test]
fn custom_theme_saves_applies_persists_and_reloads(cx: &mut TestAppContext) {
    let prefs_path = crate::desktop::preferences::install_test_preferences_path();
    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path.clone());
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        open_appearance(window, &view, cx);
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
        window.render_frame(cx);
        assert!(window.try_find("custom-theme-editor").is_some(), "the inline editor opens");
        set_name(&view, "Ocean Night", window, cx);
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
        window.render_frame(cx);

        {
            let view = view.read(cx);
            assert_eq!(view.preferences.custom_themes.len(), 1);
            let saved = &view.preferences.custom_themes[0];
            assert_eq!(saved.id, "custom-ocean-night");
            assert_eq!(saved.name, "Ocean Night");
            assert_eq!(view.theme_id, saved.id, "saving applies the new theme");
            assert_eq!(view.theme_mode, saved.mode);
        }
        assert_eq!(window.find("custom-theme-apply-custom-ocean-night").checked(), Some(true));
        assert!(window.try_find("custom-theme-editor").is_none(), "editor closes after save");
    })
    .unwrap();

    let raw = std::fs::read_to_string(&prefs_path).expect("saving writes the preferences file");
    let saved: serde_json::Value = serde_json::from_str(&raw).expect("preferences stay valid JSON");
    let themes = saved
        .pointer("/customThemes")
        .and_then(serde_json::Value::as_array)
        .expect("custom themes persist");
    assert_eq!(themes.len(), 1);
    assert_eq!(themes[0].get("id").and_then(serde_json::Value::as_str), Some("custom-ocean-night"));

    // A relaunch over the same file resolves the custom id instead of falling
    // back to the palette registry.
    let (relaunched_window, relaunched) = open_desktop_with_preferences_path(cx, prefs_path);
    cx.update_window(relaunched_window, |_, window, cx| {
        relaunched.update(cx, |view, cx| view.load_preferences(window, cx));
        let view = relaunched.read(cx);
        assert_eq!(view.theme_id, "custom-ocean-night");
        assert_eq!(view.preferences.custom_themes.len(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn custom_theme_rename_keeps_its_id_and_duplicate_names_are_rejected(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        open_appearance(window, &view, cx);
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
        set_name(&view, "Ocean Night", window, cx);
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));

        // Rename the saved row: the id stays stable so profiles and the
        // current selection keep resolving.
        view.update(cx, |view, cx| view.edit_custom_theme("custom-ocean-night", window, cx));
        set_name(&view, "Ocean Dusk", window, cx);
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
        {
            let view = view.read(cx);
            assert_eq!(view.preferences.custom_themes.len(), 1);
            assert_eq!(view.preferences.custom_themes[0].id, "custom-ocean-night");
            assert_eq!(view.preferences.custom_themes[0].name, "Ocean Dusk");
            assert_eq!(view.theme_id, "custom-ocean-night");
        }

        // A duplicate name is rejected case-insensitively and leaves the list
        // untouched.
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
        set_name(&view, "ocean dusk", window, cx);
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
        {
            let view = view.read(cx);
            assert_eq!(view.preferences.custom_themes.len(), 1, "duplicate name is not saved");
            let draft = view.theme_editor.draft.as_ref().expect("editor stays open on error");
            assert!(draft.error.is_some(), "the editor reports the duplicate");
        }
        window.render_frame(cx);
        assert!(window.find("custom-theme-error").bounds().size.height > px(0.), "the error row is laid out");
    })
    .unwrap();
}

/// The name field feeds the live preview label: typing must repaint without a
/// save, and the seed-on-open `set_value` must not leak a stale draft name.
#[gpui_kit::test]
fn custom_theme_editor_previews_the_typed_name_live(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        open_appearance(window, &view, cx);
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_editor.name_input.read(cx).value().as_str(), "");

        // A real typed edit re-renders the preview label from the input value.
        window.click("custom-theme-name", cx);
        window.input("Ocean", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).theme_editor.name_input.read(cx).value().as_str(), "Ocean");
        // The same text drives the save, so the persisted name matches.
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
        assert_eq!(view.read(cx).preferences.custom_themes.first().map(|theme| theme.name.as_str()), Some("Ocean"));
    })
    .unwrap();
}

/// A color picked in the editor must reach the staged seeds through the picker
/// subscription, and a save must persist exactly that value.
#[gpui_kit::test]
fn custom_theme_editor_commits_picked_colors(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        open_appearance(window, &view, cx);
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
        window.render_frame(cx);

        let accent = gpui_kit::rgb(0x2f9e6b);
        // Slot 2 is the accent seed; `select_color` is the commit path a palette
        // swatch click uses, and it emits `ColorPickerEvent::Change`.
        let picker = view.read(cx).theme_editor.pickers[2].clone();
        picker.update(cx, |state, cx| state.select_color(accent.into(), window, cx));
        assert_eq!(picker.read(cx).value(), Some(accent.into()), "picker committed");
        set_name(&view, "Picked", window, cx);
    })
    .unwrap();
    // gpui dispatches emitted events when the outermost update flushes, so the
    // subscription lands after `update_window` returns, not inside it.
    cx.run_until_parked();
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).theme_editor.draft.as_ref().unwrap().seeds.accent,
            gpui_kit::rgb(0x2f9e6b).into(),
            "the picker subscription copies the committed color into the draft"
        );
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).preferences.custom_themes[0].accent,
            gpui_kit::rgb(0x2f9e6b).into(),
            "the save persists the picked seed"
        );
    });
}

#[gpui_kit::test]
fn custom_theme_rejects_unreadable_text_and_empty_names(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        open_appearance(window, &view, cx);
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));

        // Empty name.
        set_name(&view, "   ", window, cx);
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
        assert!(view.read(cx).preferences.custom_themes.is_empty());
        assert!(view.read(cx).theme_editor.draft.as_ref().unwrap().error.is_some());

        // Collapse foreground onto background: below the 4.5:1 floor.
        view.update(cx, |view, _| {
            let draft = view.theme_editor.draft.as_mut().unwrap();
            draft.seeds.foreground = draft.seeds.background;
        });
        set_name(&view, "Mud", window, cx);
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
        assert!(view.read(cx).preferences.custom_themes.is_empty());
        let error = view.read(cx).theme_editor.draft.as_ref().unwrap().error.clone();
        assert!(error.is_some(), "low contrast is rejected");

        // A readable pair saves.
        view.update(cx, |view, _| {
            let draft = view.theme_editor.draft.as_mut().unwrap();
            draft.seeds.foreground = gpui_kit::rgb(0xf5f5f5).into();
            draft.seeds.background = gpui_kit::rgb(0x101010).into();
        });
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
        assert_eq!(view.read(cx).preferences.custom_themes.len(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn deleting_the_active_custom_theme_falls_back_to_the_mode_default(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        open_appearance(window, &view, cx);
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
        set_name(&view, "Ocean Night", window, cx);
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
        assert_eq!(view.read(cx).theme_id, "custom-ocean-night");

        // The deleted id must not strand either profile lane.
        view.update(cx, |view, _| {
            view.preferences.light_theme = Some("custom-ocean-night".to_string());
            view.preferences.dark_theme = Some("custom-ocean-night".to_string());
        });
        view.update(cx, |view, cx| view.remove_custom_theme("custom-ocean-night", Some(window), cx));
        window.render_frame(cx);
        {
            let view = view.read(cx);
            assert!(view.preferences.custom_themes.is_empty());
            assert_eq!(view.theme_id, super::super::themes::default_id(view.theme_mode));
            assert!(view.preferences.light_theme.is_none());
            assert!(view.preferences.dark_theme.is_none());
        }
        assert!(window.try_find("custom-themes-list").is_none());
        assert!(window.try_find("custom-theme-apply-custom-ocean-night").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn custom_theme_assignment_respects_separate_profiles(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        open_appearance(window, &view, cx);
        view.update(cx, |view, _| view.preferences.separate_theme_profiles = true);
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
        set_name(&view, "Paper", window, cx);
        view.update(cx, |view, cx| view.select_custom_theme_mode(ThemeMode::Light, cx));
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));

        let view = view.read(cx);
        assert_eq!(view.theme_mode, ThemeMode::Light);
        assert_eq!(view.preferences.light_theme.as_deref(), Some("custom-paper"));
        // The dark lane keeps whatever it had; saving a light theme never
        // rewrites the opposite profile.
        assert_ne!(view.preferences.dark_theme.as_deref(), Some("custom-paper"));
    })
    .unwrap();
}

/// A saved custom theme that no longer exists must not survive startup as a
/// dangling selection: the applied mode's default replaces it, matching what a
/// live selection does.
#[gpui_kit::test]
fn startup_replaces_a_deleted_custom_theme_with_the_mode_default(cx: &mut TestAppContext) {
    let prefs_path = crate::desktop::preferences::install_test_preferences_path();
    let preferences = crate::desktop::preferences::Preferences {
        theme: Some("custom-deleted".to_string()),
        ..Default::default()
    };
    preferences.save();

    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path);
    // The test harness constructs the view directly; `load_preferences` is the
    // startup restore step that a real launch runs.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.load_preferences(window, cx));
    })
    .unwrap();
    cx.update(|cx| {
        let view = view.read(cx);
        assert_eq!(
            view.theme_id,
            super::super::themes::default_id(view.theme_mode),
            "a stale custom id resolves to the applied mode's default"
        );
        assert_ne!(view.theme_id, "custom-deleted", "the stale id is dropped at startup");
    });
}

#[gpui_kit::test]
fn custom_themes_stay_out_of_the_cli_theme_channel(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        open_appearance(window, &view, cx);
        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
        set_name(&view, "Ocean Night", window, cx);
        view.update(cx, |view, cx| view.save_custom_theme(window, cx));
        let view = view.read(cx);
        assert_eq!(view.theme_id, "custom-ocean-night");
        assert!(super::super::themes::cli_id_for(&view.theme_id).is_none(), "the CLI owns only its palette registry");
        assert!(!view.can_sync_theme_to_cli(), "custom themes never sync to the CLI");
    })
    .unwrap();
}
