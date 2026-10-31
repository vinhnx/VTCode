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
