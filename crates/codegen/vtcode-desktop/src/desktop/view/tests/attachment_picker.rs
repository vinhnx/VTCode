use super::*;

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
        view.file_directory_paths = std::sync::Arc::new(
            view.files
                .iter()
                .filter(|entry| entry.is_dir)
                .map(|entry| entry.rel_path.clone())
                .collect(),
        );
    });
}

#[gpui_kit::test]
fn attachment_picker_opens_with_workspace_tree(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
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
        assert!(window.try_find("picker-file-list").is_some(), "the workspace tab shows a file tree");
        assert!(window.try_find("picker-file-notes.md").is_some(), "workspace file rows render");
        assert!(window.try_find("picker-file-shot.png").is_some());
        assert!(window.try_find("picker-file-src").is_some(), "directories render as collapsed rows");
    })
    .unwrap();
}

#[gpui_kit::test]
fn attachment_picker_rows_toggle_attachments(cx: &mut TestAppContext) {
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
        assert_eq!(state.attachments.len(), 1, "clicking a row attaches the file");
        assert_eq!(state.attachments[0].path, "notes.md");
        assert!(state.attachment_picker.is_some(), "the picker stays open for multi-attach");
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find("picker-file-notes.md").label(),
            Some("File: notes.md (attached)"),
            "attached state joins the spoken row label"
        );
    })
    .unwrap();
    // A second click detaches; the check needs no separate remove control.
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("picker-file-notes.md", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).attachments.is_empty(), "clicking an attached row detaches it");
    });
    // Enter runs the same toggle through the real tree key context. The
    // click above focused the tree; selection is set explicitly because
    // headless clicks carry no native selection update.
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("picker-file-notes.md").label(), Some("File: notes.md"));
        view.update(cx, |view, cx| {
            view.picker_tree.update(cx, |tree, cx| tree.set_selected_index(Some(0), cx));
        });
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    // Attaching settles on the background executor, like the click path.
    cx.run_until_parked();
    cx.update(|cx| {
        let state = view.read(cx);
        assert_eq!(state.attachments.len(), 1, "Enter attaches the selected row");
        assert_eq!(state.attachments[0].path, "notes.md");
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
        assert!(window.try_find("picker-files-empty").is_some(), "the empty picker names its state");
        assert!(window.try_find("picker-file-notes.md").is_none());
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).attachment_picker.is_some(), "the empty picker stays open");
    });
}

#[gpui_kit::test]
fn attachment_picker_filter_narrows_to_file_hits(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let (window, view) = open_desktop(cx);
    seed_workspace_files(&view, cx, workspace.path());
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_attachment_picker(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        // `set_value` emits no Change headless, so the subscription handler
        // runs explicitly, like the inspector filter test.
        view.update(cx, |view, cx| {
            view.picker_filter_input
                .update(cx, |input, cx| input.set_value("notes", window, cx));
            view.rebuild_picker_tree(cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("picker-file-notes.md").is_some());
        assert!(window.try_find("picker-file-shot.png").is_none());
        assert!(window.try_find("picker-file-src").is_none());
        assert_eq!(window.find("picker-file-filter-count").label(), Some("1 of 2 shown"));
        view.update(cx, |view, cx| {
            view.picker_filter_input
                .update(cx, |input, cx| input.set_value("src", window, cx));
            view.rebuild_picker_tree(cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("picker-file-src").is_none(), "directories never rank as attachable hits");
        view.update(cx, |view, cx| {
            view.picker_filter_input
                .update(cx, |input, cx| input.set_value("zzz-no-match", window, cx));
            view.rebuild_picker_tree(cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("picker-file-filter-empty").is_some(), "misses name their state");
        view.update(cx, |view, cx| {
            view.picker_filter_input.update(cx, |input, cx| input.set_value("", window, cx));
            view.rebuild_picker_tree(cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("picker-file-notes.md").is_some());
        assert!(window.try_find("picker-file-shot.png").is_some());
        assert!(window.try_find("picker-file-src").is_some(), "clearing restores the hierarchy");
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_switch_clears_picker_rows_until_rescan(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temporary workspace");
    let other = tempfile::TempDir::new().expect("other workspace");
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(8);
    seed_workspace_files(&view, cx, workspace.path());
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.open_attachment_picker(window, cx);
        });
        window.render_frame(cx);
        assert!(window.try_find("picker-file-notes.md").is_some());
        // No parking after the switch: the cleared state must hold across
        // the async rescan gap.
        view.update(cx, |view, cx| view.prepare_workspace(other.path(), cx));
        window.render_frame(cx);
        assert!(window.try_find("picker-file-notes.md").is_none(), "switch clears picker rows");
        assert!(window.try_find("picker-files-empty").is_some(), "cleared picker names its state");
    })
    .unwrap();
    cx.update(|cx| {
        let state = view.read(cx);
        assert!(state.attachment_picker.is_some(), "the picker survives the switch");
        assert!(state.file_search_hit_count.is_none());
        assert!(state.picker_search_hit_count.is_none());
    });
    cx.run_until_parked();
}
