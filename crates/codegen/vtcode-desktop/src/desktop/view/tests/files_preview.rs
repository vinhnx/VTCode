use super::*;

#[gpui_kit::test]
fn images_open_in_the_viewer_while_text_routes_externally(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("shot.png"), TINY_PNG).expect("fixture image");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
        view.test_launch_intercept = Some(TestLaunchIntercept::default());
    });

    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("shot.png".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "the image viewer opens");
        assert!(window.try_find("file-preview-body").is_some(), "the image body renders");
        let preview = view.read(cx).file_preview.as_ref().expect("preview state");
        assert_eq!(preview.rel_path, "shot.png");
        assert!(preview.image.is_some(), "the viewer holds the decoded image");
        assert!(
            view.read(cx)
                .test_launch_intercept
                .as_ref()
                .expect("intercept")
                .calls
                .is_empty(),
            "images never launch externally"
        );
    })
    .unwrap();

    // Text leaves the app through the preferred external destination and
    // takes a stale viewer down with it.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.is_none(), "text never opens the overlay");
        assert!(window.try_find("file-preview").is_none());
        assert_eq!(
            view.read(cx).test_launch_intercept.as_ref().expect("intercept").calls,
            vec![ExternalLaunchCall {
                rel_path: "notes.md".into(),
                editor: ExternalEditor::System
            }],
            "text routes to the default external destination"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_preview_image_keeps_a_stable_identity(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("shot.png"), TINY_PNG).expect("fixture image");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_image_viewer("shot.png".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Pinned `gpui-fast 0.1.17` `elements/img.rs` advances a multi-frame
        // image only when the element has retained state, and retained state
        // requires an `ElementId`. The viewer accepts gif/webp, so this
        // identity is what makes those formats actually animate.
        assert!(window.try_find("file-preview-image").is_some(), "the image keeps a stable id");
    })
    .unwrap();
}

#[gpui_kit::test]
fn undecodable_image_shows_the_preview_unavailable_fallback(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    // A supported extension with bytes the decoder rejects: `read_image`
    // succeeds, so only `img()`'s fallback can report this to the user.
    std::fs::write(workspace.path().join("broken.png"), b"not a png").expect("corrupt fixture");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_image_viewer("broken.png".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let expected = rust_i18n::t!("app.files.preview_unavailable", path = "broken.png").to_string();
        assert_eq!(window.find("file-preview-unavailable").label(), Some(expected.as_str()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn image_viewer_refuses_non_images(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "# Notes\n").unwrap();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_image_viewer("notes.md".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.is_none(), "the viewer fails closed on text");
        assert!(window.try_find("file-preview").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn escape_dismisses_the_preview_and_restores_focus(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("shot.png"), TINY_PNG).expect("fixture image");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.focus_composer(window, cx);
            view.open_file_preview("shot.png".into(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some(), "the preview overlay opens");
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).file_preview.is_none(), "Escape closes the preview");
        assert!(window.try_find("file-preview").is_none());
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "dismissal restores the captured focus"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_preview_traps_tab_across_its_header_actions(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("shot.png"), TINY_PNG).expect("fixture image");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("shot.png".into(), window, cx));
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

#[gpui_kit::test]
fn file_preview_toolbar_moves_focus_with_arrow_keys(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("shot.png"), TINY_PNG).expect("fixture image");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview("shot.png".into(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // The bar id scopes paths but registers nothing observable (same
        // as tab bars), so pin the roving behavior, not the structure.
        // Arrow keys rove across the header actions in visual order.
        focus_element_by_tab(window, "open-preview-editor", cx);
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(window.find("open-preview-vscode").focused(), Some(true));
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(window.find("close-preview").focused(), Some(true));
        window.press("left", cx);
        window.render_frame(cx);
        assert_eq!(window.find("open-preview-vscode").focused(), Some(true));
        // Left from the first action wraps to the last one.
        focus_element_by_tab(window, "open-preview-editor", cx);
        window.press("left", cx);
        window.render_frame(cx);
        assert_eq!(window.find("close-preview").focused(), Some(true));
    })
    .unwrap();
}
