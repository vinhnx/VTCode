use super::*;

#[test]
fn tracker_header_replaces_humanized_codename_with_first_task() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Jolly Forest",
            "items": [
                { "index_path": "1", "description": "Add vtcode exec resume to the Commands section – document the contract", "status": "pending" },
                { "index_path": "2", "description": "Verify links", "status": "pending" },
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);
    assert_eq!(rows, vec!["• Add vtcode exec resume to the Commands section 0/2"]);
    assert!(!rows[0].contains("Jolly"));

    let metadata = tracker_panel_metadata(&payload).expect("metadata");
    assert_eq!(metadata.title, "Add vtcode exec resume to the Commands section");
}
#[test]
fn tracker_header_keeps_user_title_verbatim() {
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                { "index_path": "1", "description": "Investigate", "status": "pending" },
            ]
        }
    });

    let rows = tracker_progress_lines(&payload);
    assert_eq!(rows, vec!["• Release 0/1"]);
}
#[test]
fn tracker_tree_body_lines_strip_metadata_but_keep_task_titles() {
    // Parents summarize their children, so showing their stored leaf status
    // would be misleading. Metadata remains in the structured payload but
    // must not turn into visible detail rows.
    let payload = json!({
        "status": "updated",
        "checklist": {
            "title": "Release",
            "items": [
                {
                    "index_path": "1",
                    "level": 0,
                    "description": "Prepare release",
                    "status": "in_progress",
                    "files": ["Cargo.toml"],
                    "outcome": "Version is ready",
                    "verify": ["cargo nextest run -p vtcode"]
                },
                { "index_path": "1.1", "level": 1, "description": "Update version", "status": "completed" },
                { "index_path": "1.2", "level": 1, "description": "Run checks", "status": "in_progress" },
                { "index_path": "2", "level": 0, "description": "Publish", "status": "pending" }
            ]
        }
    });

    let rows = tracker_tree_body_lines(&payload);

    assert_eq!(
        rows,
        vec![
            "  ├ Prepare release",
            "  │ Update version",
            "  │ Run checks",
            "  └ Publish",
        ]
    );
    assert!(
        rows.iter()
            .all(|line| { !line.contains("files:") && !line.contains("outcome:") && !line.contains("verify:") })
    );
    assert_eq!(payload["checklist"]["items"][0]["files"], json!(["Cargo.toml"]));
    assert_eq!(payload["checklist"]["items"][0]["outcome"], "Version is ready");
    assert_eq!(payload["checklist"]["items"][0]["verify"], json!(["cargo nextest run -p vtcode"]));
    let metadata = tracker_panel_metadata(&payload).expect("structured panel metadata");
    assert_eq!(metadata.title, "Release");
    assert_eq!((metadata.completed, metadata.total), (1, 4));
}
#[test]
fn tracker_progress_lines_keep_diagnostics_for_empty_or_malformed_tracker_responses() {
    // Compact rendering applies only to successful structured checklists.
    // Empty and malformed responses must remain diagnosable instead of
    // silently presenting a blank task panel.
    let empty = json!({});
    let malformed = json!({
        "status": "error",
        "message": "Tracker response did not include checklist items.",
        "view": { "lines": "not an array" }
    });
    let malformed_items = json!({
        "status": "error",
        "message": "Tracker response contained invalid checklist items.",
        "checklist": { "items": [{}] }
    });

    assert!(tracker_progress_lines(&empty).is_empty());
    assert_eq!(
        tracker_progress_lines(&malformed),
        vec![
            "• Tasks",
            "  Tracker status: error",
            "  Update: Tracker response did not include checklist items.",
        ]
    );
    assert_eq!(
        tracker_progress_lines(&malformed_items),
        vec![
            "• Tasks",
            "  Tracker status: error",
            "  Update: Tracker response contained invalid checklist items.",
        ]
    );

    let partial_failure = json!({
        "status": "error",
        "message": "Tracker response was only partially applied.",
        "checklist": {
            "items": [
                { "index": 1, "description": "Still present", "status": "completed" }
            ]
        }
    });
    assert_eq!(
        tracker_progress_lines(&partial_failure),
        vec![
            "• Tasks",
            "  Tracker status: error",
            "  Update: Tracker response was only partially applied.",
        ]
    );
    // Failed updates stay diagnosable in the transcript; remaining checklist
    // rows remain available on the panel body path.
    assert_eq!(tracker_tree_body_lines(&partial_failure), vec!["  └ Still present"]);
}
