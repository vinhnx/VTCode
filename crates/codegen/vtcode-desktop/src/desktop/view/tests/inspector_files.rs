use super::*;

#[gpui_kit::test]
fn file_tree_menu_copies_path_and_opens_preview(cx: &mut TestAppContext) {
    use super::super::files_panel::{CopyFilePath, OpenFilePath, RevealFilePath};

    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("README.md"), "# Notes").expect("write file");
    std::fs::create_dir(workspace.path().join("src")).expect("create dir");
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.test_launch_intercept = Some(TestLaunchIntercept::default());
            view.refresh_files(cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        window.right_click("workspace-file-README.md", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("popup-menu").role(), Some(gpui_kit::Role::Menu), "right-click opens the file menu");
        window.press("escape", cx);
        window.render_frame(cx);
        window.dispatch_action(Box::new(CopyFilePath { path: "README.md".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let expected = workspace.path().join("README.md").display().to_string();
    assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(expected.as_str()));
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(OpenFilePath { path: "README.md".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        assert_eq!(view.read(cx).file_preview.as_ref().unwrap().text, "# Notes");
    })
    .unwrap();
    // Reveal routes through the shared external-launch interception channel
    // for files and directories alike — no real Finder spawn headless.
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(RevealFilePath { path: "README.md".into() }), cx);
        window.dispatch_action(Box::new(RevealFilePath { path: "src".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        let calls = &view.read(cx).test_launch_intercept.as_ref().expect("intercept").calls;
        assert_eq!(
            *calls,
            vec![
                ExternalLaunchCall {
                    rel_path: "README.md".to_string(),
                    editor: ExternalEditor::Finder
                },
                ExternalLaunchCall {
                    rel_path: "src".to_string(),
                    editor: ExternalEditor::Finder
                },
            ]
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_files_open_a_read_only_preview(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("README.md"), "# Notes\n\nhello from the workspace").expect("write file");
    std::fs::create_dir_all(workspace.path().join("src/nested")).expect("create nested sources");
    std::fs::write(workspace.path().join("src/nested/mod.rs"), "pub fn nested() {}").expect("write nested source");
    std::fs::create_dir_all(workspace.path().join("target/debug")).expect("skip dir");
    std::fs::write(workspace.path().join("target/debug/artifact"), b"binary").expect("write artifact");

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.refresh_files(cx);
            assert!(view.files.is_empty(), "scan does not block the UI callback");
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        {
            let view = view.read(cx);
            assert!(!view.files.iter().any(|entry| entry.rel_path.starts_with("target")));
            assert!(view.files.iter().any(|entry| entry.rel_path == "README.md"));
        };
        window.render_frame(cx);
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        assert_eq!(file_tree_paths(&view, cx), vec!["src", "README.md"]);
        let directory_index = view.read(cx).file_directory_paths.clone();
        assert!(directory_index.contains("src"), "the retained tree index records directories");
        window.render_frame(cx);
        assert!(
            std::sync::Arc::ptr_eq(&directory_index, &view.read(cx).file_directory_paths),
            "redrawing the inspector should reuse the directory index"
        );
        let tree_id = format!("tree-{}", view.read(cx).file_tree.entity_id());
        let tree = window
            .find_all(tree_id)
            .into_iter()
            .find(|tree| tree.role() == Some(gpui_kit::Role::Tree))
            .expect("file tree exposes its semantic role");
        assert_eq!(tree.role(), Some(gpui_kit::Role::Tree));
        assert_eq!(window.find("focus-files").label(), Some("Focus workspace file tree"));
        let root_item = file_tree_item_snapshot(window, 0);
        assert_eq!(root_item.role(), Some(gpui_kit::Role::TreeItem));
        assert_eq!(root_item.label(), Some("src"));
        assert_eq!(window.find("workspace-file-src").label(), Some("Folder: src"));
        assert_eq!(root_item.expanded(), Some(false));

        window.click("workspace-file-src", cx);
        window.render_frame(cx);
        assert_eq!(file_tree_item_snapshot(window, 0).expanded(), Some(true));
        assert!(file_tree_paths(&view, cx).contains(&"src/nested".to_string()));
        assert!(
            window.find("workspace-file-label-src/nested").bounds().origin.x
                > window.find("workspace-file-label-src").bounds().origin.x,
            "nested entries must be indented relative to their parent"
        );
        window.click("workspace-file-src/nested", cx);
        window.render_frame(cx);
        assert!(
            window.find("workspace-file-label-src/nested/mod.rs").bounds().origin.x
                > window.find("workspace-file-label-src/nested").bounds().origin.x,
            "each nesting level adds indentation"
        );
        assert!(view.read(cx).file_preview.is_none(), "folder activation expands instead of previewing");
        window.click("workspace-file-src", cx);
        window.render_frame(cx);
        assert_eq!(file_tree_item_snapshot(window, 0).expanded(), Some(false));

        let mut reached_file_focus = false;
        for _ in 0..32 {
            window.press("tab", cx);
            window.render_frame(cx);
            if window.find("focus-files").focused() == Some(true) {
                reached_file_focus = true;
                break;
            }
        }
        assert!(reached_file_focus, "Tab reaches the file-tree focus control");
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        let tree = view.read(cx).file_tree.clone();
        let selected_path = tree.read(cx).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected_path.as_deref(), Some("src"));
        assert_eq!(file_tree_item_snapshot(window, 0).selected(), Some(true));
        window.press("right", cx);
        assert_eq!(file_tree_item_snapshot(window, 0).expanded(), Some(true));
        assert!(file_tree_paths(&view, cx).contains(&"src/nested".to_string()));
        window.press("down", cx);
        window.press("right", cx);
        assert_eq!(file_tree_item_snapshot(window, 1).expanded(), Some(true));
        assert!(file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));
        window.press("left", cx);
        assert!(!file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));
        window.press("right", cx);
        assert!(file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));

        view.update(cx, |view, cx| view.refresh_files(cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(file_tree_paths(&view, cx).contains(&"src/nested/mod.rs".to_string()));
        let tree = view.read(cx).file_tree.clone();
        let selected_path = tree.read(cx).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected_path.as_deref(), Some("src/nested"));
        window.press("down", cx);
        let tree = view.read(cx).file_tree.clone();
        let selected_path = tree.read(cx).selected_item().map(|item| item.id.to_string());
        assert_eq!(selected_path.as_deref(), Some("src/nested/mod.rs"));
        window.press("enter", cx);
        assert!(view.read(cx).file_preview.is_none(), "read does not block the UI callback");
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("file-preview").label(), Some("File preview: src/nested/mod.rs"));
        assert_eq!(window.find("open-preview-editor").label(), Some("Open in editor"));
        assert_eq!(window.find("copy-preview").label(), Some("Copy"));
        assert_eq!(window.find("close-preview").label(), Some("Close"));
        assert_eq!(window.find("open-preview-editor").role(), Some(gpui_kit::Role::Button));
        assert_eq!(window.find("open-preview-vscode").role(), Some(gpui_kit::Role::Button));
        window.click("copy-preview", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("pub fn nested() {}"));
        window.click("close-preview", cx);
        assert!(view.read(cx).file_preview.is_none(), "close dismisses the preview");
    })
    .unwrap();
    cx.update_window(window, |_, window, cx| {
        window.click("workspace-file-README.md", cx);
        assert!(view.read(cx).file_preview.is_none(), "mouse activation also reads asynchronously");
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, _, cx| {
        let preview = view.read(cx).file_preview.as_ref().map(|preview| preview.text.clone());
        assert_eq!(preview.as_deref(), Some("# Notes\n\nhello from the workspace"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn workspace_files_show_loading_and_empty_states(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.inspector_tab = super::super::InspectorTab::Files;
            view.files_scan_cancel = Some(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)));
        });
        window.render_frame(cx);
        let loading = window.find("file-list-loading");
        assert_eq!(loading.role(), Some(gpui_kit::Role::Status));
        assert_eq!(loading.label(), Some("Scanning workspace files"));
        assert!(window.find("file-scan-loading-label").bounds().size.height > px(0.));

        view.update(cx, |view, cx| {
            view.files_scan_cancel = None;
            cx.notify();
        });
        window.render_frame(cx);
        let empty = window.find("file-list-empty");
        assert_eq!(empty.role(), Some(gpui_kit::Role::Status));
        assert_eq!(empty.label(), Some("No workspace files"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn inspector_changes_list_files_and_open_a_diff_viewer(cx: &mut TestAppContext) {
    use vtcode_exec_events::{
        FileChangeItem, FileUpdateChange, ItemCompletedEvent, PatchApplyStatus, PatchChangeKind, ThreadEvent,
        ThreadItem, ThreadItemDetails,
    };

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _cx| {
            view.workspace = Some(std::env::temp_dir());
            let patch = format!(
                "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,80 +1,80 @@\n{}",
                (1..=80)
                    .map(|line| format!("-line {line}\n+{}\n", if line == 40 { "here" } else { "updated" }))
                    .collect::<String>()
            );
            view.handle_thread_event(vtcode_desktop_protocol::ThreadEventNotification {
                session_id: "session-f".to_string(),
                turn_id: Some("turn-1".to_string()),
                event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                    item: ThreadItem {
                        context: None,
                        id: "change-9".to_string(),
                        details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                            diff_incomplete: Some(false),
                            changes: vec![FileUpdateChange {
                                path: "src/app.rs".to_string(),
                                kind: PatchChangeKind::Update,
                                terminators_normalized: None,
                            }],
                            status: PatchApplyStatus::Completed,
                            unified_diff: Some(patch.clone()),
                            additions: Some(80),
                            deletions: Some(80),
                        })),
                    },
                })),
            });
            assert_eq!(view.changes.entries().len(), 1);
            assert_eq!(view.changes.entries()[0].path, "src/app.rs");
            assert_eq!(view.changes.totals(), Some((80, 80)));
        });
        window.render_frame(cx);
        window.click(opaque_path_element_id("change-row", "src/app.rs"), cx);
        {
            let view = view.read(cx);
            assert_eq!(view.selected_change.as_deref(), Some("src/app.rs"));
            assert!(view.change_viewer.is_some(), "selecting a file builds the diff viewer");
        }
        window.click("copy-change-patch", cx);
        assert!(cx.read_from_clipboard().unwrap().text().unwrap().contains("+here"));
        let viewer = view.read(cx).change_viewer.as_ref().unwrap().clone();
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Review);
        assert_eq!(window.find("diff-review-panel").role(), Some(gpui_kit::Role::Dialog));
        assert!(view.read(cx).review_focus.contains_focused(window, cx));
        window.within("diff-review-panel").click("review-split", cx);
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).mode(), gpui_kit::component::diff::DiffMode::Split);
        assert_eq!(window.find("review-split").label(), Some("Split diff view (selected)"));
        assert_eq!(window.find("review-unified").label(), Some("Unified diff view"));
        window.within("diff-review-panel").click("review-unified", cx);
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).mode(), gpui_kit::component::diff::DiffMode::Unified);
        assert_eq!(window.find("review-unified").label(), Some("Unified diff view (selected)"));
        assert_eq!(window.find("review-split").label(), Some("Split diff view"));
        tab_out_and_back_into_scope(window, cx, "the review surface", |window, cx| {
            view.read(cx).review_focus.contains_focused(window, cx)
        });
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Review);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/app.rs"));
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        window.click("close-diff-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Review);
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert_panel_embeds(window, "diff-review-panel");
        let panel = window.find("diff-review-panel").bounds();
        let diff = window.within("diff-review-panel").find("change-diff-view").bounds();
        assert!(diff.size.width > px(400.), "expanded review keeps a usable diff width: {diff:?} in {panel:?}");
        assert!(diff.right() >= panel.right() - px(60.), "diff view fills the panel width: {diff:?} in {panel:?}");
        assert!(window.within("diff-review-panel").find("change-diff-view").bounds().size.height > px(400.));
        let viewport = window.within("diff-review-panel").find("change-diff-view").bounds();
        window.scroll("change-diff-view", gpui_kit::ScrollDelta::Lines(point(0., -100.)), cx);
        window.render_frame(cx);
        let scrolled_line = window.within(("new", 52usize)).find("source").bounds();
        assert!(
            scrolled_line.top() >= viewport.top() && scrolled_line.bottom() <= viewport.bottom(),
            "the Kit Diff's virtualized list should scroll within its own review viewport"
        );
        view.update(cx, |view, cx| {
            view.reset_session_state();
            view.restore_focus_after_review_close(window, cx);
            assert!(view.current_route() != Route::Review);
            assert!(!view.review_focus.contains_focused(window, cx));
            assert!(view.composer.read(cx).focus_handle(cx).is_focused(window));
        });
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Review);
        assert!(view.read(cx).change_viewer.is_none());
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn diff_panes_group_the_file_list_above_the_viewer_with_a_divider(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.inspector_open = true;
            let mut item = TranscriptItem::new("edits".into(), "diff".into(), "Edits".into(), String::new());
            item.diff = Some(super::super::DiffCard {
                files: Vec::new(),
                additions: Some(1),
                deletions: Some(1),
                incomplete: false,
                failed: false,
                patch: "--- a/first.rs\n+++ b/first.rs\n@@ -1 +1 @@\n-old\n+new\n".into(),
            });
            view.transcript.push(item);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
            view.select_change("first.rs".into(), cx);
        });
        window.render_frame(cx);
        let list = window.find("change-list").bounds();
        let viewer = window.find("change-diff-view").bounds();
        assert!(list.top() < viewer.top(), "file list sits above the diff viewer: list={list:?} viewer={viewer:?}");
        assert!(
            viewer.top() - list.bottom() >= px(4.),
            "a divider separates the two panes: list={list:?} viewer={viewer:?}"
        );
    })
    .unwrap();
}

#[test]
fn file_tree_items_preserves_entry_order_at_every_level() {
    use super::super::files::FileEntry;
    use std::collections::HashSet;

    // Deliberately unsorted: a dir before a file at root, reverse-alphabetical
    // children below it. Levels must keep vec order, never sorted or map order.
    let entries = vec![
        FileEntry { rel_path: "b-dir".into(), depth: 0, is_dir: true },
        FileEntry {
            rel_path: "a-file.txt".into(),
            depth: 0,
            is_dir: false,
        },
        FileEntry {
            rel_path: "b-dir/z-child.txt".into(),
            depth: 1,
            is_dir: false,
        },
        FileEntry {
            rel_path: "b-dir/a-child.txt".into(),
            depth: 1,
            is_dir: false,
        },
    ];
    let expanded = HashSet::from(["b-dir".to_string()]);
    let items = super::super::file_tree_items(&entries, None, &expanded);
    let root_ids: Vec<&str> = items.iter().map(|item| item.id.as_ref()).collect();
    assert_eq!(root_ids, ["b-dir", "a-file.txt"]);
    let dir_children: Vec<&str> = items[0].children.iter().map(|item| item.id.as_ref()).collect();
    assert_eq!(dir_children, ["b-dir/z-child.txt", "b-dir/a-child.txt"]);
    assert!(items[1].children.is_empty(), "files never recurse into children");
}

#[gpui_kit::test]
fn file_rename_round_trip_refreshes_tree_repoints_preview_and_remaps_attachments(cx: &mut TestAppContext) {
    use super::super::files_panel::RenameFilePath;

    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("README.md"), "# Notes").expect("write file");
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.attachments
                .push(vtcode_desktop_protocol::TurnAttachment { path: "README.md".into(), text: "# Notes".into() });
            view.refresh_files(cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    // Preview the file first: a rename must repoint the open preview instead
    // of leaving it on a path that no longer exists.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview_overlay("README.md".to_string(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(RenameFilePath { path: "README.md".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let dialog = window.find("file-rename-dialog");
        assert_eq!(dialog.role(), Some(gpui_kit::Role::Dialog));
        assert_eq!(dialog.label(), Some("Rename “README.md”"));
        // Typing replaces the prefill outright: the dialog focuses the input
        // with the whole base name selected. The renamed file below is the
        // oracle — without select-all the name would append to the prefill.
        window.input("NOTES.md", cx);
        window.render_frame(cx);
        window.click("file-rename-commit", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!workspace.path().join("README.md").exists());
    assert_eq!(std::fs::read_to_string(workspace.path().join("NOTES.md")).unwrap(), "# Notes");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let state = view.read(cx);
        assert!(state.file_rename.is_none(), "success closes the dialog");
        assert!(file_tree_paths(&view, cx).contains(&"NOTES.md".to_string()));
        assert!(!file_tree_paths(&view, cx).contains(&"README.md".to_string()));
        assert_eq!(
            state
                .file_tree
                .read(cx)
                .selected_item()
                .map(|item| item.id.to_string())
                .as_deref(),
            Some("NOTES.md"),
            "the tree selection follows the rename"
        );
        assert_eq!(
            state.file_preview.as_ref().map(|preview| preview.rel_path.as_str()),
            Some("NOTES.md"),
            "the open preview follows the rename"
        );
        assert_eq!(
            state
                .attachments
                .iter()
                .map(|attachment| attachment.path.as_str())
                .collect::<Vec<_>>(),
            ["NOTES.md"],
            "composer attachments follow the rename"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_rename_of_a_directory_rebases_descendants_and_keeps_expansion(cx: &mut TestAppContext) {
    use super::super::files_panel::RenameFilePath;

    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::create_dir_all(workspace.path().join("src/nested")).expect("create nested");
    std::fs::write(workspace.path().join("src/nested/deep.rs"), "deep").expect("write nested");
    std::fs::write(workspace.path().join("src/top.rs"), "top").expect("write top");
    std::fs::create_dir_all(workspace.path().join("src2")).expect("create sibling");
    std::fs::write(workspace.path().join("src2/other.rs"), "other").expect("write sibling");
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.attachments.push(vtcode_desktop_protocol::TurnAttachment {
                path: "src/nested/deep.rs".into(),
                text: "deep".into(),
            });
            view.attachments
                .push(vtcode_desktop_protocol::TurnAttachment { path: "src2/other.rs".into(), text: "other".into() });
            view.refresh_files(cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        window.click("workspace-file-src", cx);
        window.render_frame(cx);
        window.click("workspace-file-src/nested", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_file_preview_overlay("src/nested/deep.rs".to_string(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(RenameFilePath { path: "src".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("file-rename-dialog").label(), Some("Rename “src”"));
        window.input("lib", cx);
        window.render_frame(cx);
        window.click("file-rename-commit", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!workspace.path().join("src").exists());
    assert!(workspace.path().join("lib/nested/deep.rs").exists());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let paths = file_tree_paths(&view, cx);
        assert!(paths.contains(&"lib/nested/deep.rs".to_string()));
        assert!(paths.contains(&"lib/top.rs".to_string()), "descendants move with the directory");
        assert!(!paths.iter().any(|path| path == "src" || path.starts_with("src/")));
        assert!(paths.contains(&"src2".to_string()));
        assert!(workspace.path().join("src2/other.rs").exists(), "prefix lookalikes stay put");
        let state = view.read(cx);
        assert_eq!(
            state
                .file_tree
                .read(cx)
                .selected_item()
                .map(|item| item.id.to_string())
                .as_deref(),
            Some("lib"),
            "the tree selection follows the rename"
        );
        assert_eq!(
            state
                .attachments
                .iter()
                .map(|attachment| attachment.path.as_str())
                .collect::<Vec<_>>(),
            ["lib/nested/deep.rs", "src2/other.rs"],
            "attachments under the renamed directory are rebased, lookalikes untouched"
        );
        assert_eq!(
            state.file_preview.as_ref().map(|preview| preview.rel_path.as_str()),
            Some("lib/nested/deep.rs"),
            "a preview under the renamed directory is repointed"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_rename_rejects_bad_names_and_same_name_is_a_no_op(cx: &mut TestAppContext) {
    use super::super::files_panel::RenameFilePath;

    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("keep.md"), "keep").expect("write file");
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
        window.dispatch_action(Box::new(RenameFilePath { path: "keep.md".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.input("sub/evil.md", cx);
        window.render_frame(cx);
        window.click("file-rename-commit", cx);
        window.render_frame(cx);
        assert_eq!(window.find("file-rename-error").role(), Some(gpui_kit::Role::Alert));
        assert!(view.read(cx).file_rename.is_some(), "failure keeps the dialog open");
        // Escape cancels the dialog and returns to the tree.
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).file_rename.is_none(), "escape cancels the dialog");
    })
    .unwrap();
    assert!(!workspace.path().join("sub").exists(), "rejected names never touch the fs");
    assert_eq!(std::fs::read_to_string(workspace.path().join("keep.md")).unwrap(), "keep");
    // The same name commits without a write and closes the dialog.
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(RenameFilePath { path: "keep.md".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("file-rename-commit", cx);
        window.render_frame(cx);
        assert!(view.read(cx).file_rename.is_none());
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(workspace.path().join("keep.md")).unwrap(), "keep");
}

#[gpui_kit::test]
fn file_delete_stays_confirm_gated_and_trashes_after_confirm(cx: &mut TestAppContext) {
    use super::super::files_panel::DeleteFilePath;

    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("doomed.md"), "doomed").expect("write file");
    std::fs::write(workspace.path().join("spared.md"), "spared").expect("write file");
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
        view.update(cx, |view, cx| view.open_file_preview_overlay("doomed.md".to_string(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    // Dispatching alone changes nothing: the native warning prompt is the
    // gate, and headless it never answers — mirroring the thread-delete test.
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(DeleteFilePath { path: "doomed.md".into() }), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(workspace.path().join("doomed.md").exists(), "delete waits for the warning prompt answer");
    // The post-confirm path is driven directly: the file leaves the tree,
    // the open preview closes, and the sibling is untouched.
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            let root = view.workspace.clone().expect("workspace");
            view.delete_file_after_confirm(root, "doomed.md".to_string(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!workspace.path().join("doomed.md").exists());
    assert_eq!(std::fs::read_to_string(workspace.path().join("spared.md")).unwrap(), "spared");
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(!file_tree_paths(&view, cx).contains(&"doomed.md".to_string()));
        assert!(file_tree_paths(&view, cx).contains(&"spared.md".to_string()));
        assert!(view.read(cx).file_preview.is_none(), "deleting the previewed file closes its preview");
    })
    .unwrap();
}

#[gpui_kit::test]
fn file_tree_keyboard_offers_rename_and_gated_delete(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("keys.md"), "keys").expect("write file");
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
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.press("f2", cx);
        window.render_frame(cx);
        assert_eq!(window.find("file-rename-dialog").label(), Some("Rename “keys.md”"));
        window.press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).file_rename.is_none());
        // Both delete chords route to the confirm-gated prompt, which never
        // answers headless — the file survives either key.
        window.press("delete", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(workspace.path().join("keys.md").exists());
    cx.update_window(window, |_, window, cx| {
        window.press("backspace", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(workspace.path().join("keys.md").exists());
}

#[gpui_kit::test]
fn file_tree_filter_narrows_to_ranked_hits_and_restores(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("main.rs"), "fn main() {}").expect("write file");
    std::fs::create_dir(workspace.path().join("src")).expect("create dir");
    std::fs::write(workspace.path().join("src/main.rs"), "fn main() {}").expect("write nested file");
    std::fs::write(workspace.path().join("src/lib.rs"), "").expect("write other file");

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
        assert_eq!(file_tree_paths(&view, cx), vec!["src", "main.rs"]);
        assert!(view.read(cx).file_search_hit_count.is_none(), "browse mode tracks no hit count");
        // `set_value` emits no Change headless (Kit suppresses it), so the
        // test drives the subscription handler directly, like the composer
        // suggestion tests.
        view.update(cx, |view, cx| {
            view.file_filter_input
                .update(cx, |input, cx| input.set_value("main", window, cx));
            view.rebuild_file_tree(cx);
        });
        window.render_frame(cx);
        assert_eq!(file_tree_paths(&view, cx), vec!["main.rs", "src/main.rs"]);
        assert_eq!(
            window.find("workspace-file-label-main.rs").bounds().origin.x,
            window.find("workspace-file-label-src/main.rs").bounds().origin.x,
            "search results stay flat regardless of their path"
        );
        assert_eq!(window.find("file-list-filter-count").label(), Some("2 of 4 shown"));
        view.update(cx, |view, cx| {
            view.file_filter_input
                .update(cx, |input, cx| input.set_value("zzz-no-match", window, cx));
            view.rebuild_file_tree(cx);
        });
        window.render_frame(cx);
        assert_eq!(window.find("file-list-filter-empty").label(), Some("No files match this filter"));
        assert_eq!(window.find("file-list-filter-count").label(), Some("0 of 4 shown"));
        view.update(cx, |view, cx| {
            view.file_filter_input.update(cx, |input, cx| input.set_value("", window, cx));
            view.rebuild_file_tree(cx);
        });
        window.render_frame(cx);
        assert_eq!(file_tree_paths(&view, cx), vec!["src", "main.rs"]);
        assert!(view.read(cx).file_search_hit_count.is_none(), "clearing restores browse mode");
    })
    .unwrap();
}

#[gpui_kit::test]
fn truncation_notice_names_the_snapshot_bound(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("notes.md"), "hello").expect("write file");
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
        // The bound itself is pinned by model tests; here the flag's render
        // wiring is what matters.
        view.update(cx, |view, _| view.files_truncated = true);
        window.render_frame(cx);
        window.within("inspector-tabs").click(1usize, cx);
        window.render_frame(cx);
        assert_eq!(window.find("file-list-truncated").label(), Some("Showing the first 5000 files"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn clicking_outside_closes_the_file_preview(cx: &mut TestAppContext) {
    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("README.md"), "# Notes").expect("write file");
    let (window, view) = open_desktop(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    cx.update_window(window, |_, _, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.refresh_files(cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| view.open_file_preview_overlay("README.md".to_string(), window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_some());
        window.click("message-composer-frame", cx);
        window.render_frame(cx);
        assert!(window.try_find("file-preview").is_none(), "clicking away closes the preview");
    })
    .unwrap();
    cx.update(|cx| {
        assert!(view.read(cx).file_preview.is_none(), "preview state must be closed");
    });
}
