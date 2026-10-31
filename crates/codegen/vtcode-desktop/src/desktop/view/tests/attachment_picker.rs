use super::*;

/// A real 1x1 PNG so picker thumbnails decode when rendered.
const TINY_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00,
    0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49,
    0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0xFC, 0xFF, 0xFF, 0x3F, 0x03, 0x00, 0x05, 0xFE, 0x02, 0xFE, 0xA7, 0x2D, 0x1E,
    0x48, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn seed_workspace_files(view: &Entity<DesktopView>, cx: &mut TestAppContext, workspace: &Path) {
    view.update(cx, |view, _| {
        view.workspace = Some(workspace.to_path_buf());
        view.files = vec![
            FileEntry {
                rel_path: "notes.md".into(),
                depth: 0,
                is_dir: false,
            },
            FileEntry {
                rel_path: "shot.png".into(),
                depth: 0,
                is_dir: false,
            },
            FileEntry { rel_path: "src".into(), depth: 0, is_dir: true },
        ];
    });
}

#[gpui_kit::test]
fn attachment_picker_opens_with_workspace_cards(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("shot.png"), TINY_PNG).expect("fixture image");
    let (window, view) = open_desktop(cx);
    seed_workspace_files(&view, cx, workspace.path());
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(view.read(cx).attachment_picker.is_some(), "picker state must be open after open_attachment_picker");
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // Children of the anchored popover resolve in the test tree; the
        // focus-trap wrapper consumes the root id itself.
        assert!(window.try_find("picker-tab-workspace").is_some(), "tabs render");
        assert!(window.try_find("picker-tab-computer").is_some());
        assert!(window.try_find("picker-file-notes.md").is_some(), "workspace file cards render");
        assert!(window.try_find("picker-file-shot.png").is_some());
        assert!(window.try_find("picker-file-src").is_none(), "directories never render as cards");
    })
    .unwrap();
    // The image card gains its thumbnail from the bounded background batch.
    cx.run_until_parked();
    cx.update(|cx| {
        let state = view.read(cx).attachment_picker.as_ref().expect("picker stays open");
        assert!(state.thumbnail("shot.png").is_some(), "image cards load thumbnails");
    });
}

#[gpui_kit::test]
fn attachment_picker_cards_attach_and_show_state(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    std::fs::write(workspace.path().join("notes.md"), "hello").unwrap();
    let (window, view) = open_desktop(cx);
    seed_workspace_files(&view, cx, workspace.path());
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("picker-file-notes.md", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.attachments.len(), 1, "clicking a card attaches the file");
        assert_eq!(state.attachments[0].path, "notes.md");
        assert!(state.attachment_picker.is_some(), "the picker stays open for multi-attach");
    });
    // The attached card carries its remove affordance.
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(opaque_path_element_id("picker-remove", "notes.md"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).attachments.is_empty(), "the card x removes the attachment");
    });
}

#[gpui_kit::test]
fn attachment_picker_computer_tab_hands_off_to_native_picker(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    seed_workspace_files(&view, cx, workspace.path());
    view.update(cx, |view, _| {
        view.test_launch_intercept = Some(TestLaunchIntercept::default());
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("picker-tab-computer", cx);
        window.render_frame(cx);
        assert!(window.try_find("picker-choose-files").is_some(), "the computer tab offers the native picker");
        window.click("picker-choose-files", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(
            view.read(cx).attachment_picker.is_none(),
            "choosing from this computer closes the picker before the native dialog"
        );
    });
}

#[gpui_kit::test]
fn escape_dismisses_the_attachment_picker_and_restores_focus(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    seed_workspace_files(&view, cx, workspace.path());
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window),
            "dismissal restores composer focus"
        );
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).attachment_picker.is_none(), "Escape closes the picker");
    });
}

#[gpui_kit::test]
fn attachment_picker_shows_an_empty_state_without_files(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    seed_workspace_files(&view, cx, workspace.path());
    view.update(cx, |view, _| {
        view.files.clear();
    });
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("picker-tab-workspace").is_some(), "the empty picker stays mounted");
        assert!(window.try_find("picker-file-notes.md").is_none());
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).attachment_picker.is_some(), "the empty picker stays open");
    });
}
