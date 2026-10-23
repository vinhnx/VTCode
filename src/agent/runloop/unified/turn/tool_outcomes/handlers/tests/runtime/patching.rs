use super::*;

#[tokio::test]
async fn read_after_write_guard_blocks_bare_full_read_of_written_file() {
    let mut backing = TestContextBacking::new(4).await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state.record_written_file(&sample_path);

    let args = json!({ "path": sample_path });
    let outcome = enforce_read_after_write_guard(&mut ctx, "bare_read_after_write", tool_names::READ_FILE, &args);
    assert!(
        matches!(outcome, Some(ValidationResult::Blocked)),
        "a bare full read of a just-written file must stay blocked"
    );
    assert!(
        ctx.working_history
            .iter()
            .any(|message| message.content.as_text().contains("was just written")),
        "the block must carry the diff-preview guidance"
    );
}

#[tokio::test]
async fn read_after_write_guard_admits_bounded_slice_read_of_written_file() {
    let mut backing = TestContextBacking::new(4).await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state.record_written_file(&sample_path);

    // The block message directs the model to "specify offset/limit for a
    // specific range" — that advice must be truthful, so a bounded slice
    // read passes. Repeated slice reads stay bounded by the family and
    // per-file-path caps enforced after this guard.
    let args = json!({ "path": sample_path, "offset": 1, "limit": 10 });
    let outcome = enforce_read_after_write_guard(&mut ctx, "slice_read_after_write", tool_names::READ_FILE, &args);
    assert!(outcome.is_none(), "a read with an explicit offset/limit slice must be admitted");

    let raw_flagged_args = json!({ "path": sample_path, "raw": true });
    let raw_outcome =
        enforce_read_after_write_guard(&mut ctx, "raw_only_read_after_write", tool_names::READ_FILE, &raw_flagged_args);
    assert!(
        matches!(raw_outcome, Some(ValidationResult::Blocked)),
        "a raw flag without a slice is an uncondensed full re-read and must stay blocked"
    );
}

#[tokio::test]
async fn read_after_write_guard_ignores_unwritten_paths_and_non_read_tools() {
    let mut backing = TestContextBacking::new(4).await;
    let sample_path = backing.sample_file.to_string_lossy().to_string();
    let mut ctx = backing.turn_processing_context();

    // No write recorded yet: even a bare read passes.
    let unwritten_args = json!({ "path": sample_path });
    let outcome = enforce_read_after_write_guard(&mut ctx, "read_unwritten", tool_names::READ_FILE, &unwritten_args);
    assert!(outcome.is_none(), "reads of paths not written this turn must pass");

    // Written path but a non-read action on the unified file tool: not the
    // guard's concern.
    ctx.harness_state.record_written_file(&sample_path);
    let mutating_args = json!({ "action": "write", "path": sample_path, "content": "x" });
    let non_read_outcome =
        enforce_read_after_write_guard(&mut ctx, "write_after_write", tool_names::UNIFIED_FILE, &mutating_args);
    assert!(non_read_outcome.is_none(), "non-read actions must not trip the read-after-write guard");
}

#[tokio::test]
async fn successful_apply_patch_arms_read_after_write_guard_for_patch_targets() {
    // The model-facing write surface is apply_patch with `input`-only args
    // (the schema has no `path` field), so arming must come from the parsed
    // patch targets, and the workspace-relative target must match an
    // absolute-path read of the same file.
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, (1..=4).map(|idx| format!("line {idx}\n")).collect::<String>())
        .expect("rewrite sample file");
    let sample_path = sample_file.to_string_lossy().to_string();
    let patch_args = json!({
        "input": "*** Begin Patch\n*** Update File: sample.txt\n@@\n-line 1\n+edited line 1\n*** End Patch\n"
    });
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch_args, PermissionGrant::Permanent).await;
    let read_args = json!({ "path": sample_path });
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &read_args, PermissionGrant::Permanent).await;
    let slice_args = json!({ "path": sample_path, "offset": 2, "limit": 2 });
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &slice_args, PermissionGrant::Permanent).await;

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    handle_single_tool_call(&mut outcome_ctx, "patch_arm", tool_names::APPLY_PATCH, patch_args)
        .await
        .expect("patch should execute");
    assert_eq!(
        std::fs::read_to_string(&sample_file)
            .expect("read patched fixture")
            .lines()
            .next(),
        Some("edited line 1"),
        "patch must succeed for the arming path to be exercised"
    );

    let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
    handle_single_tool_call(&mut outcome_ctx, "bare_read_after_patch", tool_names::READ_FILE, read_args)
        .await
        .expect("a blocked read still produces a tool response");
    assert_eq!(
        outcome_ctx.ctx.tool_registry.execution_history_len(),
        history_len,
        "a bare full read of the patched file must not execute"
    );
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("was just written")),
        "the block must carry the diff-preview guidance"
    );

    handle_single_tool_call(&mut outcome_ctx, "slice_read_after_patch", tool_names::READ_FILE, slice_args)
        .await
        .expect("a bounded slice read of the patched file should execute");
    assert!(
        outcome_ctx.ctx.tool_registry.execution_history_len() > history_len,
        "the slice-read exemption must apply to patch-armed targets too"
    );
}

#[tokio::test]
async fn failed_apply_patch_does_not_arm_read_after_write_guard() {
    // A failed patch wrote nothing, so there is no diff preview to reuse and
    // plain reads must stay available for inspecting the failure.
    let mut backing = TestContextBacking::new(4).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    let sample_path = sample_file.to_string_lossy().to_string();
    let patch_args = json!({ "input": "not a patch at all" });
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch_args, PermissionGrant::Permanent).await;
    let read_args = json!({ "path": sample_path });
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &read_args, PermissionGrant::Permanent).await;

    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };

    handle_single_tool_call(&mut outcome_ctx, "failed_patch", tool_names::APPLY_PATCH, patch_args)
        .await
        .expect("a malformed patch returns an error response");
    assert!(
        !outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("was just written")),
        "a failed patch must not produce a read-after-write block"
    );

    let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
    handle_single_tool_call(&mut outcome_ctx, "read_after_failed_patch", tool_names::READ_FILE, read_args)
        .await
        .expect("read after failed patch should execute");
    assert!(
        outcome_ctx.ctx.tool_registry.execution_history_len() > history_len,
        "reads must stay available after a failed mutation"
    );
}

#[tokio::test]
async fn patch_context_mismatch_gets_one_fresh_read_after_path_cap_then_corrected_edit() {
    assert_mismatch_recovery_at_read_cap(false).await;
}

#[tokio::test]
async fn literal_mismatch_gets_one_fresh_read_after_path_cap_then_corrected_edit() {
    assert_mismatch_recovery_at_read_cap(true).await;
}

async fn assert_mismatch_recovery_at_read_cap(literal: bool) {
    let mut backing = TestContextBacking::new(30).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, "old evidence\nsecond line\nthird line\n").expect("fixture");
    let sample_path = sample_file.to_string_lossy().to_string();
    let read_args = json!({"path":sample_path, "offset":1, "limit":2});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &read_args, PermissionGrant::Permanent).await;
    let bad_patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-vtcode.toml provider credentials\n+edited\n*** End Patch\n"});
    let failure_tool = if literal {
        tool_names::EDIT_FILE
    } else {
        tool_names::APPLY_PATCH
    };
    let bad_patch = if literal {
        json!({"path":"sample.txt", "old_str":"vtcode.toml provider credentials", "new_str":"edited"})
    } else {
        bad_patch
    };
    cache_tool_permission(&mut backing, failure_tool, &bad_patch, PermissionGrant::Permanent).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    handle_single_tool_call(&mut outcome_ctx, "initial_read", tool_names::READ_FILE, read_args.clone())
        .await
        .expect("seed cache");
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("old evidence"))
    );
    // Fill the path counter without tripping the independent identical-slice guard.
    for _ in 1..6 {
        outcome_ctx.ctx.harness_state.record_file_read_path_call(sample_path.clone());
    }
    // Simulate external file changes; a replay would return the previous text.
    std::fs::write(&sample_file, "fresh evidence\nsecond line\nthird line\n").expect("external update");
    // Accept the disk version in the conflict monitor only. Retain the stale
    // tool-result cache so this fixture isolates uncached mismatch recovery.
    outcome_ctx
        .ctx
        .tool_registry
        .edited_file_monitor_ref()
        .accept_disk_version(&sample_file)
        .await
        .unwrap();
    handle_single_tool_call(&mut outcome_ctx, "mismatched_patch", failure_tool, bad_patch.clone())
        .await
        .expect("typed patch mismatch");
    assert_eq!(std::fs::read_to_string(&sample_file).unwrap(), "fresh evidence\nsecond line\nthird line\n");
    assert!(
        outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::READ_FILE, &read_args)
    );
    let history_start = outcome_ctx.ctx.working_history.len();
    let mut source_call = uni::Message::assistant(String::new());
    source_call.tool_calls = Some(vec![uni::ToolCall::function(
        "fresh_recovery_read".into(),
        tool_names::READ_FILE.into(),
        read_args.to_string(),
    )]);
    outcome_ctx.ctx.working_history.push(source_call);
    handle_single_tool_call(&mut outcome_ctx, "fresh_recovery_read", tool_names::READ_FILE, read_args.clone())
        .await
        .expect("one read beyond cap");
    let outputs = outcome_ctx.ctx.working_history[history_start..]
        .iter()
        .map(|message| message.content.as_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(outputs.contains("fresh evidence"), "{outputs}");
    assert!(!outputs.contains("old evidence"), "cached evidence must not be reused: {outputs}");
    assert!(!outputs.contains("reused_recent_result"), "{outputs}");
    assert!(
        !outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::READ_FILE, &read_args)
    );
    // Another failure on the same path must not replenish the allowance.
    handle_single_tool_call(&mut outcome_ctx, "repeated_mismatch", failure_tool, bad_patch)
        .await
        .expect("second mismatch");
    assert!(
        !outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::READ_FILE, &read_args)
    );
    let before_block = outcome_ctx.ctx.tool_registry.execution_history_len();
    handle_single_tool_call(&mut outcome_ctx, "second_recovery_read", tool_names::READ_FILE, read_args)
        .await
        .expect("read cap rejection");
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), before_block);
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("repeated_read_path"))
    );
    outcome_ctx
        .ctx
        .tool_registry
        .reconcile_source_inspection(outcome_ctx.ctx.working_history);
    let corrected = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-fresh evidence\n+edited evidence\n*** End Patch\n"});
    let corrected = if literal {
        json!({"path":"sample.txt", "old_str":"fresh evidence", "new_str":"edited evidence"})
    } else {
        corrected
    };
    handle_single_tool_call(&mut outcome_ctx, "corrected_patch", failure_tool, corrected)
        .await
        .expect("corrected edit");
    assert_eq!(std::fs::read_to_string(&sample_file).unwrap(), "edited evidence\nsecond line\nthird line\n");
    assert!(!outcome_ctx.turn_modified_files.is_empty(), "successful mutation must be reported");
    assert!(!outcome_ctx.ctx.is_recovery_active());
}

#[test]
fn patch_preflight_correction_names_canonical_json_input() {
    for (name, error) in [
        (tool_names::APPLY_PATCH, "Missing required argument: input"),
        (
            tool_names::APPLY_PATCH,
            "Tool preflight validation failed for 'apply_patch': Missing required argument: patch",
        ),
        (tool_names::EXEC_COMMAND, "apply_patch is a tool, not a shell executable"),
    ] {
        let correction = preflight_schema_correction(name, error);
        assert!(correction.contains(r#"{"input":"*** Begin Patch\n"#));
        assert!(correction.contains("retry once"));
        assert!(correction.contains("do not run apply_patch in a shell"));
    }
}

#[tokio::test]
async fn patch_context_mismatch_allows_one_uncached_shell_range_after_six_reads() {
    let mut backing = TestContextBacking::new(30).await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, "old shell evidence\nsecond\nthird\nfourth\nfifth\nsixth\n").unwrap();
    let sample_path = sample_file.to_string_lossy().to_string();
    let patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-vtcode.toml credentials\n+edited\n*** End Patch\n"});
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch, PermissionGrant::Permanent).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    for index in 1..=6 {
        handle_single_tool_call(
            &mut outcome_ctx,
            &format!("shell_page_{index}"),
            tool_names::EXEC_COMMAND,
            json!({"cmd":format!("sed -n '{index},{index}p' {sample_path}")}),
        )
        .await
        .unwrap();
    }
    std::fs::write(&sample_file, "fresh shell evidence\nsecond\nthird\nfourth\nfifth\nsixth\n").unwrap();
    handle_single_tool_call(&mut outcome_ctx, "shell_patch_mismatch", tool_names::APPLY_PATCH, patch)
        .await
        .unwrap();
    let args = json!({"cmd":format!("sed -n '1,1p' {sample_path}")});
    assert!(
        outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::EXEC_COMMAND, &args)
    );
    // Cached identical reads can already be at the family boundary when a
    // mismatch asks for this same slice again. The one fresh read covers both caps.
    for _ in 0..3 {
        outcome_ctx
            .ctx
            .harness_state
            .record_file_read_family_call(format!("unified_exec::read::{sample_path}::off=1::lim=1"));
    }
    let start = outcome_ctx.ctx.working_history.len();
    let mut source_call = uni::Message::assistant(String::new());
    source_call.tool_calls = Some(vec![uni::ToolCall::function(
        "fresh_shell_range".into(),
        tool_names::EXEC_COMMAND.into(),
        args.to_string(),
    )]);
    outcome_ctx.ctx.working_history.push(source_call);
    handle_single_tool_call(&mut outcome_ctx, "fresh_shell_range", tool_names::EXEC_COMMAND, args.clone())
        .await
        .unwrap();
    let outputs = outcome_ctx.ctx.working_history[start..]
        .iter()
        .map(|message| message.content.as_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(outputs.contains("fresh shell evidence"), "{outputs}");
    assert!(!outputs.contains("old shell evidence"), "{outputs}");
    assert!(!outputs.contains("repeated_read_path"), "{outputs}");
    assert!(
        !outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::EXEC_COMMAND, &args)
    );
    outcome_ctx
        .ctx
        .tool_registry
        .reconcile_source_inspection(outcome_ctx.ctx.working_history);
    let patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-fresh shell evidence\n+edited shell evidence\n*** End Patch\n"});
    handle_single_tool_call(&mut outcome_ctx, "fixed_shell_patch", tool_names::APPLY_PATCH, patch)
        .await
        .unwrap();
    assert!(
        std::fs::read_to_string(&sample_file)
            .unwrap()
            .starts_with("edited shell evidence\n")
    );
}

#[tokio::test]
async fn patch_recovery_path_cap_exception_is_reserved_before_batch_execution() {
    for prior_reads in [5, 6] {
        let mut backing = TestContextBacking::new(30).await;
        backing.select_build_primary_agent();
        let sample_path = backing.sample_file.to_string_lossy().to_string();
        let bad_patch = json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-missing context\n+edited\n*** End Patch\n"});
        cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &bad_patch, PermissionGrant::Permanent).await;
        let read_args = json!({"path":sample_path, "offset":1, "limit":2});
        cache_tool_permission(&mut backing, tool_names::READ_FILE, &read_args, PermissionGrant::Permanent).await;
        let mut repeated_tool_attempts = LoopTracker::new();
        let mut turn_modified_files = BTreeSet::new();
        let mut ctx = backing.turn_processing_context();
        let mut outcome_ctx = ToolOutcomeContext {
            ctx: &mut ctx,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
        };
        handle_single_tool_call(&mut outcome_ctx, "mismatch", tool_names::APPLY_PATCH, bad_patch)
            .await
            .expect("typed mismatch");
        for _ in 0..prior_reads {
            outcome_ctx.ctx.harness_state.record_file_read_path_call(sample_path.clone());
        }
        let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
        let first = validate_tool_call(outcome_ctx.ctx, "first_batch_read", tool_names::READ_FILE, &read_args)
            .await
            .expect("first batch admission");
        assert!(matches!(first, ValidationResult::Proceed(_)), "prior reads: {prior_reads}");
        let second = validate_tool_call(
            outcome_ctx.ctx,
            "second_batch_read",
            tool_names::READ_FILE,
            &json!({"path":sample_path, "offset":3, "limit":2}),
        )
        .await
        .expect("second batch admission");
        assert!(matches!(second, ValidationResult::ReadCapBlocked), "prior reads: {prior_reads}");
        assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), history_len);
        assert!(
            outcome_ctx
                .ctx
                .tool_registry
                .has_patch_recovery_read(tool_names::READ_FILE, &read_args)
        );
    }
}

#[tokio::test]
async fn patch_recovery_rejects_ignored_limit_alias_at_the_path_cap() {
    let mut backing = TestContextBacking::new(30).await;
    backing.select_build_primary_agent();
    let path = backing.sample_file.to_string_lossy().to_string();
    std::fs::write(&backing.sample_file, (1..=400).map(|line| format!("fixture-{line}\n")).collect::<String>())
        .unwrap();
    let bad_patch =
        json!({"input":"*** Begin Patch\n*** Update File: sample.txt\n@@\n-missing\n+edited\n*** End Patch\n"});
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &bad_patch, PermissionGrant::Permanent).await;
    let bounded = json!({"path":path, "limit":1, "condense":false});
    cache_tool_permission(&mut backing, tool_names::READ_FILE, &bounded, PermissionGrant::Permanent).await;
    let mut repeated_tool_attempts = LoopTracker::new();
    let mut turn_modified_files = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut repeated_tool_attempts,
        turn_modified_files: &mut turn_modified_files,
    };
    handle_single_tool_call(&mut outcome_ctx, "mismatch", tool_names::APPLY_PATCH, bad_patch)
        .await
        .unwrap();
    for _ in 0..6 {
        outcome_ctx.ctx.harness_state.record_file_read_path_call(path.clone());
    }
    let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
    handle_single_tool_call(
        &mut outcome_ctx,
        "ignored_limit",
        tool_names::READ_FILE,
        json!({"path":path, "limit_lines":1, "condense":false}),
    )
    .await
    .unwrap();
    assert_eq!(outcome_ctx.ctx.tool_registry.execution_history_len(), history_len);
    assert!(
        outcome_ctx
            .ctx
            .tool_registry
            .has_patch_recovery_read(tool_names::READ_FILE, &bounded)
    );
    let history_start = outcome_ctx.ctx.working_history.len();
    handle_single_tool_call(&mut outcome_ctx, "bounded_read", tool_names::READ_FILE, bounded)
        .await
        .unwrap();
    let output = outcome_ctx.ctx.working_history[history_start..]
        .iter()
        .map(|message| message.content.as_text())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(output.contains("fixture-1"), "{output}");
    assert!(!output.contains("fixture-201"), "{output}");
    assert!(!output.contains("fixture-400"), "{output}");
}
