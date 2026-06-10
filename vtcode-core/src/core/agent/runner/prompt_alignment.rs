//! Prompt/catalog alignment guard for agent turns.
//!
//! Before each turn the system prompt, the tool catalog snapshot, and the live
//! planning workflow flag must all be mutually consistent.  This module provides a
//! lightweight pure-function check so the runner can detect and self-heal any
//! divergence before dispatching to the LLM provider.

use futures::future::BoxFuture;
use tracing::warn;

use crate::core::agent::harness_kernel::SessionToolCatalogSnapshot;
use crate::prompts::system::{
    PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE, PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE,
};

/// A misalignment between the system prompt and the tool catalog snapshot.
#[derive(Debug, PartialEq, Eq)]
pub enum AlignmentError {
    /// The catalog was built with a different planning workflow flag than the live registry.
    /// The caller should re-snapshot the tool catalog to self-heal.
    PlanningWorkflowMismatch {
        snapshot_planning_active: bool,
        registry_planning_active: bool,
    },
    /// The catalog was built with a different `request_user_input` flag than the
    /// live runtime.
    RequestUserInputMismatch {
        snapshot_request_user_input_enabled: bool,
        runtime_request_user_input_enabled: bool,
    },
    /// The system prompt carries the wrong planning workflow interview policy line for
    /// the current runtime.
    PlanningWorkflowPromptPolicyMismatch { expected_line: &'static str },
    /// A mutating tool name appears in a planning workflow system prompt.
    ///
    /// This should never occur after Phase 2-F/G (cache invalidation on planning workflow
    /// transition + catalog epoch in the prompt cache key).  If it fires, treat it
    /// as a canary metric indicating stale prompt generation.
    MutatingToolInPlanningWorkflowPrompt { tool_name: &'static str },
    /// The prompt's runtime tool metadata does not match the live snapshot.
    PromptCatalogMetadataMismatch {
        field: &'static str,
        prompt_value: String,
        snapshot_value: String,
    },
}

impl AlignmentError {
    #[must_use]
    pub const fn should_rebuild_runtime_prompt(&self) -> bool {
        true
    }
}

pub async fn rebuild_once_on_alignment_mismatch<S, T, E, Build, Validate>(
    state: &mut S,
    mut current: T,
    mut build: Build,
    validate: Validate,
    rebuild_message: &'static str,
    persisted_message: &'static str,
) -> Result<T, E>
where
    Build: for<'a> FnMut(&'a mut S) -> BoxFuture<'a, Result<T, E>>,
    Validate: Fn(&S, &T) -> Result<(), AlignmentError>,
{
    let mut alignment_error = validate(&*state, &current).err();

    if alignment_error
        .as_ref()
        .is_some_and(AlignmentError::should_rebuild_runtime_prompt)
    {
        warn!(error = ?alignment_error, "{rebuild_message}");
        current = build(state).await?;
        alignment_error = validate(&*state, &current).err();
    }

    if let Some(error) = alignment_error {
        warn!(error = ?error, "{persisted_message}");
    }

    Ok(current)
}

#[derive(Debug, Default)]
struct RuntimeToolCatalogMetadata {
    version: Option<u64>,
    epoch: Option<u64>,
    available_tools: Option<usize>,
    request_user_input_enabled: Option<bool>,
}

/// Validate consistency between the system prompt, tool catalog, and planning workflow flag.
///
/// Call this once per turn after both `system_instruction` and `tool_snapshot` are
/// ready, but before the request is dispatched to the LLM provider.
///
/// # Errors
///
/// Returns the first misalignment found.  Returns `Ok(())` when all three pieces of
/// state are consistent.
pub fn validate_prompt_catalog_alignment(
    system_instruction: &str,
    tool_snapshot: &SessionToolCatalogSnapshot,
    planning_active: bool,
    request_user_input_enabled: bool,
) -> Result<(), AlignmentError> {
    if tool_snapshot.planning_active != planning_active {
        return Err(AlignmentError::PlanningWorkflowMismatch {
            snapshot_planning_active: tool_snapshot.planning_active,
            registry_planning_active: planning_active,
        });
    }

    if tool_snapshot.request_user_input_enabled != request_user_input_enabled {
        return Err(AlignmentError::RequestUserInputMismatch {
            snapshot_request_user_input_enabled: tool_snapshot.request_user_input_enabled,
            runtime_request_user_input_enabled: request_user_input_enabled,
        });
    }

    if let Some(metadata) = parse_runtime_tool_catalog_metadata(system_instruction) {
        validate_runtime_tool_catalog_metadata(&metadata, tool_snapshot)?;
    }

    // Canary: mutating tool signatures that must never appear in a planning workflow prompt.
    // After Phase 2-F/G these should be unreachable; the check is intentionally cheap.
    if planning_active {
        let expected_line = if request_user_input_enabled {
            PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE
        } else {
            PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE
        };
        let unexpected_line = if request_user_input_enabled {
            PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE
        } else {
            PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE
        };
        if !system_instruction.contains(expected_line)
            || system_instruction.contains(unexpected_line)
        {
            return Err(AlignmentError::PlanningWorkflowPromptPolicyMismatch { expected_line });
        }

        const MUTATING_HINTS: &[&str] = &["apply_patch", "unified_file write", "unified_file edit"];
        for &hint in MUTATING_HINTS {
            if system_instruction.contains(hint) {
                return Err(AlignmentError::MutatingToolInPlanningWorkflowPrompt {
                    tool_name: hint,
                });
            }
        }
    }

    Ok(())
}

fn parse_runtime_tool_catalog_metadata(
    system_instruction: &str,
) -> Option<RuntimeToolCatalogMetadata> {
    let start = system_instruction.rfind("[Runtime Tool Catalog]")?;
    let mut metadata = RuntimeToolCatalogMetadata::default();

    for line in system_instruction[start..].lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('[') && !trimmed.starts_with("- ") {
            break;
        }

        let Some(entry) = trimmed.strip_prefix("- ") else {
            continue;
        };
        let Some((key, value)) = entry.split_once(':') else {
            continue;
        };
        let value = value.trim();

        match key.trim() {
            "version" => metadata.version = value.parse().ok(),
            "epoch" => metadata.epoch = value.parse().ok(),
            "available_tools" => metadata.available_tools = value.parse().ok(),
            "request_user_input_enabled" => {
                metadata.request_user_input_enabled = value.parse().ok();
            }
            _ => {}
        }
    }

    Some(metadata)
}

fn validate_runtime_tool_catalog_metadata(
    metadata: &RuntimeToolCatalogMetadata,
    tool_snapshot: &SessionToolCatalogSnapshot,
) -> Result<(), AlignmentError> {
    validate_metadata_value("version", metadata.version, tool_snapshot.version)?;
    validate_metadata_value("epoch", metadata.epoch, tool_snapshot.epoch)?;
    validate_metadata_value(
        "available_tools",
        metadata.available_tools,
        tool_snapshot.available_tools(),
    )?;
    validate_metadata_value(
        "request_user_input_enabled",
        metadata.request_user_input_enabled,
        tool_snapshot.request_user_input_enabled,
    )?;

    Ok(())
}

fn validate_metadata_value<T>(
    field: &'static str,
    prompt_value: Option<T>,
    snapshot_value: T,
) -> Result<(), AlignmentError>
where
    T: Copy + PartialEq + ToString,
{
    if let Some(prompt_value) = prompt_value
        && prompt_value != snapshot_value
    {
        return Err(AlignmentError::PromptCatalogMetadataMismatch {
            field,
            prompt_value: prompt_value.to_string(),
            snapshot_value: snapshot_value.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::Arc;

    fn snapshot(planning_active: bool) -> SessionToolCatalogSnapshot {
        SessionToolCatalogSnapshot::new(
            1,
            1,
            planning_active,
            false,
            Some(Arc::new(Vec::new())),
            false,
        )
    }

    #[tokio::test]
    async fn rebuild_once_on_alignment_mismatch_retries_exactly_once() {
        #[derive(Default)]
        struct TestState {
            rebuilds: usize,
        }

        let mut state = TestState::default();
        let validated_value = Cell::new(0usize);

        let result = rebuild_once_on_alignment_mismatch(
            &mut state,
            0usize,
            |state| {
                Box::pin(async move {
                    state.rebuilds = state.rebuilds.saturating_add(1);
                    Ok::<usize, ()>(state.rebuilds)
                })
            },
            |_, value| {
                validated_value.set(*value);
                if *value == 0 {
                    Err(AlignmentError::PromptCatalogMetadataMismatch {
                        field: "version",
                        prompt_value: "0".to_string(),
                        snapshot_value: "1".to_string(),
                    })
                } else {
                    Ok(())
                }
            },
            "rebuild test",
            "persisted test",
        )
        .await
        .expect("helper should rebuild once and recover");

        assert_eq!(result, 1);
        assert_eq!(state.rebuilds, 1);
        assert_eq!(validated_value.get(), 1);
    }

    #[test]
    fn alignment_ok_when_planning_states_match() {
        validate_prompt_catalog_alignment("normal prompt", &snapshot(false), false, false).unwrap();
        validate_prompt_catalog_alignment(
            PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE,
            &snapshot(true),
            true,
            false,
        )
        .unwrap();
    }

    #[test]
    fn planning_workflow_mismatch_detected() {
        let err = validate_prompt_catalog_alignment("any prompt", &snapshot(false), true, false)
            .expect_err("mismatch should be detected");
        assert_eq!(
            err,
            AlignmentError::PlanningWorkflowMismatch {
                snapshot_planning_active: false,
                registry_planning_active: true,
            }
        );
    }

    #[test]
    fn mutating_tool_in_planning_workflow_prompt_detected() {
        let err = validate_prompt_catalog_alignment(
            &format!(
                "{}\nyou may call apply_patch to write files",
                PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE
            ),
            &snapshot(true),
            true,
            false,
        )
        .expect_err("canary should fire");
        assert_eq!(
            err,
            AlignmentError::MutatingToolInPlanningWorkflowPrompt {
                tool_name: "apply_patch"
            }
        );
    }

    #[test]
    fn no_false_positive_in_normal_mode_with_apply_patch() {
        // Mentioning apply_patch in a normal-mode prompt is fine.
        validate_prompt_catalog_alignment(
            "you may call apply_patch",
            &snapshot(false),
            false,
            false,
        )
        .unwrap();
    }

    #[test]
    fn request_user_input_mismatch_detected() {
        let err = validate_prompt_catalog_alignment(
            PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE,
            &SessionToolCatalogSnapshot::new(1, 1, true, false, Some(Arc::new(Vec::new())), false),
            true,
            true,
        )
        .expect_err("request-user-input mismatch should be detected");
        assert_eq!(
            err,
            AlignmentError::RequestUserInputMismatch {
                snapshot_request_user_input_enabled: false,
                runtime_request_user_input_enabled: true,
            }
        );
    }

    #[test]
    fn prompt_policy_mismatch_detected() {
        let err = validate_prompt_catalog_alignment(
            PLANNING_WORKFLOW_INTERVIEW_POLICY_LINE,
            &SessionToolCatalogSnapshot::new(1, 1, true, false, Some(Arc::new(Vec::new())), false),
            true,
            false,
        )
        .expect_err("prompt policy mismatch should be detected");
        assert_eq!(
            err,
            AlignmentError::PlanningWorkflowPromptPolicyMismatch {
                expected_line: PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE,
            }
        );
    }

    #[test]
    fn runtime_tool_catalog_metadata_mismatch_detected() {
        let prompt = "normal prompt\n[Runtime Tool Catalog]\n- version: 1\n- epoch: 1\n- available_tools: 1\n- request_user_input_enabled: false";

        let err = validate_prompt_catalog_alignment(prompt, &snapshot(false), false, false)
            .expect_err("runtime tool metadata mismatch should be detected");
        assert_eq!(
            err,
            AlignmentError::PromptCatalogMetadataMismatch {
                field: "available_tools",
                prompt_value: "1".to_string(),
                snapshot_value: "0".to_string(),
            }
        );
    }

    #[test]
    fn runtime_tool_catalog_metadata_matches_snapshot() {
        let prompt = "normal prompt\n[Runtime Tool Catalog]\n- version: 1\n- epoch: 1\n- available_tools: 0\n- request_user_input_enabled: false";

        validate_prompt_catalog_alignment(prompt, &snapshot(false), false, false).unwrap();
    }
}
