use super::*;

#[gpui_kit::test]
fn quiet_welcome_keeps_workspace_utilities_in_navigation_without_creating_a_session(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("welcome").label(), Some("VT Code welcome"));
        assert!(window.find_all("thread-header").is_empty());
        for id in ["welcome-review-changes", "welcome-browse-files", "welcome-choose-model"] {
            assert!(window.find_all(id).is_empty(), "welcome should not duplicate workspace controls");
        }
        window.click("nav-utilities", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, crate::desktop::inspector::InspectorTab::Utilities);
        assert_eq!(window.find("nav-utilities").label(), Some("Workspace utilities (selected)"));
        assert!(view.read(cx).selected_session_id.is_none());
        assert_eq!(window.find("send-prompt").label(), Some("Send message"));
        view.update(cx, |view, _| {
            view.workspace = None;
            view.inspector_open = false;
            view.welcome_copy_index = 0;
        });
        window.render_frame(cx);
        assert!(window.find_all("open-first-workspace").is_empty());
        view.update(cx, |view, _| {
            view.workspace = Some(std::env::temp_dir());
            view.transcript.push(TranscriptItem::new(
                "first-message".into(),
                "user".into(),
                "You".into(),
                "Inspect this workspace".into(),
            ));
        });
        window.render_frame(cx);
        assert!(window.find_all("welcome").is_empty());
        assert_eq!(window.find_all("thread-header").len(), 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn selected_diff_tracks_runtime_updates_and_keeps_unrelated_review_state(cx: &mut TestAppContext) {
    use gpui_kit::component::diff::{DiffLineRange, DiffMode, DiffSide};
    use vtcode_exec_events::{
        FileChangeItem, FileUpdateChange, ItemCompletedEvent, PatchApplyStatus, PatchChangeKind, ThreadEvent,
        ThreadItem, ThreadItemDetails,
    };

    fn notification(id: &str, path: &str, patch: &str) -> BackendEvent {
        BackendEvent::ThreadEvent(vtcode_desktop_protocol::ThreadEventNotification {
            session_id: "review-session".into(),
            turn_id: None,
            event: vtcode_exec_events::VersionedThreadEvent::new(ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    context: None,
                    id: id.into(),
                    details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                        changes: vec![FileUpdateChange {
                            path: path.into(),
                            kind: PatchChangeKind::Update,
                            terminators_normalized: None,
                        }],
                        status: PatchApplyStatus::Completed,
                        unified_diff: Some(patch.into()),
                        diff_incomplete: Some(false),
                        additions: None,
                        deletions: None,
                    })),
                },
            })),
        })
    }
    let first = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+first\n";
    let latest = "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1,2 @@\n-first\n+latest one\n+latest two\n";
    let unrelated = "--- a/docs/app.rs.bak\n+++ b/docs/app.rs.bak\n@@ -1 +1 @@\n-note\n+mentions src/app.rs\n";
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.selected_session_id = Some("review-session".into());
            view.handle_backend_event(notification("edit", "src/app.rs", first), cx);
        });
        window.render_frame(cx);
        window.click(opaque_path_element_id("change-row", "src/app.rs"), cx);
        let viewer = view.read(cx).change_viewer.as_ref().unwrap().clone();
        viewer.update(cx, |viewer, cx| {
            viewer.set_mode(DiffMode::Split, cx);
            viewer.set_selected_lines(Some(DiffLineRange::new("src/app.rs", DiffSide::Modified, 1, 1)), cx);
        });
        let selection = viewer.read(cx).selected_lines();
        assert!(selection.is_some());
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Review);
        view.update(cx, |view, cx| view.handle_backend_event(notification("other", "docs/app.rs.bak", unrelated), cx));
        window.render_frame(cx);
        assert_eq!(
            window.find(opaque_path_element_id("change-row", "docs/app.rs.bak")).label(),
            Some("Review docs/app.rs.bak")
        );
        assert_eq!(
            window.find(opaque_path_element_id("change-row", "src/app.rs")).label(),
            Some("Review src/app.rs (selected)"),
            "a newly inserted row must not inherit an existing file's accessibility node"
        );
        assert_eq!(viewer.read(cx).selected_lines(), selection, "unrelated events preserve review selection");
        // Updating the same item id exercises the streaming/upsert path, not only a new change-set.
        view.update(cx, |view, cx| view.handle_backend_event(notification("edit", "src/app.rs", latest), cx));
        window.render_frame(cx);
        assert_eq!(viewer.read(cx).files()[0].additions(), 2, "the retained viewer must receive the accepted update");
        assert_eq!(viewer.read(cx).mode(), DiffMode::Split);
        assert!(viewer.read(cx).selected_lines().is_none(), "new text invalidates an old line selection");
        window.within("diff-review-panel").click("copy-change-patch", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(latest));
        window
            .within("diff-review-panel")
            .click(opaque_path_element_id("change-row", "docs/app.rs.bak"), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("docs/app.rs.bak"));
        assert_eq!(view.read(cx).change_viewer.as_ref().unwrap().read(cx).mode(), DiffMode::Split);
        window
            .within("diff-review-panel")
            .click(opaque_path_element_id("change-row", "src/app.rs"), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/app.rs"));
        // A malformed replacement invalidates this item's preview rather than leaving a stale viewer.
        view.update(cx, |view, cx| {
            view.event_tx.send(notification("edit", "src/app.rs", "invalid patch")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(view.selected_change.is_none());
            assert!(view.change_viewer.is_none());
            assert!(view.current_route() != Route::Review);
            assert!(!view.review_focus.contains_focused(window, cx));
            assert!(view.composer.read(cx).focus_handle(cx).is_focused(window));
        });
        assert!(view.read(cx).selected_change.is_none());
        assert!(view.read(cx).change_viewer.is_none());
        window.render_frame(cx);
        assert!(view.read(cx).current_route() != Route::Review);
        assert!(view.read(cx).composer.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn loaded_session_restores_review_without_replaying_live_run_state(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    let patch = format!(
        "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-{}\n+{}\n",
        "original word ".repeat(30),
        "restored word ".repeat(35)
    );
    let item = ThreadItem {
        context: None,
        id: "saved-edit".into(),
        details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
            changes: vec![FileUpdateChange {
                path: "src/app.rs".into(),
                kind: PatchChangeKind::Update,
                terminators_normalized: None,
            }],
            status: PatchApplyStatus::Completed,
            unified_diff: Some(patch.clone()),
            diff_incomplete: Some(false),
            additions: None,
            deletions: None,
        })),
    };
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, cx| {
            view.session_request = Some(vtcode_desktop_protocol::method::SESSION_LOAD);
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(),
                error: None,
                result: Some(json!({
                    "session": {"sessionId": "restored", "title": "Saved work", "workspace": "/workspace", "completed": true, "resumable": true},
                    "messages": [{"role": "user", "text": "Make this change"}],
                    "reviewItems": [item], "reviewHistoryIncomplete": true,
                })),
            }, cx);
            assert!(!view.running);
            assert!(view.active_turn_id.is_none());
            assert_eq!(view.selected_session_id, None);
            assert_eq!(view.session_request, Some(vtcode_desktop_protocol::method::SESSION_LOAD));
        });
    }).unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            assert_eq!(view.selected_session_id.as_deref(), Some("restored"));
            assert!(view.status.contains("partial"));
            assert!(view.restored_review_history_incomplete);
            assert!(!view.restored_activity_history_incomplete);
            assert_eq!(view.changes.entries().len(), 1);
            assert_eq!(view.changes.patch_for(&view.transcript, "src/app.rs").as_deref(), Some(patch.as_str()));
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Changes;
        });
        window.render_frame(cx);
        let warning = window.find("review-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some("Saved change history is partial. Some historical patches are unavailable."));
        window.click(opaque_path_element_id("change-row", "src/app.rs"), cx);
        window.render_frame(cx);
        window.click("copy-change-patch", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(patch.as_str()));
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Review);
        window.click("review-split", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        let viewport = window.find("change-diff-view").bounds();
        let original = window.within(("old", 0usize)).find("source").bounds();
        let modified = window.within(("new", 0usize)).find("source").bounds();
        assert!(original.size.width > px(100.));
        assert!(modified.size.width > px(100.));
        assert!(original.right() <= modified.left());
        assert!(modified.right() <= viewport.right() + px(1.), "both wrapped split columns must remain visible");
    })
    .unwrap();
}

#[gpui_kit::test]
fn changes_inspector_lists_structured_paths_without_diff_previews(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(std::env::temp_dir());
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Changes;
            view.upsert_diff_card(
                "change-no-preview".into(),
                &vtcode_exec_events::FileChangeItem {
                    diff_incomplete: Some(true),
                    changes: vec![vtcode_exec_events::FileUpdateChange {
                        path: "src/updated.rs".into(),
                        kind: vtcode_exec_events::PatchChangeKind::Update,
                        terminators_normalized: None,
                    }],
                    status: vtcode_exec_events::PatchApplyStatus::Completed,
                    unified_diff: None,
                    additions: None,
                    deletions: None,
                },
            );
            assert_eq!(view.changes.entries().len(), 1);
            assert_eq!(view.changes.totals(), None);
            cx.notify();
        });
        window.render_frame(cx);
        let row = window.find(opaque_path_element_id("change-row", "src/updated.rs"));
        assert_eq!(row.disabled(), Some(true));
        assert_eq!(row.label(), Some("Diff preview unavailable for src/updated.rs"));
        assert_eq!(window.find("previous-change-file").disabled(), Some(true));
        assert_eq!(window.find("next-change-file").disabled(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn changes_inspector_virtualizes_file_rows_and_scrolls_to_the_selected_path(cx: &mut TestAppContext) {
    use vtcode_exec_events::{FileChangeItem, FileUpdateChange, PatchApplyStatus, PatchChangeKind};

    let (window, view) = open_desktop(cx);
    let file_count = 120;
    let changes = (0..file_count)
        .map(|index| FileUpdateChange {
            path: format!("src/file-{index}.rs"),
            kind: PatchChangeKind::Update,
            terminators_normalized: None,
        })
        .collect::<Vec<_>>();
    let patch = (0..file_count)
        .map(|index| {
            format!(
                "diff --git a/src/file-{index}.rs b/src/file-{index}.rs\n--- a/src/file-{index}.rs\n+++ b/src/file-{index}.rs\n@@ -1 +1 @@\n-before\n+after\n"
            )
        })
        .collect::<String>();

    view.update(cx, |view, cx| {
        view.inspector_open = true;
        view.inspector_tab = crate::desktop::inspector::InspectorTab::Changes;
        view.upsert_diff_card(
            "many-changes".into(),
            &FileChangeItem {
                diff_incomplete: Some(false),
                changes,
                status: PatchApplyStatus::Completed,
                unified_diff: Some(patch),
                additions: Some(file_count as u64),
                deletions: Some(file_count as u64),
            },
        );
        assert_eq!(view.changes.entries().len(), file_count);
        cx.notify();
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let first_row = opaque_path_element_id("change-row", "src/file-0.rs");
        let last_row = opaque_path_element_id("change-row", "src/file-119.rs");
        let list = window.find("change-list");
        assert_eq!(list.role(), Some(gpui_kit::Role::List));
        assert_eq!(list.label(), Some("Changed files"));
        let first_item = window.find(opaque_path_element_id("change-list-item", "src/file-0.rs"));
        assert_eq!(first_item.role(), Some(gpui_kit::Role::ListItem));
        assert_eq!(first_item.label(), Some("Modified src/file-0.rs, +1 −1"));
        assert_eq!(first_item.selected(), Some(false));
        assert!(window.try_find(first_row.clone()).is_some(), "the first changed file is visible initially");
        assert!(
            window.try_find(last_row.clone()).is_none(),
            "offscreen changed files are not built into the UI tree"
        );

        view.update(cx, |view, cx| view.select_change("src/file-119.rs".into(), cx));
        window.render_frame(cx);

        assert!(window.try_find(last_row).is_some(), "selecting a file scrolls its row into view");
        assert!(window.try_find(first_row).is_none(), "rows outside the selected range leave the UI tree");
        assert_eq!(
            window
                .find(opaque_path_element_id("change-list-item", "src/file-119.rs"))
                .selected(),
            Some(true),
            "the selected state is exposed to assistive technology"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn changes_tab_filters_by_kind_and_gates_navigation_on_visible_previews(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            // One added, one modified, one deleted file: asymmetric kinds so
            // the filter cannot pass on identical rows.
            let mut item = TranscriptItem::new("edits".into(), "diff".into(), "Edits".into(), String::new());
            item.diff = Some(super::super::DiffCard {
                files: Vec::new(),
                additions: Some(2),
                deletions: Some(2),
                incomplete: false,
                failed: false,
                patch: "--- /dev/null\n+++ b/added.rs\n@@ -0,0 +1 @@\n+new\n--- a/edited.rs\n+++ b/edited.rs\n@@ -1 +1 @@\n-old\n+new\n--- a/removed.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-gone\n"
                    .into(),
            });
            view.transcript.push(item);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
            assert_eq!(view.changes.entries().len(), 3);
        });
        window.render_frame(cx);

        assert_eq!(window.find("change-files-count").label(), Some("3 files"));
        assert_eq!(window.find("change-filter-added").label(), Some("Added"));
        assert_eq!(window.find("change-filter-added").checked(), Some(false));
        assert!(window.try_find("change-filter-modified").is_some());
        assert!(window.try_find("change-filter-deleted").is_some());
        assert!(window.try_find(opaque_path_element_id("change-list-item", "added.rs")).is_some());
        assert!(window.try_find(opaque_path_element_id("change-list-item", "edited.rs")).is_some());
        assert!(window.try_find(opaque_path_element_id("change-list-item", "removed.rs")).is_some());

        window.click("change-filter-added", cx);
        window.render_frame(cx);

        assert_eq!(window.find("change-filter-added").checked(), Some(true));
        assert_eq!(window.find("change-files-count").label(), Some("1 of 3 shown"));
        assert!(window.try_find(opaque_path_element_id("change-list-item", "added.rs")).is_some());
        assert!(window.try_find(opaque_path_element_id("change-list-item", "edited.rs")).is_none());
        assert!(window.try_find(opaque_path_element_id("change-list-item", "removed.rs")).is_none());
        assert_eq!(
            window.find("next-change-file").disabled(),
            Some(true),
            "one visible preview leaves nowhere to navigate"
        );

        window.click("change-filter-added", cx);
        window.render_frame(cx);

        assert_eq!(window.find("change-filter-added").checked(), Some(false));
        assert_eq!(window.find("change-files-count").label(), Some("3 files"));
        assert!(window.try_find(opaque_path_element_id("change-list-item", "removed.rs")).is_some());
        assert_ne!(
            window.find("next-change-file").disabled(),
            Some(true),
            "clearing the filter restores navigation"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn changes_filter_stays_clearable_when_its_kind_disappears(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            let mut item = TranscriptItem::new("edits".into(), "diff".into(), "Edits".into(), String::new());
            item.diff = Some(super::super::DiffCard {
                files: Vec::new(),
                additions: Some(1),
                deletions: Some(1),
                incomplete: false,
                failed: false,
                patch: "--- /dev/null\n+++ b/added.rs\n@@ -0,0 +1 @@\n+new\n--- a/edited.rs\n+++ b/edited.rs\n@@ -1 +1 @@\n-old\n+new\n"
                    .into(),
            });
            view.transcript.push(item);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
        });
        window.render_frame(cx);

        window.click("change-filter-added", cx);
        window.render_frame(cx);
        assert!(window.try_find(opaque_path_element_id("change-list-item", "added.rs")).is_some());
        assert!(window.try_find(opaque_path_element_id("change-list-item", "edited.rs")).is_none());

        // Re-editing the added file flips its kind to Modified; the stale
        // Added toggle must stay so the filter never strands.
        view.update(cx, |view, _| {
            let mut reedit = TranscriptItem::new("reedit".into(), "diff".into(), "Edits".into(), String::new());
            reedit.diff = Some(super::super::DiffCard {
                files: Vec::new(),
                additions: Some(1),
                deletions: Some(1),
                incomplete: false,
                failed: false,
                patch: "--- a/added.rs\n+++ b/added.rs\n@@ -1 +1 @@\n-new\n+newer\n".into(),
            });
            view.transcript.push(reedit);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
            assert_eq!(view.changes.entries().len(), 2);
        });
        window.render_frame(cx);

        assert_eq!(window.find("change-filter-added").checked(), Some(true));
        assert_eq!(window.find("change-files-count").label(), Some("0 of 2 shown"));
        assert!(window.try_find(opaque_path_element_id("change-list-item", "added.rs")).is_none());

        // Switching to the sole surviving kind must keep the row: hiding it
        // would strand the new filter with no toggle to clear it.
        window.click("change-filter-modified", cx);
        window.render_frame(cx);

        assert_eq!(window.find("change-filter-modified").checked(), Some(true));
        assert_eq!(window.find("change-files-count").label(), Some("2 of 2 shown"));
        assert!(window.try_find(opaque_path_element_id("change-list-item", "added.rs")).is_some());

        window.click("change-filter-modified", cx);
        window.render_frame(cx);

        assert!(window.try_find("change-kind-filter").is_none(), "the row leaves with the last filter");
        assert!(window.try_find(opaque_path_element_id("change-list-item", "added.rs")).is_some());
        assert!(window.try_find(opaque_path_element_id("change-list-item", "edited.rs")).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn changes_filter_clears_directly_from_a_stale_toggle(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            let mut item = TranscriptItem::new("edits".into(), "diff".into(), "Edits".into(), String::new());
            item.diff = Some(super::super::DiffCard {
                files: Vec::new(),
                additions: Some(1),
                deletions: Some(1),
                incomplete: false,
                failed: false,
                patch: "--- /dev/null\n+++ b/added.rs\n@@ -0,0 +1 @@\n+new\n--- a/edited.rs\n+++ b/edited.rs\n@@ -1 +1 @@\n-old\n+new\n"
                    .into(),
            });
            view.transcript.push(item);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
        });
        window.render_frame(cx);

        window.click("change-filter-added", cx);
        window.render_frame(cx);

        view.update(cx, |view, _| {
            let mut reedit = TranscriptItem::new("reedit".into(), "diff".into(), "Edits".into(), String::new());
            reedit.diff = Some(super::super::DiffCard {
                files: Vec::new(),
                additions: Some(1),
                deletions: Some(1),
                incomplete: false,
                failed: false,
                patch: "--- a/added.rs\n+++ b/added.rs\n@@ -1 +1 @@\n-new\n+newer\n".into(),
            });
            view.transcript.push(reedit);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
        });
        window.render_frame(cx);

        assert_eq!(window.find("change-filter-added").checked(), Some(true));

        window.click("change-filter-added", cx);
        window.render_frame(cx);

        assert!(window.try_find("change-filter-added").is_none(), "absent kinds leave with the filter");
        assert!(window.try_find("change-kind-filter").is_none(), "one surviving kind needs no row");
        assert!(window.try_find(opaque_path_element_id("change-list-item", "added.rs")).is_some());
        assert!(window.try_find(opaque_path_element_id("change-list-item", "edited.rs")).is_some());
    })
    .unwrap();
}

fn restored_file_change(path: &str, patch: &str) -> vtcode_exec_events::FileChangeItem {
    vtcode_exec_events::FileChangeItem {
        changes: vec![vtcode_exec_events::FileUpdateChange {
            path: path.into(),
            kind: vtcode_exec_events::PatchChangeKind::Update,
            terminators_normalized: None,
        }],
        status: vtcode_exec_events::PatchApplyStatus::Completed,
        unified_diff: Some(patch.into()),
        diff_incomplete: Some(false),
        additions: None,
        deletions: None,
    }
}

fn restored_review_item(id: &str, path: &str, patch: &str) -> vtcode_exec_events::ThreadItem {
    vtcode_exec_events::ThreadItem {
        context: None,
        id: id.into(),
        details: vtcode_exec_events::ThreadItemDetails::FileChange(Box::new(restored_file_change(path, patch))),
    }
}

#[gpui_kit::test]
fn restored_review_batch_matches_single_rebuild_with_newest_wins(cx: &mut TestAppContext) {
    let old_patch = "--- a/src/shared.rs\n+++ b/src/shared.rs\n@@ -1 +1 @@\n-old\n+old\n";
    let new_patch = "--- a/src/shared.rs\n+++ b/src/shared.rs\n@@ -1 +1 @@\n-old\n+new\n";
    let only_patch = "--- a/src/only.rs\n+++ b/src/only.rs\n@@ -1 +1 @@\n-before\n+after\n";
    // Asymmetric batch: two change-sets overlap on src/shared.rs with
    // different patches, plus one non-change item the batch must skip.
    let batch = vec![
        restored_review_item("rev-old", "src/shared.rs", old_patch),
        vtcode_exec_events::ThreadItem {
            context: None,
            id: "note".into(),
            details: vtcode_exec_events::ThreadItemDetails::AgentMessage(vtcode_exec_events::AgentMessageItem {
                text: "not a change-set".into(),
            }),
        },
        restored_review_item("rev-new", "src/shared.rs", new_patch),
        restored_review_item("rev-only", "src/only.rs", only_patch),
    ];

    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _cx| {
        view.project_restored_review(&batch);

        let diff_rows = view.transcript.items().filter(|item| item.is_diff()).count();
        assert_eq!(diff_rows, 3, "every change-set stages one diff row; other items are skipped");

        // Independent oracle: the batch result must equal one rebuild of the
        // final transcript, which is the projection's stated contract.
        let expected = super::super::ChangesProjection::rebuild(&view.transcript);
        let paths = |projection: &super::super::ChangesProjection| {
            projection.entries().iter().map(|entry| entry.path.clone()).collect::<Vec<_>>()
        };
        assert_eq!(paths(&view.changes), paths(&expected));
        assert_eq!(view.changes.totals(), expected.totals());
        assert_eq!(view.changes.entries().len(), 2);
        assert_eq!(
            view.changes.patch_for(&view.transcript, "src/shared.rs").as_deref(),
            Some(new_patch),
            "the newest change-set wins for an overlapping path"
        );
        assert_eq!(
            view.changes.entries().first().map(|entry| entry.path.as_str()),
            Some("src/only.rs"),
            "entries list newest change-sets first"
        );
    });
    // Render once so layout consumes the batched rows without divergence.
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn restored_review_batch_dirties_viewer_only_when_selected_patch_changes(cx: &mut TestAppContext) {
    let stable_patch = "--- a/src/stable.rs\n+++ b/src/stable.rs\n@@ -1 +1 @@\n-a\n+b\n";
    let other_patch = "--- a/src/other.rs\n+++ b/src/other.rs\n@@ -1 +1 @@\n-a\n+b\n";
    let updated_patch = "--- a/src/stable.rs\n+++ b/src/stable.rs\n@@ -1 +1 @@\n-b\n+c\n";

    let (window, view) = open_desktop(cx);
    view.update(cx, |view, _cx| {
        // Pre-stage a selected change so the dirty flag has both outcomes.
        view.upsert_diff_card("stable-card".into(), &restored_file_change("src/stable.rs", stable_patch));
        view.selected_change = Some("src/stable.rs".into());
        assert!(!view.change_viewer_dirty);

        // A batch that only touches other paths leaves the viewer clean.
        view.project_restored_review(&[restored_review_item("rev-other", "src/other.rs", other_patch)]);
        assert!(!view.change_viewer_dirty, "untouched selections stay clean");
        assert_eq!(view.changes.entries().len(), 2);

        // A batch that replaces the selected patch dirties the viewer.
        view.project_restored_review(&[restored_review_item("rev-stable", "src/stable.rs", updated_patch)]);
        assert!(view.change_viewer_dirty, "a replaced selected patch refreshes the viewer");
        assert_eq!(view.changes.patch_for(&view.transcript, "src/stable.rs").as_deref(), Some(updated_patch));

        // An empty batch is a no-op: no rebuild, no dirty flag, no rows.
        view.change_viewer_dirty = false;
        let rows_before = view.transcript.len();
        view.project_restored_review(&[]);
        assert!(!view.change_viewer_dirty);
        assert_eq!(view.transcript.len(), rows_before);
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn working_tree_review_searches_changed_files_without_closing_the_review(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    let patch = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -1 +1 @@\n-before\n+after\n";
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.service_info = Some(vtcode_desktop_protocol::InitializeResult {
                protocol: vtcode_desktop_protocol::PROTOCOL_VERSION,
                workspace: "/workspace".into(),
                server_version: "test".into(),
                slash_commands: Vec::new(),
            });
            view.open_workspace_review(window, cx);
            view.refresh_workspace_review(cx);
            let request_id = view.workspace_review.as_ref().unwrap().request_id.clone();
            view.accept_workspace_review(
                vtcode_desktop_protocol::WorkspaceReviewResult {
                    include_untracked: false,
                    scope: String::new(),
                    request_id,
                    workspace: "/workspace".into(),
                    captured_at: "now".into(),
                    patch: Some(patch.into()),
                    truncated: false,
                    note: "Tracked changes against HEAD".into(),
                },
                cx,
            );
        });
        window.render_frame(cx);
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));

        window.click("workspace-review-current-file", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        assert!(view.read(cx).current_route() == Route::Review);
        assert_eq!(window.find("command-palette").label(), Some("Choose changed file"));
        assert!(
            view.read(cx).palette_open_for(&[super::super::PaletteKind::ReviewFiles]),
            "the file trigger stays marked while its palette is up"
        );
        window.within("command-palette").input("b.rs", cx);
        window.render_frame(cx);
        let target_action = super::super::PaletteAction::SelectWorkspaceReviewFile("b.rs".into());
        assert!(window.try_find(palette_action_id(&target_action)).is_some());

        window.within("command-palette").press("escape", cx);
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_some(), "Escape clears the search before dismissing the picker");
        assert!(view.read(cx).palette_query.is_empty());
        window.within("command-palette").press("escape", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(view.read(cx).palette.is_none());
        assert!(
            view.read(cx).current_route() == Route::Review,
            "Escape dismisses the picker before the underlying review"
        );
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 1 of 2 · a.rs"));
        assert!(
            !view.read(cx).palette_open_for(&[super::super::PaletteKind::ReviewFiles]),
            "the file trigger clears once its palette closes"
        );

        window.click("workspace-review-current-file", cx);
        window.render_frame(cx);
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.within("command-palette").input("b.rs", cx);
        window.render_frame(cx);
        window.within("command-palette").press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let review = view.read(cx);
        assert!(review.current_route() == Route::Review);
        assert!(review.palette.is_none());
        assert_eq!(review.workspace_review.as_ref().unwrap().selected_file.as_deref(), Some("b.rs"));
        assert_eq!(window.find("workspace-review-current-file").label(), Some("File 2 of 2 · b.rs"));
    })
    .unwrap();
}
