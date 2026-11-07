use super::*;

#[gpui_kit::test]
fn markdown_files_preview_through_the_shared_viewer(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n\nShip the **viewer**.\n").unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "plain text body").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "the preview overlay opens");
        let viewer = window.find("file-preview-markdown");
        assert!(viewer.visible(), "markdown previews reuse the shared viewer");
        let spoken = viewer.label().unwrap_or_default();
        assert!(spoken.contains("Notes"), "the viewer exposes the heading: {spoken}");
        assert!(spoken.contains("Ship the viewer."), "the viewer exposes the body text: {spoken}");
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.txt".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "text previews keep the overlay");
        assert!(window.try_find("file-preview-markdown").is_none(), "text previews keep the mono surface");
        assert_eq!(view.read(cx).file_preview.as_ref().expect("preview state").text, "plain text body");
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_preview_traps_tab_across_its_header_actions(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "the preview overlay opens");
        assert_tab_trapped_within_dialog(window, "open-preview-editor", "close-preview", cx);
    })
    .unwrap();
}
