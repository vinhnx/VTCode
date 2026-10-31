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
        assert_eq!(window.within("welcome").find("open-first-workspace").label(), Some("Choose a workspace"));
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
                        changes: vec![FileUpdateChange { path: path.into(), kind: PatchChangeKind::Update }],
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
        window.click(super::super::opaque_path_element_id("change-row", "src/app.rs"), cx);
        let viewer = view.read(cx).change_viewer.as_ref().unwrap().clone();
        viewer.update(cx, |viewer, cx| {
            viewer.set_mode(DiffMode::Split, cx);
            viewer.set_selected_lines(Some(DiffLineRange::new("src/app.rs", DiffSide::Modified, 1, 1)), cx);
        });
        let selection = viewer.read(cx).selected_lines();
        assert!(selection.is_some());
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Review);
        view.update(cx, |view, cx| view.handle_backend_event(notification("other", "docs/app.rs.bak", unrelated), cx));
        window.render_frame(cx);
        assert_eq!(
            window
                .find(super::super::opaque_path_element_id("change-row", "docs/app.rs.bak"))
                .label(),
            Some("Review docs/app.rs.bak")
        );
        assert_eq!(
            window
                .find(super::super::opaque_path_element_id("change-row", "src/app.rs"))
                .label(),
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
            .click(super::super::opaque_path_element_id("change-row", "docs/app.rs.bak"), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("docs/app.rs.bak"));
        assert_eq!(view.read(cx).change_viewer.as_ref().unwrap().read(cx).mode(), DiffMode::Split);
        window
            .within("diff-review-panel")
            .click(super::super::opaque_path_element_id("change-row", "src/app.rs"), cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/app.rs"));
        // A malformed replacement invalidates this item's preview rather than leaving a stale viewer.
        view.update(cx, |view, cx| {
            view.event_tx.send(notification("edit", "src/app.rs", "invalid patch")).unwrap();
            let _batch_full = view.drain_events(window, cx);
            assert!(view.selected_change.is_none());
            assert!(view.change_viewer.is_none());
            assert!(view.route != Route::Review);
            assert!(!view.review_focus.contains_focused(window, cx));
            assert!(view.composer.read(cx).focus_handle(cx).is_focused(window));
        });
        assert!(view.read(cx).selected_change.is_none());
        assert!(view.read(cx).change_viewer.is_none());
        window.render_frame(cx);
        assert!(view.read(cx).route != Route::Review);
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
        window.click(super::super::opaque_path_element_id("change-row", "src/app.rs"), cx);
        window.render_frame(cx);
        window.click("copy-change-patch", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(patch.as_str()));
        window.click("expand-change-review", cx);
        window.render_frame(cx);
        assert!(view.read(cx).route == Route::Review);
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
        let row = window.find(super::super::opaque_path_element_id("change-row", "src/updated.rs"));
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
        let first_row = super::super::opaque_path_element_id("change-row", "src/file-0.rs");
        let last_row = super::super::opaque_path_element_id("change-row", "src/file-119.rs");
        let list = window.find("change-list");
        assert_eq!(list.role(), Some(gpui_kit::Role::List));
        assert_eq!(list.label(), Some("Changed files"));
        let first_item = window.find(super::super::opaque_path_element_id("change-list-item", "src/file-0.rs"));
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
                .find(super::super::opaque_path_element_id("change-list-item", "src/file-119.rs"))
                .selected(),
            Some(true),
            "the selected state is exposed to assistive technology"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn loaded_activity_restores_output_and_interrupts_unfinished_history(cx: &mut TestAppContext) {
    use vtcode_exec_events::*;
    let (window, view) = open_desktop(cx);
    let output = "first line\nsecond line\n";
    let command = ThreadItem {
        id: "saved-command".into(),
        context: None,
        details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "cargo check --locked".into(),
            arguments: None,
            aggregated_output: output.into(),
            exit_code: None,
            status: CommandExecutionStatus::InProgress,
        })),
    };
    let finished = ThreadItem {
        id: "finished-command".into(),
        context: None,
        details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "previous command".into(),
            arguments: None,
            aggregated_output: "previous output".into(),
            exit_code: Some(0),
            status: CommandExecutionStatus::Completed,
        })),
    };
    cx.update_window(window, |_, _window, cx| {
        view.update(cx, |view, cx| {
            view.session_request = Some(vtcode_desktop_protocol::method::SESSION_LOAD);
            view.handle_backend_event(BackendEvent::Response {
                method: vtcode_desktop_protocol::method::SESSION_LOAD.into(), error: None,
                result: Some(json!({
                    "session": {"sessionId": "saved", "title": "Saved work", "workspace": "/workspace", "completed": true, "resumable": true},
                    "messages": [{"role": "tool", "text": "Unstructured archive result"}],
                    "activityItems": [finished, command], "activityHistoryIncomplete": true,
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
            assert!(view.interaction.is_none());
            assert_eq!(view.selected_session_id.as_deref(), Some("saved"));
            assert!(view.status.contains("partial"));
            assert!(!view.restored_review_history_incomplete);
            assert!(view.restored_activity_history_incomplete);
            let rows = crate::desktop::inspector::activity_rows(&view.transcript, &view.restored_activity);
            assert_eq!(rows.rows.len(), 2, "archive text is retained without duplicating structured activity");
            assert_eq!(
                view.transcript.items().filter(|item| item.role == "tool").count(),
                1,
                "only the original archive tool card remains in the conversation"
            );
            assert!(!view.transcript.get(0).unwrap().expanded);
            assert!(!view.transcript.get(0).unwrap().has_activity_status);
            assert_eq!(rows.rows[0].state, ActivityState::Completed);
            assert_eq!(rows.rows[1].status, "interrupted");
            assert_eq!(rows.rows[1].state, ActivityState::Interrupted);
            assert!(rows.rows[0].has_output);
            assert!(rows.rows[1].has_output);
            assert!(view.transcript.items().any(|item| item.text == "Unstructured archive result"));
            let restored_output =
                crate::desktop::inspector::output_for(&view.transcript, &view.restored_activity, None).unwrap();
            assert_eq!(restored_output.text, output);
            assert!(!restored_output.running);
            let restored_command = view
                .restored_activity
                .iter()
                .find(|item| item.id == "saved-command")
                .expect("historical command activity");
            assert!(!restored_command.text.contains(output), "output is not duplicated in activity metadata");
            assert_eq!(restored_command.output.as_deref(), Some(output), "full output remains recoverable");
            view.inspector_open = true;
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Activity;
        });
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some("Saved tool history is partial. Some output may be missing or shortened."));
        assert_eq!(window.find("activity-saved-command").role(), Some(gpui_kit::Role::ListItem));
        assert_eq!(window.find("activity-saved-command").label(), Some("cargo check --locked"));
        let output_action = window.find("activity-output-finished-command");
        assert_eq!(output_action.role(), Some(gpui_kit::Role::Button));
        assert_eq!(output_action.label(), Some("View output for previous command"));
        window.click("activity-output-finished-command", cx);
        window.render_frame(cx);
        let warning = window.find("activity-history-warning");
        assert_eq!(warning.role(), Some(gpui_kit::Role::Alert));
        assert_eq!(warning.label(), Some("Saved tool history is partial. Some output may be missing or shortened."));
        assert_eq!(window.find("output-body").label(), Some("previous output"));
        assert_eq!(view.read(cx).selected_output_id.as_deref(), Some("finished-command"));
        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("previous output"));
        assert_eq!(window.find("output-follow-latest").label(), Some("Follow latest output"));
        window.click("output-follow-latest", cx);
        window.render_frame(cx);
        assert!(view.read(cx).selected_output_id.is_none());
        assert_eq!(window.find("output-body").label(), Some(output));
        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(output));
        // A new live item still reports running, independent of historical display.
        view.update(cx, |view, _| {
            view.update_runtime_item(&ThreadItem {
                id: "saved-command".into(),
                context: None,
                details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                    command: "live".into(),
                    arguments: None,
                    aggregated_output: "live output".into(),
                    exit_code: None,
                    status: CommandExecutionStatus::InProgress,
                })),
            });
            assert!(
                crate::desktop::inspector::output_for(&view.transcript, &view.restored_activity, None)
                    .unwrap()
                    .running
            );
            assert_eq!(view.restored_activity.len(), 1, "live identity replaces its historical projection");
            assert_eq!(
                crate::desktop::inspector::activity_rows(&view.transcript, &view.restored_activity)
                    .rows
                    .len(),
                2
            );
            view.reset_session_state();
            assert!(view.restored_activity.is_empty());
            assert!(view.selected_output_id.is_none());
            assert!(!view.restored_review_history_incomplete);
            assert!(!view.restored_activity_history_incomplete);
            assert!(crate::desktop::inspector::output_for(&view.transcript, &view.restored_activity, None).is_none());
        });
    })
    .unwrap();
}

#[gpui_kit::test]
fn output_tab_bounds_rendered_large_output_but_copies_the_full_capture(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let output = format!("command output\n{}", "界".repeat(90_000));
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item =
                TranscriptItem::new("large-output".into(), "tool".into(), "Large command".into(), "completed".into());
            item.output = Some(output.clone().into());
            item.expanded = true;
            view.transcript.push(item);
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        });
        window.render_frame(cx);

        let output_body = window.find("output-body");
        let rendered = output_body.label().unwrap();
        assert!(rendered.starts_with("… Earlier output truncated"));
        assert!(rendered.len() <= 135_000, "the accessible output excerpt is byte-bounded");
        assert!(rendered.ends_with("界"));
        let tool_body = window.find("tool-output-large-output");
        let tool_excerpt = tool_body.label().unwrap();
        assert!(tool_excerpt.starts_with("… Earlier output truncated"));
        assert!(tool_excerpt.len() <= 135_000, "expanded tool details use the same byte bound");

        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(output.as_str()));
    })
    .unwrap();
}

#[test]
fn restored_output_page_validation_requires_exact_utf8_offsets() {
    let pending = super::super::PendingOutputPage {
        request_id: "request-1".into(),
        session_id: "session-1".into(),
        item_id: "output-1".into(),
        offset: 32_768,
    };
    let cursor = |total_bytes| vtcode_desktop_protocol::SessionOutputCursor {
        item_id: pending.item_id.clone(),
        next_offset: pending.offset,
        total_bytes,
    };
    let page = |next_offset, text: &str, has_more, total_bytes| vtcode_desktop_protocol::SessionOutputPage {
        session_id: pending.session_id.clone(),
        request_id: pending.request_id.clone(),
        item_id: pending.item_id.clone(),
        offset: pending.offset,
        next_offset,
        total_bytes,
        text: text.into(),
        has_more,
    };

    assert!(super::super::valid_session_output_page(
        &page(pending.offset + 3, "界", true, pending.offset + 12),
        &pending,
        &cursor(pending.offset + 12)
    ));
    assert!(super::super::valid_session_output_page(
        &page(pending.offset + 4, "four", false, pending.offset + 4),
        &pending,
        &cursor(pending.offset + 4)
    ));
    assert!(!super::super::valid_session_output_page(
        &page(pending.offset + 5, "four", false, pending.offset + 5),
        &pending,
        &cursor(pending.offset + 5)
    ));
}

#[gpui_kit::test]
fn restored_output_loads_a_bounded_page_and_removes_the_truncation_marker(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, mut requests) = tokio::sync::mpsc::channel(4);
    let prefix = "old page";
    let middle = " tail界";
    let final_page = " complete";
    let suffix = vtcode_desktop_protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX;
    let middle_end = u64::try_from(prefix.len() + middle.len()).expect("test page length fits in u64");
    let total_bytes = middle_end + u64::try_from(final_page.len()).expect("test page length fits in u64");
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.backend = Some(BackendHandle { commands });
            view.selected_session_id = Some("archive-session".into());
            view.selected_output_id = Some("historical-output".into());
            view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
            let mut item = TranscriptItem::new(
                "historical-output".into(),
                "tool".into(),
                "Archived command".into(),
                "completed".into(),
            );
            item.output = Some(format!("{prefix}{suffix}").into());
            view.push_restored_activity(item);
            let _previous = view.restored_output_cursors.insert(
                "historical-output".into(),
                vtcode_desktop_protocol::SessionOutputCursor {
                    item_id: "historical-output".into(),
                    next_offset: prefix.len() as u64,
                    total_bytes,
                },
            );
            assert!(view.restored_output_cursors.contains_key("historical-output"));
            let output = crate::desktop::inspector::output_for(
                &view.transcript,
                &view.restored_activity,
                view.selected_output_id.as_deref(),
            )
            .unwrap();
            assert_eq!(output.id, "historical-output");
            cx.notify();
        });
        window.render_frame(cx);
        assert!(view.read(cx).restored_output_cursors.contains_key("historical-output"));
        assert_eq!(view.read(cx).selected_output_id.as_deref(), Some("historical-output"));
        assert!(view.read(cx).output_page_pending.is_none());
        assert!(view.read(cx).output_page_error.is_none());
        assert_eq!(window.find("copy-output").role(), Some(gpui_kit::Role::Button));
        assert_eq!(window.find("copy-output").label(), Some("Copy loaded output"));
        assert_eq!(window.find("load-more-output").role(), Some(gpui_kit::Role::Button));
        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(format!("{prefix}{suffix}").as_str()));
        window.click("load-more-output", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("captured output page request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_OUTPUT_READ);
    assert_eq!(params["sessionId"], "archive-session");
    assert_eq!(params["itemId"], "historical-output");
    assert_eq!(params["offset"], prefix.len());
    let request_id = params["requestId"].as_str().unwrap().to_string();
    let page = vtcode_desktop_protocol::SessionOutputPage {
        session_id: "archive-session".into(),
        request_id: request_id.clone(),
        item_id: "historical-output".into(),
        offset: prefix.len() as u64,
        next_offset: middle_end,
        total_bytes,
        text: middle.into(),
        has_more: true,
    };
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::SessionOutputResponse { request_id, result: Some(json!(page)), error: None },
            cx,
        );
    });
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        view.update(cx, |view, _| {
            assert_eq!(view.restored_activity[0].output.as_deref(), Some(format!("{prefix}{middle}{suffix}").as_str()));
            assert_eq!(view.restored_output_cursors["historical-output"].next_offset, middle_end);
            assert!(view.output_page_pending.is_none());
            assert!(view.output_page_error.is_none());
        });
        window.click("copy-output", cx);
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().as_deref(),
            Some(format!("{prefix}{middle}{suffix}").as_str())
        );
        window.click("load-more-output", cx);
    })
    .unwrap();
    cx.run_until_parked();

    let BackendCommand::Request { method, params } = requests.try_recv().unwrap() else {
        panic!("second captured output page request")
    };
    assert_eq!(method, vtcode_desktop_protocol::method::SESSION_OUTPUT_READ);
    assert_eq!(params["sessionId"], "archive-session");
    assert_eq!(params["itemId"], "historical-output");
    assert_eq!(params["offset"], middle_end);
    let request_id = params["requestId"].as_str().unwrap().to_string();
    let page = vtcode_desktop_protocol::SessionOutputPage {
        session_id: "archive-session".into(),
        request_id: request_id.clone(),
        item_id: "historical-output".into(),
        offset: middle_end,
        next_offset: total_bytes,
        total_bytes,
        text: final_page.into(),
        has_more: false,
    };
    view.update(cx, |view, cx| {
        view.handle_backend_event(
            BackendEvent::SessionOutputResponse { request_id, result: Some(json!(page)), error: None },
            cx,
        );
    });
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("output-body").label(), Some("old page tail界 complete"));
        assert!(window.try_find("load-more-output").is_none());
        view.update(cx, |view, _| {
            assert_eq!(view.restored_activity[0].output.as_deref(), Some("old page tail界 complete"));
            assert!(!view.restored_output_cursors.contains_key("historical-output"));
            assert!(view.output_page_pending.is_none());
            assert!(view.output_page_error.is_none());
        });
        window.click("copy-output", cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("old page tail界 complete"));
    })
    .unwrap();
}

#[test]
fn copying_restored_output_preserves_partial_notice_and_literal_suffix() {
    let suffix = vtcode_desktop_protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX;
    let preview: gpui_kit::SharedString = format!("first{suffix}").into();
    let mut pages = std::collections::HashMap::new();
    assert_eq!(restored_output_for_copy("item", &preview, &pages, true), format!("first{suffix}"));
    assert_eq!(restored_output_for_copy("literal", &preview, &pages, false), format!("first{suffix}"));
    let _previous = pages.insert("item".into(), vec!["first".into(), " second".into()]);
    assert_eq!(restored_output_for_copy("item", &preview, &pages, true), format!("first second{suffix}"));
    assert_eq!(restored_output_for_copy("item", &preview, &pages, false), "first second");
}

#[test]
fn restored_output_pages_keep_a_bounded_preview_and_copy_complete_text() {
    let page_len = vtcode_desktop_protocol::MAX_SESSION_OUTPUT_PAGE_BYTES;
    let prefix = "p".repeat(page_len);
    let truncation_suffix = vtcode_desktop_protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX;
    let mut item = TranscriptItem::new("output-1".into(), "tool".into(), "Command".into(), "completed".into());
    item.output = Some(format!("{prefix}{truncation_suffix}").into());
    let mut pages_by_item = std::collections::HashMap::new();
    let mut complete_output = prefix;

    for (index, has_more) in [(0, true), (1, true), (2, true), (3, true), (4, false)] {
        let marker = char::from(b'a' + u8::try_from(index).expect("test index fits in u8"));
        let text = marker.to_string().repeat(page_len);
        complete_output.push_str(&text);
        let mut page = vtcode_desktop_protocol::SessionOutputPage {
            session_id: "session-1".into(),
            request_id: format!("request-{index}"),
            item_id: "output-1".into(),
            offset: 0,
            next_offset: u64::try_from(text.len()).expect("page length fits in u64"),
            total_bytes: u64::try_from(text.len()).expect("page length fits in u64"),
            text,
            has_more,
        };
        assert!(append_restored_output_page(&mut item, &mut pages_by_item, &mut page));
    }

    let preview = item.output.as_ref().expect("restored output remains available");
    assert!(preview.len() < 128 * 1024 + 1024);
    assert!(preview.contains("Earlier output truncated"));
    assert!(preview.ends_with(&"e".repeat(page_len)));
    let copied = restored_output_for_copy("output-1", preview, &pages_by_item, false);
    assert_eq!(copied, complete_output);
    assert!(!copied.ends_with(truncation_suffix));
}

#[gpui_kit::test]
fn stale_output_page_error_does_not_clear_the_current_request(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        let pending = super::super::PendingOutputPage {
            request_id: "output-current".into(),
            session_id: "archive-session".into(),
            item_id: "historical-output".into(),
            offset: 32_768,
        };
        view.output_page_pending = Some(pending.clone());
        view.handle_backend_event(
            BackendEvent::SessionOutputResponse {
                request_id: "output-stale".into(),
                result: None,
                error: Some("old page failed".into()),
            },
            cx,
        );
        assert_eq!(view.output_page_pending.as_ref(), Some(&pending));
        assert!(view.output_page_error.is_none());
    });
}

#[gpui_kit::test]
fn failed_historical_output_page_keeps_the_incomplete_marker_and_retry_control(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let suffix = vtcode_desktop_protocol::RESTORED_OUTPUT_TRUNCATION_SUFFIX;
    view.update(cx, |view, cx| {
        view.selected_session_id = Some("archive-session".into());
        view.selected_output_id = Some("historical-output".into());
        view.inspector_open = true;
        view.inspector_tab = crate::desktop::inspector::InspectorTab::Output;
        let mut item = TranscriptItem::new(
            "historical-output".into(),
            "tool".into(),
            "Archived command".into(),
            "completed".into(),
        );
        item.output = Some(format!("valid prefix{suffix}").into());
        view.push_restored_activity(item);
        let _previous = view.restored_output_cursors.insert(
            "historical-output".into(),
            vtcode_desktop_protocol::SessionOutputCursor {
                item_id: "historical-output".into(),
                next_offset: 12,
                total_bytes: 20,
            },
        );
        view.output_page_pending = Some(super::super::PendingOutputPage {
            request_id: "output-page-1".into(),
            session_id: "archive-session".into(),
            item_id: "historical-output".into(),
            offset: 12,
        });
        view.handle_backend_event(
            BackendEvent::SessionOutputResponse {
                request_id: "output-page-1".into(),
                result: None,
                error: Some("captured output ends with an incomplete UTF-8 sequence".into()),
            },
            cx,
        );
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find("output-body").label().unwrap().ends_with(suffix));
        assert!(window.try_find("output-page-error").is_some());
        assert!(window.try_find("load-more-output").is_some());
        let state = view.read(cx);
        assert!(state.restored_output_cursors.contains_key("historical-output"));
        assert!(state.output_page_pending.is_none());
        assert!(state.output_page_error.is_some());
    })
    .unwrap();
}
