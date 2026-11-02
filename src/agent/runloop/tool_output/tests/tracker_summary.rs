use super::*;

#[test]
fn tracker_summary_lines_hide_successful_tracker_details() {
    let payload = json!({
        "status": "updated",
        "message": "Item 2 status changed: pending -> in_progress",
        "checklist": {
            "total": 4,
            "completed": 1,
            "in_progress": 2,
            "pending": 1,
            "blocked": 0,
            "progress_percent": 25,
            "items": [
                { "index": 1, "description": "A", "status": "completed" },
                { "index": 2, "description": "B", "status": "in_progress" },
                { "index": 3, "description": "C", "status": "in_progress" },
                { "index": 4, "description": "D", "status": "pending" }
            ]
        }
    });

    assert!(tracker_summary_lines(&payload).is_empty());
}
#[test]
fn tracker_summary_lines_still_show_message_without_checklist() {
    let payload = json!({
        "status": "empty",
        "message": "No active checklist."
    });
    let lines = tracker_summary_lines(&payload);
    assert!(lines.iter().any(|line| line == "  Tracker status: empty"));
    assert!(lines.iter().any(|line| line == "  Update: No active checklist."));
}
#[test]
fn tracker_progress_lines_keep_counts_when_tree_body_is_empty() {
    // Explicit completed/total without renderable step titles still answers
    // "how far along is this work?" on the user-facing surface.
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "completed": 2,
            "total": 5,
            "items": []
        }
    });

    let rows = tracker_progress_lines(&payload);

    assert_eq!(rows, vec!["• Release 2/5"]);
    assert!(tracker_tree_body_lines(&payload).is_empty());
}
#[test]
fn tracker_progress_lines_show_title_and_progress_only() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate", "status": "pending" },
                { "index_path": "2", "description": "Implement", "status": "in_progress" },
                { "index_path": "3", "description": "Verify", "status": "completed" },
                { "index_path": "4", "description": "Resolve dependency", "status": "blocked" }
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);

    assert_eq!(rows, vec!["• Release 1/4"]);
    assert!(rows.iter().all(|row| !row.contains("next:")));
    assert!(rows.iter().all(|row| !row.contains("├") && !row.contains("└")));
}
#[test]
fn tracker_tree_body_lines_are_panel_only_compact_tree() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate", "status": "pending" },
                { "index_path": "2", "description": "Implement", "status": "in_progress" },
                { "index_path": "3", "description": "Verify", "status": "completed" },
                { "index_path": "4", "description": "Resolve dependency", "status": "blocked" }
            ]
        }
    });

    let rows = tracker_tree_body_lines(&payload);

    assert_eq!(
        rows,
        vec![
            "  ├ Investigate",
            "  ├ Implement",
            "  ├ Verify",
            "  └ Resolve dependency",
        ]
    );
    assert!(rows.iter().all(|row| !row.starts_with("• ")));
    assert!(
        rows.iter()
            .all(|row| !row.contains("□") && !row.contains("[x]") && !row.contains("[-]")),
        "status surfaces through styling, not glyphs: {rows:?}"
    );
    assert_eq!(
        tracker_panel_rows(&payload),
        (
            vec![
                "  ├ Investigate".to_string(),
                "  ├ Implement".to_string(),
                "  ├ Verify".to_string(),
                "  └ Resolve dependency".to_string(),
            ],
            vec![
                TaskItemStatus::Pending,
                TaskItemStatus::InProgress,
                TaskItemStatus::Completed,
                TaskItemStatus::Blocked,
            ],
            Some(1),
        )
    );
}
#[test]
fn humanize_tracker_title_strips_timestamp_prefix_from_generated_ids() {
    assert_eq!(humanize_tracker_title("1789108823046-kind-lagoon"), "Kind Lagoon");
    assert_eq!(humanize_tracker_title("1789108823046-kind_lagoon"), "Kind Lagoon");
}
#[test]
fn humanize_tracker_title_keeps_user_titles_verbatim() {
    assert_eq!(humanize_tracker_title("Release"), "Release");
    assert_eq!(humanize_tracker_title("2024-report"), "2024-report");
    assert_eq!(humanize_tracker_title("  Task tracker  "), "Task tracker");
}
#[test]
fn tracker_progress_lines_humanize_generated_title_without_raw_slug() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "1789108823046-kind-lagoon",
            "items": [
                { "index_path": "1", "description": "Investigate", "status": "pending" },
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);

    assert_eq!(rows, vec!["• Investigate 0/1"]);
    assert!(!rows[0].contains("1789108823046"));
    assert!(!rows[0].contains("Lagoon"));
    assert!(!rows[0].contains("next:"));
}
#[test]
fn tracker_progress_lines_prefer_explicit_counts_over_derived() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "1789108823046-kind-lagoon",
            "completed": 2,
            "total": 5,
            "items": [
                { "index_path": "1", "description": "Done one", "status": "completed" },
                { "index_path": "2", "description": "Done two", "status": "completed" },
                { "index_path": "3", "description": "Audit heavy-crate linkage", "status": "pending" },
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);

    assert_eq!(rows, vec!["• Done one 2/5"]);
    assert!(!rows[0].contains("1789108823046"));
    assert!(!rows[0].contains("Lagoon"));
    assert!(!rows[0].contains("Audit heavy-crate linkage"));
}
