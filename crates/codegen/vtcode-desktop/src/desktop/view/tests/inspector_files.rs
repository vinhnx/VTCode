use super::*;

#[gpui_kit::test]
fn file_tree_menu_copies_path_and_opens_preview(cx: &mut TestAppContext) {
    use super::super::files_panel::{CopyFilePath, OpenFilePath};

    let workspace = tempfile::TempDir::new().expect("temp workspace");
    std::fs::write(workspace.path().join("README.md"), "# Notes").expect("write file");
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
        for _ in 0..8 {
            window.press("tab", cx);
            window.render_frame(cx);
        }
        assert!(view.read(cx).review_focus.contains_focused(window, cx));
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
