use super::*;

pub(super) fn resolve_permission_decision(
    permissions: &PermissionsConfig,
    active_agent_permissions: Option<&vtcode_config::core::permissions::AgentPermissionsConfig>,
    workspace_root: &std::path::Path,
    current_dir: &std::path::Path,
    request: &PermissionRequest,
) -> ResolvedPermissionDecision {
    if let Some(agent_permissions) = active_agent_permissions {
        return evaluate_effective_permissions(permissions, agent_permissions, workspace_root, current_dir, request);
    }

    let matches = evaluate_permissions(permissions, workspace_root, current_dir, request);
    if matches.deny {
        return ResolvedPermissionDecision::Deny;
    }
    if matches.ask {
        return ResolvedPermissionDecision::Ask;
    }
    if matches.allow {
        return ResolvedPermissionDecision::Allow;
    }

    ResolvedPermissionDecision::Ask
}

pub(super) fn build_permission_suggestions(
    prompt_kind: permission_prompt::ToolPermissionPromptKind,
    persistent_approval_target: Option<&shell_approval::PersistentApprovalTarget>,
) -> Vec<Value> {
    let mut suggestions = vec![
        serde_json::json!({
            "id": "allow_once",
            "behavior": "allow",
            "scope": "once",
        }),
        serde_json::json!({
            "id": "allow_session",
            "behavior": "allow",
            "scope": "session",
        }),
        serde_json::json!({
            "id": "deny_once",
            "behavior": "deny",
            "scope": "once",
        }),
    ];

    if persistent_approval_target.is_some() {
        suggestions.push(serde_json::json!({
            "id": "allow_permanent",
            "behavior": "allow",
            "scope": "permanent",
        }));
    }

    if matches!(persistent_approval_target, Some(shell_approval::PersistentApprovalTarget::ToolLevel))
        && prompt_kind != permission_prompt::ToolPermissionPromptKind::Mcp
    {
        suggestions.push(serde_json::json!({
            "id": "deny_permanent",
            "behavior": "deny",
            "scope": "permanent",
        }));
    }

    suggestions
}

pub(super) async fn apply_permission_hook_updates(
    tool_registry: &ToolRegistry,
    permissions_state: &Arc<RwLock<PermissionsConfig>>,
    behavior: PermissionDecisionBehavior,
    updates: &[vtcode_core::hooks::PermissionUpdateRequest],
) -> Vec<vtcode_core::hooks::HookMessage> {
    use std::collections::BTreeSet;
    use vtcode_core::hooks::{HookMessage, PermissionUpdateDestination, PermissionUpdateKind};

    let mut messages = Vec::new();
    if updates.is_empty() {
        return messages;
    }

    let updates = bounded_permission_updates(updates, &mut messages);

    let mut state = permissions_state.write().await;
    let mut changed = false;
    let mut persist_project = false;

    for update in updates {
        match (&update.destination, &update.kind) {
            (PermissionUpdateDestination::Unsupported(destination), _) => {
                messages.push(HookMessage::warning(format!(
                    "PermissionRequest hook ignored unsupported destination `{destination}`"
                )));
            }
            (_, PermissionUpdateKind::Unsupported(field)) => {
                messages.push(HookMessage::warning(format!(
                    "PermissionRequest hook ignored unsupported permission update `{field}`"
                )));
            }
            (destination, PermissionUpdateKind::AddRules(rules)) => {
                let rules = bounded_permission_rules("add_rules", rules, &mut messages);
                let target = match behavior {
                    PermissionDecisionBehavior::Allow => &mut state.allow,
                    PermissionDecisionBehavior::Deny => &mut state.deny,
                };
                let mut seen = target.iter().cloned().collect::<BTreeSet<_>>();
                for rule in rules {
                    if seen.insert(rule.clone()) {
                        target.push(rule.clone());
                        changed = true;
                    }
                }
                persist_project |= matches!(destination, PermissionUpdateDestination::ProjectSettings);
            }
            (destination, PermissionUpdateKind::ReplaceRules(rules)) => {
                let rules = bounded_permission_rules("replace_rules", rules, &mut messages);
                let target = match behavior {
                    PermissionDecisionBehavior::Allow => &mut state.allow,
                    PermissionDecisionBehavior::Deny => &mut state.deny,
                };
                if target.as_slice() != rules {
                    *target = rules.to_vec();
                    changed = true;
                }
                persist_project |= matches!(destination, PermissionUpdateDestination::ProjectSettings);
            }
            (destination, PermissionUpdateKind::RemoveRules(rules)) => {
                let rules = bounded_permission_rules("remove_rules", rules, &mut messages);
                let target = match behavior {
                    PermissionDecisionBehavior::Allow => &mut state.allow,
                    PermissionDecisionBehavior::Deny => &mut state.deny,
                };
                let initial_len = target.len();
                target.retain(|rule| !rules.iter().any(|candidate| candidate == rule));
                changed |= target.len() != initial_len;
                persist_project |= matches!(destination, PermissionUpdateDestination::ProjectSettings);
            }
        }
    }

    if !changed {
        return messages;
    }

    tool_registry.apply_permissions_config(&state);

    if persist_project {
        let config_for_persistence = state.clone();
        drop(state);
        let workspace_root = tool_registry.workspace_root().clone();
        match ConfigManager::load_from_workspace(&workspace_root) {
            Ok(mut manager) => {
                let mut config = manager.config().clone();
                config.permissions = config_for_persistence;
                if let Err(err) = manager.save_config(&config) {
                    messages.push(HookMessage::error(format!(
                        "PermissionRequest hook failed to persist project settings: {err}"
                    )));
                }
            }
            Err(err) => messages.push(HookMessage::error(format!(
                "PermissionRequest hook failed to load project configuration: {err}"
            ))),
        }
    }

    messages
}

async fn approve_tool_permission(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    cache_keys: &[&str],
    tool_permission_cache: Option<&Arc<RwLock<ToolPermissionCache>>>,
    grant: Option<PermissionGrant>,
    updated_args: Option<Value>,
) -> ToolPermissionFlow {
    tool_registry.mark_tool_preapproved(tool_name).await;

    if let (Some(grant), Some(cache)) = (grant, tool_permission_cache) {
        let mut permission_cache = cache.write().await;
        for cache_key in cache_keys {
            permission_cache.cache_grant((*cache_key).to_string(), grant);
        }
    }

    ToolPermissionFlow::Approved { updated_args }
}

/// Lightweight approval helper for paths that don't need cache updates.
/// Centralizes `mark_tool_preapproved` + `Approved` return to prevent
/// the bug where a path returns `Approved` without preapproving.
pub(super) async fn approve_tool_permission_no_cache(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    updated_args: Option<Value>,
) -> ToolPermissionFlow {
    tool_registry.mark_tool_preapproved(tool_name).await;
    ToolPermissionFlow::Approved { updated_args }
}

pub(super) fn map_permission_decision(
    behavior: PermissionDecisionBehavior,
    scope: PermissionDecisionScope,
    interrupt: bool,
) -> HitlDecision {
    match (behavior, scope, interrupt) {
        (PermissionDecisionBehavior::Allow, PermissionDecisionScope::Once, _) => HitlDecision::Approved,
        (PermissionDecisionBehavior::Allow, PermissionDecisionScope::Session, _) => HitlDecision::ApprovedSession,
        (PermissionDecisionBehavior::Allow, PermissionDecisionScope::Permanent, _) => HitlDecision::ApprovedPermanent,
        (PermissionDecisionBehavior::Deny, _, true) => HitlDecision::Interrupt,
        (PermissionDecisionBehavior::Deny, PermissionDecisionScope::Permanent, _) => HitlDecision::Denied,
        (PermissionDecisionBehavior::Deny, _, false) => HitlDecision::DeniedOnce,
    }
}

pub(super) fn should_allow_without_prompt(
    workspace_root: &std::path::Path,
    permission_request: &PermissionRequest,
    permission_decision: ResolvedPermissionDecision,
    requires_rule_prompt: bool,
    requires_sandbox_prompt: bool,
) -> bool {
    if requires_rule_prompt || requires_sandbox_prompt {
        return false;
    }

    permission_decision == ResolvedPermissionDecision::Allow
        || (permission_decision == ResolvedPermissionDecision::Auto
            && auto_permission_safe_builtin_allow(workspace_root, permission_request))
}

fn full_auto_unavailable_reason(tool_name: &str) -> String {
    format!("Auto permission review is unavailable for `{tool_name}` while full-auto permission review is active.")
}

/// Resolve the tool name used for policy evaluation, qualifying it with the
/// requested action when the action changes the tool's risk profile.
///
/// Network-bearing actions are evaluated under action-qualified names so the
/// policy and risk scorer make the same decision.
pub(super) fn policy_evaluation_name<'a>(tool_name: &'a str, tool_args: Option<&Value>) -> Cow<'a, str> {
    use vtcode_core::tools::tool_intent::action_qualified_policy_name;

    if let Some(qualified) = action_qualified_policy_name(tool_name, tool_args) {
        return Cow::Borrowed(qualified);
    }

    Cow::Borrowed(tool_name)
}

pub(super) async fn reuse_saved_approval(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
    cache_key: &str,
    tool_permission_cache: Option<&Arc<RwLock<ToolPermissionCache>>>,
    approval_learning_target: &shell_approval::ApprovalLearningTarget,
    exact_shell_approval_target: Option<&shell_approval::ApprovalLearningTarget>,
    hook_rewritten_args: Option<Value>,
) -> Option<ToolPermissionFlow> {
    if let Some(approval_key) = persisted_segment_approval_hit_key(tool_registry, normalized_tool_name, tool_args).await
    {
        tracing::debug!(
            approval_key = %approval_key,
            "Using persisted segmented shell approval cache entries"
        );
        return Some(
            approve_tool_permission(
                tool_registry,
                tool_name,
                &[cache_key],
                tool_permission_cache,
                Some(PermissionGrant::Permanent),
                hook_rewritten_args.clone(),
            )
            .await,
        );
    }

    if let Some(approval_key) =
        persisted_approval_hit_key(tool_registry, approval_learning_target, exact_shell_approval_target).await
    {
        tracing::debug!(
            approval_key = %approval_key,
            "Using persisted approval cache entry"
        );
        return Some(
            approve_tool_permission(
                tool_registry,
                tool_name,
                &[cache_key],
                tool_permission_cache,
                Some(PermissionGrant::Permanent),
                hook_rewritten_args,
            )
            .await,
        );
    }

    let cache = tool_permission_cache?;

    let permission_cache = cache.read().await;
    if session_approval_cache_keys(tool_name, cache_key, approval_learning_target, exact_shell_approval_target)
        .any(|key| permission_cache.can_use_cached(key))
    {
        tracing::debug!("Using cached ACP permission for tool invocation: {}", cache_key);
        drop(permission_cache);
        return Some(approve_tool_permission_no_cache(tool_registry, tool_name, hook_rewritten_args).await);
    }

    None
}

#[expect(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
pub(super) async fn finalize_permission_decision(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
    cache_key: &str,
    tool_permission_cache: Option<&Arc<RwLock<ToolPermissionCache>>>,
    approval_recorder: Option<&vtcode_core::tools::ApprovalRecorder>,
    approval_learning_target: &shell_approval::ApprovalLearningTarget,
    exact_shell_approval_target: Option<&shell_approval::ApprovalLearningTarget>,
    persistent_approval_target: &shell_approval::PersistentApprovalTarget,
    decision: HitlDecision,
    updated_args: Option<Value>,
) -> Result<ToolPermissionFlow> {
    match decision {
        HitlDecision::Approved | HitlDecision::ApprovedSession => {
            let grant = if decision == HitlDecision::Approved {
                PermissionGrant::Once
            } else {
                PermissionGrant::Session
            };

            if let Some(recorder) = approval_recorder {
                record_approval_blocking(recorder, approval_learning_target, true).await;
            }

            Ok(approve_tool_permission(
                tool_registry,
                tool_name,
                &session_approval_cache_keys(
                    tool_name,
                    cache_key,
                    approval_learning_target,
                    exact_shell_approval_target,
                )
                .collect::<Vec<_>>(),
                tool_permission_cache,
                Some(grant),
                updated_args,
            )
            .await)
        }
        HitlDecision::ApprovedPermanent => {
            if let shell_approval::PersistentApprovalTarget::PrefixRule { prefix_rule, .. } = persistent_approval_target
            {
                match persist_shell_approval_prefix_rule(tool_registry, tool_name, tool_args, prefix_rule.as_slice())
                    .await
                {
                    Ok(rendered_rule) => {
                        tracing::info!(
                            tool = %tool_name,
                            prefix_rule = %rendered_rule,
                            "✓ Shell approval prefix persisted"
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            tool = %tool_name,
                            error = %err,
                            "Failed to persist shell approval prefix"
                        );
                    }
                }
            }

            // Persist the learned family key too. Without it, "Always approve"
            // on a safe family member (e.g. one `awk` line range) writes only
            // the exact key, which changes with the next argument and re-prompts.
            if let shell_approval::PersistentApprovalTarget::LearnedPattern { key, .. } = persistent_approval_target {
                persist_approval_key_with_default_scope(
                    tool_registry,
                    tool_name,
                    key,
                    "Failed to persist learned shell family approval",
                )
                .await;
            }

            persist_approval_key_with_default_scope(
                tool_registry,
                tool_name,
                &approval_learning_target.approval_key,
                "Failed to persist approval cache entry",
            )
            .await;
            if let Some(exact_target) = exact_shell_approval_target
                && exact_target.approval_key != approval_learning_target.approval_key
            {
                persist_approval_key_with_default_scope(
                    tool_registry,
                    tool_name,
                    &exact_target.approval_key,
                    "Failed to persist exact shell approval cache entry",
                )
                .await;
            }
            persist_segment_approval_cache_keys(tool_registry, tool_name, normalized_tool_name, tool_args).await;

            if let Some(recorder) = approval_recorder {
                record_approval_blocking(recorder, approval_learning_target, true).await;
            }

            Ok(approve_tool_permission(
                tool_registry,
                tool_name,
                &session_approval_cache_keys(
                    tool_name,
                    cache_key,
                    approval_learning_target,
                    exact_shell_approval_target,
                )
                .collect::<Vec<_>>(),
                tool_permission_cache,
                Some(PermissionGrant::Permanent),
                updated_args,
            )
            .await)
        }
        HitlDecision::Denied | HitlDecision::DeniedOnce => {
            if decision == HitlDecision::Denied {
                if let Some(cache) = tool_permission_cache {
                    let mut perm_cache = cache.write().await;
                    perm_cache.cache_grant(tool_name.to_string(), PermissionGrant::Denied);
                }

                if let Err(err) = tool_registry.set_tool_policy(tool_name, ToolPolicy::Deny).await {
                    tracing::warn!("Failed to persist denial for tool '{}': {}", tool_name, err);
                }

                if let Err(err) = tool_registry.persist_mcp_tool_policy(tool_name, ToolPolicy::Deny).await {
                    tracing::warn!("Failed to persist MCP denial for tool '{}': {}", tool_name, err);
                }
            }

            if let Some(recorder) = approval_recorder {
                // Record denial against the same key family as approvals so
                // pattern approval-rate stays honest when the user pushes back.
                record_approval_blocking(recorder, approval_learning_target, false).await;
            }

            Ok(ToolPermissionFlow::Denied)
        }
        HitlDecision::Exit => Ok(ToolPermissionFlow::Exit),
        HitlDecision::Interrupt => Ok(ToolPermissionFlow::Interrupted),
        // Enable is handled inline in the policy denial check, never reaches here
        HitlDecision::Enable => Ok(ToolPermissionFlow::Denied),
    }
}

pub(super) fn auto_permission_safe_builtin_allow(
    workspace_root: &std::path::Path,
    request: &PermissionRequest,
) -> bool {
    match &request.kind {
        PermissionRequestKind::Read { .. } => true,
        PermissionRequestKind::Edit { paths } | PermissionRequestKind::Write { paths } => {
            request.builtin_file_mutation
                && request.protected_write_paths.is_empty()
                && !paths.is_empty()
                && paths.iter().all(|path| path.strip_prefix(workspace_root).is_ok())
        }
        PermissionRequestKind::Bash { .. }
        | PermissionRequestKind::WebFetch { .. }
        | PermissionRequestKind::Mcp { .. }
        | PermissionRequestKind::Other => false,
    }
}

#[cold]
fn headless_auto_permission_fallback_reason(
    tool_name: &str,
    denial: Option<&crate::agent::runloop::unified::state::AutoPermissionDenial>,
) -> String {
    let Some(denial) = denial else {
        return format!(
            "Auto permission review cannot fall back to manual prompts for `{tool_name}` in non-interactive runs."
        );
    };

    let mut reason = format!(
        "Auto permission review blocked `{tool_name}` and reached its denial threshold in non-interactive runs: {}",
        denial.reason
    );
    if let Some(rule) = denial.matched_rule.as_deref() {
        let _ = write!(reason, " (matched rule: {rule})");
    }
    if let Some(exception) = denial.matched_exception.as_deref() {
        let _ = write!(reason, " (matched exception: {exception})");
    }
    reason
}

pub(super) async fn resolve_auto_permission(
    renderer: &mut AnsiRenderer,
    tool_registry: &ToolRegistry,
    tool_name: &str,
    tool_args: Option<&Value>,
    permission_request: &PermissionRequest,
    permissions: &PermissionsConfig,
    auto_permission_runtime: Option<AutoPermissionRuntimeContext<'_>>,
    session_stats: Option<&mut SessionStats>,
    allow_prompt_fallback: bool,
) -> Result<AutoPermissionPermissionOutcome> {
    let Some(stats) = session_stats else {
        tracing::warn!(tool = %tool_name, "auto permission review reviewer missing session stats");
        if !allow_prompt_fallback {
            return Ok(AutoPermissionPermissionOutcome::AbortHeadless {
                reason: full_auto_unavailable_reason(tool_name),
            });
        }
        return Ok(AutoPermissionPermissionOutcome::PromptFallback);
    };

    if stats.auto_permission_prompt_fallback_active() {
        tracing::trace!(tool = %tool_name, "auto permission review prompt fallback active");
        if !allow_prompt_fallback || !renderer.supports_inline_ui() {
            return Ok(AutoPermissionPermissionOutcome::AbortHeadless {
                reason: headless_auto_permission_fallback_reason(tool_name, stats.last_auto_permission_denial()),
            });
        }
        return Ok(AutoPermissionPermissionOutcome::PromptFallback);
    }

    let Some(auto_permission_runtime) = auto_permission_runtime else {
        tracing::warn!(tool = %tool_name, "auto permission review reviewer missing runtime context");
        if !allow_prompt_fallback {
            return Ok(AutoPermissionPermissionOutcome::AbortHeadless {
                reason: full_auto_unavailable_reason(tool_name),
            });
        }
        return Ok(AutoPermissionPermissionOutcome::PromptFallback);
    };

    match review_tool_call(
        auto_permission_runtime.provider_client,
        auto_permission_runtime.config,
        auto_permission_runtime.vt_cfg,
        permissions,
        tool_registry.workspace_root(),
        auto_permission_runtime.working_history,
        tool_name,
        tool_args,
        permission_request,
    )
    .await
    {
        Ok(AutoPermissionReviewDecision::Allow { stage }) => {
            stats.record_auto_permission_allow();
            tracing::trace!(tool = %tool_name, stage, "auto permission review approved tool");
            Ok(AutoPermissionPermissionOutcome::Allow)
        }
        Ok(AutoPermissionReviewDecision::Block(denial)) => {
            let fallback_was_active = stats.auto_permission_prompt_fallback_active();
            let fallback = stats.record_auto_permission_denial(
                denial.clone(),
                permissions.auto_permission.max_consecutive_denials,
                permissions.auto_permission.max_total_denials,
            );
            tracing::trace!(
                tool = %tool_name,
                stage = denial.stage,
                matched_rule = denial.matched_rule.as_deref().unwrap_or(""),
                matched_exception = denial.matched_exception.as_deref().unwrap_or(""),
                fallback,
                "auto permission review blocked tool"
            );

            if fallback && !fallback_was_active {
                if !renderer.supports_inline_ui() {
                    return Ok(AutoPermissionPermissionOutcome::AbortHeadless {
                        reason: headless_auto_permission_fallback_reason(
                            tool_name,
                            stats.last_auto_permission_denial(),
                        ),
                    });
                }
                renderer.line(
                    MessageStyle::Info,
                    "Auto permission review fell back to manual prompts after repeated classifier denials.",
                )?;
            }

            Ok(AutoPermissionPermissionOutcome::Block)
        }
        Err(err) => {
            tracing::warn!(tool = %tool_name, error = %err, "auto permission review reviewer failed");
            if !allow_prompt_fallback {
                return Ok(AutoPermissionPermissionOutcome::AbortHeadless {
                    reason: full_auto_unavailable_reason(tool_name),
                });
            }
            Ok(AutoPermissionPermissionOutcome::PromptFallback)
        }
    }
}

fn segmented_shell_approval_keys(normalized_tool_name: &str, tool_args: Option<&Value>) -> Option<Vec<String>> {
    let scope_signature = extract_shell_permission_scope_signature(normalized_tool_name, tool_args)?;
    let command_words = extract_shell_approval_command_words(normalized_tool_name, tool_args)?;
    let segments = parse_bash_lc_commands(&command_words)
        .or_else(|| split_command_words_on_operators(&command_words))
        .or_else(|| {
            vtcode_core::command_safety::shell_parser::parse_shell_commands(&shell_words::join(
                command_words.iter().map(String::as_str),
            ))
            .ok()
        })?;

    let keys = segments
        .into_iter()
        .filter(|segment| !segment.is_empty())
        .take(MAX_SEGMENTED_APPROVAL_KEYS)
        .map(|segment| {
            let rendered = shell_words::join(segment.iter().map(String::as_str));
            format!("{rendered}|{scope_signature}")
        })
        .collect::<Vec<_>>();

    (!keys.is_empty()).then_some(keys)
}

pub(super) fn strip_default_scope_suffix(key: &str) -> Option<&str> {
    const DEFAULT_SUFFIX: &str = "|sandbox_permissions=\"use_default\"|additional_permissions=null";
    key.strip_suffix(DEFAULT_SUFFIX)
}

pub(super) async fn persisted_segment_approval_hit_key(
    tool_registry: &ToolRegistry,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let keys = segmented_shell_approval_keys(normalized_tool_name, tool_args)?;

    for key in &keys {
        let has_match = tool_registry.has_persisted_approval(key).await || {
            let no_scope = strip_default_scope_suffix(key);
            if let Some(no_scope_key) = no_scope {
                tool_registry.has_persisted_approval(no_scope_key).await
            } else {
                false
            }
        };
        if !has_match {
            return None;
        }
    }

    Some(keys.join(", "))
}

pub(super) async fn persist_segment_approval_cache_keys(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
) {
    let Some(approval_keys) = segmented_shell_approval_keys(normalized_tool_name, tool_args) else {
        return;
    };

    for approval_key in approval_keys {
        persist_approval_key_with_default_scope(
            tool_registry,
            tool_name,
            &approval_key,
            "Failed to persist segmented shell approval cache entry",
        )
        .await;
    }
}
