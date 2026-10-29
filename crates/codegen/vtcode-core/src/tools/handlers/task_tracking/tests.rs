use super::*;

#[test]
fn scope_review_rows_round_trip_without_functional_verification_credit() {
    let mut metadata = TaskStepMetadata::default();
    let mut in_verify_block = false;
    for row in [
        "  files: README.md",
        "  scope-review: `git status --porcelain`",
        "  scope-review: git diff --stat HEAD -- README.md",
    ] {
        assert!(apply_metadata_line(&mut metadata, row, &mut in_verify_block));
    }
    assert_eq!(metadata.scope_review, vec!["git status --porcelain", "git diff --stat HEAD -- README.md"]);
    assert!(metadata.verify.is_empty());
    let mut markdown = String::new();
    append_task_step_metadata(&mut markdown, "", &metadata);
    assert_eq!(
        markdown,
        "  files: README.md\n  scope-review: git status --porcelain\n  scope-review: git diff --stat HEAD -- README.md\n"
    );
    let old: TaskStepMetadata = serde_json::from_value(json!({"verify": ["cargo check"]})).unwrap();
    assert!(old.scope_review.is_empty());
    assert_eq!(old.verify, vec!["cargo check"]);
    let restored: TaskStepMetadata = serde_json::from_value(serde_json::to_value(&metadata).unwrap()).unwrap();
    assert_eq!(restored, metadata);
    let previous = [TaskMetadataRef { description: "Review scope", metadata: &metadata }];
    let updated = parse_task_items(&[TaskItemInput::Text("[x] Review scope".to_string())], &previous).unwrap();
    assert_eq!(updated[0].metadata.scope_review, metadata.scope_review);
    assert!(updated[0].metadata.verify.is_empty());
    let cleared = parse_task_items(
        &[TaskItemInput::Structured(TaskItemInputObject {
            description: "Review scope".to_string(),
            scope_review: Some(Vec::new()),
            ..Default::default()
        })],
        &previous,
    )
    .unwrap();
    assert!(cleared[0].metadata.scope_review.is_empty());
}

#[test]
fn compact_tree_retains_scope_metadata_in_a_distinct_field() {
    let rows = compact_task_tree_view_from_items(&[json!({
        "description": "Review scope", "scope_review": ["git status --porcelain"],
        "verify": ["rg -n 'Overview' README.md"],
    })]);
    assert_eq!(rows[0]["scope_review"], json!(["git status --porcelain"]));
    assert_eq!(rows[0]["verify"], json!(["rg -n 'Overview' README.md"]));
}

#[test]
fn parse_files_metadata_keeps_quoted_commas_as_single_item() {
    let items = parse_files_metadata("'src/a.rs, b.rs', src/c.rs");
    assert_eq!(items, vec!["'src/a.rs, b.rs'".to_string(), "src/c.rs".to_string()]);
}

#[test]
fn apply_metadata_line_drives_verify_block_state_machine() {
    let mut metadata = TaskStepMetadata::default();
    let mut in_verify_block = false;

    // Non-metadata lines are rejected without touching state.
    assert!(!apply_metadata_line(&mut metadata, "just a description", &mut in_verify_block));
    assert!(!in_verify_block);
    assert!(metadata.verify.is_empty());

    // files:/outcome: lines are consumed directly (persisted bracket-free).
    assert!(apply_metadata_line(&mut metadata, "  files: a.rs, b.rs", &mut in_verify_block));
    assert!(apply_metadata_line(&mut metadata, "  outcome: did the thing", &mut in_verify_block));
    assert_eq!(metadata.files, vec!["a.rs".to_string(), "b.rs".to_string()]);
    assert_eq!(metadata.outcome.as_deref(), Some("did the thing"));

    // Bare `verify:` opens a block; mixed bullet markers accumulate.
    assert!(apply_metadata_line(&mut metadata, "  verify:", &mut in_verify_block));
    assert!(in_verify_block);
    assert!(apply_metadata_line(&mut metadata, "    - cargo test", &mut in_verify_block));
    assert!(apply_metadata_line(&mut metadata, "    * cargo clippy", &mut in_verify_block));
    assert_eq!(metadata.verify, vec!["cargo test".to_string(), "cargo clippy".to_string()]);

    // A non-bullet line closes the block and is itself rejected.
    assert!(!apply_metadata_line(&mut metadata, "  other: x", &mut in_verify_block));
    assert!(!in_verify_block);

    // Inline `verify:` replaces the accumulated block.
    assert!(apply_metadata_line(&mut metadata, "  verify: make check", &mut in_verify_block));
    assert_eq!(metadata.verify, vec!["make check".to_string()]);
}

#[test]
fn verify_ingestion_strips_code_span_decoration() {
    // Regression for session-vtcode-20261019T075619Z: models emit verify
    // in the taught code-span form, and a stored code span is (a) never
    // valid shell and (b) forbidden to type back at preflight, which
    // deadlocked the completion-verification gate. Ingestion must store
    // the executable command in every input shape.
    let mut metadata = TaskStepMetadata::default();
    let mut in_verify_block = false;
    assert!(apply_metadata_line(
        &mut metadata,
        "  verify: `./scripts/update-contributors-readme.sh --dry-run`",
        &mut in_verify_block
    ));
    assert_eq!(metadata.verify, vec!["./scripts/update-contributors-readme.sh --dry-run".to_string()]);
    // Multi-line block entries unwrap too.
    assert!(apply_metadata_line(&mut metadata, "  verify:", &mut in_verify_block));
    assert!(apply_metadata_line(&mut metadata, "    - `git show --stat HEAD`", &mut in_verify_block));
    assert_eq!(metadata.verify, vec!["git show --stat HEAD".to_string()]);
    // Structured API input (create/update items) unwraps as well.
    let mut structured = TaskStepMetadata::default();
    apply_metadata_input(
        &mut structured,
        Some(&["README.md".to_string()]),
        None,
        Some(&["`cargo check --locked`".to_string()]),
    );
    assert_eq!(structured.verify, vec!["cargo check --locked".to_string()]);
    // Nested/unbalanced spans are left untouched (fail-safe).
    let mut nested = TaskStepMetadata::default();
    apply_metadata_input(&mut nested, None, None, Some(&["`grep -n '`x`' f`".to_string()]));
    assert_eq!(nested.verify, vec!["`grep -n '`x`' f`".to_string()]);
}

#[test]
fn parse_marked_status_prefix_rejects_unmarked_text() {
    let parsed = parse_marked_status_prefix("plain text without marker");
    assert!(parsed.is_none());
}

#[test]
fn parse_status_prefix_defaults_to_pending_for_unmarked_text() {
    let (status, description) = parse_status_prefix("plain text without marker");
    assert_eq!(status, TaskTrackingStatus::Pending);
    assert_eq!(description, "plain text without marker");
}

#[test]
fn parse_status_prefix_supports_both_in_progress_markers() {
    let (status_tilde, text_tilde) = parse_status_prefix("[~] do thing");
    let (status_slash, text_slash) = parse_status_prefix("[/] do thing");
    assert_eq!(status_tilde, TaskTrackingStatus::InProgress);
    assert_eq!(status_slash, TaskTrackingStatus::InProgress);
    assert_eq!(text_tilde, "do thing");
    assert_eq!(text_slash, "do thing");
}

#[test]
fn append_notes_joins_with_single_newline() {
    let merged = append_notes(Some("left".to_string()), Some("right"));
    assert_eq!(merged, Some("left\nright".to_string()));
}

#[test]
fn append_notes_section_ignores_blank_notes() {
    let mut markdown = "# Title\n".to_string();
    append_notes_section(&mut markdown, Some("   "));
    assert_eq!(markdown, "# Title\n");
}

#[test]
fn bulk_sync_validation_preserves_text_and_actual_structured_items() {
    let valid = vec![
        TaskItemInput::Text(r#"[x] Document {"status":"completed"}"#.to_string()),
        TaskItemInput::Text(r#"{"example":"literal JSON"}"#.to_string()),
        TaskItemInput::Structured(TaskItemInputObject {
            description: "Original step".to_string(),
            status: Some("completed".to_string()),
            ..Default::default()
        }),
    ];
    validate_task_item_inputs(&valid).unwrap();
    for text in [
        r#"{"index":1,"status":"completed"}"#,
        r#"{"description":"Encoded step","status":"completed"}"#,
    ] {
        for prefix in ["", "[x] ", "[X] ", "[~] ", "[/] ", "[!] ", "[ ] "] {
            let item = TaskItemInput::Text(format!("  {prefix}{text}  "));
            assert!(validate_task_item_inputs(&[item]).is_err(), "prefix {prefix:?}");
        }
    }
}

#[test]
fn is_bulk_sync_update_requires_items_and_missing_single_item_fields() {
    let items = vec!["Step".to_string()];
    let no_items: Option<&[String]> = None;
    assert!(is_bulk_sync_update(Some(&items), None, None, None));
    assert!(!is_bulk_sync_update(no_items, Some(1), None, Some("completed")));
}

#[test]
fn is_bulk_sync_update_rejects_mixed_single_item_fields() {
    let items = vec!["Step".to_string()];
    let error = validate_update_shape(Some(&items), Some(1), None, Some("completed"))
        .expect_err("mixed bulk and single update fields must fail closed");

    assert!(error.to_string().contains("cannot combine 'items'"));
}

#[test]
fn task_tracker_update_rejects_both_index_forms() {
    let error = validate_update_shape::<String>(None, Some(1), Some("1"), Some("completed"))
        .expect_err("an update must select exactly one index form");

    assert!(error.to_string().contains("cannot combine 'index' and 'index_path'"));
}

#[test]
fn task_counts_tracks_progress() {
    let mut counts = TaskCounts::default();
    counts.add(&TaskTrackingStatus::Completed);
    counts.add(&TaskTrackingStatus::Pending);
    counts.add(&TaskTrackingStatus::Blocked);
    counts.add(&TaskTrackingStatus::InProgress);
    assert_eq!(counts.total, 4);
    assert_eq!(counts.completed, 1);
    assert_eq!(counts.pending, 1);
    assert_eq!(counts.blocked, 1);
    assert_eq!(counts.in_progress, 1);
    assert_eq!(counts.progress_percent(), 25);
}

#[test]
fn compact_task_tree_view_formats_flat_leaf_statuses_exactly() {
    let nodes = [
        TaskTreeNode {
            index_path: "1".to_string(),
            description: "Pending".to_string(),
            status: TaskTrackingStatus::Pending,
            metadata: TaskStepMetadata::default(),
            children: Vec::new(),
        },
        TaskTreeNode {
            index_path: "2".to_string(),
            description: "In progress".to_string(),
            status: TaskTrackingStatus::InProgress,
            metadata: TaskStepMetadata::default(),
            children: Vec::new(),
        },
        TaskTreeNode {
            index_path: "3".to_string(),
            description: "Completed".to_string(),
            status: TaskTrackingStatus::Completed,
            metadata: TaskStepMetadata::default(),
            children: Vec::new(),
        },
        TaskTreeNode {
            index_path: "4".to_string(),
            description: "Blocked".to_string(),
            status: TaskTrackingStatus::Blocked,
            metadata: TaskStepMetadata::default(),
            children: Vec::new(),
        },
    ];

    let displays = compact_task_tree_view(&nodes)
        .into_iter()
        .filter_map(|line| line.get("display").and_then(Value::as_str).map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(
        displays,
        vec![
            "  ├ □ Pending".to_string(),
            "  ├ [-] In progress".to_string(),
            "  ├ [x] Completed".to_string(),
            "  └ [!] Blocked".to_string(),
        ]
    );
}

#[test]
fn compact_task_tree_view_omits_parent_status_but_keeps_metadata_fields() {
    let nodes = [TaskTreeNode {
        index_path: "1".to_string(),
        description: "Parent".to_string(),
        status: TaskTrackingStatus::InProgress,
        metadata: TaskStepMetadata {
            files: vec!["Cargo.toml".to_string()],
            outcome: Some("Ready".to_string()),
            verify: vec!["cargo check".to_string()],
            ..Default::default()
        },
        children: vec![TaskTreeNode {
            index_path: "1.1".to_string(),
            description: "Child".to_string(),
            status: TaskTrackingStatus::Completed,
            metadata: TaskStepMetadata::default(),
            children: Vec::new(),
        }],
    }];

    let lines = compact_task_tree_view(&nodes);
    assert_eq!(lines[0]["display"], "  └ Parent");
    assert_eq!(lines[1]["display"], "    [x] Child");
    assert_eq!(lines[0]["files"], json!(["Cargo.toml"]));
    assert_eq!(lines[0]["outcome"], "Ready");
    assert_eq!(lines[0]["verify"], json!(["cargo check"]));
}

#[test]
fn split_task_description_metadata_strips_files_and_verify_suffix() {
    let (clean, files, verify) =
        split_task_description_metadata("Emit summary -> files: [src/a.rs, src/b.rs] -> verify: [cargo check]");
    assert_eq!(clean, "Emit summary");
    assert_eq!(files, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
    assert_eq!(verify, vec!["cargo check".to_string()]);
}

#[test]
fn split_task_description_metadata_keeps_inline_code_arrows_intact() {
    let (clean, files, verify) = split_task_description_metadata("Use `a -> files: b` in code -> files: [src/a.rs]");
    assert_eq!(clean, "Use `a -> files: b` in code");
    assert_eq!(files, vec!["src/a.rs".to_string()]);
    assert!(verify.is_empty());
}

#[test]
fn split_task_description_metadata_without_markers_returns_trimmed_description() {
    let (clean, files, verify) = split_task_description_metadata("  Just an action  ");
    assert_eq!(clean, "Just an action");
    assert!(files.is_empty());
    assert!(verify.is_empty());
}

#[test]
fn split_task_description_metadata_preserves_quoted_commas_in_files_and_verify() {
    let (clean, files, verify) = split_task_description_metadata(
        "Inspect range -> files: ['src/a.rs, b.rs', src/c.rs] -> verify: [sed -n '/^## A/,/^## B/p']",
    );
    assert_eq!(clean, "Inspect range");
    assert_eq!(files, vec!["'src/a.rs, b.rs'".to_string(), "src/c.rs".to_string()]);
    assert_eq!(verify, vec!["sed -n '/^## A/,/^## B/p'".to_string()]);
}

#[test]
fn compact_task_tree_view_from_items_strips_inline_metadata_into_fields() {
    let items = vec![json!({
        "index_path": "1",
        "description": "Update parser -> files: [src/a.rs] -> verify: [cargo check]",
        "status": "pending",
    })];
    let rows = compact_task_tree_view_from_items(&items);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["display"], "  └ □ Update parser");
    assert_eq!(rows[0]["text"], "Update parser");
    assert_eq!(rows[0]["files"], json!(["src/a.rs"]));
    assert_eq!(rows[0]["verify"], json!(["cargo check"]));
}

#[test]
fn short_task_description_keeps_leading_clause_only() {
    assert_eq!(
        short_task_description(
            "Add vtcode exec resume to the Commands section – document the cross-turn contract, sourced from ExecSubcommand::Resume"
        ),
        "Add vtcode exec resume to the Commands section"
    );
    assert_eq!(
        short_task_description("Update the Everyday recipes block — add a headless example"),
        "Update the Everyday recipes block"
    );
    assert_eq!(short_task_description("Emit summary -> files: [src/a.rs] -> verify: [cargo check]"), "Emit summary");
    assert_eq!(short_task_description("Verify with cargo check"), "Verify with cargo check");
    assert_eq!(short_task_description(""), "");
}

#[test]
fn compact_task_tree_view_from_items_shortens_detail_tail() {
    let items = vec![json!({
        "index_path": "1",
        "description": "Add vtcode exec resume to the Commands section – document the cross-turn contract in the command table",
        "status": "pending",
    })];
    let rows = compact_task_tree_view_from_items(&items);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["display"], "  └ □ Add vtcode exec resume to the Commands section");
    assert_eq!(rows[0]["text"], "Add vtcode exec resume to the Commands section");
}

#[test]
fn shared_schema_helpers_stay_actionable_and_drift_free() {
    assert_eq!(TASK_STATUS_VALUES, ["pending", "in_progress", "completed", "blocked"]);

    let status = task_status_schema();
    assert_eq!(status["type"], "string");
    assert_eq!(status["enum"], json!(TASK_STATUS_VALUES));
    assert!(status.get("description").is_none());

    let with_desc = task_status_property_schema("New status.");
    assert_eq!(with_desc["description"], "New status.");
    assert_eq!(with_desc["enum"], json!(TASK_STATUS_VALUES));

    // Asymmetric: plain string item vs structured item with metadata.
    let items_schema = task_items_array_schema("Items desc.");
    let validator = jsonschema::validator_for(&items_schema).expect("items schema is valid");
    assert!(validator.validate(&json!(["plain step"])).is_ok());
    assert!(
        validator
            .validate(&json!([{
                "description": "Structured step",
                "status": "in_progress",
                "files": ["src/a.rs"],
                "outcome": "Done",
                "verify": ["cargo check"],
            }]))
            .is_ok()
    );
    // Boundary: verify also accepts a bare string; missing description fails.
    assert!(validator.validate(&json!([{"status": "completed"}])).is_err());
    assert!(validator.validate(&json!([{"description": "Bad", "status": "done"}])).is_err());

    assert_eq!(task_files_property_schema("F.")["description"], "F.");
    assert_eq!(task_verify_property_schema("V.")["description"], "V.");
}

#[test]
fn shared_status_values_match_runtime_parser() {
    use std::str::FromStr;
    for status in TASK_STATUS_VALUES {
        assert!(TaskTrackingStatus::from_str(status).is_ok(), "schema status {status:?} must parse");
    }
    assert!(TaskTrackingStatus::from_str("done").is_err());
}

/// Drift guard: `TaskTrackingStatus` reaches the wire through three
/// independent paths — the derived serde `snake_case` form, `as_str()`, and
/// the hand-written `FromStr`. Nothing in the type system ties them
/// together, so this locks the as_str <-> serde <-> FromStr triangle for
/// every variant and pins the schema enum list to the same source. A
/// one-sided edit to any path fails here instead of silently producing a
/// value the runtime cannot parse back.
#[test]
fn task_status_as_str_serde_and_from_str_stay_in_lockstep() {
    use std::str::FromStr;
    let variants = [
        TaskTrackingStatus::Pending,
        TaskTrackingStatus::InProgress,
        TaskTrackingStatus::Completed,
        TaskTrackingStatus::Blocked,
    ];
    let strings: Vec<&str> = variants.iter().map(TaskTrackingStatus::as_str).collect();
    assert_eq!(strings, TASK_STATUS_VALUES);

    for variant in variants {
        let expected = variant.as_str();
        assert_eq!(
            serde_json::to_value(variant).expect("serializes"),
            json!(expected),
            "serde form drifted for {variant:?}"
        );
        assert_eq!(
            TaskTrackingStatus::from_str(expected).expect("as_str output must parse back"),
            variant,
            "FromStr drift for {variant:?}"
        );
    }
}

/// Cross-crate drift guard. `vtcode-commons::ui_protocol::TaskItemStatus`
/// (the TUI-facing row status) is a distinct type from
/// `TaskTrackingStatus` but must carry the *same* canonical strings — its
/// doc comment promises they match, and the TUI parses runtime strings
/// through it (`raw.parse::<TaskItemStatus>()`). The type system does not
/// connect the two enums, so pin them token-for-token here.
#[test]
fn task_item_status_matches_tracking_status_strings() {
    use std::str::FromStr;
    use vtcode_commons::ui_protocol::TaskItemStatus;

    for variant in [
        TaskTrackingStatus::Pending,
        TaskTrackingStatus::InProgress,
        TaskTrackingStatus::Completed,
        TaskTrackingStatus::Blocked,
    ] {
        let canonical = variant.as_str();
        let display = TaskItemStatus::from_str(canonical)
            .unwrap_or_else(|_| panic!("TaskItemStatus must parse runtime status {canonical:?}"));
        assert_eq!(display.as_str(), canonical, "TaskItemStatus/TaskTrackingStatus string drift for {variant:?}");
    }
}
