use super::*;
use crate::agent::runloop::unified::turn::tool_outcomes::handlers::guards::read_guard::DECLARED_EDIT_GRACE_INSPECTIONS;

fn declared_edit_markers(ctx: &TurnProcessingContext<'_>) -> usize {
    ctx.working_history
        .iter()
        .filter(|message| message.content.as_text().contains("declared_edit_pending"))
        .count()
}

fn sed_line(path: &str, idx: u32) -> serde_json::Value {
    json!({"cmd": format!("sed -n '{idx},{idx}p' {path}")})
}

#[cfg(unix)]
#[tokio::test]
async fn declared_edit_refuses_reads_past_grace_until_mutation_lands() {
    // session-vtcode-20261117T162700Z turn 7c0e1b36: ~12 "now applying"
    // declarations followed only by reads. Past a small grace, reads are
    // refused until the edit lands; the edit reopens reads.
    let mut backing = TestContextBacking::new().await;
    backing.select_build_primary_agent();
    backing.harness_state.set_approved_plan_execution(true);
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, (1..=16).map(|idx| format!("line {idx}\n")).collect::<String>())
        .expect("rewrite sample file");
    let sample_path = sample_file.to_string_lossy().to_string();
    let patch_args = json!({
        "input": "*** Begin Patch\n*** Update File: sample.txt\n@@\n-line 1\n+edited line 1\n*** End Patch\n"
    });
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &patch_args, PermissionGrant::Permanent).await;

    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state
        .arm_declared_edit("now applying the spacing fixes".to_string());
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
    };

    for idx in 1..=u32::from(DECLARED_EDIT_GRACE_INSPECTIONS) {
        let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
        let outcome = handle_single_tool_call(
            &mut outcome_ctx,
            &format!("grace_read_{idx}"),
            tool_names::EXEC_COMMAND,
            sed_line(&sample_path, idx),
        )
        .await
        .expect("grace read completes");
        assert!(outcome.is_none());
        assert_eq!(declared_edit_markers(outcome_ctx.ctx), 0);
        assert!(outcome_ctx.ctx.tool_registry.execution_history_len() > history_len, "grace read {idx} must execute");
    }

    let history_len = outcome_ctx.ctx.tool_registry.execution_history_len();
    let outcome =
        handle_single_tool_call(&mut outcome_ctx, "refused_read", tool_names::EXEC_COMMAND, sed_line(&sample_path, 9))
            .await
            .expect("refused read completes");
    assert!(outcome.is_none(), "single refusal must not schedule recovery");
    assert_eq!(declared_edit_markers(outcome_ctx.ctx), 1);
    assert_eq!(
        outcome_ctx.ctx.tool_registry.execution_history_len(),
        history_len,
        "the refused read must not execute"
    );
    assert!(
        outcome_ctx
            .ctx
            .working_history
            .iter()
            .any(|message| message.content.as_text().contains("now applying the spacing fixes")),
        "the refusal must quote the declaration"
    );

    let patch = handle_single_tool_call(&mut outcome_ctx, "declared_edit", tool_names::APPLY_PATCH, patch_args)
        .await
        .expect("edit executes");
    assert!(patch.is_none());
    assert_eq!(
        std::fs::read_to_string(&sample_file)
            .expect("read edited fixture")
            .lines()
            .next(),
        Some("edited line 1")
    );

    let reopened = handle_single_tool_call(
        &mut outcome_ctx,
        "reopened_read",
        tool_names::EXEC_COMMAND,
        sed_line(&sample_path, 10),
    )
    .await
    .expect("reopened read completes");
    assert!(reopened.is_none());
    assert_eq!(declared_edit_markers(outcome_ctx.ctx), 1, "no new refusal after the edit");
}

#[cfg(unix)]
#[tokio::test]
async fn declared_edit_failed_patch_keeps_pending_but_recovery_read_passes() {
    // A failed edit is not the declared edit landing: enforcement stays
    // armed — but the reserved patch-recovery read (the fix-up path for
    // the failed edit) is never refused, exactly once.
    let mut backing = TestContextBacking::new().await;
    backing.select_build_primary_agent();
    backing.harness_state.set_approved_plan_execution(true);
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, (1..=16).map(|idx| format!("line {idx}\n")).collect::<String>())
        .expect("rewrite sample file");
    let sample_path = sample_file.to_string_lossy().to_string();
    let bad_patch = json!({
        "input": "*** Begin Patch\n*** Update File: sample.txt\n@@\n-no such line\n+edited line 1\n*** End Patch\n"
    });
    cache_tool_permission(&mut backing, tool_names::APPLY_PATCH, &bad_patch, PermissionGrant::Permanent).await;

    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state
        .arm_declared_edit("now applying the spacing fixes".to_string());
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
    };
    outcome_ctx.ctx.tool_registry.begin_patch_recovery_turn();

    for idx in 1..=u32::from(DECLARED_EDIT_GRACE_INSPECTIONS) {
        handle_single_tool_call(
            &mut outcome_ctx,
            &format!("grace_read_{idx}"),
            tool_names::EXEC_COMMAND,
            sed_line(&sample_path, idx),
        )
        .await
        .expect("grace read completes");
    }

    let failed = handle_single_tool_call(&mut outcome_ctx, "bad_edit", tool_names::APPLY_PATCH, bad_patch)
        .await
        .expect("failed edit completes");
    assert!(failed.is_none());
    assert_eq!(
        std::fs::read_to_string(&sample_file).expect("read fixture").lines().next(),
        Some("line 1"),
        "the bad patch must not apply"
    );

    // The first post-failure read of the patched path is the reserved
    // patch-recovery read: admitted, never refused, and not counted.
    let recovery =
        handle_single_tool_call(&mut outcome_ctx, "recovery_read", tool_names::EXEC_COMMAND, sed_line(&sample_path, 9))
            .await
            .expect("recovery read completes");
    assert!(recovery.is_none());
    assert_eq!(declared_edit_markers(outcome_ctx.ctx), 0, "the reserved patch-recovery read is never refused");

    // The exception is one-shot and the failed edit did not disarm: the
    // next read is refused again.
    let still_refused = handle_single_tool_call(
        &mut outcome_ctx,
        "refused_after_recovery",
        tool_names::EXEC_COMMAND,
        sed_line(&sample_path, 10),
    )
    .await
    .expect("refused read completes");
    assert!(still_refused.is_none());
    assert_eq!(
        declared_edit_markers(outcome_ctx.ctx),
        1,
        "a failed edit keeps enforcement armed; the recovery exception is one-shot"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn declared_edit_guard_requires_approved_execution() {
    // Same arming without the approved-execution flag: reads flow, nothing
    // is refused, existing behavior is preserved outside approved execution.
    let mut backing = TestContextBacking::new().await;
    backing.select_build_primary_agent();
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, (1..=16).map(|idx| format!("line {idx}\n")).collect::<String>())
        .expect("rewrite sample file");
    let sample_path = sample_file.to_string_lossy().to_string();

    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state
        .arm_declared_edit("now applying the spacing fixes".to_string());
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
    };

    for idx in 1..=u32::from(DECLARED_EDIT_GRACE_INSPECTIONS) + 2 {
        let outcome = handle_single_tool_call(
            &mut outcome_ctx,
            &format!("free_read_{idx}"),
            tool_names::EXEC_COMMAND,
            sed_line(&sample_path, idx),
        )
        .await
        .expect("read completes");
        assert!(outcome.is_none());
    }
    assert_eq!(declared_edit_markers(outcome_ctx.ctx), 0);
}

#[cfg(unix)]
#[tokio::test]
async fn declared_edit_keeps_tracker_usable_while_reads_refused() {
    // Checklist updates are progress signals, not evidence gathering: the
    // tracker stays usable while reads are refused, and neither counts
    // against nor clears the pending declaration.
    let mut backing = TestContextBacking::new().await;
    backing.select_build_primary_agent();
    backing.harness_state.set_approved_plan_execution(true);
    let sample_file = backing.sample_file.clone();
    std::fs::write(&sample_file, (1..=16).map(|idx| format!("line {idx}\n")).collect::<String>())
        .expect("rewrite sample file");
    let sample_path = sample_file.to_string_lossy().to_string();
    let tracker_list = json!({"action": "list"});
    cache_tool_permission(&mut backing, tool_names::TASK_TRACKER, &tracker_list, PermissionGrant::Permanent).await;

    let mut tracker = LoopTracker::new();
    let mut modified = BTreeSet::new();
    let mut ctx = backing.turn_processing_context();
    ctx.harness_state
        .arm_declared_edit("now applying the spacing fixes".to_string());
    let mut outcome_ctx = ToolOutcomeContext {
        ctx: &mut ctx,
        repeated_tool_attempts: &mut tracker,
        turn_modified_files: &mut modified,
    };

    for idx in 1..=u32::from(DECLARED_EDIT_GRACE_INSPECTIONS) {
        handle_single_tool_call(
            &mut outcome_ctx,
            &format!("grace_read_{idx}"),
            tool_names::EXEC_COMMAND,
            sed_line(&sample_path, idx),
        )
        .await
        .expect("grace read completes");
    }
    let listed = handle_single_tool_call(&mut outcome_ctx, "tracker_list", tool_names::TASK_TRACKER, tracker_list)
        .await
        .expect("tracker list executes");
    assert!(listed.is_none());
    assert_eq!(declared_edit_markers(outcome_ctx.ctx), 0, "tracker use is not an inspection");

    let refused = handle_single_tool_call(
        &mut outcome_ctx,
        "still_refused_read",
        tool_names::EXEC_COMMAND,
        sed_line(&sample_path, 9),
    )
    .await
    .expect("refused read completes");
    assert!(refused.is_none());
    assert_eq!(
        declared_edit_markers(outcome_ctx.ctx),
        1,
        "tracker use neither counts against nor clears the declaration"
    );
}
