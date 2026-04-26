//! Prompt/catalog alignment guard for agent turns.
//!
//! Before each turn the system prompt, the tool catalog snapshot, and the live
//! plan-mode flag must all be mutually consistent.  This module provides a
//! lightweight pure-function check so the runner can detect and self-heal any
//! divergence before dispatching to the LLM provider.

use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;

/// A misalignment between the system prompt and the tool catalog snapshot.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum AlignmentError {
    /// The catalog was built with a different plan-mode flag than the live registry.
    /// The caller should re-snapshot the tool catalog to self-heal.
    PlanModeMismatch {
        snapshot_plan_mode: bool,
        registry_plan_mode: bool,
    },
    /// A mutating tool name appears in a plan-mode system prompt.
    ///
    /// This should never occur after Phase 2-F/G (cache invalidation on plan mode
    /// transition + catalog epoch in the prompt cache key).  If it fires, treat it
    /// as a canary metric indicating stale prompt generation.
    MutatingToolInPlanModePrompt { tool_name: &'static str },
}

/// Validate consistency between the system prompt, tool catalog, and plan mode flag.
///
/// Call this once per turn after both `system_instruction` and `tool_snapshot` are
/// ready, but before the request is dispatched to the LLM provider.
///
/// # Errors
///
/// Returns the first misalignment found.  Returns `Ok(())` when all three pieces of
/// state are consistent.
pub(super) fn validate_prompt_catalog_alignment(
    system_instruction: &str,
    tool_snapshot: &SessionToolCatalogSnapshot,
    plan_mode: bool,
) -> Result<(), AlignmentError> {
    if tool_snapshot.plan_mode != plan_mode {
        return Err(AlignmentError::PlanModeMismatch {
            snapshot_plan_mode: tool_snapshot.plan_mode,
            registry_plan_mode: plan_mode,
        });
    }

    // Canary: mutating tool signatures that must never appear in a plan-mode prompt.
    // After Phase 2-F/G these should be unreachable; the check is intentionally cheap.
    if plan_mode {
        const MUTATING_HINTS: &[&str] = &["apply_patch", "unified_file write", "unified_file edit"];
        for &hint in MUTATING_HINTS {
            if system_instruction.contains(hint) {
                return Err(AlignmentError::MutatingToolInPlanModePrompt { tool_name: hint });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn snapshot(plan_mode: bool) -> SessionToolCatalogSnapshot {
        SessionToolCatalogSnapshot::new(1, 1, plan_mode, false, Some(Arc::new(Vec::new())), false)
    }

    #[test]
    fn alignment_ok_when_plan_modes_match() {
        assert!(
            validate_prompt_catalog_alignment("normal prompt", &snapshot(false), false).is_ok()
        );
        assert!(validate_prompt_catalog_alignment("plan prompt", &snapshot(true), true).is_ok());
    }

    #[test]
    fn plan_mode_mismatch_detected() {
        let err = validate_prompt_catalog_alignment("any prompt", &snapshot(false), true)
            .expect_err("mismatch should be detected");
        assert_eq!(
            err,
            AlignmentError::PlanModeMismatch {
                snapshot_plan_mode: false,
                registry_plan_mode: true,
            }
        );
    }

    #[test]
    fn mutating_tool_in_plan_mode_prompt_detected() {
        let err = validate_prompt_catalog_alignment(
            "you may call apply_patch to write files",
            &snapshot(true),
            true,
        )
        .expect_err("canary should fire");
        assert_eq!(
            err,
            AlignmentError::MutatingToolInPlanModePrompt {
                tool_name: "apply_patch"
            }
        );
    }

    #[test]
    fn no_false_positive_in_normal_mode_with_apply_patch() {
        // Mentioning apply_patch in a normal-mode prompt is fine.
        assert!(
            validate_prompt_catalog_alignment("you may call apply_patch", &snapshot(false), false)
                .is_ok()
        );
    }
}
