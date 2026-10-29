//! Execution guard gates for `execute_tool_ref_internal`.
//!
//! Fast-reuse and identical-call loop detection run before policy checks so
//! repeated read-only calls short-circuit without another round-trip.

use std::time::Duration;

use serde_json::Value;

use super::*;
use crate::tools::registry::execution_history::HarnessContextSnapshot;

/// When a read-only tool call has been repeated this many times, stop returning
/// cached results and return a hard error instead.  Must be greater than
/// MIN_READONLY_IDENTICAL_LIMIT (currently 2).
const LOOP_HARD_BLOCK_REPEAT_COUNT: usize = 5;

/// Failure recorder shared by execution guards (see `record_failure` in
/// `execute_tool_ref_internal`).
pub(super) type FailureRecorder<'a> = &'a (
        dyn Fn(String, bool, Option<String>, Value, String, Option<String>, Option<u64>, Option<u64>, Option<u64>, bool)
            + Send
            + Sync
    );

/// Mutable loop-gate state threaded through reuse/loop detection.
pub(super) struct LoopGate<'a> {
    pub skip_loop_detection: bool,
    pub reusable_result: bool,
    pub is_verification_command: bool,
    pub fresh_patch_read: bool,
    pub tool_name: &'a str,
    pub display_name: &'a str,
    pub tool_name_owned: &'a str,
    pub args: &'a Value,
    pub requested_name: &'a str,
    pub args_for_recording: &'a Value,
    pub context_snapshot: &'a HarnessContextSnapshot,
    pub timeout_category_label: &'a Option<String>,
    pub base_timeout_ms: Option<u64>,
    pub adaptive_timeout_ms: Option<u64>,
    pub record_failure: FailureRecorder<'a>,
}

/// How tool execution proceeds after the loop gate.
pub(super) enum LoopGateOutcome {
    EarlyReturn(Value),
    Proceed {
        repeat_recovery_generation: Option<u64>,
        loop_repeat_count: usize,
        loop_limit: usize,
    },
}

impl ToolRegistry {
    /// Fast-reuse recent read-only results and block repeated identical calls.
    pub(super) async fn check_loop_gate(&self, gate: LoopGate<'_>) -> Result<LoopGateOutcome> {
        let LoopGate {
            skip_loop_detection,
            reusable_result,
            is_verification_command,
            fresh_patch_read,
            tool_name,
            display_name,
            tool_name_owned,
            args,
            requested_name,
            args_for_recording,
            context_snapshot,
            timeout_category_label,
            base_timeout_ms,
            adaptive_timeout_ms,
            record_failure,
        } = gate;
        if skip_loop_detection {
            trace!(
                tool = %tool_name,
                "Skipping identical-call loop detection for stateful exec continuation"
            );
        }

        // FAST REUSE: Read-only inspection calls are often repeated verbatim within a
        // single turn (e.g. `diff a b | wc -l`, `find ... | grep`). Reuse the most recent
        // successful result immediately instead of paying for another round-trip and
        // another spool file. This is gated by a short TTL and the read-only classification
        // so mutating calls never take this path.
        //
        // Verification commands (`cargo check`, `cargo nextest run`, `cargo fmt
        // --check`, and pure `&&` chains thereof) are read-only by intent but
        // must always re-execute: reusing a stale success would clear the
        // anti-blind-editing gate without verifying the current worktree, and
        // reusing a stale failure would keep the gate pending after a fix.
        // (`is_verification_command` is bound once at preview-budget
        // resolution above; the stripped output-metadata field does not
        // affect shell classification.)
        if reusable_result && !is_verification_command && !skip_loop_detection && !fresh_patch_read {
            let fast_reuse_max_age = Duration::from_secs(60);
            let fast_reused = self
                .execution_history
                .find_recent_spooled_result(tool_name, args, fast_reuse_max_age)
                .or_else(|| {
                    self.execution_history
                        .find_recent_successful_result(tool_name, args, fast_reuse_max_age)
                });
            if let Some(mut reused_value) = fast_reused {
                if let Some(obj) = reused_value.as_object_mut() {
                    obj.insert("reused_recent_result".into(), json!(true));
                    obj.insert("tool".into(), json!(display_name));
                    let reused_spooled = obj.get("spool_path").and_then(|v| v.as_str()).is_some();
                    let note = if reused_spooled {
                        "Reusing a recent spooled output for this identical read-only call. Continue from the spool file instead of re-running the tool."
                    } else {
                        "Reusing a recent successful output for this identical read-only call."
                    };
                    obj.insert("reused_result_note".into(), json!(note));
                }
                // Record a synthetic "reused" entry so subsequent
                // `detect_loop` / `find_recent_*` queries see this call in the
                // history.  Previously the fast-reuse path returned the
                // cached payload without recording, which meant `detect_loop`
                // could not account for the reused call — leading to
                // undercounting on the very next turn and silent cache
                // poisoning.
                //
                // We pass `is_mcp_tool = false` and `mcp_provider = None`
                // because the fast-reuse gate has already filtered for
                // read-only calls and we don't have those locals in scope at
                // this point in the function.  The cached record itself
                // (added when the original call ran) carries the true MCP
                // metadata; the synthetic record exists only to make
                // `detect_loop` see this call in the rolling window.
                self.execution_history.add_record(ToolExecutionRecord::success(
                    tool_name.to_owned(),
                    requested_name.to_owned(),
                    false,
                    None,
                    args_for_recording.clone(),
                    reused_value.clone(),
                    context_snapshot.clone(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                ));
                trace!(
                    tool = %tool_name,
                    "Fast-reusing recent successful read-only result"
                );
                self.observe_source_output(tool_name, args, &reused_value).await;
                return Ok(LoopGateOutcome::EarlyReturn(reused_value));
            }
        }

        // LOOP DETECTION: Check if we're calling the same tool repeatedly with identical params
        let loop_limit = if skip_loop_detection {
            0
        } else {
            self.execution_history.loop_limit_for(tool_name, args)
        };
        let loop_result = if skip_loop_detection {
            crate::tools::registry::execution_history::LoopDetectionResult {
                detected: false,
                repeat_count: 0,
                tool_name: tool_name.to_owned(),
            }
        } else {
            self.execution_history.detect_loop(tool_name, args)
        };
        let repeat_recovery_generation =
            if loop_limit > 0 && loop_result.detected && loop_result.repeat_count >= loop_limit {
                self.completion_verifier_repeat_recovery_generation(tool_name, args)
            } else {
                None
            };
        if loop_result.detected && loop_result.repeat_count > 1 {
            let delay_ms = (LOOP_THROTTLE_REGISTRY_BASE_MS * loop_result.repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
        if loop_limit > 0 && loop_result.detected {
            warn!(
                tool = %tool_name,
                repeats = loop_result.repeat_count,
                "Loop detected: agent calling same tool with identical parameters {} times",
                loop_result.repeat_count
            );
            if loop_result.repeat_count >= loop_limit && repeat_recovery_generation.is_none() {
                // Hard block: when the model has been repeating the same call far
                // beyond the limit, stop returning cached results and return an
                // error.  Returning a cached result at high repeat counts causes
                // the model to see "success" and keep retrying.
                let hard_block = loop_result.repeat_count >= LOOP_HARD_BLOCK_REPEAT_COUNT;

                if reusable_result && !is_verification_command && !hard_block && !fresh_patch_read {
                    let reuse_max_age = Duration::from_secs(120);
                    let reused = self
                        .execution_history
                        .find_recent_spooled_result(tool_name, args, reuse_max_age)
                        .or_else(|| {
                            self.execution_history
                                .find_recent_successful_result(tool_name, args, reuse_max_age)
                        });
                    if let Some(mut reused_value) = reused {
                        if let Some(obj) = reused_value.as_object_mut() {
                            obj.insert("reused_recent_result".into(), json!(true));
                            obj.insert("loop_detected".into(), json!(true));
                            obj.insert("repeat_count".into(), json!(loop_result.repeat_count));
                            obj.insert("limit".into(), json!(loop_limit));
                            obj.insert("tool".into(), json!(display_name));
                            let reused_spooled = obj.get("spool_path").and_then(|v| v.as_str()).is_some();
                            let note = if reused_spooled {
                                "Loop detected: this identical read-only call has been repeated, so the earlier result was reused. The full output is in the spool file and the conversation history; further repeats return an error instead."
                            } else {
                                "Loop detected: this identical read-only call has been repeated with no new information, so the earlier result was reused. It is already in the conversation history; further repeats return an error instead."
                            };
                            obj.insert("loop_detected_note".into(), json!(note));
                        }
                        self.observe_source_output(tool_name, args, &reused_value).await;
                        return Ok(LoopGateOutcome::EarlyReturn(reused_value));
                    }
                }

                let delay_ms =
                    (LOOP_THROTTLE_REGISTRY_BASE_MS * loop_result.repeat_count as u64).min(LOOP_THROTTLE_MAX_MS);
                if delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                }

                let payload =
                    loop_detection_block_payload(tool_name_owned, display_name, loop_result.repeat_count, loop_limit);

                record_failure(
                    tool_name_owned.to_owned(),
                    false,
                    None,
                    args_for_recording.clone(),
                    "Tool call blocked due to repeated identical invocations".to_string(),
                    timeout_category_label.clone(),
                    base_timeout_ms,
                    adaptive_timeout_ms,
                    None,
                    false,
                );

                return Ok(LoopGateOutcome::EarlyReturn(payload));
            }
        }
        Ok(LoopGateOutcome::Proceed {
            repeat_recovery_generation,
            loop_repeat_count: loop_result.repeat_count,
            loop_limit,
        })
    }

    /// Enter the tool reentrancy guard, rejecting recursive invocation.
    pub(super) fn check_tool_reentrancy(
        &self,
        tool_name: &str,
        display_name: &str,
        prevalidated: bool,
        args: &Value,
        args_for_recording: &Value,
        record_failure: FailureRecorder<'_>,
    ) -> Result<ToolReentrancyGuard> {
        let allow_parallel_sibling = prevalidated && tool_intent::is_parallel_safe_call(tool_name, args);
        match ToolReentrancyGuard::enter(tool_name, allow_parallel_sibling) {
            Ok(guard) => Ok(guard),
            Err(violation) => {
                let reentry_count = violation.tool_reentry_count + 1;
                let error_message = format!(
                    "Reentrancy guard: tool '{}' is already running in this call stack, so this recursive call was blocked. \
                     Repeating the same call is blocked the same way; change the control flow or use a different tool.\n\
                     Current stack depth: {}. Re-entry count for this tool in the current task: {}.\n\
                     Stack trace: {}",
                    display_name, violation.stack_depth, reentry_count, violation.stack_trace
                );
                let error = ToolExecutionError::new(
                    tool_name.to_owned(),
                    ToolErrorType::PolicyViolation,
                    error_message.clone(),
                );
                let mut payload = error.to_json_value();
                if let Some(obj) = payload.as_object_mut() {
                    obj.insert("reentrant_call_blocked".into(), json!(true));
                    obj.insert("stack_depth".into(), json!(violation.stack_depth));
                    obj.insert("reentry_count".into(), json!(reentry_count));
                    obj.insert("tool".into(), json!(display_name));
                    obj.insert("stack_trace".into(), json!(violation.stack_trace));
                }
                record_failure(
                    tool_name.to_owned(),
                    false,
                    None,
                    args_for_recording.clone(),
                    error_message.clone(),
                    None,
                    None,
                    None,
                    None,
                    false,
                );
                Err(anyhow!(error_message).context("tool reentrancy blocked"))
            }
        }
    }

    /// Validate the resolved call and enforce planning-workflowmutating-tool rules.
    pub(super) fn validate_preflight_and_planning(
        &self,
        tool_name: &str,
        display_name: &str,
        args: &Value,
        prevalidated: bool,
        args_for_recording: &Value,
        record_failure: FailureRecorder<'_>,
    ) -> Result<bool> {
        // Classify the tool intent once: the prevalidated fast path skips full
        // preflight and classifies here; the full-preflight path reuses the
        // intent the kernel already computed on the validated (executed) args,
        // so planning enforcement below cannot disagree with
        // `readonly_classification`.
        let (intent, readonly_classification) = if prevalidated {
            #[cfg(debug_assertions)]
            {
                if let Err(err) = execution_kernel::preflight_validate_resolved_call(self, tool_name, args)
                    && !agent_execution::is_planning_active_denial(&err.to_string())
                {
                    debug_assert!(false, "prevalidated execution received invalid call for '{tool_name}': {err}");
                }
            }
            let intent = tool_intent::classify_tool_intent(tool_name, args);
            (intent, !intent.mutating)
        } else {
            match execution_kernel::preflight_validate_resolved_call(self, tool_name, args) {
                Ok(outcome) => (outcome.intent, outcome.readonly_classification),
                Err(err) => {
                    let err_msg = err.to_string();
                    record_failure(
                        tool_name.to_owned(),
                        false,
                        None,
                        args_for_recording.clone(),
                        err_msg,
                        None,
                        None,
                        None,
                        None,
                        false,
                    );
                    return Err(err);
                }
            }
        };

        if readonly_classification {
            trace!(tool = %tool_name, "Validation classified tool as read-only");
        }

        // Defense-in-depth: prevalidated fast path skips full preflight, but planning workflow
        // mutating-tool enforcement remains a hard safety invariant. Reuse the already-computed
        // `intent` so we don't reclassify on the hot path.
        if self.is_planning_active() && !self.is_planning_active_allowed_with_intent(tool_name, args, &intent) {
            let error_msg = agent_execution::planning_workflow_denial_message(display_name);
            record_failure(
                tool_name.to_owned(),
                false,
                None,
                args_for_recording.clone(),
                error_msg.clone(),
                None,
                None,
                None,
                None,
                false,
            );
            return Err(anyhow!(error_msg).context(agent_execution::PLANNING_DENIED_CONTEXT));
        }
        Ok(readonly_classification)
    }
}
