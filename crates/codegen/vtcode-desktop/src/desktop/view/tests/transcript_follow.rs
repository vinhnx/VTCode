//! Tail-following and row interactions: expanding a tool call while the
//! runloop streams must freeze the viewport exactly where it is — no
//! re-anchor to the top visible row, no drift as the tail keeps growing.

use super::*;

const STREAMED_ROWS: usize = 20;

fn projected_command(id: &str, aggregated_output: &str) -> vtcode_exec_events::ThreadItem {
    use vtcode_exec_events::*;
    ThreadItem {
        id: id.into(),
        context: None,
        details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "cargo check".into(),
            arguments: None,
            aggregated_output: aggregated_output.into(),
            exit_code: Some(0),
            status: CommandExecutionStatus::Completed,
        })),
    }
}

/// Seed a transcript whose wrapped rows overflow the viewport, with a
/// toggleable tool row at the tail.
fn seed_streamed_transcript(view: &Entity<DesktopView>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        view.update(cx, |view, _| {
            for index in 0..STREAMED_ROWS {
                view.transcript.push(TranscriptItem::new(
                    format!("follow-message-{index}"),
                    "assistant".to_string(),
                    "VT Code".into(),
                    format!("Message {index}\n\n{}", "wrapped words ".repeat(index + 1)),
                ));
            }
            view.project_runtime_item(&projected_command("follow-tool", &"output line\n".repeat(40)), false);
        });
    });
}

fn assert_anchor_frozen(before: gpui_kit::ListOffset, after: gpui_kit::ListOffset, stage: &str) {
    assert_eq!(after.item_ix, before.item_ix, "{stage}: viewport item moved");
    assert!((before.offset_in_item - after.offset_in_item).abs() <= px(1.), "{stage}: viewport row offset moved");
}

#[gpui_kit::test]
fn toggling_a_tool_while_tail_following_freezes_the_viewport_in_place(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_streamed_transcript(&view, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        assert!(scroller.read(cx).is_following_tail(), "fixture starts at the tail");
        let before = scroller.read(cx).logical_scroll_top();

        // Toggle the tail tool while tail-following the streamed transcript.
        view.update(cx, |view, _| view.transcript.toggle_tool("follow-tool"));
        window.render_frame(cx);

        assert!(!scroller.read(cx).is_following_tail(), "the interaction disengages tail-following");
        assert_anchor_frozen(before, scroller.read(cx).logical_scroll_top(), "toggle");

        // Streaming continues below the fold: the frozen viewport stays put.
        view.update(cx, |view, _| {
            for index in 0..3 {
                view.transcript.push(TranscriptItem::new(
                    format!("follow-growth-{index}"),
                    "assistant".to_string(),
                    "VT Code".into(),
                    format!("Growth {index}"),
                ));
            }
        });
        window.render_frame(cx);
        assert_anchor_frozen(before, scroller.read(cx).logical_scroll_top(), "tail growth");

        // Jump-to-latest returns to the tail and resumes following.
        view.update(cx, |view, _| view.transcript.follow_latest());
        window.render_frame(cx);
        assert!(scroller.read(cx).is_following_tail(), "jump-to-latest resumes the tail");
    })
    .unwrap();
}

#[gpui_kit::test]
fn collapsing_a_diff_while_tail_following_also_freezes_in_place(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    seed_streamed_transcript(&view, cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            let mut item = TranscriptItem::new(
                "follow-diff".into(),
                crate::desktop::transcript::TranscriptRole::Diff.as_str().to_string(),
                "Edits".into(),
                "patch applied".into(),
            );
            item.diff = Some(crate::desktop::transcript::DiffCard {
                files: vec![crate::desktop::transcript::DiffFile {
                    path: "src/lib.rs".into(),
                    kind: crate::desktop::transcript::DiffFileKind::Update,
                }],
                additions: Some(2),
                deletions: None,
                incomplete: false,
                failed: false,
                patch: "patch applied".into(),
            });
            view.transcript.push(item);
        });
        window.render_frame(cx);
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        assert!(scroller.read(cx).is_following_tail(), "fixture starts at the tail");
        let before = scroller.read(cx).logical_scroll_top();

        view.update(cx, |view, _| view.transcript.toggle_diff("follow-diff"));
        window.render_frame(cx);

        assert!(!scroller.read(cx).is_following_tail(), "the interaction disengages tail-following");
        assert_anchor_frozen(before, scroller.read(cx).logical_scroll_top(), "diff toggle");
    })
    .unwrap();
}
