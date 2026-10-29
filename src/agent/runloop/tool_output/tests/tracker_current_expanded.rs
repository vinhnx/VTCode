use super::*;

#[test]
fn tracker_transcript_lines_compact_shows_header_plus_current_task() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate cache miss", "status": "completed" },
                { "index_path": "2", "description": "Defer eager setup", "status": "in_progress" },
                { "index_path": "3", "description": "Verify with cargo check", "status": "pending" },
            ]
        }
    });

    let rows = tracker_transcript_lines(&payload, false);

    assert_eq!(transcript_texts(&rows), vec!["• Release 1/3", "  » Defer eager setup"]);
    assert_eq!(transcript_statuses(&rows), vec![None, Some(TaskItemStatus::InProgress)]);
    assert!(rows.iter().all(|row| !row.text.contains("[-]") && !row.text.contains("□")));
}
#[test]
fn tracker_panel_and_transcript_focus_actionable_leaf_before_parent() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Prepare release", "status": "in_progress" },
                { "index_path": "1.1", "description": "Update version", "status": "completed" },
                { "index_path": "1.2", "description": "Run checks", "status": "pending" },
                { "index_path": "2", "description": "Publish", "status": "blocked" }
            ]
        }
    });

    let (texts, statuses, current) = tracker_panel_rows(&payload);
    assert_eq!(current, Some(2));
    assert!(texts[2].ends_with("Run checks"));
    assert_eq!(statuses[2], TaskItemStatus::Pending);
    assert_eq!(
        tracker_current_tree_row(&payload),
        Some(TrackerLine::row("  » Run checks".to_string(), TaskItemStatus::Pending))
    );
}
#[test]
fn tracker_current_tree_row_prefers_in_progress_over_pending_and_blocked() {
    // Asymmetric statuses: pending comes first in document order, but the
    // in-progress leaf later must win as current.
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Verify with cargo check", "status": "pending" },
                { "index_path": "2", "description": "Defer eager setup", "status": "in_progress" },
                { "index_path": "3", "description": "Resolve dependency", "status": "blocked" },
            ]
        }
    });

    assert_eq!(
        tracker_current_tree_row(&payload),
        Some(TrackerLine::row("  » Defer eager setup".to_string(), TaskItemStatus::InProgress))
    );
}
#[test]
fn tracker_current_tree_row_falls_back_to_pending_then_blocked() {
    let pending_only = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Verify with cargo check", "status": "completed" },
                { "index_path": "2", "description": "Defer eager setup", "status": "pending" },
            ]
        }
    });
    assert_eq!(
        tracker_current_tree_row(&pending_only),
        Some(TrackerLine::row("  » Defer eager setup".to_string(), TaskItemStatus::Pending))
    );

    let blocked_only = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Verify with cargo check", "status": "completed" },
                { "index_path": "2", "description": "Resolve dependency", "status": "blocked" },
            ]
        }
    });
    assert_eq!(
        tracker_current_tree_row(&blocked_only),
        Some(TrackerLine::row("  » Resolve dependency".to_string(), TaskItemStatus::Blocked))
    );
}
#[test]
fn tracker_current_tree_row_stays_header_only_when_all_completed_or_empty() {
    let all_done = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate cache miss", "status": "completed" },
                { "index_path": "2", "description": "Defer eager setup", "status": "completed" },
            ]
        }
    });
    assert_eq!(tracker_current_tree_row(&all_done), None);
    assert_eq!(tracker_transcript_lines(&all_done, false), vec![TrackerLine::plain("• Release 2/2".to_string())]);

    let empty = json!({
        "status": "updated",
        "checklist": { "title": "Release", "completed": 0, "total": 0, "items": [] }
    });
    assert_eq!(tracker_current_tree_row(&empty), None);
}
#[test]
fn tracker_current_row_marker_is_detected() {
    assert!(is_tracker_current_row("  » Defer eager setup"));
    assert!(is_tracker_current_row("  » Verify with cargo check"));
    assert!(!is_tracker_current_row("  ├ Defer eager setup"));
    assert!(!is_tracker_current_row("• Release 1/3"));
}
#[test]
fn tracker_glyph_strip_keeps_mid_string_brackets_and_branches() {
    // Only the leading status glyph is stripped; bracket text inside the
    // description and the branch structure survive verbatim.
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Fix [-] flag handling", "status": "pending" },
                { "index_path": "2", "description": "Ship [x] marked docs", "status": "completed" },
            ]
        }
    });

    let rows = tracker_transcript_lines(&payload, true);

    assert_eq!(
        transcript_texts(&rows),
        vec!["• Release 1/2", "  ├ Fix [-] flag handling", "  └ Ship [x] marked docs"]
    );
    assert_eq!(
        transcript_statuses(&rows),
        vec![None, Some(TaskItemStatus::Pending), Some(TaskItemStatus::Completed)]
    );
}
#[test]
fn tracker_transcript_lines_expanded_shows_each_task_item() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate cache miss", "status": "completed" },
                { "index_path": "2", "description": "Defer eager setup", "status": "in_progress" },
                { "index_path": "3", "description": "Verify with cargo check", "status": "pending" },
            ]
        }
    });

    let rows = tracker_transcript_lines(&payload, true);

    assert_eq!(transcript_texts(&rows)[0], "• Release 1/3");
    assert_eq!(
        transcript_texts(&rows)[1..],
        [
            "  ├ Investigate cache miss",
            "  ├ Defer eager setup",
            "  └ Verify with cargo check",
        ]
    );
    assert_eq!(
        transcript_statuses(&rows),
        vec![
            None,
            Some(TaskItemStatus::Completed),
            Some(TaskItemStatus::InProgress),
            Some(TaskItemStatus::Pending),
        ]
    );
}
#[test]
fn tracker_transcript_lines_expanded_truncates_large_checklists() {
    let items: Vec<serde_json::Value> = (1..=(TRACKER_TRANSCRIPT_MAX_ROWS + 5))
        .map(|index| {
            json!({
                "index_path": index.to_string(),
                "description": format!("Distinct task {index}"),
                "status": if index == 1 { "completed" } else { "pending" },
            })
        })
        .collect();
    let payload = json!({
        "status": "updated",
        "checklist": { "title": "Release", "items": items }
    });

    let rows = tracker_transcript_lines(&payload, true);

    assert_eq!(rows.len(), 1 + TRACKER_TRANSCRIPT_MAX_ROWS + 1);
    assert_eq!(rows[0].text, format!("• Release 1/{}", TRACKER_TRANSCRIPT_MAX_ROWS + 5));
    assert_eq!(rows[0].status, None);
    assert!(rows[1].text.contains("Distinct task 1"));
    assert_eq!(rows[1].status, Some(TaskItemStatus::Completed));
    assert!(
        rows[TRACKER_TRANSCRIPT_MAX_ROWS]
            .text
            .contains(format!("Distinct task {TRACKER_TRANSCRIPT_MAX_ROWS}").as_str())
    );
    assert_eq!(rows[TRACKER_TRANSCRIPT_MAX_ROWS + 1].text, "  … 5 more");
    assert_eq!(rows[TRACKER_TRANSCRIPT_MAX_ROWS + 1].status, None);
    assert!(
        rows.iter()
            .all(|row| !row.text.contains("Distinct task 31") || row.text.starts_with("  …"))
    );
}
#[test]
fn tracker_transcript_lines_expanded_keeps_diagnostics_without_tree() {
    let payload = json!({
        "status": "error",
        "message": "Tracker response was only partially applied.",
        "checklist": {
            "items": [
                { "index": 1, "description": "Still present", "status": "completed" }
            ]
        }
    });

    let rows = tracker_transcript_lines(&payload, true);

    assert_eq!(
        transcript_texts(&rows),
        vec![
            "• Tasks",
            "  Tracker status: error",
            "  Update: Tracker response was only partially applied.",
        ]
    );
    assert!(rows.iter().all(|row| row.status.is_none()));
    assert!(rows.iter().all(|row| !row.text.contains("Still present")));
}
