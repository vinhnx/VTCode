use super::super::shortcuts::{CommandId, action};
use super::*;

#[gpui_kit::test]
fn shortcut_search_filters_rows_and_reports_empty_results(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        window.render_frame(cx);
        assert!(window.try_find("shortcut-row-model").is_some());
        assert!(window.try_find("shortcut-empty").is_none());
        view.update(cx, |view, cx| {
            view.shortcut_search_input
                .update(cx, |input, cx| input.set_value("zen", window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("shortcut-row-zen_mode").is_some());
        assert!(window.try_find("shortcut-row-model").is_none());
        assert!(window.try_find("shortcut-empty").is_none());
        view.update(cx, |view, cx| {
            view.shortcut_search_input
                .update(cx, |input, cx| input.set_value("toggle palette", window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("shortcut-row-zen_mode").is_none());
        assert_eq!(window.find("shortcut-empty").role(), Some(gpui_kit::Role::Status));
        view.update(cx, |view, cx| {
            view.shortcut_search_input
                .update(cx, |input, cx| input.set_value("", window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("shortcut-row-model").is_some());
        assert!(window.try_find("shortcut-empty").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn shortcut_capture_suppresses_bound_chords_and_rejects_reserved(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        let sidebar_open = view.read(cx).sidebar_open;
        window.click("shortcut-edit-commands", cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_capture.is_some(), "pencil click arms capture");
        assert!(
            view.read(cx).shortcut_capture_focus.contains_focused(window, cx),
            "pill holds focus while capturing"
        );
        window.press("tab", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(!view.read(cx).shortcut_capture_focus.contains_focused(window, cx), "tab moves focus on");
        assert!(view.read(cx).shortcut_message.is_none(), "tab never commits nor errors");
        assert!(window.try_find("shortcut-capture-commands").is_none(), "tabbing away hides the pill");
        assert!(window.try_find("shortcut-badge-commands").is_some());
        assert!(window.try_find("keyboard-settings").is_some());
        window.click("shortcut-edit-commands", cx);
        window.render_frame(cx);
        window.press("cmd-b", cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("already assigned"));
        assert!(view.read(cx).shortcut_capture.is_some(), "rejections keep capturing");
        assert_eq!(view.read(cx).sidebar_open, sidebar_open, "captured chords stay inert inside settings");
        window.press("left", cx);
        window.render_frame(cx);
        assert!(window.try_find("keyboard-settings").is_some(), "arrows never navigate away mid-capture");
        assert!(view.read(cx).shortcut_capture.is_some());
        window.press("cmd-f", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none(), "settings search never opens mid-capture");
        assert!(view.read(cx).shortcut_capture.is_some());
        window.press("enter", cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("include cmd"));
        assert!(view.read(cx).shortcut_capture.is_some(), "typing-only keys keep capturing");
        window.press("cmd-z", cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("reserved"));
        assert!(view.read(cx).shortcut_capture.is_some());
        assert!(window.try_find("keyboard-settings").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn shortcut_reset_from_defaults_reports_noop(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        window.click("reset-shortcuts", cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("already match"));
        assert!(window.try_find("save-shortcuts").is_none(), "noop reset stages nothing");
    })
    .unwrap();
}

#[gpui_kit::test]
fn shortcut_invalid_staged_value_marks_badge_and_blocks_save(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        view.update(cx, |view, _| {
            let _previous = view.preferences.shortcuts.insert("model".into(), "a-b".into());
            view.shortcut_draft = super::super::shortcuts::draft_from(&view.preferences.shortcuts);
            view.shortcuts_editing = true;
        });
        window.render_frame(cx);
        assert_eq!(window.find("shortcut-badge-model").label(), Some("Shortcut for Choose model: invalid shortcut"));
        window.click("save-shortcuts", cx);
        window.render_frame(cx);
        assert!(view.read(cx).shortcut_message.as_ref().unwrap().contains("invalid"));
        assert!(crate::desktop::preferences::Preferences::load().shortcuts.is_empty());
    })
    .unwrap();
}

#[gpui_kit::test]
fn shortcut_unbind_marks_unassigned_and_persists(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        view.update(cx, |view, cx| {
            view.shortcut_search_input
                .update(cx, |input, cx| input.set_value("model picker", window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("shortcut-row-model").is_some());
        assert!(window.try_find("shortcut-row-commands").is_none());
        window.click("shortcut-remove-model", cx);
        window.render_frame(cx);
        assert_eq!(window.find("shortcut-badge-model").label(), Some("Shortcut for Choose model: not set"));
        window.click("save-shortcuts", cx);
        window.render_frame(cx);
        assert_eq!(
            crate::desktop::preferences::Preferences::load()
                .shortcuts
                .get("model")
                .map(String::as_str),
            Some("")
        );
        assert!(window.try_find("save-shortcuts").is_none(), "save hides once the draft is clean");
    })
    .unwrap();
}

#[gpui_kit::test]
fn command_palette_badges_shortcuts_and_deeplinks_to_shortcut_settings(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        // Badge viewer: exact-command rows carry their saved chord in a
        // trailing Kbd with its own accessible label (the row label itself
        // is unchanged). Zen mode is always available, so its row renders
        // in the fresh baseline (unavailable rows are filtered before
        // render and never appear).
        let zen_mode = super::super::PaletteAction::ToggleZenMode;
        let badge = format!("{}-shortcut", palette_action_id(&zen_mode));
        assert!(window.try_find(badge.clone()).is_some(), "zen mode shows its chord badge");
        let formatted =
            gpui_kit::component::kbd::Kbd::format(&gpui_kit::Keystroke::parse("ctrl-cmd-z").expect("default parses"));
        assert!(
            window.find(badge).label().is_some_and(|label| label.contains(&formatted)),
            "badge announces the chord"
        );
        // Near-miss neighbor stays badge-free.
        let theme_badge = format!("{}-shortcut", palette_action_id(&super::super::PaletteAction::ToggleTheme));
        assert!(window.try_find(theme_badge).is_none(), "theme toggle has no exact command");
        // Real search path: typing "keyboard" surfaces the deeplink row and
        // filters out unrelated rows.
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("keyboard", cx);
        window.render_frame(cx);
        let deeplink_row = palette_action_id(&super::super::PaletteAction::SelectSettingsTab(
            super::super::settings::SettingsTab::Shortcuts,
        ));
        assert!(window.try_find(deeplink_row).is_some(), "keyboard query surfaces the shortcuts deeplink");
        assert!(
            window
                .try_find(palette_action_id(&super::super::PaletteAction::ToggleTheme))
                .is_none(),
            "keyboard query filters out unrelated rows"
        );
        // Deeplink entry resolves with keyboard-searchable keywords.
        let deeplink = {
            let state = view.read(cx);
            let entry = state
                .palette_groups
                .iter()
                .flat_map(|(_, entries)| entries)
                .find(|entry| {
                    matches!(
                        &entry.action,
                        super::super::PaletteAction::SelectSettingsTab(tab)
                            if *tab == super::super::settings::SettingsTab::Shortcuts
                    )
                })
                .expect("command palette deeplinks to shortcut settings");
            assert!(entry.keywords.iter().any(|keyword| keyword == "keyboard"));
            assert!(entry.keywords.iter().any(|keyword| keyword == "keybinding"));
            entry.action.clone()
        };
        // Confirming the deeplink lands on the Shortcuts settings tab.
        view.update(cx, |view, cx| {
            view.run_palette_action(deeplink, window, cx);
        });
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Settings);
        assert_eq!(view.read(cx).settings_tab, super::super::settings::SettingsTab::Shortcuts);
    })
    .unwrap();
}

#[gpui_kit::test]
fn command_palette_badges_follow_saved_overrides_and_hide_unbound(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let _previous = view.preferences.shortcuts.insert("zen_mode".into(), "ctrl-alt-z".into());
        });
        view.update(cx, |view, cx| view.open_command_palette(window, cx));
        window.render_frame(cx);
        let zen_mode = super::super::PaletteAction::ToggleZenMode;
        let badge = format!("{}-shortcut", palette_action_id(&zen_mode));
        let overridden =
            gpui_kit::component::kbd::Kbd::format(&gpui_kit::Keystroke::parse("ctrl-alt-z").expect("override parses"));
        let default =
            gpui_kit::component::kbd::Kbd::format(&gpui_kit::Keystroke::parse("ctrl-cmd-z").expect("default parses"));
        assert!(
            window
                .find(badge)
                .label()
                .is_some_and(|label| label.contains(&overridden) && !label.contains(&default)),
            "badge shows the saved override, not the default"
        );
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let _previous = view.preferences.shortcuts.insert("zen_mode".into(), String::new());
            view.close_palette(window, cx);
            view.open_command_palette(window, cx);
        });
        window.render_frame(cx);
        let zen_mode = super::super::PaletteAction::ToggleZenMode;
        let badge = format!("{}-shortcut", palette_action_id(&zen_mode));
        assert!(window.try_find(badge).is_none(), "unbound commands hide their badge");
        assert!(window.try_find(palette_action_id(&zen_mode)).is_some(), "unbinding hides the badge, not the row");
    })
    .unwrap();
}

#[gpui_kit::test]
fn shortcut_save_applies_rebind_live_and_retires_old_chord(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.open_settings(window, cx);
        });
        window.render_frame(cx);
        select_settings_category(window, 2, cx);
        view.update(cx, |view, _| {
            let _previous = view.shortcut_draft.insert("model".into(), "ctrl-shift-m".into());
            view.shortcuts_editing = true;
        });
        window.render_frame(cx);
        window.click("save-shortcuts", cx);
        window.render_frame(cx);
        assert_eq!(
            crate::desktop::preferences::Preferences::load()
                .shortcuts
                .get("model")
                .map(String::as_str),
            Some("ctrl-shift-m")
        );
    })
    .unwrap();
    let desktop = crate::desktop::strings::key_context::DESKTOP;
    let model = action(CommandId::Model);
    let commands = action(CommandId::Commands);
    cx.update(|cx| {
        assert_chord_dead(cx, "cmd-shift-m", desktop);
        assert_chord_resolves_once_to(cx, "ctrl-shift-m", desktop, model.as_ref());
        assert_chord_resolves_to(cx, "cmd-k", desktop, commands.as_ref());
    });
}
