use super::{
    ActivityState, MAX_TRANSCRIPT_ITEMS, ToolActivityStatus, ToolCategory, ToolCategoryCounts, Transcript,
    TranscriptItem, TranscriptRole, UserAttachment, describe_activity_verbs, tool_row_gist,
};
use vtcode_exec_events::{
    AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, DecisionItem, ErrorItem, FileChangeItem,
    HarnessEventItem, HarnessEventKind, McpToolCallItem, PatchApplyStatus, PlanItem, ReasoningItem, ThreadItemDetails,
    ToolCallStatus, ToolInvocationItem, ToolOutputItem, WebSearchItem,
};

fn item(id: usize, role: &str, text: &str) -> TranscriptItem {
    TranscriptItem::new(id.to_string(), role.to_string(), "Activity".into(), text.to_string())
}

mod turn_tests;

#[test]
fn tool_and_diff_rows_skip_unused_markdown_projections() {
    let mut transcript = Transcript::default();
    for role in ["tool", "diff"] {
        let row = item(1, role, "status\n\nmetadata");
        assert!(row.markdown.is_empty());
        assert!(row.accessibility_text.is_empty());
        transcript.push(row);
        let prepared = transcript.get(transcript.len() - 1).unwrap();
        assert!(prepared.markdown.is_empty());
        assert!(prepared.accessibility_text.is_empty());
    }

    transcript.push(item(2, "assistant", "A **readable** response."));
    let message = transcript.get(2).unwrap();
    assert_eq!(message.markdown.as_str(), "A **readable** response.");
    assert!(message.accessibility_text.contains("readable"));
}

#[test]
fn unchanged_message_snapshots_skip_projection_and_layout_work() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "assistant", "A **stable** response."));
    let _initial_layout = transcript.take_layout_changes();
    let original_markdown = transcript.get(0).unwrap().markdown.clone();
    let original_accessibility = transcript.get(0).unwrap().accessibility_text.clone();

    let mut duplicate = item(1, "assistant", "A **stable** response.");
    duplicate.markdown = "unexpected projection".into();
    duplicate.accessibility_text = "unexpected accessibility text".into();
    transcript.upsert(duplicate);

    assert!(transcript.take_layout_changes().is_empty());
    assert_eq!(transcript.get(0).unwrap().markdown, original_markdown);
    assert_eq!(transcript.get(0).unwrap().accessibility_text, original_accessibility);

    transcript.upsert(item(1, "assistant", "A **changed** response."));
    assert_eq!(transcript.get(0).unwrap().markdown.as_str(), "A **changed** response.");
}

#[test]
fn metadata_only_upserts_clone_render_projections_without_repreparing() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "assistant", "A response.\n\n- kept"));

    // Same text and role but a new title: content changed, so the row is
    // replaced and the projections must be cloned, not recomputed or dropped.
    let mut retitled = item(1, "assistant", "A response.\n\n- kept");
    retitled.title = "Retitled".into();
    retitled.markdown = "unexpected projection".into();
    retitled.has_list = false;
    transcript.upsert(retitled);

    let stored = transcript.get(0).unwrap();
    assert_eq!(stored.title.as_str(), "Retitled");
    assert_eq!(stored.markdown.as_str(), "A response.\n\n- kept");
    assert!(stored.has_list);
}

#[test]
fn prepending_history_preserves_layout_anchor_and_tool_group_state() {
    let mut transcript = Transcript::default();
    transcript.push(item(3, "tool", "completed"));
    transcript.push(item(4, "tool", "running"));
    let _initial_layout = transcript.take_layout_changes();
    transcript.toggle_activity_group("3");
    let _expanded_layout = transcript.take_layout_changes();

    let inserted =
        transcript.prepend_prepared(vec![item(1, "assistant", "Earlier context"), item(2, "tool", "completed")]);

    assert_eq!(inserted, 2);
    assert_eq!(transcript.items().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["1", "2", "3", "4"]);
    assert_eq!(transcript.activity_group(1), Some(1..4));
    assert!(transcript.get(1).unwrap().activity_expanded);
    assert!(transcript.get(2).unwrap().activity_expanded);
    assert_eq!(
        transcript.take_layout_changes(),
        vec![
            super::TranscriptLayoutChange::Prepend(2),
            super::TranscriptLayoutChange::RemeasureItems(0..3),
        ]
    );
}

#[test]
fn activity_group_summary_tracks_appends_status_changes_and_removal() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "running"));
    transcript.push(item(2, "tool", "running"));
    let _initial_layout = transcript.take_layout_changes();

    assert_eq!(
        transcript.get(0).unwrap().activity_group_summary,
        super::ActivityGroupSummary { total: 2, running: 2, failed: 0 }
    );

    transcript.upsert(item(2, "tool", "failed"));
    assert_eq!(
        transcript.get(0).unwrap().activity_group_summary,
        super::ActivityGroupSummary { total: 2, running: 1, failed: 1 }
    );

    transcript.push(item(3, "tool", "completed"));
    assert_eq!(transcript.get(0).unwrap().activity_group_summary.total, 3);
    transcript.remove("2");
    assert_eq!(
        transcript.get(0).unwrap().activity_group_summary,
        super::ActivityGroupSummary { total: 2, running: 1, failed: 0 }
    );
}

#[test]
fn collapsed_tool_output_updates_skip_geometry_until_expanded() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "running\n\nfirst"));
    transcript.push(item(2, "tool", "running\n\nfirst"));
    let _initial_layout = transcript.take_layout_changes();

    transcript.upsert(item(2, "tool", "running\n\nstreamed output grew"));
    assert!(transcript.take_layout_changes().is_empty());
    assert_eq!(transcript.get(1).unwrap().text.as_str(), "running\n\nstreamed output grew");

    transcript.toggle_tool("2");
    let _expanded_layout = transcript.take_layout_changes();
    transcript.upsert(item(2, "tool", "running\n\nexpanded output grew again"));
    let changes = transcript.take_layout_changes();
    assert!(
        changes.iter().any(|change| {
            matches!(change, super::TranscriptLayoutChange::RemeasureItems(range) if *range == (1..2))
        })
    );
    assert!(
        !changes.iter().any(|change| {
            matches!(change, super::TranscriptLayoutChange::RemeasureItems(range) if *range == (0..1))
        })
    );
}

#[test]
fn activity_group_boundaries_follow_removal_role_changes_and_eviction() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "completed"));
    transcript.push(item(2, "tool", "running"));
    transcript.push(item(3, "assistant", "commentary"));
    transcript.push(item(4, "tool", "completed"));
    assert_eq!(transcript.take_layout_changes(), vec![super::TranscriptLayoutChange::Reset(4)]);
    assert_eq!(transcript.activity_group(1), Some(0..2));
    assert_eq!(transcript.activity_group(3), None);
    transcript.toggle_activity_group("1");
    transcript.upsert(item(1, "tool", "completed with output"));
    assert!(transcript.get(0).unwrap().activity_expanded);
    transcript.remove("1");
    assert_eq!(transcript.activity_group(0), None);
    transcript.upsert(item(3, "tool", "completed"));
    assert_eq!(transcript.activity_group(0), Some(0..3));
    for id in 5..MAX_TRANSCRIPT_ITEMS + 5 {
        transcript.push(item(id, "tool", "completed"));
    }
    assert_eq!(transcript.activity_group(0), Some(0..MAX_TRANSCRIPT_ITEMS));
    assert_eq!(transcript.len(), MAX_TRANSCRIPT_ITEMS);
    assert_eq!(transcript.get(MAX_TRANSCRIPT_ITEMS - 1).unwrap().text, "completed");

    transcript.toggle_activity_group("5");
    transcript.push(item(MAX_TRANSCRIPT_ITEMS + 5, "tool", "completed"));
    let promoted_head = transcript.get(0).unwrap();
    assert_eq!(promoted_head.id, "6");
    assert!(promoted_head.activity_group_head);
    assert!(promoted_head.activity_expanded, "the promoted group head retains expansion state");
    assert_eq!(usize::from(promoted_head.activity_group_summary.total), MAX_TRANSCRIPT_ITEMS);
    assert_eq!(transcript.activity_group(0), Some(0..MAX_TRANSCRIPT_ITEMS));
}

#[test]
fn updates_keep_tool_expansion_and_queue_layout_changes() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "before"));
    transcript.push(item(2, "assistant", "reply"));
    let _initial_layout = transcript.take_layout_changes();
    transcript.toggle_tool("1");
    transcript.upsert(item(1, "tool", "after\nwith details"));

    assert_eq!(transcript.len(), 2);
    assert!(transcript.get(0).unwrap().expanded);
    assert_eq!(transcript.get(0).unwrap().text, "after\nwith details");
    assert!(
        transcript
            .take_layout_changes()
            .iter()
            .any(|change| { matches!(change, super::TranscriptLayoutChange::PauseFollowingTail) })
    );
    transcript.clear();
    assert!(transcript.is_empty());
    assert_eq!(transcript.len(), 0);
    assert_eq!(transcript.take_layout_changes(), vec![super::TranscriptLayoutChange::Reset(0)]);
}

#[test]
fn appending_tool_rows_remeasures_only_the_shared_group_header() {
    let mut transcript = Transcript::default();
    transcript.push(item(0, "user", "prompt"));
    let _initial_layout = transcript.take_layout_changes();

    for id in 1..=100 {
        transcript.push(item(id, "tool", "completed"));
    }

    let changes = transcript.take_layout_changes();
    assert_eq!(transcript.get(1).unwrap().activity_group_summary.total, 100);
    assert_eq!(transcript.get(1).unwrap().activity_group_summary.running, 0);
    let remeasured_ranges = changes
        .iter()
        .filter_map(|change| match change {
            super::TranscriptLayoutChange::RemeasureItems(range) => Some(range.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(remeasured_ranges.len(), 99, "each added row updates the existing group header");
    assert!(remeasured_ranges.iter().all(|range| *range == (1..2)));
}

#[test]
fn row_lookup_keeps_newest_duplicate_id_semantics() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "older"));
    transcript.push(item(2, "assistant", "middle"));
    transcript.push(item(1, "assistant", "newest"));

    assert_eq!(transcript.position_by_id("1"), Some(2));
    assert_eq!(transcript.position_by_id("2"), Some(1));
    assert_eq!(transcript.position_by_id("missing"), None);
    transcript.toggle_tool("1");
    assert!(transcript.get(0).unwrap().expanded, "role-specific lookup still finds the latest matching tool row");
}

#[test]
fn tool_status_updates_remeasure_the_group_header_and_changed_row() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "running"));
    transcript.push(item(2, "tool", "running"));
    let _initial_layout = transcript.take_layout_changes();

    transcript.upsert(item(2, "tool", "failed"));

    let changes = transcript.take_layout_changes();
    assert!(transcript.get(1).unwrap().activity_grouped, "status updates preserve group membership");
    assert!(
        changes.iter().any(|change| {
            matches!(change, super::TranscriptLayoutChange::RemeasureItems(range) if *range == (1..2))
        })
    );
    assert!(
        changes.iter().any(|change| {
            matches!(change, super::TranscriptLayoutChange::RemeasureItems(range) if *range == (0..1))
        })
    );
}

#[test]
fn output_only_updates_preserve_transcript_geometry_and_existing_capture() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "running\n\nmetadata"));
    let _initial_layout = transcript.take_layout_changes();

    transcript.set_output("1", Some("first output".into()));
    assert!(transcript.take_layout_changes().is_empty());

    transcript.upsert(item(1, "tool", "running\n\nmetadata"));
    assert!(transcript.take_layout_changes().is_empty());
    let output = transcript.get(0).and_then(|row| row.output.as_ref()).map(ToString::to_string);
    assert_eq!(output.as_deref(), Some("first output"));

    transcript.set_output("1", Some("updated output".into()));
    assert!(transcript.take_layout_changes().is_empty());
    let output = transcript.get(0).and_then(|row| row.output.as_ref()).map(ToString::to_string);
    assert_eq!(output.as_deref(), Some("updated output"));

    transcript.toggle_tool("1");
    let _expanded_layout = transcript.take_layout_changes();
    transcript.set_output("1", Some("a longer\noutput".into()));
    assert_eq!(transcript.take_layout_changes(), vec![super::TranscriptLayoutChange::RemeasureItems(0..1)]);
}

#[test]
fn notice_rows_carry_their_severity_at_construction() {
    for severity in [
        super::NoticeSeverity::Info,
        super::NoticeSeverity::Warning,
        super::NoticeSeverity::Error,
    ] {
        let item = TranscriptItem::notice("id".to_string(), "title".into(), "body".to_string(), severity);
        assert!(item.is_notice());
        assert_eq!(item.notice_severity, severity);
        assert!(!item.has_activity_status);
    }
}

#[test]
fn transcript_roles_round_trip_with_case_sensitive_parse() {
    for role in TranscriptRole::ALL {
        assert_eq!(TranscriptRole::from_str(role.as_str()), Some(role), "{role:?}");
    }
    // Unknown and case-shifted roles stay unclassified so new service
    // roles keep rendering instead of vanishing.
    assert_eq!(TranscriptRole::from_str("unknown"), None);
    assert_eq!(TranscriptRole::from_str(""), None);
    assert_eq!(TranscriptRole::from_str("Tool"), None);
    assert_eq!(TranscriptRole::from_str("ASSISTANT"), None);
}

#[test]
fn searchable_roles_cover_messages_and_tool_rows_only() {
    for role in TranscriptRole::ALL {
        let expected = matches!(role, TranscriptRole::User | TranscriptRole::Assistant | TranscriptRole::Tool);
        assert_eq!(role.is_searchable_conversation(), expected, "{role:?}");
    }
}

#[test]
fn activity_state_classifies_each_side_of_every_boundary() {
    // Both sides of each boundary, including near-miss prefixes that must
    // not classify: the parser is shared by group summaries, the Activity
    // tab, and historical-status rewrites.
    for (label, expected) in [
        ("running", ActivityState::Running),
        ("in progress", ActivityState::Running),
        ("started", ActivityState::Running),
        ("completed", ActivityState::Completed),
        ("failed", ActivityState::Failed),
        ("failed (exit 0)", ActivityState::Failed),
        ("failed (exit 17)", ActivityState::Failed),
        ("interrupted", ActivityState::Interrupted),
        // Near misses: prefix traps and case shifts stay Other.
        ("failed (exit", ActivityState::Other),
        ("failed (exit ", ActivityState::Failed),
        ("failed loudly", ActivityState::Other),
        ("Running", ActivityState::Other),
        ("COMPLETED", ActivityState::Other),
        ("", ActivityState::Other),
        ("activity", ActivityState::Other),
        ("verification requested", ActivityState::Other),
    ] {
        assert_eq!(ActivityState::from_status(label), expected, "{label:?}");
    }
}

#[test]
fn group_summary_status_matches_the_shared_classifier() {
    // ToolActivityStatus delegates to ActivityState::from_status; prove
    // the delegation agrees on every classified label plus multi-line
    // bodies (only the first line classifies).
    for (label, running, failed) in [
        ("running", true, false),
        ("in progress", true, false),
        ("started", true, false),
        ("completed", false, false),
        ("failed", false, true),
        ("failed (exit 3)", false, true),
        ("interrupted", false, false),
        ("activity", false, false),
        ("failed loudly", false, false),
    ] {
        let inline = ToolActivityStatus::from_text(label);
        assert_eq!((inline.running, inline.failed), (running, failed), "{label:?}");
        let with_body = ToolActivityStatus::from_text(&format!("{label}\n\n{{\"details\": true}}"));
        assert_eq!((with_body.running, with_body.failed), (running, failed), "{label:?} with body");
    }
}

#[test]
fn revision_bumps_on_item_mutations_but_not_layout_only_changes() {
    // The revision is the invalidation key for memoized inspector/palette
    // projections: a missing bump is a stale-projection bug, so every
    // item-mutating method must bump and every layout-only method must not.
    let mut transcript = Transcript::default();
    assert_eq!(transcript.revision(), 0);

    transcript.follow_latest();
    transcript.remeasure();
    let _drained = transcript.take_layout_changes();
    assert_eq!(transcript.revision(), 0, "layout-only changes must not bump");

    transcript.push(item(1, "assistant", "first"));
    assert_eq!(transcript.revision(), 1);

    // New rows via upsert bump (the push path bumps again; only the increase matters).
    let before = transcript.revision();
    transcript.upsert(item(2, "tool", "running"));
    transcript.upsert(item(3, "tool", "completed"));
    assert!(transcript.revision() > before, "upserted rows must bump");

    // Updates to existing rows bump, including output-only changes.
    let before = transcript.revision();
    transcript.upsert(item(1, "assistant", "first, edited"));
    assert!(transcript.revision() > before, "text updates must bump");
    let before = transcript.revision();
    transcript.set_output("2", Some("output".into()));
    assert_eq!(transcript.revision(), before + 1, "output changes must bump");

    // No-op writes do not bump (upsert always bumps; setters check first).
    let before = transcript.revision();
    transcript.set_output("2", Some("output".into()));
    transcript.set_output("missing", Some("output".into()));
    transcript.remove("missing");
    assert_eq!(transcript.revision(), before, "no-op writes must not bump");

    // Expansion toggles mutate rows, so they bump when they hit.
    let before = transcript.revision();
    transcript.toggle_tool("2");
    transcript.toggle_activity_group("2");
    assert!(transcript.revision() > before, "toggles on live rows must bump");
    let before = transcript.revision();
    transcript.toggle_tool("missing");
    transcript.toggle_diff("missing");
    assert_eq!(transcript.revision(), before, "toggles on missing rows must not bump");

    // Structural mutations bump; empty prepends do not.
    let before = transcript.revision();
    transcript.remove("3");
    assert_eq!(transcript.revision(), before + 1, "removal must bump");
    let before = transcript.revision();
    assert_eq!(transcript.prepend_prepared(Vec::new()), 0);
    assert_eq!(transcript.revision(), before, "empty prepend must not bump");
    let before = transcript.revision();
    transcript.clear();
    assert_eq!(transcript.revision(), before + 1, "clear must bump");

    // Prepends, preview updates, and reveals bump when they land.
    let before = transcript.revision();
    assert_eq!(transcript.prepend_prepared(vec![item(9, "tool", "running")]), 1);
    assert_eq!(transcript.revision(), before + 1, "non-empty prepend must bump");
    let before = transcript.revision();
    transcript.set_app_previews("9", Vec::new());
    assert_eq!(transcript.revision(), before, "unchanged previews must not bump");
    let before = transcript.revision();
    assert_eq!(transcript.reveal_tool_item("9"), Some(0));
    assert_eq!(transcript.revision(), before + 1, "reveal must bump");
}

#[test]
fn append_text_concatenates_streaming_deltas_onto_one_row() {
    let mut transcript = Transcript::default();
    // Empty deltas are no-ops: no row and no revision bump.
    let before = transcript.revision();
    transcript.append_text("plan".to_string(), "assistant", "Plan".into(), "");
    assert!(transcript.is_empty());
    assert_eq!(transcript.revision(), before, "empty deltas must not bump");

    transcript.append_text("plan".to_string(), "assistant", "Plan".into(), "Hello ");
    assert!(transcript.revision() > before, "the first delta creates the row");
    let _initial_layout = transcript.take_layout_changes();

    transcript.append_text("plan".to_string(), "assistant", "Plan".into(), "**world**");
    transcript.append_text("plan".to_string(), "assistant", "Plan".into(), "!");
    assert_eq!(transcript.len(), 1, "deltas for one id must share one row");

    let row = transcript.get(0).unwrap();
    assert_eq!(row.id, "plan");
    assert!(row.is_assistant());
    assert_eq!(row.text.as_str(), "Hello **world**!");
    // Render projections always derive from the full concatenated text.
    assert_eq!(row.markdown.as_str(), "Hello **world**!");
    assert!(row.accessibility_text.contains("world"));
    assert!(!row.has_list, "prose without a list must not take the list layout");

    assert_eq!(
        transcript.take_layout_changes(),
        vec![
            super::TranscriptLayoutChange::RemeasureItems(0..1),
            super::TranscriptLayoutChange::RemeasureItems(0..1),
        ],
        "follow-up deltas remeasure only their own row"
    );

    // A new id starts a second row; list detection follows the full text.
    transcript.append_text("steps".to_string(), "assistant", "Plan".into(), "Do:\n\n- one");
    assert_eq!(transcript.len(), 2);
    let row = transcript.get(1).unwrap();
    assert_eq!(row.text.as_str(), "Do:\n\n- one");
    assert!(row.has_list, "a delta that completes a list must flip the layout flag");
}

#[test]
fn append_text_sanitizes_markup_across_delta_boundaries() {
    let mut transcript = Transcript::default();
    // The tag opens in one delta and closes in the next: only a full-text
    // derivation strips it, so the pin fails if appends ever sanitize per delta.
    transcript.append_text("markup".to_string(), "assistant", "Plan".into(), "<a hr");
    transcript.append_text("markup".to_string(), "assistant", "Plan".into(), "ef=\"u\">z</a>");
    let row = transcript.get(0).unwrap();
    assert_eq!(row.text.as_str(), "<a href=\"u\">z</a>");
    assert_eq!(row.markdown.as_str(), "z");
    assert_eq!(row.accessibility_text.as_str(), "z");
}

#[test]
fn consecutive_full_remeasures_collapse_to_one_probe() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "assistant", "A **stable** response."));
    let _initial_layout = transcript.take_layout_changes();

    // Theme browsing while the transcript is unmounted: twenty clicks
    // enqueue twenty bursts, but the drain must carry one full probe so
    // the consumer remeasures rows once instead of once per click.
    for _ in 0..20 {
        transcript.remeasure();
    }
    assert_eq!(transcript.take_layout_changes(), vec![super::TranscriptLayoutChange::Remeasure]);

    // Interleaved work breaks the run: a row pushed between two probes
    // keeps both probes, since the second measures newer state.
    transcript.remeasure();
    transcript.push(item(2, "assistant", "Another response."));
    transcript.remeasure();
    assert_eq!(
        transcript.take_layout_changes(),
        vec![
            super::TranscriptLayoutChange::Remeasure,
            super::TranscriptLayoutChange::Splice { range: 1..1, count: 1 },
            super::TranscriptLayoutChange::Remeasure,
        ]
    );
}

#[test]
fn diagnostic_notices_toggle_details_while_plain_notices_stay_fixed() {
    use super::NoticeSeverity;
    let mut transcript = Transcript::default();
    transcript.push(TranscriptItem::diagnostic_notice(
        "diag-1".into(),
        "Tool 'task_tracker' failed.".into(),
        "Likely cause: duplicate create.".to_string(),
        NoticeSeverity::Warning,
    ));
    transcript.push(TranscriptItem::notice(
        "plain-1".into(),
        "Run status".into(),
        "Turn ended.".to_string(),
        NoticeSeverity::Info,
    ));

    transcript.toggle_notice("diag-1");
    assert!(transcript.get(0).unwrap().expanded);
    assert!(!transcript.get(1).unwrap().expanded, "plain notices have no details toggle");

    let before = transcript.revision();
    transcript.toggle_notice("plain-1");
    transcript.toggle_notice("missing");
    assert_eq!(transcript.revision(), before, "no-op toggles must not bump");

    // A streaming refresh keeps expansion and swaps the payload.
    transcript.upsert(TranscriptItem::diagnostic_notice(
        "diag-1".into(),
        "Tool 'task_tracker' failed.".into(),
        "Likely cause: duplicate create. Attempts: 2.".to_string(),
        NoticeSeverity::Warning,
    ));
    let stored = transcript.get(0).unwrap();
    assert!(stored.expanded, "streaming updates preserve expansion");
    assert!(stored.text.contains("Attempts: 2"), "streaming updates refresh the details");

    // Boundary: empty evidence offers no toggle.
    transcript.push(TranscriptItem::diagnostic_notice(
        "diag-empty".into(),
        "Tool 'noop' failed.".into(),
        "   ".to_string(),
        NoticeSeverity::Error,
    ));
    let before = transcript.revision();
    transcript.toggle_notice("diag-empty");
    assert_eq!(transcript.revision(), before, "empty evidence must not toggle");
    assert!(!transcript.get(2).unwrap().expanded);
}

fn invocation(name: &str) -> ThreadItemDetails {
    ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
        tool_name: name.into(),
        arguments: None,
        tool_call_id: None,
        status: ToolCallStatus::Completed,
        outcome: None,
    }))
}

fn invocation_with_action(name: &str, action: &str) -> ThreadItemDetails {
    ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
        tool_name: name.into(),
        arguments: Some(serde_json::json!({"action": action})),
        tool_call_id: None,
        status: ToolCallStatus::Completed,
        outcome: None,
    }))
}

#[test]
fn tool_category_mirrors_service_labels_at_summary_granularity() {
    use ToolCategory::{Edit, Other, Read, Run, Search, Tracker};
    let classified = [
        ("read_file", Read),
        ("read_pty_session", Read),
        ("web_fetch", Read),
        ("fetch_url", Read),
        ("defuddle_fetch", Read),
        ("fetch", Read),
        ("get_errors", Read),
        ("list_files", Read),
        ("edit_file", Edit),
        ("write_file", Edit),
        ("create_file", Edit),
        ("delete_file", Edit),
        ("apply_patch", Edit),
        ("search_replace", Edit),
        ("exec_command", Run),
        ("code_search", Search),
        ("grep", Search),
        ("grep_file", Search),
        ("task_tracker", Tracker),
        ("request_user_input", Other),
        ("some_future_tool", Other),
    ];
    for (name, expected) in classified {
        assert_eq!(ToolCategory::from_details(&invocation(name)), Some(expected), "tool {name}");
    }
    // MCP prefixes normalize to the suffixed builtin, matching the service.
    assert_eq!(ToolCategory::from_details(&invocation("mcp__github__read_file")), Some(Read));
    assert_eq!(ToolCategory::from_details(&invocation("mcp::github::exec_command")), Some(Run));
    assert_eq!(ToolCategory::from_details(&invocation("mcp__github__list_issues")), Some(Other));
    // Unified tools dispatch on their action arg; a missing action reads
    // (file) or runs (exec), and matching is case-insensitive.
    assert_eq!(ToolCategory::from_details(&invocation("file_operation_internal")), Some(Read));
    assert_eq!(ToolCategory::from_details(&invocation_with_action("file_operation_internal", "edit")), Some(Edit));
    assert_eq!(ToolCategory::from_details(&invocation_with_action("file_operation_internal", "DELETE")), Some(Edit));
    assert_eq!(
        ToolCategory::from_details(&invocation_with_action("file_operation_internal", "frobnicate")),
        Some(Other)
    );
    assert_eq!(ToolCategory::from_details(&invocation("command_session_internal")), Some(Run));
    assert_eq!(ToolCategory::from_details(&invocation_with_action("command_session_internal", "poll")), Some(Read));
    assert_eq!(ToolCategory::from_details(&invocation_with_action("command_session_internal", "wait")), Some(Other));
}

#[test]
fn tool_category_covers_every_event_variant() {
    assert_eq!(
        ToolCategory::from_details(&ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
            command: "cargo check".into(),
            arguments: None,
            aggregated_output: String::new(),
            exit_code: None,
            status: CommandExecutionStatus::Completed,
        }))),
        Some(ToolCategory::Run)
    );
    assert_eq!(
        ToolCategory::from_details(&ThreadItemDetails::WebSearch(Box::new(WebSearchItem {
            query: "gpui".into(),
            provider: None,
            results: None,
        }))),
        Some(ToolCategory::Search)
    );
    for details in [
        ThreadItemDetails::McpToolCall(Box::new(McpToolCallItem {
            tool_name: "list_issues".into(),
            arguments: None,
            result: None,
            status: None,
        })),
        ThreadItemDetails::Harness(Box::new(HarnessEventItem::new(HarnessEventKind::PlanningStarted))),
        ThreadItemDetails::Error(ErrorItem { message: "boom".into() }),
    ] {
        assert_eq!(ToolCategory::from_details(&details), Some(ToolCategory::Other));
    }
    // Follow-up output keeps its invocation's category; non-tool rows carry none.
    for details in [
        ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
            is_delta: None,
            call_id: "call".into(),
            tool_call_id: None,
            spool_path: None,
            output: "done".into(),
            exit_code: None,
            status: ToolCallStatus::Completed,
            app_previews: Vec::new(),
        })),
        ThreadItemDetails::AgentMessage(AgentMessageItem { text: "hi".into() }),
        ThreadItemDetails::Plan(PlanItem { text: "plan".into() }),
        ThreadItemDetails::Reasoning(Box::new(ReasoningItem { diagnostic: None, text: "why".into(), stage: None })),
        ThreadItemDetails::Decision(Box::new(DecisionItem {
            summary: "s".into(),
            rationale: "r".into(),
            alternatives: Vec::new(),
            evidence_ids: Vec::new(),
        })),
        ThreadItemDetails::FileChange(Box::new(FileChangeItem {
            diff_incomplete: None,
            changes: Vec::new(),
            status: PatchApplyStatus::Completed,
            unified_diff: None,
            additions: None,
            deletions: None,
        })),
    ] {
        assert_eq!(ToolCategory::from_details(&details), None);
    }
}

#[test]
fn activity_verbs_compose_in_fixed_order_with_english_plurals() {
    assert_eq!(describe_activity_verbs(&ToolCategoryCounts::default()), None);
    let mut other_only = ToolCategoryCounts::default();
    other_only.add(ToolCategory::Other);
    other_only.add(ToolCategory::Other);
    assert_eq!(describe_activity_verbs(&other_only), None, "verb-less groups keep the count label");
    assert!(!other_only.has_verbs());
    // Asymmetric insertion: edits arrive before reads, but Read still leads.
    let mut counts = ToolCategoryCounts::default();
    assert!(!counts.has_verbs());
    counts.add(ToolCategory::Edit);
    counts.add(ToolCategory::Edit);
    counts.add(ToolCategory::Read);
    counts.add(ToolCategory::Run);
    counts.add(ToolCategory::Search);
    counts.add(ToolCategory::Search);
    counts.add(ToolCategory::Other);
    assert!(counts.has_verbs());
    assert_eq!(
        describe_activity_verbs(&counts).as_deref(),
        Some("Read 1 file · Edited 2 files · Ran 1 command · Searched 2 queries · 1 other action")
    );
    let mut single = ToolCategoryCounts::default();
    single.add(ToolCategory::Run);
    single.add(ToolCategory::Other);
    single.add(ToolCategory::Other);
    assert_eq!(describe_activity_verbs(&single).as_deref(), Some("Ran 1 command · 2 other actions"));
    // Tracker rows name their verb instead of collapsing into "other actions".
    let mut tracker = ToolCategoryCounts::default();
    assert!(!tracker.has_verbs());
    tracker.add(ToolCategory::Tracker);
    tracker.add(ToolCategory::Tracker);
    assert!(tracker.has_verbs());
    assert_eq!(describe_activity_verbs(&tracker).as_deref(), Some("Updated 2 task lists"));
}

fn invocation_item(name: &str, arguments: Option<serde_json::Value>) -> ToolInvocationItem {
    ToolInvocationItem {
        tool_name: name.into(),
        arguments,
        tool_call_id: None,
        status: ToolCallStatus::Completed,
        outcome: None,
    }
}

fn gist(name: &str, arguments: serde_json::Value) -> String {
    tool_row_gist(&invocation_item(name, Some(arguments)), None)
}

#[test]
fn tool_row_gist_verbs_file_objects_with_basenames() {
    use serde_json::json;
    assert_eq!(gist("read_file", json!({"path": "src/lib/Cargo.toml"})), "Read Cargo.toml");
    assert_eq!(gist("edit_file", json!({"file_path": "crates/x/main.rs"})), "Edited main.rs");
    assert_eq!(gist("write_file", json!({"filepath": "notes.txt"})), "Edited notes.txt");
    assert_eq!(gist("delete_file", json!({"target_path": "/tmp/stale.log"})), "Edited stale.log");
    // Trailing separators resolve to the last real segment; separators alone
    // carry no name and fall back.
    assert_eq!(gist("read_file", json!({"path": "src/"})), "Read src");
    assert_eq!(gist("read_file", json!({"path": "C:\\work\\lib.rs"})), "Read lib.rs");
    assert_eq!(gist("read_file", json!({"path": "///"})), "Read File");
    // Fetch tools read URLs, which keep the host: a basename would mislead.
    assert_eq!(
        gist("web_fetch", json!({"url": "https://example.com/page?x=1"})),
        "Read https://example.com/page?x=1"
    );
}

#[test]
fn tool_row_gist_key_order_matches_service_precedence() {
    use serde_json::json;
    // Every adjacent pair disagrees, so each assert pins one precedence edge.
    assert_eq!(gist("read_file", json!({"path": "a.rs", "file_path": "b.rs"})), "Read a.rs");
    assert_eq!(gist("read_file", json!({"file_path": "b.rs", "filepath": "c.rs"})), "Read b.rs");
    assert_eq!(gist("read_file", json!({"filepath": "c.rs", "target_path": "d.rs"})), "Read c.rs");
    assert_eq!(gist("code_search", json!({"query": "Widget", "pattern": "Widg.*"})), "Search code for Widget");
    assert_eq!(gist("exec_command", json!({"command": "cargo check", "cmd": "cargo build"})), "Ran cargo check");
    // A blank winner does not shadow a usable fallback key.
    assert_eq!(gist("read_file", json!({"path": "  ", "file_path": "fallback.rs"})), "Read fallback.rs");
}

#[test]
fn tool_row_gist_leads_runs_and_searches_with_verbs() {
    use serde_json::json;
    assert_eq!(gist("exec_command", json!({"cmd": "cargo check --all"})), "Ran cargo check --all");
    assert_eq!(gist("exec_command", json!({"command": "  make test  "})), "Ran make test");
    assert_eq!(gist("exec_command", json!({"command": ["cargo", "check", "--all"]})), "Ran cargo check --all");
    assert_eq!(gist("exec_command", json!({"command": ["echo", 42, true, "hi"]})), "Ran echo hi");
    assert_eq!(gist("code_search", json!({"query": "Widget"})), "Search code for Widget");
    // Grep phrases the pattern; its scope stays in the detail lines.
    assert_eq!(gist("grep", json!({"pattern": "TODO", "path": "src"})), "Search files for TODO");
}

#[test]
fn tool_row_gist_renders_all_launch_tools_like_exec_command() {
    use serde_json::json;
    // Mirrors `tool_intent::is_command_run_tool_call`: every launch tool shows its
    // command with the `Ran` verb and classifies Run so its output merges into its row.
    for name in ["run_pty_cmd", "create_pty_session", "shell", "bash"] {
        assert_eq!(gist(name, json!({"command": "cargo check"})), "Ran cargo check");
        let details =
            ThreadItemDetails::ToolInvocation(Box::new(invocation_item(name, Some(json!({"command": "cargo check"})))));
        assert_eq!(ToolCategory::from_details(&details), Some(ToolCategory::Run), "{name}");
    }
}

#[test]
fn tool_row_gist_falls_back_to_pretty_tool_name() {
    use serde_json::json;
    assert_eq!(tool_row_gist(&invocation_item("read_file", None), None), "Read File");
    for arguments in [
        json!({}),
        json!({"path": ""}),
        json!({"path": "   "}),
        json!({"path": 42}),
        json!({"path": true}),
        json!({"path": ["a.rs"]}),
        json!({"path": null}),
        json!({"command": ["", 42]}),
    ] {
        assert_eq!(gist("read_file", arguments.clone()), "Read File");
        assert_eq!(gist("exec_command", arguments), "Exec Command");
    }
    // Other-category tools prettify instead of echoing raw identifiers.
    assert_eq!(gist("task_tracker", json!({"path": "plan.md"})), "Task Tracker");
    assert_eq!(gist("request_user_input", json!({"query": "color?"})), "Request User Input");
}

#[test]
fn tool_row_gist_covers_unified_and_mcp_names() {
    use serde_json::json;
    assert_eq!(gist("file_operation_internal", json!({"action": "edit", "path": "src/main.rs"})), "Edited main.rs");
    assert_eq!(gist("file_operation_internal", json!({"action": "read", "path": "README.md"})), "Read README.md");
    assert_eq!(gist("command_session_internal", json!({"action": "run", "cmd": "ls"})), "Ran ls");
    assert_eq!(
        gist("command_session_internal", json!({"action": "poll", "session_id": "run-1"})),
        "Read command session"
    );
    assert_eq!(gist("mcp__github__read_file", json!({"path": "issues.md"})), "Read issues.md");
}

#[test]
fn tool_row_gist_titles_session_followups_like_cli() {
    use serde_json::json;
    // Fixed follow-up labels.
    assert_eq!(gist("send_pty_input", json!({"session_id": "s"})), "Send command input");
    assert_eq!(gist("read_pty_session", json!({"session_id": "run-1"})), "Read command session");
    // write_stdin: explicit actions win; otherwise input sniffing decides.
    assert_eq!(gist("write_stdin", json!({"action": "wait", "session_id": "s"})), "Wait for command session");
    assert_eq!(gist("write_stdin", json!({"session_id": "s", "input": "ls"})), "Send command input");
    assert_eq!(gist("write_stdin", json!({"session_id": "s"})), "Read command session");
    // Asymmetric: explicit write with empty input polls instead of sending.
    assert_eq!(
        gist("write_stdin", json!({"action": "write", "session_id": "s", "input": ""})),
        "Read command session"
    );
    assert_eq!(
        gist("write_stdin", json!({"action": "write", "session_id": "s", "input": "ls"})),
        "Send command input"
    );
    // Unified action labels; launches keep the `Ran` command gist.
    assert_eq!(
        gist("command_session_internal", json!({"action": "wait", "session_id": "s"})),
        "Wait for command session"
    );
    assert_eq!(gist("command_session_internal", json!({"code": "print(1)"})), "Run code");
    assert_eq!(gist("command_session_internal", json!({"action": "frobnicate"})), "Exec action");
    // MCP-wrapped session tools strip to the same titles.
    assert_eq!(
        gist("mcp__tmux__write_stdin", json!({"action": "wait", "session_id": "s"})),
        "Wait for command session"
    );
}

#[test]
fn user_echo_with_attachments_keeps_bare_prompt_and_announces_paths() {
    let mut transcript = Transcript::default();
    let mut echo = item(1, "user", "Review these files");
    // Asymmetric fixtures: a text file with content bytes and an image with none.
    echo.user_attachments = vec![
        UserAttachment {
            path: "src/main.rs".into(),
            byte_len: 120,
            is_image: false,
        },
        UserAttachment {
            path: "shot.png".into(),
            byte_len: 0,
            is_image: true,
        },
    ];
    transcript.push(echo);
    let row = transcript.get(0).unwrap();
    assert_eq!(row.text.as_str(), "Review these files");
    assert_eq!(row.markdown.as_str(), "Review these files", "filenames must not leak into the Markdown body");
    assert!(row.accessibility_text.contains("Review these files"));
    assert!(row.accessibility_text.contains("src/main.rs"), "text-file path stays announced");
    assert!(row.accessibility_text.contains("shot.png"), "image path stays announced");
    let keywords = row.search_keywords();
    assert_eq!(keywords.len(), 4, "title, body, and both attached paths");
    assert!(keywords.iter().any(|keyword| keyword.contains("main.rs")));
    assert!(keywords.iter().any(|keyword| keyword.contains("shot.png")));
}

#[test]
fn attachment_accessibility_suffix_skips_plain_and_non_user_rows() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "user", "Plain follow-up"));
    assert_eq!(
        transcript.get(0).unwrap().accessibility_text.as_str(),
        "Plain follow-up",
        "text-only echoes keep the previous label shape"
    );

    let mut assistant = item(2, "assistant", "A response.");
    assistant.user_attachments = vec![UserAttachment {
        path: "stray.txt".into(),
        byte_len: 1,
        is_image: false,
    }];
    transcript.push(assistant);
    assert!(
        !transcript.get(1).unwrap().accessibility_text.contains("stray.txt"),
        "the suffix guard is role-scoped"
    );
}

#[test]
fn activity_category_survives_follow_up_output_upserts() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "in-progress\n\nmeta"));
    transcript.set_activity_category("1", ToolCategory::Read);
    assert_eq!(transcript.get(0).unwrap().activity_category, Some(ToolCategory::Read));
    // Follow-up output arrives as a fresh unclassified row for the same id.
    let mut output = item(1, "tool", "completed\n\nmeta");
    output.output = Some("done".into());
    transcript.upsert(output);
    assert_eq!(
        transcript.get(0).unwrap().activity_category,
        Some(ToolCategory::Read),
        "output upsert must not wipe the invocation's verb"
    );
    assert_eq!(transcript.get(0).unwrap().output.as_deref(), Some("done"));
}
