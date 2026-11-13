#![allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
mod approval_cache;
mod approval_persistence;
mod approval_policy;
mod hook_messages;
mod limit_prompts;
mod permission_events;
mod permission_prompt;
mod preflight;
mod shell_approval;

use std::borrow::Cow;
use std::fmt::Write;
#[cfg(test)]
use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use tokio::sync::{Notify, RwLock};

use serde_json::Value;
use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
use vtcode_core::command_safety::parse_bash_lc_commands;
use vtcode_core::config::PermissionsConfig;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::exec::events::PermissionDecision;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::hooks::{
    LifecycleHookEngine, PermissionDecisionBehavior, PermissionDecisionScope, PreToolHookDecision,
};
use vtcode_core::permissions::{
    PermissionRequest, PermissionRequestKind, ResolvedPermissionDecision, build_permission_request,
    evaluate_effective_permissions, evaluate_permissions,
};
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::{ToolPermissionDecision, ToolRegistry};
use vtcode_core::tools::{JustificationExtractor, ToolRiskScorer};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::InlineHandle;

use crate::agent::runloop::unified::auto_permission::{AutoPermissionReviewDecision, review_tool_call};
use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::run_loop_context::AutoPermissionRuntimeContext;
use crate::agent::runloop::unified::state::SessionStats;

use super::state::CtrlCState;
use approval_cache::{cache_key, record_approval_blocking};
use approval_persistence::{persist_shell_approval_prefix_rule, persisted_shell_approval};
use approval_policy::{approval_policy_rejects_prompt, build_tool_risk_context};
use hook_messages::render_hook_messages;
use permission_events::{emit_permission_requested, emit_permission_resolved};
use permission_prompt::{
    extract_shell_approval_command_words, extract_shell_permission_scope_signature, prompt_tool_permission,
    split_command_words_on_operators,
};
use preflight::handle_policy_denied;
use shell_approval::{
    approval_learning_target, exact_shell_approval_target, persistent_approval_target, tool_display_labels,
};

const MAX_PERMISSION_HOOK_UPDATES: usize = 64;
const MAX_PERMISSION_UPDATE_RULES: usize = 128;
const MAX_SEGMENTED_APPROVAL_KEYS: usize = 5;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitlDecision {
    Approved,
    ApprovedSession,
    ApprovedPermanent,
    Denied,
    DeniedOnce,
    Enable,
    Exit,
    Interrupt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolPermissionFlow {
    Approved { updated_args: Option<Value> },
    Denied,
    Blocked { reason: String },
    Exit,
    Interrupted,
}

enum AutoPermissionPermissionOutcome {
    Allow,
    Block,
    PromptFallback,
    AbortHeadless { reason: String },
}

fn bounded_permission_updates<'a>(
    updates: &'a [vtcode_core::hooks::PermissionUpdateRequest],
    messages: &mut Vec<vtcode_core::hooks::HookMessage>,
) -> &'a [vtcode_core::hooks::PermissionUpdateRequest] {
    if updates.len() <= MAX_PERMISSION_HOOK_UPDATES {
        return updates;
    }

    messages.push(vtcode_core::hooks::HookMessage::warning(format!(
        "PermissionRequest hook returned {} updates; only the first {} were applied.",
        updates.len(),
        MAX_PERMISSION_HOOK_UPDATES
    )));
    &updates[..MAX_PERMISSION_HOOK_UPDATES]
}

fn bounded_permission_rules<'a, T>(
    field: &str,
    rules: &'a [T],
    messages: &mut Vec<vtcode_core::hooks::HookMessage>,
) -> &'a [T] {
    if rules.len() <= MAX_PERMISSION_UPDATE_RULES {
        return rules;
    }

    messages.push(vtcode_core::hooks::HookMessage::warning(format!(
        "PermissionRequest hook returned {} rules for `{field}`; only the first {} were applied.",
        rules.len(),
        MAX_PERMISSION_UPDATE_RULES
    )));
    &rules[..MAX_PERMISSION_UPDATE_RULES]
}

async fn persisted_approval_hit_key(
    tool_registry: &ToolRegistry,
    primary_target: &shell_approval::ApprovalLearningTarget,
    exact_shell_target: Option<&shell_approval::ApprovalLearningTarget>,
) -> Option<String> {
    // Check every key the target contributes: the exact invocation first, then
    // any learned family key. A persisted family key (e.g. a safe `awk` read
    // family) auto-approves equivalent safe invocations whose exact arguments
    // differ. The candidate's family is only attached when
    // `learned_shell_pattern` proved it read-only, so a mutating sibling can
    // never match.
    for (key, _) in primary_target.iter_keys() {
        if tool_registry.has_persisted_approval(key).await {
            return Some(key.to_string());
        }
    }

    let exact_target = exact_shell_target?;
    tool_registry
        .has_persisted_approval(&exact_target.approval_key)
        .await
        .then(|| exact_target.approval_key.clone())
}

fn session_approval_cache_keys<'a>(
    tool_name: &'a str,
    cache_key: &'a str,
    approval_learning_target: &'a shell_approval::ApprovalLearningTarget,
    exact_shell_approval_target: Option<&'a shell_approval::ApprovalLearningTarget>,
) -> impl Iterator<Item = &'a str> {
    // Include the bare tool name only when it differs from the cache key AND
    // the cache key is not already command-scoped. For command-run tools the
    // cache key embeds the exact command (`tool:command`), so also granting
    // the bare tool name would let a single "Approve Once" for one command
    // silently approve every later invocation of that tool, regardless of
    // the command it runs.
    let command_scoped = cache_key.contains(':');
    let bare_if_different = if cache_key != tool_name && !command_scoped {
        Some(tool_name)
    } else {
        None
    };
    std::iter::once(cache_key)
        .chain(bare_if_different)
        .chain(std::iter::once(approval_learning_target.approval_key.as_str()))
        // Also grant the learned family key for "Session" decisions, so a safe
        // `awk`/`find`/`sed` family approved for the session covers equivalent
        // invocations whose exact arguments change (e.g. a new `awk` line range).
        .chain(approval_learning_target.pattern.iter().map(|pattern| pattern.key.as_str()))
        .chain(exact_shell_approval_target.map(|target| target.approval_key.as_str()))
}

async fn persist_approval_cache_key(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    approval_key: &str,
    log_message: &str,
) {
    if let Err(err) = tool_registry.persist_approval_cache_key(approval_key).await {
        tracing::warn!(
            tool = %tool_name,
            approval_key = %approval_key,
            error = %err,
            message = %log_message,
            "Failed to persist approval cache key"
        );
    }
}

/// Persist an approval key and, when it carries the default permission scope,
/// its scope-less variant too. Older cache entries predate scope suffixes, so
/// writing both forms keeps lookups working across the legacy boundary.
async fn persist_approval_key_with_default_scope(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    approval_key: &str,
    log_message: &str,
) {
    persist_approval_cache_key(tool_registry, tool_name, approval_key, log_message).await;
    if let Some(no_scope_key) = strip_default_scope_suffix(approval_key) {
        persist_approval_cache_key(tool_registry, tool_name, no_scope_key, log_message).await;
    }
}

/// Context for tool permission checks to reduce argument count
pub(crate) struct ToolPermissionsContext<'a, S: UiSession + ?Sized> {
    pub tool_registry: &'a ToolRegistry,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session: &'a mut S,
    pub active_thread_label: Option<&'a str>,
    pub default_placeholder: Option<String>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub hooks: Option<&'a LifecycleHookEngine>,
    pub justification: Option<&'a vtcode_core::tools::ToolJustification>,
    pub approval_recorder: Option<&'a vtcode_core::tools::ApprovalRecorder>,
    pub decision_ledger: Option<&'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>>,
    pub tool_permission_cache: Option<&'a Arc<RwLock<ToolPermissionCache>>>,
    pub permissions_state: Option<&'a Arc<RwLock<PermissionsConfig>>>,
    pub active_agent_permissions: Option<&'a vtcode_config::core::permissions::AgentPermissionsConfig>,
    pub hitl_notification_bell: bool,
    pub approval_policy: AskForApproval,
    pub skip_confirmations: bool,
    pub permissions_config: Option<&'a PermissionsConfig>,
    pub auto_permission_runtime: Option<AutoPermissionRuntimeContext<'a>>,
    pub session_stats: Option<&'a mut SessionStats>,
    /// Justification from the safety gateway when it flagged this call as
    /// requiring approval. Folded into the HITL prompt trigger so that
    /// high-risk implicit-allow tools (auto-approved by the low-risk heuristic)
    /// still get gated by the gateway's risk/destructive assessment.
    pub safety_approval_justification: Option<String>,
    /// Optional event emitter for permission request/resolved telemetry.
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
}

mod decisions;

use decisions::{
    apply_permission_hook_updates, approve_tool_permission_no_cache, auto_permission_safe_builtin_allow,
    build_permission_suggestions, finalize_permission_decision, map_permission_decision, policy_evaluation_name,
    resolve_auto_permission, resolve_permission_decision, reuse_saved_approval, should_allow_without_prompt,
    strip_default_scope_suffix,
};
#[cfg(test)]
use decisions::{persist_segment_approval_cache_keys, persisted_segment_approval_hit_key};

#[cfg(test)]
pub(crate) fn ensure_tool_permission<'a, S: UiSession + ?Sized>(
    ctx: ToolPermissionsContext<'a, S>,
    tool_name: &'a str,
    tool_args: Option<&'a Value>,
) -> impl Future<Output = Result<ToolPermissionFlow>> + 'a {
    ensure_tool_permission_with_call_id(ctx, tool_name, tool_args, None, None)
}

#[cfg(test)]
/// Test-only entry that forwards a precomputed PreToolUse hook phase into the
/// permission flow, mirroring the pipeline/validate/copilot callers.
pub(crate) fn ensure_tool_permission_forwarded<'a, S: UiSession + ?Sized>(
    ctx: ToolPermissionsContext<'a, S>,
    tool_name: &'a str,
    tool_args: Option<&'a Value>,
    tool_call_id: Option<&'a str>,
    hook_phase: Option<PreToolHookPhaseResult>,
) -> impl Future<Output = Result<ToolPermissionFlow>> + 'a {
    ensure_tool_permission_with_call_id(ctx, tool_name, tool_args, tool_call_id, hook_phase)
}

/// Outcome of the PreToolUse hook phase.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreToolHookPhaseResult {
    /// A hook denied the tool call; downstream checks must refuse it.
    Deny,
    /// Hooks allowed or did not decide; carries rewritten args (if any) and
    /// whether a hook requested the mandatory prompt path.
    Proceed {
        rewritten_args: Option<Value>,
        requires_prompt: bool,
    },
}

/// Run the PreToolUse hook phase, surfacing Deny decisions explicitly so
/// callers can refuse the tool call.
pub(crate) async fn run_pre_tool_hook_phase(
    hooks: Option<&LifecycleHookEngine>,
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    tool_args: Option<&Value>,
    tool_call_id: Option<&str>,
) -> Result<PreToolHookPhaseResult> {
    let mut rewritten: Option<Value> = None;
    let mut requires_prompt = false;
    if let Some(hooks) = hooks {
        match hooks.run_pre_tool_use(tool_name, tool_args, tool_call_id).await {
            Ok(outcome) => {
                render_hook_messages(renderer, &outcome.messages)?;
                rewritten = outcome.updated_input;
                match outcome.decision {
                    PreToolHookDecision::Allow => {}
                    PreToolHookDecision::Deny => return Ok(PreToolHookPhaseResult::Deny),
                    PreToolHookDecision::Ask => requires_prompt = true,
                    PreToolHookDecision::Continue => {}
                }
            }
            Err(err) => {
                renderer.line(MessageStyle::Error, &format!("Failed to run pre-tool hooks: {err}"))?;
            }
        }
    }
    Ok(PreToolHookPhaseResult::Proceed { rewritten_args: rewritten, requires_prompt })
}

/// Run the PreToolUse hook phase and return the phase result for
/// [`ensure_tool_permission_with_call_id`]. The tool pipeline calls this
/// before safety validation so the gateway evaluates rewritten arguments,
/// then forwards the result into the permission flow.
pub(crate) async fn pipeline_pre_tool_hooks(
    hooks: Option<&LifecycleHookEngine>,
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args_val: &Value,
    tool_call_id: &str,
) -> Result<Option<PreToolHookPhaseResult>> {
    let phase = run_pre_tool_hook_phase(hooks, renderer, tool_name, Some(args_val), Some(tool_call_id)).await?;
    Ok(Some(phase))
}

/// Runs the full permission flow for a tool call.
///
/// `pipeline_pre_tool_hook_result` carries the PreToolUse hook phase result
/// when the caller (the tool pipeline) already ran it before safety
/// validation, so hooks see exactly one invocation per call and the safety
/// gateway evaluates the rewritten arguments. Pass `None` when the caller has
/// not run the hook phase.
pub(crate) async fn ensure_tool_permission_with_call_id<S: UiSession + ?Sized>(
    ctx: ToolPermissionsContext<'_, S>,
    tool_name: &str,
    tool_args: Option<&Value>,
    tool_call_id: Option<&str>,
    pipeline_pre_tool_hook_result: Option<PreToolHookPhaseResult>,
) -> Result<ToolPermissionFlow> {
    let ToolPermissionsContext {
        tool_registry,
        renderer,
        handle,
        session,
        active_thread_label,
        default_placeholder,
        ctrl_c_state,
        ctrl_c_notify,
        hooks,
        justification,
        approval_recorder,
        decision_ledger,
        tool_permission_cache,
        permissions_state,
        active_agent_permissions,
        hitl_notification_bell,
        approval_policy,
        skip_confirmations,
        permissions_config,
        auto_permission_runtime,
        session_stats,
        safety_approval_justification,
        harness_emitter,
    } = ctx;

    // PreToolUse hooks may rewrite tool input (e.g. wrapping shell commands in
    // an output compressor). When the pipeline already ran the hook phase
    // upstream (for safety-gateway visibility), it passes the phase result
    // here; otherwise we run the phase ourselves.
    let phase = match pipeline_pre_tool_hook_result {
        Some(precomputed) => precomputed,
        None => run_pre_tool_hook_phase(hooks, renderer, tool_name, tool_args, tool_call_id).await?,
    };
    let (hook_rewritten_args, hook_requires_prompt) = match phase {
        PreToolHookPhaseResult::Deny => return Ok(ToolPermissionFlow::Denied),
        PreToolHookPhaseResult::Proceed { rewritten_args, requires_prompt } => (rewritten_args, requires_prompt),
    };

    let effective_args: Option<Value> = match (&hook_rewritten_args, tool_args) {
        (Some(updated), _) => Some(updated.clone()),
        (None, original) => original.cloned(),
    };
    let tool_args: Option<&Value> = effective_args.as_ref();

    let cache_key = cache_key(tool_name, tool_args);

    // Check tool permission cache for persisted denials up front.
    if let Some(cache) = tool_permission_cache {
        let permission_cache = cache.read().await;
        if permission_cache.is_denied(&cache_key) || permission_cache.is_denied(tool_name) {
            return Ok(ToolPermissionFlow::Denied);
        }
    }

    let normalized_tool_name = tool_args
        .and_then(|args| {
            tool_registry
                .preflight_validate_call(tool_name, args)
                .ok()
                .map(|outcome| outcome.normalized_tool_name)
        })
        .unwrap_or_else(|| tool_name.to_string());

    // Evaluate the tool policy against the intent-qualified name where the
    // tool has distinct permission classes.
    let policy_eval_name = policy_evaluation_name(tool_name, tool_args);
    let policy_decision = tool_registry.evaluate_tool_policy(policy_eval_name.as_ref()).await?;
    if policy_decision == ToolPermissionDecision::Deny {
        if tool_registry.sandbox_config().is_full_access() {
            return Ok(ToolPermissionFlow::Denied);
        }
        if let Some(flow) = handle_policy_denied(
            tool_registry,
            tool_name,
            tool_args,
            tool_permission_cache,
            renderer,
            handle,
            ctrl_c_state,
            ctrl_c_notify,
            session,
            skip_confirmations,
            tool_call_id,
            harness_emitter,
        )
        .await?
        {
            return Ok(flow);
        }
    }

    let current_dir = tool_registry.workspace_root();
    let permission_request = build_permission_request(current_dir, current_dir, &normalized_tool_name, tool_args);
    let permission_decision = if let Some(state) = permissions_state {
        let permissions_snapshot = state.read().await;
        resolve_permission_decision(
            &permissions_snapshot,
            active_agent_permissions,
            current_dir,
            current_dir,
            &permission_request,
        )
    } else {
        let default_permissions = PermissionsConfig::default();
        resolve_permission_decision(
            permissions_config.unwrap_or(&default_permissions),
            active_agent_permissions,
            current_dir,
            current_dir,
            &permission_request,
        )
    };

    if permission_decision == ResolvedPermissionDecision::Deny {
        return Ok(ToolPermissionFlow::Denied);
    }

    let full_auto_allowlist_active = tool_registry.current_full_auto_allowlist().await.is_some();
    if full_auto_allowlist_active && !tool_registry.is_allowed_in_full_auto(&normalized_tool_name).await {
        return Ok(ToolPermissionFlow::Denied);
    }

    // Full access is an explicit session opt-in. Keep explicit policy denies,
    // hook denials, and the full-auto tool allowlist above authoritative.
    if tool_registry.sandbox_config().is_full_access() && !full_auto_allowlist_active {
        return Ok(approve_tool_permission_no_cache(tool_registry, tool_name, hook_rewritten_args).await);
    }

    // `request_user_input` is itself the user-facing HITL surface. In an
    // inline session it must go directly to the wizard instead of being
    // classified as an ordinary `Other` request and stopped by a second
    // generic permission prompt. Explicit policy/permission denies and the
    // full-auto allowlist have already been enforced above.
    if normalized_tool_name == tools::REQUEST_USER_INPUT
        && renderer.supports_inline_ui()
        && policy_decision == ToolPermissionDecision::Allow
    {
        return Ok(approve_tool_permission_no_cache(tool_registry, tool_name, hook_rewritten_args).await);
    }

    let effective_permission_decision =
        if full_auto_allowlist_active && permission_decision == ResolvedPermissionDecision::Ask {
            ResolvedPermissionDecision::Auto
        } else {
            permission_decision
        };
    let requires_protected_write_prompt = permission_request.requires_protected_write_prompt();
    // The safety gateway flagged this call as requiring approval (destructive,
    // network access, etc.). Force a prompt unless the user explicitly set the
    // tool to Allow — explicit Allow means the user already reviewed the risk.
    let safety_requires_prompt =
        safety_approval_justification.is_some() && policy_decision != ToolPermissionDecision::Allow;
    let raw_requires_rule_prompt = hook_requires_prompt
        || permission_decision == ResolvedPermissionDecision::Ask
        || requires_protected_write_prompt
        || safety_requires_prompt;

    let persisted_shell_approval = persisted_shell_approval(tool_registry, &normalized_tool_name, tool_args).await;

    let mut shell_approval_reason = tool_registry
        .shell_run_approval_reason(&normalized_tool_name, tool_args)
        .await?;
    if persisted_shell_approval.is_some() {
        shell_approval_reason = None;
    }
    if let Some(reason) = shell_approval_reason.as_deref() {
        tracing::debug!(
            tool = %tool_name,
            reason = %reason,
            "Shell execution requires interactive approval"
        );
    }

    let display_labels = tool_display_labels(tool_name, tool_args);
    let approval_learning_target =
        approval_learning_target(&normalized_tool_name, tool_args, &display_labels.learning_label);
    let exact_shell_approval_target =
        exact_shell_approval_target(&normalized_tool_name, tool_args, &display_labels.learning_label);
    let persistent_approval_target =
        persistent_approval_target(&normalized_tool_name, tool_args, &display_labels.learning_label);

    let cached_command_approval = if let Some(cache) = tool_permission_cache {
        let permission_cache = cache.read().await;
        session_approval_cache_keys(
            tool_name,
            &cache_key,
            &approval_learning_target,
            exact_shell_approval_target.as_ref(),
        )
        .any(|key| permission_cache.can_use_cached(key))
    } else {
        false
    };
    if cached_command_approval {
        shell_approval_reason = None;
    }

    let raw_requires_sandbox_prompt = shell_approval_reason.is_some();
    let policy_requires_prompt = policy_decision == ToolPermissionDecision::Prompt;
    let full_auto_promptable = full_auto_allowlist_active
        && (raw_requires_rule_prompt || raw_requires_sandbox_prompt || policy_requires_prompt);
    let requires_rule_prompt = raw_requires_rule_prompt && !full_auto_allowlist_active;
    let auto_permission_classifier_review = (effective_permission_decision == ResolvedPermissionDecision::Auto
        && !auto_permission_safe_builtin_allow(current_dir, &permission_request))
        || full_auto_promptable;
    let requires_sandbox_prompt = raw_requires_sandbox_prompt && !auto_permission_classifier_review;
    // Check saved approval regardless of permission-config "Ask" — the user's
    // explicit permanent/session approval overrides the config-level prompt.
    // However, when the safety gateway flagged this call as needing approval,
    // saved approvals must not short-circuit the prompt — the gateway's risk
    // assessment may differ from the original approval context.
    // In full-auto mode, allow reuse when the session cache confirms a prior
    // approval for this exact command — the user already said yes.
    let can_reuse_saved_approval =
        !safety_requires_prompt && (!auto_permission_classifier_review || cached_command_approval);

    if can_reuse_saved_approval
        && let Some(flow) = reuse_saved_approval(
            tool_registry,
            tool_name,
            &normalized_tool_name,
            tool_args,
            &cache_key,
            tool_permission_cache,
            &approval_learning_target,
            exact_shell_approval_target.as_ref(),
            hook_rewritten_args.clone(),
        )
        .await
    {
        return Ok(flow);
    }

    // If the session permission cache confirmed a prior approval for this
    // exact command, skip the popup even when reuse_saved_approval returned
    // None (e.g. the persisted approval store doesn't have it yet).
    if cached_command_approval && !safety_requires_prompt {
        return Ok(approve_tool_permission_no_cache(tool_registry, tool_name, hook_rewritten_args).await);
    }

    if should_allow_without_prompt(
        current_dir,
        &permission_request,
        effective_permission_decision,
        requires_rule_prompt,
        requires_sandbox_prompt,
    ) {
        return Ok(approve_tool_permission_no_cache(tool_registry, tool_name, hook_rewritten_args).await);
    }

    if policy_decision == ToolPermissionDecision::Allow
        && !requires_rule_prompt
        && !requires_sandbox_prompt
        && !auto_permission_classifier_review
        && !full_auto_allowlist_active
    {
        return Ok(approve_tool_permission_no_cache(tool_registry, tool_name, hook_rewritten_args).await);
    }

    let mut requires_auto_fallback_prompt = false;
    // When the safety gateway flagged this call as needing approval, the
    // auto-permission classifier must not override that decision — the
    // gateway's risk/destructive assessment is the authoritative signal.
    if auto_permission_classifier_review && !safety_requires_prompt {
        let outcome = if let Some(state) = permissions_state {
            let guard = state.read().await;
            resolve_auto_permission(
                renderer,
                tool_registry,
                &normalized_tool_name,
                tool_args,
                &permission_request,
                &guard,
                auto_permission_runtime,
                session_stats,
                !full_auto_allowlist_active,
            )
            .await?
        } else {
            let default_permissions = PermissionsConfig::default();
            resolve_auto_permission(
                renderer,
                tool_registry,
                &normalized_tool_name,
                tool_args,
                &permission_request,
                permissions_config.unwrap_or(&default_permissions),
                auto_permission_runtime,
                session_stats,
                !full_auto_allowlist_active,
            )
            .await?
        };
        match outcome {
            AutoPermissionPermissionOutcome::Allow => {
                return Ok(approve_tool_permission_no_cache(tool_registry, tool_name, hook_rewritten_args).await);
            }
            AutoPermissionPermissionOutcome::Block => return Ok(ToolPermissionFlow::Denied),
            AutoPermissionPermissionOutcome::PromptFallback => {
                requires_auto_fallback_prompt = true;
            }
            AutoPermissionPermissionOutcome::AbortHeadless { reason } => {
                return Ok(ToolPermissionFlow::Blocked { reason });
            }
        }
    }

    // skip_confirmations bypasses the normal HITL prompt flow, but must NOT
    // bypass the safety gateway's NeedsApproval decision. The safety gateway
    // flags destructive/risky tool calls that require explicit human review
    // regardless of the skip_confirmations setting.
    if skip_confirmations && !full_auto_allowlist_active && !safety_requires_prompt {
        return Ok(approve_tool_permission_no_cache(tool_registry, tool_name, hook_rewritten_args).await);
    }

    let should_prompt = requires_rule_prompt
        || requires_sandbox_prompt
        || requires_auto_fallback_prompt
        || (policy_decision == ToolPermissionDecision::Prompt && !auto_permission_classifier_review);
    if !should_prompt {
        return Ok(approve_tool_permission_no_cache(tool_registry, tool_name, hook_rewritten_args).await);
    }

    if approval_policy_rejects_prompt(
        approval_policy,
        requires_rule_prompt || requires_auto_fallback_prompt || policy_decision == ToolPermissionDecision::Prompt,
        requires_sandbox_prompt,
    ) {
        return Ok(ToolPermissionFlow::Denied);
    }

    let prompt_kind = permission_prompt::tool_permission_prompt_kind(tool_name);
    let permission_suggestions = build_permission_suggestions(prompt_kind, Some(&persistent_approval_target));
    if let Some(hooks) = hooks {
        match hooks
            .run_permission_request(tool_name, tool_args, &permission_request, &permission_suggestions)
            .await
        {
            Ok(outcome) => {
                render_hook_messages(renderer, &outcome.messages)?;
                if let Some(decision) = outcome.decision {
                    let update_messages = if let Some(permissions_state) = permissions_state {
                        apply_permission_hook_updates(
                            tool_registry,
                            permissions_state,
                            decision.behavior,
                            &decision.permission_updates,
                        )
                        .await
                    } else if !decision.permission_updates.is_empty() {
                        vec![vtcode_core::hooks::HookMessage::warning(
                            "PermissionRequest hook returned permission updates without runtime permission state; ignoring updates.",
                        )]
                    } else {
                        Vec::new()
                    };
                    render_hook_messages(renderer, &update_messages)?;

                    return finalize_permission_decision(
                        tool_registry,
                        tool_name,
                        &normalized_tool_name,
                        tool_args,
                        &cache_key,
                        tool_permission_cache,
                        approval_recorder,
                        &approval_learning_target,
                        exact_shell_approval_target.as_ref(),
                        &persistent_approval_target,
                        map_permission_decision(decision.behavior, decision.scope, decision.interrupt),
                        decision.updated_input.or_else(|| hook_rewritten_args.clone()),
                    )
                    .await;
                }
            }
            Err(err) => {
                renderer.line(MessageStyle::Error, &format!("Failed to run permission request hooks: {err}"))?
            }
        }
    }

    let mut risk_context = build_tool_risk_context(&normalized_tool_name, tool_args);

    if let Some(recorder) = approval_recorder {
        risk_context.recent_approvals =
            recorder.get_approval_count(&approval_learning_target.approval_key).await as usize;
    }
    let risk_level = ToolRiskScorer::calculate_risk(&risk_context);

    // Extract justification from decision ledger if not provided
    let extracted_justification = if justification.is_none() {
        if let Some(ledger_ref) = decision_ledger {
            let ledger = ledger_ref.read().await;
            if let Some(latest) = ledger.latest_decision() {
                JustificationExtractor::extract_from_decision(latest, tool_name, &risk_level)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let final_justification = justification.or(extracted_justification.as_ref());
    let permission_start = Instant::now();
    emit_permission_requested(harness_emitter, tool_name, tool_call_id);
    let decision = match prompt_tool_permission(
        &display_labels.prompt_label,
        tool_name,
        tool_args,
        &approval_learning_target,
        renderer,
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        shell_approval_reason.as_deref(),
        final_justification,
        Some(&persistent_approval_target),
        approval_recorder,
        hitl_notification_bell,
        active_thread_label.filter(|label| *label != "main"),
    )
    .await
    {
        Ok(d) => d,
        Err(e) => {
            let wait_ms = permission_start.elapsed().as_millis() as u64;
            emit_permission_resolved(harness_emitter, tool_name, tool_call_id, PermissionDecision::Cancelled, wait_ms);
            return Err(e);
        }
    };
    let wait_ms = permission_start.elapsed().as_millis() as u64;
    emit_permission_resolved(
        harness_emitter,
        tool_name,
        tool_call_id,
        permission_events::map_hitl_to_permission_decision(&decision),
        wait_ms,
    );
    finalize_permission_decision(
        tool_registry,
        tool_name,
        &normalized_tool_name,
        tool_args,
        &cache_key,
        tool_permission_cache,
        approval_recorder,
        &approval_learning_target,
        exact_shell_approval_target.as_ref(),
        &persistent_approval_target,
        decision,
        hook_rewritten_args,
    )
    .await
}

pub(crate) async fn prompt_external_tool_permission<S: UiSession + ?Sized>(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    tool_name: &str,
    tool_args: Option<&Value>,
    display_name: &str,
    approval_learning_key: &str,
    approval_learning_label: &str,
    approval_reason: Option<&str>,
    approval_recorder: Option<&vtcode_core::tools::ApprovalRecorder>,
    hitl_notification_bell: bool,
    active_thread_label: Option<&str>,
) -> Result<HitlDecision> {
    let learning_target = shell_approval::ApprovalLearningTarget::new(
        approval_learning_key.to_owned(),
        approval_learning_label.to_owned(),
    );
    prompt_tool_permission(
        display_name,
        tool_name,
        tool_args,
        &learning_target,
        renderer,
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        approval_reason,
        None,
        None,
        approval_recorder,
        hitl_notification_bell,
        active_thread_label.filter(|label| *label != "main"),
    )
    .await
}

pub(crate) async fn prompt_tool_loop_limit_increase<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_limit: usize,
    hard_cap: usize,
    agent_name: Option<&str>,
) -> Result<Option<usize>> {
    limit_prompts::prompt_tool_loop_limit_increase(
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        max_limit,
        hard_cap,
        agent_name,
    )
    .await
}
