use super::*;
use vtcode_desktop_protocol::ThreadEventNotification;
use vtcode_exec_events::{
    ItemUpdatedEvent, ThreadEvent, ThreadItem, ThreadItemDetails, ToolCallStatus, ToolOutputItem, VersionedThreadEvent,
};

#[gpui_kit::test]
fn output_burst_preserves_scrolled_view_and_order(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.selected_session_id = Some("session".into());
            for index in 0..1_000 {
                view.transcript.push(TranscriptItem::new(
                    format!("row-{index}"),
                    "assistant".into(),
                    "VT Code".into(),
                    format!("message {index}"),
                ));
            }
            view.transcript
                .push(TranscriptItem::new("output".into(), "tool".into(), "Tool".into(), "running".into()));
            view.transcript.set_output("output", Some("x".repeat(64 * 1024).into()));
        });
        window.render_frame(cx);
        window.render_frame(cx);
        let scroller = view.read(cx).message_scroller.clone();
        scroller.update(cx, |state, cx| {
            assert!(state.scroll_to_item(500, cx));
        });
        window.render_frame(cx);
        let before = scroller.read(cx).logical_scroll_top();
        view.update(cx, |view, cx| {
            for index in 0..MAX_UI_EVENTS_PER_TICK {
                let item = ThreadItem {
                    id: "output".into(),
                    context: None,
                    details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                        call_id: "output".into(),
                        tool_call_id: None,
                        spool_path: None,
                        output: format!("{index:02};"),
                        is_delta: Some(true),
                        exit_code: None,
                        status: ToolCallStatus::InProgress,
                        app_previews: Vec::new(),
                    })),
                };
                view.event_tx
                    .try_send(BackendEvent::ThreadEvent(ThreadEventNotification {
                        session_id: "session".into(),
                        turn_id: None,
                        event: VersionedThreadEvent::new(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item })),
                    }))
                    .unwrap();
            }
            assert!(view.drain_events(window, cx));
            let output = view
                .transcript
                .items()
                .find(|row| row.id == "output")
                .and_then(|row| row.output.as_deref())
                .unwrap();
            let suffix = (0..MAX_UI_EVENTS_PER_TICK).map(|i| format!("{i:02};")).collect::<String>();
            assert!(output.ends_with(&suffix));
        });
        window.render_frame(cx);
        let after = scroller.read(cx).logical_scroll_top();
        assert_eq!(before.item_ix, after.item_ix);
        assert!((before.offset_in_item - after.offset_in_item).abs() <= px(1.));
        scroller.update(cx, |state, cx| assert!(state.scroll_to_item(400, cx)));
        window.render_frame(cx);
        assert_eq!(scroller.read(cx).logical_scroll_top().item_ix, 400);
    })
    .unwrap();
}
