//! Coalescing live-event channel regression tests.

use super::*;

fn tool_output_update(
    id: &str,
    output: &str,
    is_delta: Option<bool>,
    status: vtcode_exec_events::ToolCallStatus,
    exit_code: Option<i32>,
) -> VersionedThreadEvent {
    use vtcode_exec_events::{ItemUpdatedEvent, ThreadItem, ThreadItemDetails, ToolOutputItem};
    VersionedThreadEvent::new(vtcode_exec_events::ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: ThreadItem {
            context: None,
            id: id.to_string(),
            details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: format!("{id}-call"),
                tool_call_id: None,
                spool_path: None,
                output: output.to_string(),
                is_delta,
                exit_code,
                status,
                app_previews: Vec::new(),
            })),
        },
    }))
}

fn tool_output_details(event: &VersionedThreadEvent) -> vtcode_exec_events::ToolOutputItem {
    match event.event() {
        vtcode_exec_events::ThreadEvent::ItemUpdated(update) => match &update.item.details {
            vtcode_exec_events::ThreadItemDetails::ToolOutput(output) => output.as_ref().clone(),
            other => panic!("expected tool output, got {other:?}"),
        },
        other => panic!("expected item update, got {other:?}"),
    }
}

#[tokio::test]
async fn saturated_queue_never_fails_the_producer_and_keeps_lifecycle_events() {
    let (sender, receiver) = live_event_channel(8);
    // Fill the queue past capacity with streaming snapshots and deltas.
    for round in 0..64 {
        sender
            .publish(item_update(&format!("item-{}", round % 4), &format!("round {round}")))
            .expect("publish");
        sender.publish(plan_delta("plan", &format!("{round} "))).expect("publish");
    }
    // A lifecycle event must be admitted even while the queue is full.
    sender.publish(turn_completed()).expect("lifecycle publish must not fail");
    // The producer going away ends the stream after draining.
    drop(sender);

    let mut ordered = Vec::new();
    while let Some(event) = receiver.recv().await {
        ordered.push(event);
    }

    assert_eq!(
        ordered.last().expect("drained events"),
        &turn_completed(),
        "lifecycle event must survive saturation"
    );
    let plan_events: Vec<_> = ordered
        .iter()
        .filter(|event| matches!(event.event(), vtcode_exec_events::ThreadEvent::PlanDelta(_)))
        .collect();
    assert_eq!(plan_events.len(), 1, "all plan deltas for one item merge into one event");
    if let vtcode_exec_events::ThreadEvent::PlanDelta(merged) = plan_events[0].event() {
        assert_eq!(merged.delta, (0..64).map(|round| format!("{round} ")).collect::<String>());
    } else {
        panic!("expected merged plan delta");
    }
    assert!(ordered.len() <= 8 + 1, "queue stays bounded: {}", ordered.len());
    assert_eq!(receiver.take_dropped_ordered(), 0, "no ordered event dropped while coalescable entries existed");
}

#[tokio::test]
async fn saturated_ordered_events_drop_oldest_and_count_the_gap() {
    let (sender, receiver) = live_event_channel(4);
    for round in 0..10 {
        sender.publish(thread_started(&round.to_string())).expect("publish");
    }
    drop(sender);
    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 4, "queue stays bounded");
    assert_eq!(receiver.take_dropped_ordered(), 6, "every displaced ordered event is counted");
    if let vtcode_exec_events::ThreadEvent::ThreadStarted(last) = drained[3].event() {
        assert_eq!(last.thread_id, "9", "the newest ordered event must survive");
    } else {
        panic!("expected thread.started");
    }
}

#[tokio::test]
async fn publish_fails_only_after_the_consumer_half_drops() {
    let (sender, receiver) = live_event_channel(4);
    sender.publish(item_update("item", "text")).expect("publish with live consumer");
    drop(receiver);
    assert!(
        sender.publish(item_update("item", "more")).is_err(),
        "publishing after the consumer dropped must fail"
    );
}

#[tokio::test]
async fn tool_output_deltas_for_one_item_merge_without_losing_bytes() {
    use vtcode_exec_events::ToolCallStatus;
    let (sender, receiver) = live_event_channel(4);
    // Asymmetric chunks: differing lengths, unicode split across chunks, and a
    // status transition with an exit code on the final chunk.
    sender
        .publish(tool_output_update("capture", "alpha\n", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture", "βeta\n", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture", "gamma", Some(true), ToolCallStatus::Failed, Some(3)))
        .expect("publish");
    drop(sender);

    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 1, "pending deltas for one item merge into one event");
    let merged = tool_output_details(&drained[0]);
    assert_eq!(merged.output, "alpha\nβeta\ngamma");
    assert_eq!(merged.is_delta, Some(true));
    assert_eq!(merged.status, ToolCallStatus::Failed, "newest status wins");
    assert_eq!(merged.exit_code, Some(3), "newest exit code wins");
    assert_eq!(merged.call_id, "capture-call");
}

#[tokio::test]
async fn tool_output_snapshots_still_replace_pending_updates() {
    use vtcode_exec_events::ToolCallStatus;
    let (sender, receiver) = live_event_channel(4);
    // Both spellings of "snapshot": absent flag and explicit false.
    sender
        .publish(tool_output_update("capture", "first", None, ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture", "second", Some(false), ToolCallStatus::InProgress, None))
        .expect("publish");
    drop(sender);

    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 1, "snapshots keep latest-wins replacement");
    let latest = tool_output_details(&drained[0]);
    assert_eq!(latest.output, "second");
    assert_eq!(latest.is_delta, Some(false));
}

#[tokio::test]
async fn tool_output_delta_and_snapshot_for_one_item_share_a_slot() {
    use vtcode_exec_events::ToolCallStatus;
    let (sender, receiver) = live_event_channel(4);
    sender
        .publish(tool_output_update("capture", "chunk-", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture", "snapshot", Some(false), ToolCallStatus::Completed, None))
        .expect("publish");
    drop(sender);

    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    // One slot per item: the terminal snapshot supersedes the accumulated
    // delta instead of leaving a second slot that saturation could evict.
    assert_eq!(drained.len(), 1, "delta and snapshot share the item's slot");
    assert_eq!(tool_output_details(&drained[0]).output, "snapshot");
    assert_eq!(tool_output_details(&drained[0]).is_delta, Some(false));
}

#[tokio::test]
async fn interleaved_tool_output_deltas_merge_per_item_in_queue_order() {
    use vtcode_exec_events::ToolCallStatus;
    let (sender, receiver) = live_event_channel(4);
    sender
        .publish(tool_output_update("capture-a", "a1-", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture-b", "b1", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("capture-a", "a2", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    drop(sender);

    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 2, "each stream merges independently");
    // An active stream moves to the back on each merge, so the quieter `b`
    // slot leads; LRU eviction then protects the still-running `a` stream.
    assert_eq!(tool_output_details(&drained[0]).output, "b1");
    assert_eq!(tool_output_details(&drained[1]).output, "a1-a2");
}

#[tokio::test]
async fn saturation_keeps_the_active_long_running_stream_intact() {
    use vtcode_exec_events::ToolCallStatus;
    let (sender, receiver) = live_event_channel(2);
    sender
        .publish(tool_output_update("running", "a-", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("quiet", "b", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    // The running command keeps producing: each chunk moves its slot to the
    // back, leaving `quiet` as the eviction victim when the third item arrives.
    sender
        .publish(tool_output_update("running", "c-", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("running", "d", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    sender
        .publish(tool_output_update("new", "e", Some(true), ToolCallStatus::InProgress, None))
        .expect("publish");
    drop(sender);

    let mut drained = Vec::new();
    while let Some(event) = receiver.recv().await {
        drained.push(event);
    }
    assert_eq!(drained.len(), 2, "the bounded channel keeps its capacity");
    let running = drained
        .iter()
        .find(|event| tool_output_details(event).call_id == "running-call")
        .expect("the long-running stream survived saturation");
    assert_eq!(tool_output_details(running).output, "a-c-d");
}
