use super::*;

use crate::desktop::inspector::InspectorTab;

#[gpui_kit::test]
fn activity_group_cta_opens_activity_without_toggling_group(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // Asymmetric group: the head carries output, the tail fails
            // without any, so output assertions cannot pass on either row.
            view.transcript.push(TranscriptItem::new(
                "intro".into(),
                "assistant".into(),
                "VT Code".into(),
                "Inspecting the build.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "read".into(),
                "tool".into(),
                "Read Cargo.toml".into(),
                "completed\nmanifest".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "build".into(),
                "tool".into(),
                "cargo check".into(),
                "failed (exit 1)\nstructured details".into(),
            ));
            view.transcript.set_output("read", Some("read-output-marker".into()));
            view.inspector_open = false;
        });
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").label(), Some("Expand 2 tool actions · 1 failed"));
        // One entry point per group: the per-tab triplets were collapsed, so
        // the removed deep links must stay absent while Activity keeps its id.
        let cta = window.find("activity-group-open-activity-read");
        assert_eq!(cta.role(), Some(gpui_kit::Role::Button));
        assert_eq!(cta.label(), Some("Activity"));
        assert!(window.try_find("activity-group-open-output-read").is_none());
        assert!(window.try_find("activity-group-open-utilities-read").is_none());

        window.hover("transcript-row-read", cx);
        window.render_frame(cx);
        window.click("activity-group-open-activity-read", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open, "CTA opens a closed inspector");
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Activity);
        assert!(window.try_find("activity-list").is_some());
        assert_eq!(window.find("activity-group-read").checked(), Some(false), "CTA must not toggle the group");

        // The dropped tabs stay one keyboard switch away through the real tab
        // strip: Activity -> Output -> Utilities.
        view.update(cx, |view, cx| view.inspector_focus.focus(window, cx));
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Output);
        assert!(window.try_find("inspector-output-tab").is_some());
        assert!(
            window.find("output-body").label().unwrap().contains("read-output-marker"),
            "tab switch lands on the newest output"
        );
        window.press("right", cx);
        window.render_frame(cx);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Utilities);
        assert!(window.try_find("utilities-pane").is_some());

        // The toggle keeps its own behavior beside the CTA.
        window.click("activity-group-read", cx);
        window.render_frame(cx);
        assert_eq!(window.find("activity-group-read").checked(), Some(true));
        assert!(window.try_find("activity-group-open-activity-read").is_some(), "CTA persists while expanded");
        assert!(view.read(cx).transcript.get(1).unwrap().activity_expanded);
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_group_cta_exits_zen_to_reveal_inspector(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // Same asymmetric group as the normal-mode test: the head
            // carries output, the tail fails without any.
            view.transcript.push(TranscriptItem::new(
                "intro".into(),
                "assistant".into(),
                "VT Code".into(),
                "Inspecting the build.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "read".into(),
                "tool".into(),
                "Read Cargo.toml".into(),
                "completed\nmanifest".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "build".into(),
                "tool".into(),
                "cargo check".into(),
                "failed (exit 1)\nstructured details".into(),
            ));
            view.transcript.set_output("read", Some("read-output-marker".into()));
            view.inspector_open = false;
            view.zen_mode = true;
        });
        window.render_frame(cx);
        assert!(window.try_find("utilities-pane").is_none(), "zen hides the inspector");
        assert!(window.try_find("activity-group-open-activity-read").is_some(), "CTA stays reachable in zen");
        assert!(window.try_find("activity-group-open-output-read").is_none());
        assert!(window.try_find("activity-group-open-utilities-read").is_none());

        window.hover("transcript-row-read", cx);
        window.render_frame(cx);
        window.click("activity-group-open-activity-read", cx);
        window.render_frame(cx);
        assert!(!view.read(cx).zen_mode, "explicit reveal exits zen");
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Activity);
        assert!(window.try_find("activity-list").is_some());
        assert_eq!(window.find("activity-group-read").checked(), Some(false), "CTA must not toggle the group");

        // Asymmetric partner: inline expansion from a fresh zen state keeps
        // zen, since it reveals no hidden panel.
        view.update(cx, |view, _| {
            view.zen_mode = true;
            view.inspector_open = false;
        });
        window.render_frame(cx);
        window.click("activity-group-read", cx);
        window.render_frame(cx);
        assert!(view.read(cx).zen_mode, "inline toggle keeps zen");
        assert!(!view.read(cx).inspector_open);
        assert_eq!(window.find("activity-group-read").checked(), Some(true));
    })
    .unwrap();
}

#[gpui_kit::test]
fn tool_output_cta_selects_that_tool_output_and_hides_without_output(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // Commentary breaks grouping so each tool row stands alone with
            // distinct titles and outputs; the third row has no output.
            view.transcript.push(TranscriptItem::new(
                "first".into(),
                "tool".into(),
                "First command".into(),
                "completed".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "gap-one".into(),
                "assistant".into(),
                "VT Code".into(),
                "First done.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "second".into(),
                "tool".into(),
                "Second command".into(),
                "completed".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "gap-two".into(),
                "assistant".into(),
                "VT Code".into(),
                "Second done.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "third".into(),
                "tool".into(),
                "Third command".into(),
                "completed".into(),
            ));
            view.transcript.set_output("first", Some("first-output-marker".into()));
            view.transcript.set_output("second", Some("second-output-marker".into()));
            view.inspector_open = false;
        });
        window.render_frame(cx);

        window.click("tool-second", cx);
        window.render_frame(cx);
        // The output and copy actions remain visible even after hover and
        // keyboard highlighting leave the row.
        view.update(cx, |view, _| view.keyboard_focused_message = None);
        window.hover("transcript-scroll", cx);
        window.render_frame(cx);
        assert!(window.try_find("tool-open-output-second").is_some(), "jump keeps its space");
        assert!(window.find("tool-open-output-second").visible(), "full output remains available at rest");
        assert!(window.find("copy-tool-output-second").visible(), "copy stays visible");
        let row = window.find("transcript-row-second").bounds();
        window.hover("transcript-row-second", cx);
        window.render_frame(cx);
        assert!(window.find("tool-open-output-second").visible(), "hover reveals the jump");
        assert_eq!(window.find("transcript-row-second").bounds(), row, "reveal must not reflow the row");
        let cta = window.find("tool-open-output-second");
        assert_eq!(cta.role(), Some(gpui_kit::Role::Button));
        assert_eq!(cta.label(), Some("View output: Second command"));
        window.click("tool-open-output-second", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Output);
        assert_eq!(view.read(cx).selected_output_id.as_deref(), Some("second"));
        let output = window.find("output-body");
        let body = output.label().unwrap();
        assert!(body.contains("second-output-marker"), "selected output renders: {body}");
        assert!(!body.contains("first-output-marker"), "sibling output must not leak in: {body}");

        // Rows without captured output offer no jump.
        window.click("tool-third", cx);
        window.render_frame(cx);
        assert!(window.try_find("tool-output-third").is_some(), "third row expands");
        assert!(window.try_find("tool-open-output-third").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn diff_card_cta_selects_first_previewable_file_or_just_opens_changes(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // Asymmetric card: the first file has no preview, so the CTA must
            // skip it for the previewable second file rather than taking [0].
            let mut edits = TranscriptItem::new("edits".into(), "diff".into(), "Edits".into(), String::new());
            edits.diff = Some(super::super::DiffCard {
                files: vec![
                    super::super::DiffFile {
                        path: "ghost.rs".into(),
                        kind: super::super::DiffFileKind::Update,
                    },
                    super::super::DiffFile {
                        path: "src/app.rs".into(),
                        kind: super::super::DiffFileKind::Add,
                    },
                ],
                additions: Some(1),
                deletions: Some(1),
                incomplete: false,
                failed: false,
                patch: "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old\n+new\n".into(),
            });
            view.transcript.push(edits);
            // Card with structured paths but no parseable preview at all.
            let mut pending = TranscriptItem::new("pending".into(), "diff".into(), "Pending".into(), String::new());
            pending.diff = Some(super::super::DiffCard {
                files: vec![super::super::DiffFile {
                    path: "src/app.rs".into(),
                    kind: super::super::DiffFileKind::Update,
                }],
                additions: None,
                deletions: None,
                incomplete: true,
                failed: false,
                patch: String::new(),
            });
            view.transcript.push(pending);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
            view.inspector_open = false;
        });
        window.render_frame(cx);
        assert_eq!(window.find("diff-open-changes-edits").role(), Some(gpui_kit::Role::Button));
        assert_eq!(window.find("diff-open-changes-edits").label(), Some("Changes"));

        window.click("diff-open-changes-edits", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Changes);
        assert_eq!(view.read(cx).selected_change.as_deref(), Some("src/app.rs"));
        assert!(window.try_find("inspector-changes-tab").is_some());

        // No preview anywhere: the CTA still opens the tab instead of no-op.
        view.update(cx, |view, _| {
            view.selected_change = None;
            view.inspector_open = false;
        });
        window.render_frame(cx);
        window.click("diff-open-changes-pending", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Changes);
        assert_eq!(view.read(cx).selected_change, None);
    })
    .unwrap();
}

#[gpui_kit::test]
fn selecting_change_item_uses_its_patch_for_same_path(cx: &mut TestAppContext) {
    let (_window, view) = open_desktop(cx);
    view.update(cx, |view, cx| {
        for (id, added) in [("old", "+one\n"), ("new", "+one\n+two\n")] {
            let mut row = TranscriptItem::new(id.into(), "diff".into(), "Edits".into(), String::new());
            row.diff = Some(super::super::DiffCard {
                files: vec![super::super::DiffFile {
                    path: "src/app.rs".into(),
                    kind: super::super::DiffFileKind::Update,
                }],
                additions: None,
                deletions: None,
                incomplete: false,
                failed: false,
                patch: format!(
                    "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -0,0 +1,{} @@\n{added}",
                    if id == "old" { 1 } else { 2 }
                ),
            });
            view.transcript.push(row);
        }
        view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
        assert!(view.select_change_from_item("old", "src/app.rs", cx));
        assert_eq!(view.change_viewer.as_ref().unwrap().read(cx).files()[0].additions(), 1);
        let newer = vtcode_exec_events::FileChangeItem {
            changes: vec![vtcode_exec_events::FileUpdateChange {
                path: "src/app.rs".into(),
                kind: vtcode_exec_events::PatchChangeKind::Update,
                terminators_normalized: None,
            }],
            status: vtcode_exec_events::PatchApplyStatus::Completed,
            unified_diff: Some("--- a/src/app.rs\n+++ b/src/app.rs\n@@ -0,0 +1,3 @@\n+one\n+two\n+three\n".into()),
            additions: Some(3),
            deletions: Some(0),
            diff_incomplete: None,
        };
        view.upsert_diff_card("new".into(), &newer);
        view.refresh_selected_change(cx);
        assert_eq!(view.change_viewer.as_ref().unwrap().read(cx).files()[0].additions(), 1);
        assert!(view.select_change_from_item("new", "src/app.rs", cx));
        assert_eq!(view.change_viewer.as_ref().unwrap().read(cx).files()[0].additions(), 3);
        assert!(!view.select_change_from_item("old", "missing.rs", cx));
    });
}

#[gpui_kit::test]
fn activity_group_header_summarizes_tool_verbs(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ThreadItem, ThreadItemDetails, ToolCallStatus, ToolInvocationItem};

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let invocation = |id: &str, tool_name: &str, status: ToolCallStatus| ThreadItem {
            context: None,
            id: id.into(),
            details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: tool_name.into(),
                arguments: None,
                tool_call_id: None,
                status,
                outcome: None,
            })),
        };
        view.update(cx, |view, _| {
            view.update_runtime_item(&invocation("read-1", "read_file", ToolCallStatus::Completed));
            view.update_runtime_item(&invocation("exec-1", "exec_command", ToolCallStatus::Completed));
            view.update_runtime_item(&invocation("track-1", "task_tracker", ToolCallStatus::Failed));
        });
        window.render_frame(cx);
        assert_eq!(
            window.find("activity-group-read-1").label(),
            Some("Expand Read 1 file · Ran 1 command · Updated 1 task list · 1 failed")
        );
        // The single inspector entry keeps working beside the verb phrase.
        window.hover("transcript-row-read-1", cx);
        window.render_frame(cx);
        window.click("activity-group-open-activity-read-1", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Activity);
    })
    .unwrap();
}

#[gpui_kit::test]
fn expanded_tool_rows_show_argument_gists_not_registry_names(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ThreadItem, ThreadItemDetails, ToolCallStatus, ToolInvocationItem};

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let invocation = |id: &str, tool_name: &str, arguments: serde_json::Value| ThreadItem {
            context: None,
            id: id.into(),
            details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: tool_name.into(),
                arguments: Some(arguments),
                tool_call_id: None,
                status: ToolCallStatus::Completed,
                outcome: None,
            })),
        };
        view.update(cx, |view, _| {
            view.update_runtime_item(&invocation("read-1", "read_file", serde_json::json!({"path": "src/Cargo.toml"})));
            view.update_runtime_item(&invocation("exec-1", "exec_command", serde_json::json!({"cmd": "cargo check"})));
        });
        window.render_frame(cx);
        window.click("activity-group-read-1", cx);
        window.render_frame(cx);
        assert_eq!(window.find("tool-read-1").label(), Some("Show details for Read Cargo.toml, status: completed"));
        assert_eq!(window.find("tool-exec-1").label(), Some("Show details for Ran cargo, status: completed"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn expanded_tool_rows_show_summaries_not_json_dumps(cx: &mut TestAppContext) {
    use vtcode_exec_events::{ThreadItem, ThreadItemDetails, ToolCallStatus, ToolInvocationItem};

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        let item = ThreadItem {
            context: None,
            id: "read-1".into(),
            details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "read_file".into(),
                arguments: Some(serde_json::json!({"path": "src/Cargo.toml", "limit": 50})),
                tool_call_id: None,
                status: ToolCallStatus::Completed,
                outcome: None,
            })),
        };
        view.update(cx, |view, _| {
            view.update_runtime_item(&item);
        });
        window.render_frame(cx);
        window.click("tool-read-1", cx);
        window.render_frame(cx);
        assert_eq!(window.find("tool-read-1").label(), Some("Hide details for Read Cargo.toml, status: completed"));
        let tool_body = window.find("tool-output-read-1");
        let body = tool_body.label().unwrap();
        assert!(body.contains("Limit: 50"), "relevant parameter survives: {body}");
        for marker in ["\"tool_name\"", "\"arguments\"", "{\n"] {
            assert!(!body.contains(marker), "no JSON dump marker {marker:?} in {body:?}");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn tool_titles_split_verb_from_command_text(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // An assistant message between the rows keeps both ungrouped so
            // each renders its own callout directly.
            view.transcript.push(TranscriptItem::new(
                "exec-1".into(),
                "tool".into(),
                "Ran cargo check --locked".into(),
                "completed\noutput".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "note".into(),
                "assistant".into(),
                "VT Code".into(),
                "Between the tools.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "solo-1".into(),
                "tool".into(),
                "Harness".into(),
                "completed".into(),
            ));
        });
        window.render_frame(cx);

        // The verb leads in its own span; the command follows in mono.
        let verb = window.find("tool-title-verb-exec-1");
        let rest = window.find("tool-title-rest-exec-1");
        assert!(verb.visible(), "verb span renders");
        assert!(rest.visible(), "command span renders");
        assert!(
            rest.bounds().origin.x > verb.bounds().origin.x,
            "command text trails the verb: verb={:?} rest={:?}",
            verb.bounds(),
            rest.bounds()
        );
        let row = window.find("tool-title-exec-1").bounds();
        assert!(
            rest.bounds().right() <= row.right() + px(1.),
            "command text stays inside the callout: rest={:?} row={row:?}",
            rest.bounds()
        );
        // Single-word titles keep the flat shape: the title container
        // renders with no verb/remainder spans.
        assert!(window.find("tool-title-solo-1").visible());
        for id in ["tool-title-verb-solo-1", "tool-title-rest-solo-1"] {
            assert!(window.try_find(id).is_none(), "single-word titles render no split spans ({id})");
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_group_cta_recedes_until_hover_or_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "read".into(),
                "tool".into(),
                "Read Cargo.toml".into(),
                "completed".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "exec".into(),
                "tool".into(),
                "cargo check".into(),
                "completed".into(),
            ));
        });
        window.render_frame(cx);
        // Present but hidden at rest; the toggle stays visible.
        assert!(window.try_find("activity-group-open-activity-read").is_some(), "receded CTA keeps its space");
        assert!(!window.find("activity-group-open-activity-read").visible(), "CTA hides at rest");
        assert!(window.find("activity-group-read").visible(), "toggle stays visible");
        let row = window.find("transcript-row-read").bounds();
        // Hover reveals without reflow.
        window.hover("transcript-row-read", cx);
        window.render_frame(cx);
        assert!(window.find("activity-group-open-activity-read").visible(), "hover reveals the CTA");
        assert_eq!(window.find("transcript-row-read").bounds(), row, "reveal must not reflow the row");
        assert_eq!(view.read(cx).hovered_message.as_deref(), Some("read"));
        // Leaving hides again.
        window.hover("transcript-scroll", cx);
        window.render_frame(cx);
        assert!(!window.find("activity-group-open-activity-read").visible(), "leaving hides the CTA");
        assert!(view.read(cx).hovered_message.is_none());
        // Keyboard highlight reveals without the mouse.
        view.update(cx, |view, _| view.keyboard_focused_message = Some("read".into()));
        window.render_frame(cx);
        assert!(window.find("activity-group-open-activity-read").visible(), "focus reveals the CTA");
    })
    .unwrap();
}

#[gpui_kit::test]
fn activity_group_leading_slot_prefers_spinner_over_cta_while_running(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.transcript.push(TranscriptItem::new(
                "read".into(),
                "tool".into(),
                "Read Cargo.toml".into(),
                "in progress\nreading".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "exec".into(),
                "tool".into(),
                "cargo check".into(),
                "completed".into(),
            ));
        });
        window.render_frame(cx);
        // Running groups show status, not the shortcut: one slot, one occupant.
        assert!(window.try_find("activity-group-running-indicator-read").is_some());
        assert!(window.try_find("activity-group-open-activity-read").is_none());
        // Settling to idle restores the shortcut and drops the spinner.
        view.update(cx, |view, _| {
            view.transcript.upsert(TranscriptItem::new(
                "read".into(),
                "tool".into(),
                "Read Cargo.toml".into(),
                "completed\nmanifest".into(),
            ));
        });
        window.render_frame(cx);
        assert!(window.try_find("activity-group-running-indicator-read").is_none());
        assert!(window.try_find("activity-group-open-activity-read").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn transcript_header_rows_share_one_trailing_edge(cx: &mut TestAppContext) {
    use crate::desktop::transcript::NoticeSeverity;

    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            // One row per header idiom: severity notice, tool group, single tool.
            view.transcript.push(TranscriptItem::diagnostic_notice(
                "notice".into(),
                "Tool 'apply_patch' failed.".into(),
                "context mismatch".into(),
                NoticeSeverity::Warning,
            ));
            view.transcript.push(TranscriptItem::new(
                "read".into(),
                "tool".into(),
                "Read Cargo.toml".into(),
                "completed\nmanifest".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "build".into(),
                "tool".into(),
                "cargo check".into(),
                "completed".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "comment".into(),
                "assistant".into(),
                "VT Code".into(),
                "Done.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "retry".into(),
                "tool".into(),
                "cargo check".into(),
                "completed".into(),
            ));
        });
        window.render_frame(cx);
        // Every header spans the full content width, so all disclosure
        // chevrons dock on one trailing edge (a trailing CTA sibling would
        // pull the group edge inward).
        let notice = window.find("notice-header-notice").bounds();
        let group = window.find("activity-group-read").bounds();
        let single = window.find("tool-retry").bounds();
        // The notice is now a flat row with no card edge, so its header docks
        // on the same trailing edge as the disclosure rows.
        assert!(
            (notice.right() - group.right()).abs() <= px(1.),
            "notice and group share the trailing edge: notice={notice:?} group={group:?}"
        );
        assert_eq!(single.right(), group.right(), "single tool and group share the trailing edge");
    })
    .unwrap();
}

#[gpui_kit::test]
fn diff_changes_cta_stays_visible_without_hover_or_focus(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut edits = TranscriptItem::new("edits".into(), "diff".into(), "Edits".into(), String::new());
            edits.diff = Some(super::super::DiffCard {
                files: vec![super::super::DiffFile {
                    path: "src/app.rs".into(),
                    kind: super::super::DiffFileKind::Update,
                }],
                additions: Some(1),
                deletions: Some(1),
                incomplete: false,
                failed: false,
                patch: "@@ -1 +1 @@\n-old\n+new\n".into(),
            });
            view.transcript.push(edits);
        });
        window.render_frame(cx);
        assert!(window.find("diff-open-changes-edits").visible(), "jump stays visible at rest");
        assert!(window.find("diff-edits").visible(), "toggle stays visible");
        let row = window.find("transcript-row-edits").bounds();
        window.hover("transcript-row-edits", cx);
        window.render_frame(cx);
        assert!(window.find("diff-open-changes-edits").visible(), "hover keeps the jump visible");
        assert_eq!(window.find("transcript-row-edits").bounds(), row, "hover must not reflow the row");
        window.hover("transcript-scroll", cx);
        window.render_frame(cx);
        assert!(window.find("diff-open-changes-edits").visible(), "leaving the row keeps the jump visible");
    })
    .unwrap();
}

#[gpui_kit::test]
fn running_command_row_shows_live_tail_and_terminal_cta(cx: &mut TestAppContext) {
    use crate::desktop::transcript::ToolCategory;

    let (window, view) = open_desktop(cx);
    let dir = tempfile::tempdir().unwrap();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.workspace = Some(dir.path().to_path_buf());
            // Asymmetric pair: the running command streams 12 lines (only
            // the last 8 may show inline), while the settled sibling with
            // output shows no inline excerpt at all.
            let live_lines: Vec<String> = (0..12).map(|n| format!("live-line-{n}")).collect();
            view.transcript.push(TranscriptItem::new(
                "live".into(),
                "tool".into(),
                "Ran cargo".into(),
                "in progress\ncargo check".into(),
            ));
            view.transcript.set_output("live", Some(live_lines.join("\n").into()));
            view.transcript.set_activity_category("live", ToolCategory::Run);
            view.transcript.push(TranscriptItem::new(
                "gap".into(),
                "assistant".into(),
                "VT Code".into(),
                "Working.".into(),
            ));
            view.transcript.push(TranscriptItem::new(
                "settled".into(),
                "tool".into(),
                "Ran cargo".into(),
                "completed".into(),
            ));
            view.transcript.set_output("settled", Some("settled-output-marker".into()));
            view.transcript.set_activity_category("settled", ToolCategory::Run);
        });
        window.render_frame(cx);

        window.click("tool-live", cx);
        window.render_frame(cx);
        let excerpt = window.find("tool-output-live");
        let body = excerpt.label().unwrap();
        assert!(body.contains("live-line-11"), "live tail keeps the newest line: {body}");
        assert!(body.contains("live-line-9"), "live tail keeps 3 lines: {body}");
        assert!(!body.contains("live-line-8\n"), "live tail drops the oldest lines: {body}");
        let cta = window.find("tool-open-terminal-live");
        assert_eq!(cta.role(), Some(gpui_kit::Role::Button));
        window.click("tool-open-terminal-live", cx);
        window.render_frame(cx);
        assert!(view.read(cx).terminal_visible, "CTA opens the built-in terminal");
        assert_eq!(view.read(cx).terminal_monitor_row.as_deref(), Some("live"), "CTA mirrors this row");
        assert!(
            view.read(cx).terminal_monitor.as_ref().is_some_and(|tab| !tab.has_process()),
            "monitor must not spawn a second shell process"
        );
        // No shell process was spawned for the monitor.
        assert!(view.read(cx).terminal.as_ref().is_none_or(|tab| !tab.has_process()));

        // Settled rows with captured output keep no inline excerpt and no
        // terminal CTA: the Output tab owns them.
        window.click("tool-settled", cx);
        window.render_frame(cx);
        assert!(window.try_find("tool-output-settled").is_none());
        assert!(window.try_find("tool-open-terminal-settled").is_none());
        assert!(window.try_find("tool-open-output-settled").is_some());
    })
    .unwrap();
}
