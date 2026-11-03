use super::{ActivityState, MAX_TRANSCRIPT_ITEMS, ToolActivityStatus, Transcript, TranscriptItem, TranscriptRole};

fn item(id: usize, role: &str, text: &str) -> TranscriptItem {
    TranscriptItem::new(id.to_string(), role.to_string(), "Activity".into(), text.to_string())
}

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
