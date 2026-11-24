//! Shared file-tree row contracts: names and the desktop-convention edge
//! jumps Kit's base Tree does not bind. Split from `inspector_files.rs` to
//! stay under the test-file line ceiling.

use super::*;

#[test]
fn file_name_returns_the_final_path_segment() {
    use super::super::files_panel::file_name;

    assert_eq!(file_name("README.md"), "README.md", "a root file is its own name");
    assert_eq!(file_name("src/main.rs"), "main.rs", "nested files use the last segment");
    assert_eq!(file_name("src/nested"), "nested", "directories use the same rule");
    assert_eq!(file_name(""), "", "an empty path has no name");
}

#[test]
fn file_tree_row_accessibility_label_keeps_attached_state_only_on_files() {
    use super::super::files_panel::file_tree_row_accessibility_label;

    let folder = file_tree_row_accessibility_label("src", true, false);
    let file = file_tree_row_accessibility_label("src/main.rs", false, false);
    let attached_file = file_tree_row_accessibility_label("src/main.rs", false, true);
    let attached_folder = file_tree_row_accessibility_label("src", true, true);

    // Check locale-specific prefixes, but pin the contract that matters: the
    // spoken name includes the path and attachment is announced on files only.
    assert!(folder.contains("src"), "folder name carries its path: {folder}");
    assert!(file.contains("src/main.rs"), "file name carries its path: {file}");
    assert!(attached_file.ends_with("(attached)"), "attached files announce the pinned chip: {attached_file}");
    assert!(!attached_folder.contains("(attached)"), "folders cannot be attached: {attached_folder}");
}

#[gpui_kit::test]
fn file_tree_home_end_jump_to_edges_without_leaving_the_tree(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("first.md"), "first").expect("write first");
    std::fs::create_dir(workspace.path().join("src")).expect("create dir");
    std::fs::write(workspace.path().join("middle.md"), "middle").expect("write middle");

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.refresh_files(cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        window.click("focus-files", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let tree = view.read(cx).file_tree.clone();
        let selected = |app: &gpui_kit::App| tree.read(app).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected(cx).as_deref(), Some("src"), "focus selects the first row");
        let paths = file_tree_paths(&view, cx);
        let last = paths.last().expect("rows exist").clone();
        assert_ne!(last, "src", "the last row differs from the first");

        window.press("end", cx);
        assert_eq!(selected(cx).as_deref(), Some(last.as_str()), "End selects the last row");
        window.press("home", cx);
        assert_eq!(selected(cx).as_deref(), Some("src"), "Home returns to the first row");
        assert_eq!(
            view.read(cx).inspector_tab,
            super::super::InspectorTab::Files,
            "Tree-scoped edges do not move tabs"
        );
    })
    .unwrap();
}
