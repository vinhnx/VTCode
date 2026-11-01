use super::*;
use gpui_kit::{ExternalPaths, FileDropEvent, InputEvent};

/// Deliver an external Finder drop through the same platform-event route the
/// OS takes: Entered sets the active drag, Submit translates to mouse-up and
/// fires drop listeners, Ended closes the session.
fn dispatch_file_drop(window: AnyWindowHandle, paths: ExternalPaths, cx: &mut TestAppContext) {
    let position = point(px(640.), px(450.));
    cx.update_window(window, |_, window, cx| {
        let _ = window.dispatch_event(FileDropEvent::Entered { position, paths }.to_platform_input(), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let _ = window.dispatch_event(FileDropEvent::Submit { position }.to_platform_input(), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        let _ = window.dispatch_event(FileDropEvent::Ended.to_platform_input(), cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn dropped_workspace_file_attaches_asynchronously(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let file = workspace.path().join("notes.md");
    std::fs::write(&file, "# notes").expect("fixture file");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    let paths = ExternalPaths([file].into_iter().collect());
    dispatch_file_drop(window, paths, cx);
    cx.update(|cx| {
        let attachments = &view.read(cx).attachments;
        assert_eq!(attachments.len(), 1, "dropped workspace file must attach");
        assert_eq!(attachments[0].path, "notes.md");
        assert!(view.read(cx).error.is_none(), "valid drop must not set an error");
    });
    // With a workspace open, the drop affordance is mounted (visible while
    // external files hover).
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("drop-overlay").is_some(), "the drop affordance mounts with a workspace");
    })
    .unwrap();
}

#[gpui_kit::test]
fn dropped_file_outside_the_workspace_attaches_with_its_absolute_path(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let outside = tempfile::TempDir::new().expect("outside directory");
    let file = outside.path().join("outside.md");
    std::fs::write(&file, "shared").expect("fixture file");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    let paths = ExternalPaths([file.clone()].into_iter().collect());
    dispatch_file_drop(window, paths, cx);
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.attachments.len(), 1, "outside files attach like workspace files");
        // Temp roots sit behind a symlink, so the label carries the canonical path.
        let canonical = vtcode_commons::paths::canonicalize(&file).expect("canonical fixture path");
        assert_eq!(state.attachments[0].path, canonical.display().to_string());
        assert_eq!(state.attachments[0].text, "shared");
        assert!(state.error.is_none(), "valid outside drop must not set an error");
    });
}

#[gpui_kit::test]
fn drop_batch_stops_at_the_attachment_limit(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let files: Vec<_> = (0..6)
        .map(|index| {
            let path = workspace.path().join(format!("file-{index}.md"));
            std::fs::write(&path, format!("content {index}")).expect("fixture file");
            path
        })
        .collect();
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    let paths = ExternalPaths(files.into_iter().collect());
    dispatch_file_drop(window, paths, cx);
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.attachments.len(), 5, "drop batch must stop at the five-attachment limit");
        assert!(state.error.is_some(), "overflowing batch must surface the limit notice");
    });
}

#[gpui_kit::test]
fn dropping_without_a_workspace_is_a_silent_no_op(cx: &mut TestAppContext) {
    let outside = tempfile::TempDir::new().expect("outside directory");
    let file = outside.path().join("stray.md");
    std::fs::write(&file, "content").expect("fixture file");
    // The baseline fixture opens with a workspace; clear it so the gated drop
    // listeners detach on the next render and the tint never advertises an
    // unavailable action.
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        view.workspace = None;
        cx.notify();
    });
    let paths = ExternalPaths([file].into_iter().collect());
    dispatch_file_drop(window, paths, cx);
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.attachments.is_empty(), "no workspace means nothing to attach");
        assert!(
            state.error.is_none(),
            "workspace absence is not a containment failure; it must not surface the boundary notice"
        );
    });
    // Without a workspace the affordance is unmounted: nothing advertises an
    // action that would no-op.
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("drop-overlay").is_none(), "no workspace means no drop affordance");
    })
    .unwrap();
}

/// A real 1x1 PNG so the renderer can decode the thumbnail and preview.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xFC, 0xFF, 0xFF, 0x3F, 0x03, 0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x2D, 0x1E,
    0x48, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[gpui_kit::test]
fn dropped_image_attaches_by_path_and_caches_a_thumbnail(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let image = workspace.path().join("shot.png");
    std::fs::write(&image, TINY_PNG).expect("fixture image");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    let paths = ExternalPaths([image].into_iter().collect());
    dispatch_file_drop(window, paths, cx);
    cx.update(|cx| {
        let state = view.read(cx);
        let attachments = &state.attachments;
        assert_eq!(attachments.len(), 1, "dropped image must attach");
        assert_eq!(attachments[0].path, "shot.png");
        assert!(attachments[0].text.is_empty(), "images attach by path, never inline text");
        assert!(
            state.attachment_thumbnails.contains_key("shot.png"),
            "the image thumbnail must be cached for the composer chip"
        );
    });
}

#[gpui_kit::test]
fn image_files_open_the_image_preview(cx: &mut TestAppContext) {
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
    })
    .unwrap();
    cx.update(|cx| {
        let preview = view.read(cx).file_preview.as_ref().expect("preview state set");
        assert_eq!(preview.rel_path, "shot.png");
        assert!(preview.image.is_some(), "image files preview as pictures, not text");
        assert!(preview.text.is_empty());
    });
}

#[gpui_kit::test]
fn attachment_chip_actions_view_open_and_delete(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.txt"), "hello").unwrap();
    std::fs::write(workspace.path().join("shot.png"), TINY_PNG).expect("fixture image");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    let paths = ExternalPaths(
        [workspace.path().join("notes.txt"), workspace.path().join("shot.png")]
            .into_iter()
            .collect(),
    );
    dispatch_file_drop(window, paths, cx);
    cx.update(|cx| {
        assert_eq!(view.read(cx).attachments.len(), 2, "both files attach");
    });

    // View opens the in-app overlay for the text chip; the read parks, so
    // settle the async load before asserting.
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(opaque_path_element_id("view-attachment", "notes.txt"), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let preview = view.read(cx).file_preview.as_ref().expect("view opens the overlay");
        assert_eq!(preview.rel_path, "notes.txt");
        assert!(preview.image.is_none());
    });

    // Escape closes the overlay so the chips are clickable again.
    cx.update_window(window, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
        view.update(cx, |view, _| view.test_launch_intercept = Some(TestLaunchIntercept::default()));
        window.click(opaque_path_element_id("open-attachment", "notes.txt"), cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).file_preview.is_none(), "open must not show the overlay");
        assert_eq!(
            view.read(cx).test_launch_intercept.as_ref().expect("intercept set").calls,
            vec![ExternalLaunchCall {
                rel_path: "notes.txt".into(),
                editor: ExternalEditor::System
            }]
        );
    });

    // Delete removes the image chip and its cached thumbnail.
    cx.update_window(window, |_, window, cx| {
        window.click(opaque_path_element_id("remove-attachment", "shot.png"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.attachments.len(), 1, "delete removes the image chip");
        assert_eq!(state.attachments[0].path, "notes.txt");
        assert!(!state.attachment_thumbnails.contains_key("shot.png"), "delete must drop the cached thumbnail");
    });
}

#[gpui_kit::test]
fn pasting_copied_files_attaches_them(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "hello").expect("fixture file");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    // A Finder copy puts the file on the clipboard as ExternalPaths.
    cx.update(|cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem {
            entries: vec![gpui_kit::ClipboardEntry::ExternalPaths(ExternalPaths(
                [workspace.path().join("notes.md")].into_iter().collect(),
            ))],
        });
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_composer(window, cx));
        window.render_frame(cx);
        window.dispatch_action(Box::new(gpui_kit::component::input::Paste), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.attachments.len(), 1, "pasted file attaches");
        assert_eq!(state.attachments[0].path, "notes.md");
    });
    // Files never land inside the text input.
    cx.update_window(window, |_, _window, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value().as_str(), "");
    })
    .unwrap();
}

#[gpui_kit::test]
fn pasting_text_still_inserts_into_the_composer(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update(|cx| {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("plain text".into()));
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_composer(window, cx));
        window.render_frame(cx);
        window.dispatch_action(Box::new(gpui_kit::component::input::Paste), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _window, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value().as_str(), "plain text");
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).attachments.is_empty(), "text paste never attaches");
    });
}

#[gpui_kit::test]
fn copied_file_from_the_tree_pastes_as_an_attachment(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "hello").expect("fixture file");
    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.path().to_path_buf());
    });
    // Copy from the tree: the clipboard now carries the path text plus a file
    // entry, mirroring a Finder copy.
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| view.copy_file_path("notes.md", cx));
    })
    .unwrap();
    // Paste in the composer attaches instead of inserting the path.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.focus_composer(window, cx));
        window.render_frame(cx);
        window.dispatch_action(Box::new(gpui_kit::component::input::Paste), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.attachments.len(), 1, "copied file pastes as an attachment");
        assert_eq!(state.attachments[0].path, "notes.md");
    });
    cx.update_window(window, |_, _window, cx| {
        assert_eq!(view.read(cx).composer.read(cx).value().as_str(), "");
    })
    .unwrap();
}
