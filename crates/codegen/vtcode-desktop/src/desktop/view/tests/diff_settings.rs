use super::*;
use crate::desktop::diff_preferences::{DiffPreferences, new_diff_state};
use gpui_kit::component::diff::{
    DiffChangeIndicator, DiffFile, DiffHoverHighlight, DiffHunkSeparator, DiffInlineUnit, DiffMode,
};

const PATCH: &str =
    "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n fn main() {\n-    run();\n+    run_app();\n }\n";

#[gpui_kit::test]
fn diff_state_constructor_honors_saved_presentation_prefs(cx: &mut TestAppContext) {
    let prefs_path = crate::desktop::preferences::install_test_preferences_path();
    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path);
    let mut prefs = DiffPreferences::default();
    prefs.set_context_lines(None);
    prefs.set_inline_unit(Some(DiffInlineUnit::Character));
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |_, cx| {
            let files = DiffFile::parse(PATCH).expect("fixture patch parses");
            let state = cx.new(|cx| new_diff_state(files, DiffMode::Split, &prefs, cx));
            let guard = state.read(cx);
            assert_eq!(guard.mode(), DiffMode::Split);
            assert_eq!(guard.context_lines(), None);
            assert_eq!(guard.inline_unit(), Some(DiffInlineUnit::Character));

            let files = DiffFile::parse(PATCH).expect("fixture patch parses");
            let defaults = cx.new(|cx| new_diff_state(files, DiffMode::Unified, &DiffPreferences::default(), cx));
            let guard = defaults.read(cx);
            assert_eq!(guard.mode(), DiffMode::Unified);
            assert_eq!(guard.context_lines(), Some(3));
            assert_eq!(guard.inline_unit(), Some(DiffInlineUnit::Word));
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn diff_appearance_clicks_persist_and_relaunch_applies_them(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let prefs_path = crate::desktop::preferences::install_test_preferences_path();
    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path.clone());

    // Drive the real controls like a user: flip a switch, pick segmented options.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_settings(window, cx));
        window.render_frame(cx);
        for id in ["diff-soft-wrap", "diff-indicator-bars", "diff-context-unfolded"] {
            scroll_settings_into_view(window, id, cx);
            window.click(id, cx);
            window.render_frame(cx);
        }
        assert_eq!(window.find("diff-soft-wrap").checked(), Some(false));
        assert!(!view.read(cx).preferences.diff.soft_wrap());
        assert_eq!(view.read(cx).preferences.diff.change_indicator(), DiffChangeIndicator::Bars);
        assert_eq!(view.read(cx).preferences.diff.context_lines(), None);
    })
    .unwrap();
    cx.run_until_parked();
    let raw = std::fs::read_to_string(&prefs_path).expect("the clicks write the preferences file");
    let saved: serde_json::Value = serde_json::from_str(&raw).expect("preferences stay valid JSON");
    assert_eq!(saved.pointer("/diff/softWrap").and_then(serde_json::Value::as_bool), Some(false), "{raw}");
    assert_eq!(saved.pointer("/diff/changeIndicator").and_then(serde_json::Value::as_str), Some("bars"), "{raw}");
    assert_eq!(saved.pointer("/diff/contextLines").and_then(serde_json::Value::as_str), Some("unfolded"), "{raw}");

    // Simulate a relaunch: a fresh window over the same preferences file.
    let (relaunched_window, relaunched) = open_desktop_with_preferences_path(cx, prefs_path);
    cx.update_window(relaunched_window, |_, _, cx| {
        relaunched.update(cx, |relaunched, _| {
            let diff = &relaunched.preferences.diff;
            assert!(!diff.soft_wrap(), "the relaunch applies the saved switch");
            assert_eq!(diff.change_indicator(), DiffChangeIndicator::Bars);
            assert_eq!(diff.context_lines(), None);
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn diff_setters_persist_stable_ids(cx: &mut TestAppContext) {
    let prefs_path = crate::desktop::preferences::install_test_preferences_path();
    let (window, view) = open_desktop_with_preferences_path(cx, prefs_path.clone());
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.set_diff_context_lines(None, cx);
            view.set_diff_inline_unit(Some(DiffInlineUnit::Character), cx);
            view.set_diff_hunk_separator(DiffHunkSeparator::Simple, cx);
            view.set_diff_hover_highlight(DiffHoverHighlight::Both, cx);
            view.set_diff_file_headers(false, cx);
            view.set_diff_line_numbers(false, cx);
            view.set_diff_syntax_highlight(false, cx);
            view.set_diff_change_background(false, cx);
        });
    })
    .unwrap();
    let raw = std::fs::read_to_string(&prefs_path).expect("the setters write the preferences file");
    let saved: serde_json::Value = serde_json::from_str(&raw).expect("preferences stay valid JSON");
    assert_eq!(saved.pointer("/diff/contextLines").and_then(serde_json::Value::as_str), Some("unfolded"), "{raw}");
    assert_eq!(saved.pointer("/diff/inlineUnit").and_then(serde_json::Value::as_str), Some("characters"), "{raw}");
    assert_eq!(saved.pointer("/diff/hunkSeparator").and_then(serde_json::Value::as_str), Some("simple"), "{raw}");
    assert_eq!(saved.pointer("/diff/hoverHighlight").and_then(serde_json::Value::as_str), Some("both"), "{raw}");
    for key in [
        "/diff/fileHeaders",
        "/diff/lineNumbers",
        "/diff/syntaxHighlight",
        "/diff/changeBackground",
    ] {
        assert_eq!(saved.pointer(key).and_then(serde_json::Value::as_bool), Some(false), "{key}: {raw}");
    }
    let loaded = crate::desktop::preferences::Preferences::load();
    assert_eq!(loaded.diff.context_lines(), None);
    assert_eq!(loaded.diff.inline_unit(), Some(DiffInlineUnit::Character));
    assert_eq!(loaded.diff.hunk_separator(), DiffHunkSeparator::Simple);
    assert_eq!(loaded.diff.hover_highlight(), DiffHoverHighlight::Both);
}
