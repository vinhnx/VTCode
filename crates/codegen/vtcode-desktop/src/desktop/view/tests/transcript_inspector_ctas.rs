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
        let cta = window.find("tool-open-output-second");
        assert_eq!(cta.role(), Some(gpui_kit::Role::Button));
        assert_eq!(cta.label(), Some("View output for Second command"));
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
                    path: "missing.rs".into(),
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
        window.click("diff-open-changes-pending", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Changes);
        assert_eq!(view.read(cx).selected_change, None);
    })
    .unwrap();
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
            Some("Expand Read 1 file · Ran 1 command · 1 other action · 1 failed")
        );
        // The single inspector entry keeps working beside the verb phrase.
        window.click("activity-group-open-activity-read-1", cx);
        window.render_frame(cx);
        assert!(view.read(cx).inspector_open);
        assert_eq!(view.read(cx).inspector_tab, InspectorTab::Activity);
    })
    .unwrap();
}
