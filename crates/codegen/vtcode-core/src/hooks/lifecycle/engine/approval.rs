//! Workspace lifecycle-hook approval: digesting and restoring approvals.
//!
//! These helpers bind a persisted or in-memory approval to the exact compiled
//! command set of an engine, so any configuration change leaves the engine
//! fail-closed. Split from `engine.rs` so approval policy stays separate from
//! hook orchestration; all items are re-exported from the parent.

use super::*;

/// Stable digest over the engine's exact compiled command set. Deterministic
/// across processes so approvals can be persisted and revalidated. Commands
/// are serialized as JSON lines (length-safe, no delimiter ambiguity) and
/// hashed with SHA-256, matching the codebase's config-fingerprint approach
/// (collision-resistant; a crafted command set cannot collide with an
/// approved one).
pub(super) fn commands_digest(hooks: &CompiledLifecycleHooks) -> String {
    let mut lines: Vec<String> = Vec::new();
    hooks.for_each_command(|event, matcher, command| {
        let entry = LifecycleHookCommandPreview {
            event,
            matcher: matcher.map(str::to_string),
            command: command.command.clone(),
            timeout_seconds: command.timeout_seconds,
        };
        if let Ok(line) = serde_json::to_string(&entry) {
            lines.push(line);
        }
    });
    lines.sort();
    vtcode_commons::utils::calculate_sha256(lines.join("\n").as_bytes())
}

/// Restore a persisted workspace lifecycle-hook approval onto a freshly built
/// engine. The persisted record only applies when its digest still matches the
/// engine's exact command set; any configuration change since approval leaves
/// the engine fail-closed.
pub async fn restore_workspace_hook_approval(engine: &LifecycleHookEngine, workspace: &std::path::Path) {
    if !engine.workspace_gated() {
        return;
    }
    let digest = engine.command_digest().to_string();
    let Ok(Some(record)) = load_lifecycle_hook_approval(workspace).await else {
        return;
    };
    if record.config_digest == digest {
        engine.approve_workspace_hooks().await;
    }
}

/// Carry an in-memory approval from a previous engine onto a rebuilt engine
/// (primary-agent switch or config reload), falling back to the persisted
/// record. A session-only approval that could not be persisted (e.g. dot-config
/// write failure) survives the rebuild while the command set is unchanged,
/// instead of being silently dropped mid-session. Fails closed in every other
/// case: a changed command set, a previously unapproved engine, or a gated
/// engine with no matching persisted record leaves the rebuilt engine gated.
pub async fn carry_or_restore_workspace_hook_approval(
    previous: Option<&LifecycleHookEngine>,
    next: &LifecycleHookEngine,
    workspace: &std::path::Path,
) {
    let previous_approved_same_set = match previous {
        Some(prev) => {
            prev.workspace_gated()
                && prev.command_digest() == next.command_digest()
                && !prev.workspace_hooks_need_approval().await
        }
        None => false,
    };
    if previous_approved_same_set {
        next.approve_workspace_hooks().await;
    } else {
        restore_workspace_hook_approval(next, workspace).await;
    }
}
