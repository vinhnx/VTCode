use std::sync::Arc;

use serde_json::Value;

use crate::core::agent::features::FeatureSet;
use crate::llm::provider::ToolDefinition;
use crate::tools::tool_intent;
use crate::tools::validation::commands;

pub fn should_expose_tool_in_mode(
    tool: &ToolDefinition,
    planning_active: bool,
    request_user_input_enabled: bool,
) -> bool {
    let Some(name) = tool.function.as_ref().map(|func| func.name.as_str()) else {
        return true;
    };

    FeatureSet::tool_enabled_for_mode(name, planning_active, request_user_input_enabled)
}

/// Whether a tool is on the wire in a cache-stable catalog (union of planning
/// and execution exposure). Used when `prompt_cache.stable_tool_catalog_across_modes`
/// is enabled so planning toggles do not rewrite the tool array.
pub fn should_expose_tool_in_stable_catalog(tool: &ToolDefinition, request_user_input_enabled: bool) -> bool {
    should_expose_tool_in_mode(tool, false, request_user_input_enabled)
        || should_expose_tool_in_mode(tool, true, request_user_input_enabled)
}

pub fn filter_tool_definitions_for_mode(
    tools: Option<Arc<Vec<ToolDefinition>>>,
    planning_active: bool,
    request_user_input_enabled: bool,
) -> Option<Arc<Vec<ToolDefinition>>> {
    filter_tool_definitions_for_mode_with_stability(tools, planning_active, request_user_input_enabled, false)
}

pub fn filter_tool_definitions_for_mode_with_stability(
    tools: Option<Arc<Vec<ToolDefinition>>>,
    planning_active: bool,
    request_user_input_enabled: bool,
    stable_catalog: bool,
) -> Option<Arc<Vec<ToolDefinition>>> {
    let tools = tools?;
    // Cache-stable catalog: union of planning/execution exposure and no action
    // masking, so planning toggles cannot rewrite the wire tool array.
    if stable_catalog {
        // Fast path first, mirroring the arms below: when every tool
        // survives, reuse the Arc instead of deep-cloning the catalog (each
        // definition carries its description + JSON schema). No-op filters
        // must not copy shared state.
        if tools
            .iter()
            .all(|tool| should_expose_tool_in_stable_catalog(tool, request_user_input_enabled))
        {
            return Some(tools);
        }
        let filtered: Vec<ToolDefinition> = tools
            .iter()
            .filter(|tool| should_expose_tool_in_stable_catalog(tool, request_user_input_enabled))
            .cloned()
            .collect();
        return if filtered.is_empty() {
            None
        } else {
            Some(Arc::new(filtered))
        };
    }

    if !planning_active {
        // No action masking needed; only filter whole tools.
        if tools
            .iter()
            .all(|tool| should_expose_tool_in_mode(tool, false, request_user_input_enabled))
        {
            return Some(tools);
        }
        let filtered: Vec<ToolDefinition> = tools
            .iter()
            .filter(|tool| should_expose_tool_in_mode(tool, false, request_user_input_enabled))
            .cloned()
            .collect();
        return if filtered.is_empty() {
            None
        } else {
            Some(Arc::new(filtered))
        };
    }

    // Planning active: filter whole tools and mask action enums.
    if !tools.is_empty()
        && tools.iter().all(|tool| {
            should_expose_tool_in_mode(tool, true, request_user_input_enabled) && planning_action_mask(tool).is_none()
        })
    {
        return Some(tools);
    }

    let filtered: Vec<ToolDefinition> = tools
        .iter()
        .filter(|tool| should_expose_tool_in_mode(tool, true, request_user_input_enabled))
        .map(|tool| mask_tool_actions_for_mode(tool, true))
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(Arc::new(filtered))
    }
}

/// When planning mode is active, mask the `action` enum in a multi-action
/// tool's JSON schema to only include read-only actions. This prevents the
/// LLM from seeing write/edit/delete actions that would be rejected at
/// execution time.
fn planning_action_mask(tool: &ToolDefinition) -> Option<&'static [&'static str]> {
    let function = tool.function.as_ref()?;
    let allowed = tool_intent::planning_allowed_actions(function.name.as_str())?;
    let action_enum = function.parameters.get("properties")?.get("action")?.get("enum")?;
    action_enum.is_array().then_some(allowed)
}

fn mask_tool_actions_for_mode(tool: &ToolDefinition, planning_active: bool) -> ToolDefinition {
    if !planning_active {
        return tool.clone();
    }
    let Some(allowed) = planning_action_mask(tool) else {
        return tool.clone();
    };

    let mut masked = tool.clone();
    if let Some(func) = masked.function.as_mut()
        && let Some(action_prop) = func.parameters.get_mut("properties").and_then(|p| p.get_mut("action"))
        && let Some(obj) = action_prop.as_object_mut()
    {
        obj.insert("enum".to_string(), Value::Array(allowed.iter().map(|a| Value::String((*a).to_string())).collect()));
    }
    masked
}

pub use crate::core::agent::hash_utils::{
    hash_tool_definitions, hash_value, low_signal_attempt_key, stable_system_prefix_hash,
};
pub use crate::core::agent::request_plan::{HarnessRequestPlan, HarnessRequestPlanInput, build_harness_request_plan};
pub use crate::core::agent::result_reducers::{
    project_model_tool_result, reduce_tool_result, strip_tui_display_fields,
};
pub use crate::core::agent::tool_batching::{
    FallbackRecommendation, FallbackStep, PreparedToolBatch, PreparedToolBatchKind, PreparedToolCall,
    is_parallel_safe_tool_batch,
};
pub use crate::core::agent::tool_catalog::SessionToolCatalogSnapshot;

pub fn looks_like_grep_style_command(command: &str) -> bool {
    let lower = command.trim().to_ascii_lowercase();
    lower.starts_with("grep ") || lower.starts_with("rg ") || lower.contains("/grep ") || lower.contains("/rg ")
}

pub fn command_is_safe(command: &str) -> bool {
    commands::validate_command_safety(command).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::constants::tools;
    use crate::llm::provider::{Message, ToolChoice};

    fn function_tool(name: &str) -> ToolDefinition {
        ToolDefinition::function(name.to_string(), name.to_string(), serde_json::json!({}))
    }

    #[test]
    fn request_plan_keeps_stable_prefix_hash() {
        let plan = build_harness_request_plan(HarnessRequestPlanInput {
            messages: Arc::new(vec![Message::user("hello".to_string())]),
            system_prompt: Arc::from("base\n[Runtime Context]\n- turns: 1"),
            tools: Some(Arc::new(vec![function_tool(tools::CODE_SEARCH)])),
            model: "gpt-5".to_string(),
            max_tokens: Some(128),
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            stream: true,
            tool_choice: Some(ToolChoice::auto()),
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
            metadata: None,
            context_management: None,
            previous_response_id: None,
            prompt_cache_key: None,
            prompt_cache_profile: None,
            tool_catalog_hash: None,
            system_prompt_prefix_hash: None,
        });

        assert!(plan.has_tools);
        assert!(plan.tool_catalog_hash.is_some());
        assert_eq!(plan.stable_prefix_hash, stable_system_prefix_hash("base\n[Runtime Context]\n- turns: 1"));
    }

    #[test]
    fn request_plan_drops_empty_tool_catalog() {
        let plan = build_harness_request_plan(HarnessRequestPlanInput {
            messages: Arc::new(vec![Message::user("hello".to_string())]),
            system_prompt: Arc::from("base"),
            tools: Some(Arc::new(Vec::new())),
            model: "gpt-5".to_string(),
            max_tokens: Some(128),
            temperature: Some(0.7),
            top_p: None,
            top_k: None,
            presence_penalty: None,
            frequency_penalty: None,
            stream: true,
            tool_choice: Some(ToolChoice::auto()),
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
            metadata: None,
            context_management: None,
            previous_response_id: None,
            prompt_cache_key: None,
            prompt_cache_profile: None,
            tool_catalog_hash: None,
            system_prompt_prefix_hash: None,
        });

        assert!(!plan.has_tools);
        assert!(plan.request.tools.is_none());
        assert!(plan.tool_catalog_hash.is_none());
    }

    #[test]
    fn request_plan_carries_sampling_overrides_into_request() {
        let plan = build_harness_request_plan(HarnessRequestPlanInput {
            messages: Arc::new(vec![Message::user("hello".to_string())]),
            system_prompt: Arc::from("base"),
            tools: None,
            model: "gpt-5".to_string(),
            max_tokens: Some(4_096),
            temperature: Some(0.2),
            top_p: Some(0.9),
            top_k: Some(40),
            presence_penalty: Some(0.1),
            frequency_penalty: Some(-0.5),
            stream: true,
            tool_choice: None,
            parallel_tool_config: None,
            reasoning_effort: None,
            verbosity: None,
            metadata: None,
            context_management: None,
            previous_response_id: None,
            prompt_cache_key: None,
            prompt_cache_profile: None,
            tool_catalog_hash: None,
            system_prompt_prefix_hash: None,
        });

        assert_eq!(plan.request.temperature, Some(0.2));
        assert_eq!(plan.request.top_p, Some(0.9));
        assert_eq!(plan.request.top_k, Some(40));
        assert_eq!(plan.request.presence_penalty, Some(0.1));
        assert_eq!(plan.request.frequency_penalty, Some(-0.5));
        assert_eq!(plan.request.max_tokens, Some(4_096));
    }

    #[test]
    fn prepared_tool_batches_group_contiguous_parallel_reads() {
        let batches = PreparedToolBatch::plan(
            vec![
                PreparedToolCall::new("read_a".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("read_b".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("edit".to_string(), false, false, serde_json::json!({})),
            ],
            true,
        );

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].kind, PreparedToolBatchKind::ParallelReadonly);
        assert_eq!(batches[0].calls.len(), 2);
        assert_eq!(batches[1].kind, PreparedToolBatchKind::Sequential);
    }

    #[test]
    fn prepared_tool_batches_preserve_order_around_mutating_calls() {
        let batches = PreparedToolBatch::plan(
            vec![
                PreparedToolCall::new("read_a".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("edit".to_string(), false, false, serde_json::json!({})),
                PreparedToolCall::new("read_b".to_string(), true, true, serde_json::json!({})),
            ],
            true,
        );

        assert_eq!(batches.len(), 3);
        assert!(batches.iter().all(|batch| batch.kind == PreparedToolBatchKind::Sequential));
        assert_eq!(batches[0].calls[0].canonical_name, "read_a");
        assert_eq!(batches[1].calls[0].canonical_name, "edit");
        assert_eq!(batches[2].calls[0].canonical_name, "read_b");
    }

    #[test]
    fn prepared_tool_batches_group_duplicate_parallel_tool_names() {
        let batches = PreparedToolBatch::plan(
            vec![
                PreparedToolCall::new("read_file".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("read_file".to_string(), true, true, serde_json::json!({})),
            ],
            true,
        );

        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].kind, PreparedToolBatchKind::ParallelReadonly);
        assert_eq!(batches[0].calls.len(), 2);
    }

    #[test]
    fn prepared_tool_batches_keep_duplicate_nonparallel_calls_sequential() {
        let batches = PreparedToolBatch::plan(
            vec![
                PreparedToolCall::new("exec_command".to_string(), false, false, serde_json::json!({})),
                PreparedToolCall::new("exec_command".to_string(), false, false, serde_json::json!({})),
            ],
            true,
        );

        assert_eq!(batches.len(), 2);
        assert!(batches.iter().all(|batch| batch.kind == PreparedToolBatchKind::Sequential));
        assert!(batches.iter().all(|batch| batch.calls.len() == 1));
    }

    #[test]
    fn prepared_tool_batches_serializes_all_calls_when_parallel_disabled() {
        let batches = PreparedToolBatch::plan(
            vec![
                PreparedToolCall::new("read_a".to_string(), true, true, serde_json::json!({})),
                PreparedToolCall::new("read_b".to_string(), true, true, serde_json::json!({})),
            ],
            false,
        );

        assert_eq!(batches.len(), 2);
        assert!(batches.iter().all(|batch| batch.kind == PreparedToolBatchKind::Sequential));
    }

    #[test]
    fn filter_tool_definitions_respects_request_user_input_toggle() {
        let tools = Arc::new(vec![
            function_tool(tools::CODE_SEARCH),
            function_tool(tools::REQUEST_USER_INPUT),
        ]);

        let filtered = filter_tool_definitions_for_mode(Some(tools), true, false).expect("filtered tools");
        let names: Vec<&str> = filtered.iter().map(|tool| tool.function_name()).collect();

        assert!(names.contains(&tools::CODE_SEARCH));
        assert!(!names.contains(&tools::REQUEST_USER_INPUT));
    }

    #[test]
    fn filter_tool_definitions_hides_mutating_only_tools_in_planning_workflow() {
        let tools = Arc::new(vec![
            function_tool(tools::CODE_SEARCH),
            function_tool(tools::UNIFIED_FILE),
            function_tool(tools::APPLY_PATCH),
            function_tool(tools::WRITE_STDIN),
            function_tool(tools::WRITE_FILE),
        ]);

        let filtered = filter_tool_definitions_for_mode(Some(tools), true, false).expect("filtered tools");
        let names: Vec<&str> = filtered.iter().map(|tool| tool.function_name()).collect();

        assert!(names.contains(&tools::CODE_SEARCH));
        assert!(names.contains(&tools::UNIFIED_FILE));
        assert!(!names.contains(&tools::APPLY_PATCH));
        assert!(!names.contains(&tools::WRITE_STDIN));
        assert!(!names.contains(&tools::WRITE_FILE));
    }

    #[test]
    fn stable_catalog_keeps_mutating_tools_across_planning_toggles() {
        let tools = Arc::new(vec![
            function_tool(tools::CODE_SEARCH),
            function_tool(tools::UNIFIED_FILE),
            function_tool(tools::APPLY_PATCH),
            function_tool(tools::WRITE_FILE),
        ]);

        let planning = filter_tool_definitions_for_mode_with_stability(Some(tools.clone()), true, false, true)
            .expect("stable planning catalog");
        let execution = filter_tool_definitions_for_mode_with_stability(Some(tools), false, false, true)
            .expect("stable execution catalog");
        let planning_names: Vec<&str> = planning.iter().map(|tool| tool.function_name()).collect();
        let execution_names: Vec<&str> = execution.iter().map(|tool| tool.function_name()).collect();
        assert_eq!(planning_names, execution_names, "stable catalog must match across modes");
        assert!(planning_names.contains(&tools::APPLY_PATCH));
        assert!(planning_names.contains(&tools::WRITE_FILE));
    }

    #[test]
    fn stable_catalog_reuses_arc_when_nothing_is_filtered() {
        // Asymmetric triple: all-exposed input keeps its Arc (no deep clone
        // of descriptions + schemas); input with a mode-hidden tool gets a
        // new Arc without it; fully-hidden input stays None.
        let tools = Arc::new(vec![function_tool(tools::CODE_SEARCH), function_tool(tools::APPLY_PATCH)]);

        let filtered = filter_tool_definitions_for_mode_with_stability(Some(tools.clone()), true, false, true)
            .expect("stable catalog");
        assert!(Arc::ptr_eq(&tools, &filtered), "no-op stable filter must reuse the Arc");

        let execution = filter_tool_definitions_for_mode_with_stability(Some(tools.clone()), false, false, true)
            .expect("stable catalog");
        assert!(Arc::ptr_eq(&filtered, &execution), "stable catalog must be identical across toggles");

        let mixed = Arc::new(vec![
            function_tool(tools::CODE_SEARCH),
            function_tool(tools::REQUEST_USER_INPUT),
        ]);
        let pruned = filter_tool_definitions_for_mode_with_stability(Some(mixed.clone()), true, false, true)
            .expect("stable catalog");
        assert!(!Arc::ptr_eq(&mixed, &pruned));
        let names: Vec<&str> = pruned.iter().map(|tool| tool.function_name()).collect();
        assert!(names.contains(&tools::CODE_SEARCH));
        assert!(!names.contains(&tools::REQUEST_USER_INPUT));

        let hidden = Arc::new(vec![function_tool(tools::REQUEST_USER_INPUT)]);
        assert!(
            filter_tool_definitions_for_mode_with_stability(Some(hidden), false, false, true).is_none(),
            "fully-hidden stable catalog stays None"
        );
    }

    #[test]
    fn stable_prefix_hash_ignores_runtime_tool_sections() {
        let base = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 5";
        let with_runtime_sections = format!(
            "{base}\n\n## Active Tools\n- Capabilities: read-only.\n[Runtime Tool Catalog]\n- version: 1\n- epoch: 2\n- available_tools: 3\n- request_user_input_enabled: false\n\n# PLANNING WORKFLOW (READ-ONLY)\nread-only\n\n# FULL-AUTO: Complete task autonomously until done or blocked.\n- autonomous"
        );

        assert_eq!(stable_system_prefix_hash(base), stable_system_prefix_hash(&with_runtime_sections));
    }

    #[test]
    fn tool_catalog_hash_matches_legacy_json_string_hash() {
        let tools = vec![
            function_tool(tools::CODE_SEARCH),
            ToolDefinition::function(
                "custom_tool".to_string(),
                "Custom".to_string(),
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "line": { "type": "integer" }
                    }
                }),
            )
            .with_strict(true)
            .with_defer_loading(true),
        ];

        let expected = serde_json::to_string(&tools).ok().map(|text| hash_value(&text));

        assert_eq!(hash_tool_definitions(Some(&tools)), expected);
    }

    #[test]
    fn reduce_command_result_truncates_large_output() {
        let stdout = (0..2200).map(|_| "a").collect::<Vec<_>>().join("\n");
        let reduced = reduce_tool_result(
            tools::UNIFIED_EXEC,
            serde_json::json!({
                "stdout": stdout
            }),
        );

        assert_eq!(reduced.get("is_truncated"), Some(&Value::Bool(true)));
    }

    fn tool_with_action_enum(name: &str, actions: &[&str]) -> ToolDefinition {
        ToolDefinition::function(
            name.to_string(),
            name.to_string(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": actions
                    }
                }
            }),
        )
    }

    #[test]
    fn filter_tool_definitions_masks_unified_file_actions_in_planning() {
        let tools = Arc::new(vec![tool_with_action_enum(
            tools::UNIFIED_FILE,
            &["read", "write", "edit", "patch", "delete", "move", "copy"],
        )]);

        let filtered = filter_tool_definitions_for_mode(Some(tools), true, false).expect("filtered tools");
        assert_eq!(filtered.len(), 1);

        let action_enum = filtered[0]
            .function
            .as_ref()
            .unwrap()
            .parameters
            .get("properties")
            .and_then(|p| p.get("action"))
            .and_then(|a| a.get("enum"))
            .and_then(|e| e.as_array())
            .expect("action enum should exist");

        let action_strings: Vec<&str> = action_enum.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(action_strings, vec!["read"]);
    }

    #[test]
    fn filter_tool_definitions_masks_unified_exec_actions_in_planning() {
        let tools = Arc::new(vec![tool_with_action_enum(
            tools::UNIFIED_EXEC,
            &["run", "write", "poll", "continue", "inspect", "list", "close", "code"],
        )]);

        let filtered = filter_tool_definitions_for_mode(Some(tools), true, false).expect("filtered tools");
        assert_eq!(filtered.len(), 1);

        let action_enum = filtered[0]
            .function
            .as_ref()
            .unwrap()
            .parameters
            .get("properties")
            .and_then(|p| p.get("action"))
            .and_then(|a| a.get("enum"))
            .and_then(|e| e.as_array())
            .expect("action enum should exist");

        let action_strings: Vec<&str> = action_enum.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(action_strings, vec!["run", "poll", "wait", "list", "inspect", "continue"]);
    }

    #[test]
    fn filter_tool_definitions_preserves_actions_when_planning_inactive() {
        let tools = Arc::new(vec![tool_with_action_enum(
            tools::UNIFIED_FILE,
            &["read", "write", "edit", "patch", "delete", "move", "copy"],
        )]);

        let filtered = filter_tool_definitions_for_mode(Some(tools), false, false).expect("filtered tools");
        assert_eq!(filtered.len(), 1);

        let action_enum = filtered[0]
            .function
            .as_ref()
            .unwrap()
            .parameters
            .get("properties")
            .and_then(|p| p.get("action"))
            .and_then(|a| a.get("enum"))
            .and_then(|e| e.as_array())
            .expect("action enum should exist");

        let action_strings: Vec<&str> = action_enum.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(action_strings, vec!["read", "write", "edit", "patch", "delete", "move", "copy"]);
    }

    #[test]
    fn filter_tool_definitions_skips_masking_for_non_multi_action_tools() {
        let tools = Arc::new(vec![function_tool(tools::CODE_SEARCH)]);

        let filtered = filter_tool_definitions_for_mode(Some(Arc::clone(&tools)), true, false).expect("filtered tools");
        assert_eq!(filtered.len(), 1);
        // No action property to mask — just verify the tool is still present.
        assert_eq!(filtered[0].function_name(), tools::CODE_SEARCH);
        assert!(Arc::ptr_eq(&tools, &filtered));
    }

    #[test]
    fn filter_tool_definitions_preserves_action_schema_without_enum() {
        let tools = Arc::new(vec![ToolDefinition::function(
            tools::UNIFIED_FILE.to_string(),
            "Unified file operation".to_string(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string" }
                }
            }),
        )]);

        let filtered = filter_tool_definitions_for_mode(Some(tools), true, false).expect("filtered tools");
        let action = filtered[0]
            .function
            .as_ref()
            .and_then(|function| function.parameters.get("properties"))
            .and_then(|properties| properties.get("action"))
            .expect("action schema should remain");

        assert_eq!(action.get("type"), Some(&Value::String("string".to_string())));
        assert!(action.get("enum").is_none());
    }

    #[test]
    fn filter_tool_definitions_masks_actions_even_when_no_whole_tool_filter() {
        // All tools pass the whole-tool filter (none are Mutating-only),
        // but action masking should still apply.
        let tools = Arc::new(vec![
            function_tool(tools::CODE_SEARCH),
            tool_with_action_enum(tools::UNIFIED_FILE, &["read", "write", "edit"]),
        ]);

        let filtered = filter_tool_definitions_for_mode(Some(tools), true, false).expect("filtered tools");
        assert_eq!(filtered.len(), 2);

        let unified_file = filtered
            .iter()
            .find(|t| t.function_name() == tools::UNIFIED_FILE)
            .expect("unified_file should be present");

        let action_enum = unified_file
            .function
            .as_ref()
            .unwrap()
            .parameters
            .get("properties")
            .and_then(|p| p.get("action"))
            .and_then(|a| a.get("enum"))
            .and_then(|e| e.as_array())
            .expect("action enum should exist");

        let action_strings: Vec<&str> = action_enum.iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(action_strings, vec!["read"]);
    }
}
