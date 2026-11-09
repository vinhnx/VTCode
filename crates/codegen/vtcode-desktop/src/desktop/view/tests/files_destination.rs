use super::*;

#[gpui_kit::test]
fn file_destination_dropdown_click_persists_and_relaunch_applies_it(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let prefs_path = crate::desktop::preferences::install_test_preferences_path();
    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path.clone());

    // Drive the real dropdown like a user: open the menu, click an option.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        scroll_settings_into_view(window, "file-open-destination-select", cx);
        window.within("file-open-destination-select").click("input", cx);
        window.render_frame(cx);
        window.click("destination-option-system", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(view.read(cx).preferences.file_open_destination(), FileOpenDestination::System);
        assert_eq!(window.find("file-open-destination-select").value(), Some("System default"));
    })
    .unwrap();
    assert_eq!(
        crate::desktop::preferences::Preferences::load().file_open_destination(),
        FileOpenDestination::System,
        "a real dropdown click persists the choice"
    );

    // Simulate a relaunch: a fresh window over the same preferences file.
    let (relaunched_window, relaunched) = open_desktop_with_preferences_path(cx, prefs_path);
    cx.update_window(relaunched_window, |_, relaunched_window, cx| {
        relaunched.update(cx, |relaunched, cx| {
            relaunched.open_settings(relaunched_window, cx);
            relaunched.test_launch_intercept = Some(TestLaunchIntercept::default());
            relaunched.open_file_preview("notes.txt".to_string(), relaunched_window, cx);
            assert_eq!(
                relaunched.preferences.file_open_destination(),
                FileOpenDestination::System,
                "the relaunch applies the saved choice"
            );
            assert!(relaunched.file_preview.is_none(), "the saved external destination routes without the overlay");
            assert_eq!(
                relaunched.test_launch_intercept.as_ref().unwrap().calls,
                vec![ExternalLaunchCall {
                    rel_path: "notes.txt".into(),
                    editor: ExternalEditor::System
                }]
            );
            assert_eq!(
                relaunched.file_destination_select.read(cx).selected_value().copied(),
                Some(FileOpenDestination::System),
                "the relaunched dropdown shows the saved choice"
            );
        });
        relaunched_window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn explicit_viewer_selection_writes_the_choice_to_disk(cx: &mut TestAppContext) {
    let prefs_path = crate::desktop::preferences::install_test_preferences_path();
    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path.clone());
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| view.set_file_open_destination(FileOpenDestination::Viewer, cx));
    })
    .unwrap();
    let raw = std::fs::read_to_string(&prefs_path).expect("the selection writes the preferences file");
    let saved: serde_json::Value = serde_json::from_str(&raw).expect("preferences stay valid JSON");
    assert_eq!(
        saved.get("fileOpenDestination").and_then(serde_json::Value::as_str),
        Some(FileOpenDestination::Viewer.id()),
        "an explicit viewer selection persists even though it matches the default: {raw}"
    );
}
