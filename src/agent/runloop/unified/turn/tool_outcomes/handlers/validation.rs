use super::*;

/// Validates a tool call against all safety and permission checks.
/// Returns Some(TurnHandlerOutcome) if the turn loop should break/exit/cancel.
/// Returns None if execution should proceed (or if a local error was already handled/pushed).
pub(crate) async fn validate_tool_call<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    tool_name: &str,
    args_val: &serde_json::Value,
) -> Result<ValidationResult> {
    ctx.handle
        .set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::CheckingPermissions);
    // Early guard: reject empty tool names with a clear error message.
    // This handles malformed LLM responses where tool name is missing.
    // The "empty tool name" phrase is matched by `preflight_failure_is_llm_mistake`.
    if tool_name.trim().is_empty() {
        let outcome = handle_preflight_failure(
            ctx,
            tool_call_id,
            "<empty tool name>",
            "Tool call has an empty tool name. Provide a valid tool name.",
            Some(args_val),
            None,
        );
        return Ok(outcome.map_or(ValidationResult::Handled, ValidationResult::Outcome));
    }

    if let Some(notice) = ctx.harness_state.record_wall_clock_exhaustion_notice() {
        // Emit the full policy message once (first notice); subsequent rejected
        // calls in the same batch get a compact stub to avoid repeating it N
        // times. The "synthesize now" system directive is pushed after the whole
        // batch by the caller (see `take_wall_clock_directive_pending`) so it is
        // never interleaved between tool responses of the same assistant message.
        let error_msg = if notice.first_notice {
            notice.exhaustion.policy_violation_message()
        } else {
            notice.exhaustion.skipped_call_message()
        };
        ctx.push_rejected_tool_response(
            tool_call_id,
            Some(tool_name),
            Some(args_val),
            build_turn_budget_error_content(error_msg),
        );
        return Ok(ValidationResult::Blocked);
    }

    let mut prepared = match ctx.tool_registry.admit_public_tool_call(tool_name, args_val) {
        Ok(prepared) => prepared,
        Err(err) => {
            if let Some(recovered_prepared) = try_recover_preflight_with_fallback(ctx, tool_name, args_val, &err) {
                tracing::info!(
                    tool = tool_name,
                    recovered_tool = %recovered_prepared.canonical_name,
                    "Recovered tool preflight by applying fallback arguments"
                );
                recovered_prepared
            } else {
                let fallback = preflight_validation_fallback(tool_name, args_val, &err);
                let (fallback_tool, fallback_tool_args) = fallback
                    .as_ref()
                    .map(|(tool, args)| (Some(tool.clone()), Some(args.clone())))
                    .unwrap_or((None, None));
                let error_text = format!("{err:#}");
                if check_is_argument_error(&error_text)
                    || error_text.to_ascii_lowercase().contains("tool preflight validation failed")
                {
                    let outcome =
                        handle_preflight_failure(ctx, tool_call_id, tool_name, &error_text, Some(args_val), fallback);
                    return Ok(outcome.map_or(ValidationResult::Handled, ValidationResult::Outcome));
                }
                ctx.push_rejected_tool_response(
                    tool_call_id,
                    Some(tool_name),
                    Some(args_val),
                    build_validation_error_content_with_fallback(
                        format!("Tool preflight validation failed: {err:#}"),
                        "preflight",
                        fallback_tool,
                        fallback_tool_args,
                    ),
                );
                return Ok(ValidationResult::Blocked);
            }
        }
    };

    // Check correction only after admission against the live schema. Unrelated
    // operations retain the validation episode and its independent failure cap.
    if ctx.session_stats.preflight_recovery.exhausted() {
        ctx.push_rejected_tool_response(
            tool_call_id,
            Some(tool_name),
            Some(args_val),
            build_failure_error_content(
                "Automatic preflight recovery is exhausted; resume with corrected arguments.".to_string(),
                "preflight_circuit_breaker",
            ),
        );
        return Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
            reason: Some("Automatic preflight recovery is exhausted; type continue to resume.".to_string()),
        })));
    }

    let canonical_tool_name = prepared.canonical_name.clone();
    if !primary_agent_allows_tool(ctx.active_primary_agent.active(), &canonical_tool_name) {
        ctx.push_rejected_tool_response(
            tool_call_id,
            Some(&canonical_tool_name),
            Some(args_val),
            serde_json::to_string(
                &ToolExecutionError::policy_violation(
                    canonical_tool_name.clone(),
                    format!("Tool '{canonical_tool_name}' execution denied by active primary agent policy"),
                )
                .to_json_value(),
            )
            .unwrap_or_else(|_| "{}".to_string()),
        );
        return Ok(ValidationResult::Blocked);
    }

    prepared.effective_args =
        maybe_apply_spool_read_offset_hint(ctx.tool_registry, &canonical_tool_name, &prepared.effective_args);
    prepared.parallel_safe_after_preflight =
        vtcode_core::tools::tool_intent::is_parallel_safe_call(&canonical_tool_name, &prepared.effective_args);
    let effective_args = &prepared.effective_args;

    // PreToolUse hooks run before the argument-dependent guards, the safety
    // gateway, and permission evaluation so rewritten arguments are what every
    // downstream check sees (read-after-write, repeated-read-only, mutation
    // bookkeeping, parallel grouping). Mirrors the pipeline path.
    let hook_phase = match crate::agent::runloop::unified::tool_routing::pipeline_pre_tool_hooks(
        ctx.lifecycle_hooks,
        ctx.renderer,
        &canonical_tool_name,
        effective_args,
        tool_call_id,
    )
    .await
    {
        Ok(Some(PreToolHookPhaseResult::Deny)) => {
            ctx.harness_state.record_denied_tool_call();
            // Surface the denial to the model so it does not silently retry
            // the same call; the reason is also rendered for the user.
            ctx.push_rejected_tool_response(
                tool_call_id,
                Some(&canonical_tool_name),
                Some(effective_args),
                build_failure_error_content(
                    format!("Tool '{canonical_tool_name}' denied by PreToolUse hook"),
                    "policy",
                ),
            );
            return Ok(ValidationResult::Blocked);
        }
        Ok(Some(PreToolHookPhaseResult::Proceed { rewritten_args: Some(rewritten), requires_prompt })) => {
            // Re-validate the rewritten arguments: preflight ran on the
            // original payload, so a rewrite could otherwise bypass schema
            // checks. A hook-produced invalid payload is a hook error — block
            // with a clear message instead of executing malformed input.
            if let Err(err) = ctx
                .tool_registry
                .preflight_validate_harness_call(&canonical_tool_name, &rewritten)
            {
                ctx.harness_state.record_denied_tool_call();
                ctx.push_rejected_tool_response(
                    tool_call_id,
                    Some(&canonical_tool_name),
                    Some(effective_args),
                    build_failure_error_content(
                        format!("PreToolUse hook produced invalid arguments for '{canonical_tool_name}': {err}"),
                        "policy",
                    ),
                );
                return Ok(ValidationResult::Blocked);
            }
            prepared.effective_args = rewritten;
            // Re-derive the intent classification for the rewritten arguments:
            // the parallel-readonly grouping and the mutation bookkeeping below
            // must see what will actually execute, not the pre-rewrite form.
            prepared.readonly_classification =
                !vtcode_core::tools::tool_intent::classify_tool_intent(&canonical_tool_name, &prepared.effective_args)
                    .mutating;
            prepared.parallel_safe_after_preflight =
                vtcode_core::tools::tool_intent::is_parallel_safe_call(&canonical_tool_name, &prepared.effective_args);
            Some(PreToolHookPhaseResult::Proceed { rewritten_args: None, requires_prompt })
        }
        Ok(phase) => phase,
        Err(err) => {
            ctx.harness_state.record_denied_tool_call();
            ctx.push_system_message(format!("Pre-tool hook phase failed: {err}"));
            ctx.push_rejected_tool_response(
                tool_call_id,
                Some(&canonical_tool_name),
                Some(effective_args),
                build_failure_error_content(format!("Pre-tool hook phase failed: {err}"), "policy"),
            );
            return Ok(ValidationResult::Blocked);
        }
    };

    // Only after the hook phase (and possible rewrite) do the argument- and
    // classification-dependent bookkeeping and guards run, so they observe the
    // arguments that will actually execute.
    if !prepared.readonly_classification {
        ctx.harness_state.reset_file_read_family_streak();
    }
    let fallback_recommendation =
        recovery_fallback_for_tool(&canonical_tool_name, &prepared.effective_args).map(|(tool_name, args)| {
            vtcode_core::core::agent::harness_kernel::FallbackRecommendation { tool_name, args, chain: Vec::new() }
        });
    prepared = prepared.with_fallback_recommendation(fallback_recommendation);
    let effective_args = &prepared.effective_args;

    if let Some(outcome) =
        enforce_duplicate_task_tracker_create_guard(ctx, tool_call_id, &canonical_tool_name, effective_args)
    {
        return Ok(outcome);
    }

    if let Some(outcome) = enforce_read_after_write_guard(ctx, tool_call_id, &canonical_tool_name, effective_args) {
        return Ok(outcome);
    }

    if let Some(outcome) = enforce_repeated_read_only_call_guard(
        ctx,
        tool_call_id,
        &canonical_tool_name,
        effective_args,
        prepared.readonly_classification,
    ) {
        return Ok(outcome);
    }

    if let Some(outcome) = enforce_repeated_shell_run_guard(ctx, tool_call_id, &canonical_tool_name, effective_args) {
        return Ok(outcome);
    }

    if let Some(outcome) = enforce_spool_chunk_read_guard(ctx, tool_call_id, &canonical_tool_name, effective_args).await
    {
        return Ok(outcome);
    }

    // Phase 4 Check: Per-tool Circuit Breaker
    let circuit_breaker_blocked = !ctx.circuit_breaker.allow_request_for_tool(&canonical_tool_name);
    if circuit_breaker_blocked {
        let display_tool = tool_action_label(&canonical_tool_name, args_val);
        let (fallback_tool, fallback_tool_args) = prepared
            .fallback_recommendation
            .as_ref()
            .map(|fallback| (Some(fallback.tool_name.clone()), Some(fallback.args.clone())))
            .unwrap_or((None, None));
        let block_reason = format!(
            "Circuit breaker blocked '{display_tool}' due to high failure rate. Switching to autonomous fallback strategy."
        );
        tracing::warn!(tool = %canonical_tool_name, "Circuit breaker open, tool disabled");

        // In interactive mode, attempt recovery prompt; None = user chose to proceed.
        if let Some(result) =
            try_interactive_circuit_recovery(ctx, tool_call_id, &canonical_tool_name, fallback_tool, fallback_tool_args)
                .await?
        {
            ctx.push_system_message(block_reason);
            return Ok(result);
        }
    }

    // Phase 4 Check: Adaptive Rate Limiter
    if let Some(outcome) = acquire_adaptive_rate_limit_slot(ctx, tool_call_id, &canonical_tool_name).await? {
        return Ok(outcome);
    }

    // Unified interactive turns own loop/recovery policy via turn-local guards and
    // the turn balancer. The legacy core loop detector remains available for
    // non-unified autonomous execution paths only.

    let effective_args = &prepared.effective_args;

    let mut safety_approval_justification = None;
    if let Some((outcome, justification)) =
        run_safety_validation_loop(ctx, tool_call_id, &canonical_tool_name, effective_args).await?
    {
        safety_approval_justification = justification;
        if matches!(outcome, ValidationResult::Blocked) {
            return Ok(outcome);
        }
    }

    // Ensure tool permission. The PreToolUse hook phase already ran above;
    // forwarding it here prevents a second hook invocation.
    let permission_result = ensure_tool_permission_with_call_id(
        build_tool_permissions_context_with_safety(ctx, safety_approval_justification.as_deref()),
        &canonical_tool_name,
        Some(effective_args),
        Some(tool_call_id),
        hook_phase,
    )
    .await;

    match permission_result {
        Ok(ToolPermissionFlow::Approved { updated_args }) => {
            if let Some(updated_args) = updated_args {
                // A PermissionRequest hook may supply its own rewrite via
                // `updated_input`; it replaces the arguments the safety
                // gateway and argument-dependent guards evaluated. Validate
                // the schema and re-run the safety gateway against the final
                // arguments so the replacement does not execute under
                // decisions made for earlier arguments.
                if let Err(err) = ctx
                    .tool_registry
                    .preflight_validate_harness_call(&canonical_tool_name, &updated_args)
                {
                    ctx.harness_state.record_denied_tool_call();
                    ctx.push_rejected_tool_response(
                        tool_call_id,
                        Some(&canonical_tool_name),
                        Some(effective_args),
                        build_failure_error_content(
                            format!(
                                "PermissionRequest hook produced invalid arguments for '{canonical_tool_name}': {err}"
                            ),
                            "policy",
                        ),
                    );
                    return Ok(ValidationResult::Blocked);
                }
                let rewritten_differ = *effective_args != updated_args;
                prepared.effective_args = updated_args;
                if rewritten_differ
                    && let Some((outcome, _)) =
                        run_safety_validation_loop(ctx, tool_call_id, &canonical_tool_name, &prepared.effective_args)
                            .await?
                    && matches!(outcome, ValidationResult::Blocked)
                {
                    ctx.harness_state.record_denied_tool_call();
                    return Ok(outcome);
                }
            }
            if canonical_tool_name == tool_names::START_PLANNING {
                ctx.harness_state.clear_task_tracker_create_signatures();
            }
            // Count only calls that pass all validation/permission gates.
            record_tool_call_usage(ctx);
            Ok(ValidationResult::Proceed(prepared))
        }
        Ok(ToolPermissionFlow::Denied) => {
            ctx.harness_state.record_denied_tool_call();
            // A permanent `request_user_input` denial (non-interactive runtime,
            // inline UI unavailable, etc.) must be recorded so the planning
            // workflow stops re-forcing the interview AND the tool is suppressed
            // from subsequent catalogs. Without this, the model sees a generic
            // "execution denied by policy" and retries across turns —
            // checkpoint turn_724 shows 7 retries. `handle_failure` is NOT
            // reached on this path (permission denial returns Blocked, not
            // Failure), so we must call `mark_interview_denied()` here too.
            if canonical_tool_name == tool_names::REQUEST_USER_INPUT {
                ctx.plan_session.mark_interview_denied();
                if ctx.is_planning_active() {
                    ctx.harness_state.arm_interview_denial_recovery();
                }
            }
            let denial = if let Some(denial) = ctx.session_stats.last_auto_permission_denial() {
                serde_json::json!({
                    "error": format!("Auto permission review blocked tool '{}': {}", prepared.canonical_name, denial.reason),
                    "reason": denial.reason,
                    "matched_rule": denial.matched_rule,
                    "matched_exception": denial.matched_exception,
                    "review_stage": denial.stage,
                    "next_action": "Choose a safer tool or narrower action that stays within the user's explicit request."
                })
            } else {
                let mut error_json = ToolExecutionError::policy_violation(
                    canonical_tool_name.as_str(),
                    format!("Tool '{}' execution denied by policy", prepared.canonical_name),
                )
                .to_json_value();
                if let Some(diagnostic) = tool_denial_diagnostic(&canonical_tool_name)
                    && let Some(obj) = error_json.as_object_mut()
                {
                    obj.insert("diagnostic".to_string(), diagnostic);
                }
                error_json
            };
            ctx.push_rejected_tool_response(
                tool_call_id,
                Some(&canonical_tool_name),
                Some(effective_args),
                serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
            );
            Ok(ValidationResult::Blocked)
        }
        Ok(ToolPermissionFlow::Blocked { reason }) => {
            // A terminal permission flow is still a blocked tool call. Keep
            // its exact denial reason for the first attempts, while routing
            // accounting and fuse handling through the same dispatch guard as
            // every other blocked validation result.
            let guard_response_expected = !ctx.is_recovery_active() || ctx.recovery_pass_used();
            if let Some(outcome) =
                enforce_blocked_tool_call_guard(ctx, tool_call_id, &canonical_tool_name, effective_args)
            {
                if !guard_response_expected {
                    ctx.push_rejected_tool_response(
                        tool_call_id,
                        Some(&canonical_tool_name),
                        Some(effective_args),
                        build_failure_error_content(reason.clone(), "policy"),
                    );
                }
                Ok(ValidationResult::Outcome(outcome))
            } else {
                ctx.push_rejected_tool_response(
                    tool_call_id,
                    Some(&canonical_tool_name),
                    Some(effective_args),
                    build_failure_error_content(reason.clone(), "policy"),
                );
                Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                    reason: Some(reason),
                })))
            }
        }
        Ok(ToolPermissionFlow::Exit) => Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(TurnLoopResult::Exit))),
        Ok(ToolPermissionFlow::Interrupted) => {
            Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)))
        }
        Err(err) => {
            let err_json = serde_json::json!({
                "error": format!("Failed to evaluate policy for tool '{}': {}", tool_name, err)
            });
            ctx.push_rejected_tool_response(tool_call_id, Some(tool_name), Some(args_val), err_json.to_string());
            Ok(ValidationResult::Blocked)
        }
    }
}
