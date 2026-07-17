//! Turn-request build orchestrator.
//!
//! Ties together the sibling `llm_request` submodules -- `snapshot`
//! (per-turn state), `prompt_assembly` (system prompt + tool catalog),
//! `tool_shaping` (wire-facing tool filtering), `context_management`
//! (provider compaction/edits payload), and `response_chain`
//! (Responses-API history handling) -- into the single wire-ready
//! [`uni::LLMRequest`] for a turn via [`build_turn_request`]. Invariant:
//! this module owns no turn-state derivation of its own; it only
//! sequences calls into the submodules above and assembles their outputs
//! into [`TurnRequestBuildResult`].

use anyhow::Result;
use std::fmt::Write as _;
use std::sync::Arc;

use vtcode_core::config::build_openai_prompt_cache_key;
use vtcode_core::core::agent::harness_kernel::{
    HarnessRequestPlanInput, build_harness_request_plan, stable_system_prefix_hash,
};
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig};

use super::context_management::resolve_context_management;
use super::metrics::{
    TokenBudgetBreakdown, ToolCatalogCacheMetrics, emit_token_budget_breakdown,
    emit_tool_catalog_cache_metrics, estimate_message_history_tokens, estimate_tool_schema_tokens,
};
use super::prompt_assembly::{
    PromptAssemblyInput, assemble_prompt, render_primary_agent_runtime_context,
};
use super::response_chain::{prepare_responses_request_history, prepend_request_context_message};
use super::snapshot::{TurnRequestSnapshot, resolve_effective_reasoning_effort};
use super::tool_shaping::{client_local_wire_tools, uses_out_of_band_copilot_tools};
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

pub(super) struct TurnRequestBuildResult {
    pub request: uni::LLMRequest,
    pub has_tools: bool,
    pub runtime_tools: Option<Arc<Vec<uni::ToolDefinition>>>,
    pub continuation_messages: Vec<uni::Message>,
}

pub(super) fn interrupted_provider_error(provider_name: &str) -> anyhow::Error {
    anyhow::Error::new(uni::LLMError::Provider {
        message: vtcode_core::llm::error_display::format_llm_error(
            provider_name,
            "Interrupted by user",
        ),
        metadata: None,
    })
}

pub(super) async fn build_turn_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    _active_model: &str,
    turn_snapshot: &TurnRequestSnapshot,
    max_tokens_opt: Option<u32>,
    parallel_cfg_opt: Option<Box<ParallelToolConfig>>,
    use_streaming: bool,
) -> Result<TurnRequestBuildResult> {
    let request_model = turn_snapshot.active_model.as_str();
    let mut prompt_output =
        assemble_prompt(ctx, PromptAssemblyInput { turn: turn_snapshot }).await?;

    let reasoning_effort = resolve_effective_reasoning_effort(ctx.vt_cfg, turn_snapshot);
    let primary_agent_context = render_primary_agent_runtime_context(
        ctx,
        turn_snapshot,
        &prompt_output.tool_snapshot,
        &turn_snapshot.active_primary_agent,
        reasoning_effort,
        prompt_output.agent_prompt_context.as_ref(),
    )
    .await;
    let _ = writeln!(prompt_output.system_prompt, "\n{primary_agent_context}");
    let temperature = if reasoning_effort.is_some()
        && matches!(turn_snapshot.provider_name.as_str(), "anthropic" | "minimax")
    {
        None
    } else {
        Some(0.7)
    };
    let parallel_config = if prompt_output.tool_snapshot.has_tools()
        && !turn_snapshot.tool_free_recovery
        && turn_snapshot.capabilities.parallel_tool_config
    {
        parallel_cfg_opt
    } else {
        None
    };
    let use_out_of_band_copilot_tools =
        uses_out_of_band_copilot_tools(&turn_snapshot.provider_name);
    let tool_choice = if turn_snapshot.tool_free_recovery {
        Some(uni::ToolChoice::none())
    } else if use_out_of_band_copilot_tools {
        None
    } else if prompt_output.tool_snapshot.has_tools() {
        Some(uni::ToolChoice::auto())
    } else {
        None
    };

    let metadata = match ctx.turn_metadata().await {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(error = %err, "Turn metadata collection failed");
            None
        }
    };
    let prompt_cache_key = build_openai_prompt_cache_key(
        turn_snapshot.openai_prompt_cache_enabled,
        &turn_snapshot.openai_prompt_cache_key_mode,
        ctx.session_stats.prompt_cache_lineage_id(),
    );
    let stable_prefix_hash = stable_system_prefix_hash(&prompt_output.system_prompt);
    let tool_catalog_hash = prompt_output.tool_snapshot.tool_catalog_hash;
    let prefix_change_reason = ctx.session_stats.record_prompt_cache_fingerprint(
        request_model,
        stable_prefix_hash,
        tool_catalog_hash,
    );
    emit_tool_catalog_cache_metrics(
        ctx,
        ToolCatalogCacheMetrics {
            step_count,
            model: request_model,
            cache_hit: prompt_output.tool_snapshot.cache_hit,
            planning_active: turn_snapshot.planning_active,
            request_user_input_enabled: turn_snapshot.request_user_input_enabled,
            available_tools: prompt_output.tool_snapshot.available_tools(),
            stable_prefix_hash,
            tool_catalog_hash,
            prefix_change_reason,
        },
    );
    let context_management = resolve_context_management(ctx, turn_snapshot, request_model);
    let continuation_messages =
        ctx.context_manager.normalize_history_for_request(ctx.working_history);
    let (request_messages, previous_response_id) = prepare_responses_request_history(
        ctx.session_stats,
        &turn_snapshot.provider_name,
        turn_snapshot.capabilities.responses_compaction,
        request_model,
        &continuation_messages,
    );
    let request_messages = request_messages.into_owned();
    let request_messages = prepend_request_context_message(
        request_messages,
        ctx.context_manager.request_editor_context_message(),
    );
    let request_plan = build_harness_request_plan(HarnessRequestPlanInput {
        messages: Arc::new(request_messages),
        system_prompt: prompt_output.system_prompt,
        tools: if use_out_of_band_copilot_tools || turn_snapshot.tool_free_recovery {
            // Strip tool definitions during tool-free recovery (including
            // wall-clock exhaustion recovery) so the model cannot even attempt
            // tool calls. ToolChoice::none() alone is advisory — the model
            // still sees definitions and may try (observed in turn_637).
            None
        } else if turn_snapshot.client_local_tool_deferral {
            // No hosted tool search for this provider: deferred tools are
            // not sent eagerly. The model discovers them through the relevant
            // local discovery tool (see `[Deferred Tools]` in
            // the system prompt, appended in `build_prompt_output`).
            client_local_wire_tools(prompt_output.tool_snapshot.snapshot.clone())
        } else {
            prompt_output.tool_snapshot.snapshot.clone()
        },
        model: turn_snapshot.active_model.clone(),
        max_tokens: max_tokens_opt,
        temperature,
        stream: use_streaming,
        tool_choice,
        parallel_tool_config: parallel_config,
        reasoning_effort,
        verbosity: None,
        metadata,
        context_management,
        previous_response_id,
        prompt_cache_key,
        prompt_cache_profile: ctx.session_stats.prompt_cache_profile(),
        tool_catalog_hash,
        system_prompt_prefix_hash: Some(stable_prefix_hash),
    });

    // Phase 1.2 observability: record how the assembled first-request prefix is
    // spent across system prompt, tool schemas, and message history, using the
    // real on-wire request payload. Cache read/write/miss are already surfaced
    // via `SessionStats` prompt-cache diagnostics, so they are not duplicated.
    let request = &request_plan.request;
    let system_prompt_tokens = request.system_prompt.as_ref().map(|sp| sp.len() / 4).unwrap_or(0);
    let (on_wire_tools, tool_schema_tokens) = request
        .tools
        .as_ref()
        .map(|tools| (tools.len(), estimate_tool_schema_tokens(tools.as_slice())))
        .unwrap_or((0, 0));
    let message_history_tokens = estimate_message_history_tokens(request.messages.as_slice());
    emit_token_budget_breakdown(
        ctx,
        TokenBudgetBreakdown {
            step_count,
            model: request_model,
            system_prompt_tokens,
            tool_schema_tokens,
            message_history_tokens,
            on_wire_tools,
            client_local_deferral: turn_snapshot.client_local_tool_deferral,
            tool_free_recovery: turn_snapshot.tool_free_recovery,
        },
    );

    Ok(TurnRequestBuildResult {
        request: request_plan.request,
        has_tools: prompt_output.tool_snapshot.has_tools(),
        runtime_tools: prompt_output.tool_snapshot.snapshot,
        continuation_messages,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;
    use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
    use vtcode_config::{SubagentMemoryScope, SubagentSource, SubagentSpec};
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::config::types::ReasoningEffortLevel;
    use vtcode_core::llm::provider::{self as uni, ToolDefinition};
    use vtcode_core::{EditorContextSnapshot, EditorFileContext};

    use super::super::response_chain::update_previous_response_chain_after_success;
    use super::super::snapshot::capture_turn_request_snapshot;
    use super::{build_turn_request, stable_system_prefix_hash};
    use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

    fn test_primary_agent_spec(name: &str, prompt: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: format!("{name} description"),
            prompt: prompt.to_string(),
            tools: Some(vec!["code_search".to_string()]),
            disallowed_tools: vec!["shell".to_string()],
            model: None,
            color: None,
            reasoning_effort: None,
            permissions: AgentPermissionsConfig::new(PermissionDefault::Deny),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            mode: vtcode_config::AgentMode::Primary,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::ProjectVtcode,
            file_path: None,
            warnings: Vec::new(),
            tool_policy_overrides: std::collections::BTreeMap::new(),
        }
    }

    fn named_tool(name: &str) -> ToolDefinition {
        ToolDefinition::function(
            name.to_string(),
            format!("{name} tool"),
            json!({
                "type": "object",
                "properties": {
                    "input": { "type": "string" }
                }
            }),
        )
    }

    fn request_tool_names(request: &uni::LLMRequest) -> Vec<String> {
        request
            .tools
            .as_deref()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .map(|tool| tool.function_name().to_string())
            .collect()
    }

    fn non_runtime_request_messages(request: &uni::LLMRequest) -> Vec<uni::Message> {
        request.messages.as_ref().clone()
    }

    fn system_prompt_text(request: &uni::LLMRequest) -> &str {
        request.system_prompt.as_ref().expect("system prompt").as_str()
    }

    #[tokio::test]
    async fn recovery_request_omits_tools_and_disables_tool_choice() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.select_primary_agent_from_specs(
            &[vtcode_config::builtin_primary_build_agent()],
            "build",
        );
        backing
            .add_tool_definition(ToolDefinition::function(
                "code_search".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "path": { "type": "string" },
                        "file_types": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "result_types": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["definition", "usage", "text", "path"]
                            }
                        },
                        "max_results": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 100
                        }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ))
            .await;

        let mut ctx = backing.turn_processing_context();
        let mut vt_cfg = VTCodeConfig::default();
        vt_cfg.agent.reasoning_effort = ReasoningEffortLevel::High;
        ctx.vt_cfg = Some(&vt_cfg);
        ctx.activate_recovery("loop detector");

        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", true);
        let mut normal_snapshot = snapshot.clone();
        normal_snapshot.tool_free_recovery = false;
        normal_snapshot.capabilities.reasoning_effort = true;

        let normal_built =
            build_turn_request(&mut ctx, 1, "noop-model", &normal_snapshot, Some(320), None, false)
                .await
                .expect("normal request should build");
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("recovery request should build");

        assert_eq!(normal_built.request.reasoning_effort, Some(ReasoningEffortLevel::High));
        assert!(built.request.reasoning_effort.is_none());
        assert!(!built.has_tools);
        assert!(built.request.tools.is_none());
        assert!(matches!(built.request.tool_choice, Some(uni::ToolChoice::None)));
        assert_eq!(built.request.max_tokens, Some(320));

        let system_prompt = built.request.system_prompt.as_ref().expect("system prompt").as_str();
        assert!(system_prompt.contains("[Recovery Mode]"));
        assert!(system_prompt.contains("do_not_request_more_tools: true"));
        assert!(system_prompt.contains("recovery_reason: loop detector"));
        assert!(!system_prompt.contains("<budget:token_budget>"));
    }

    #[tokio::test]
    async fn text_only_provider_request_omits_tools_and_tool_choice() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing
            .add_tool_definition(ToolDefinition::function(
                "code_search".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "path": { "type": "string" },
                        "file_types": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "result_types": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["definition", "usage", "text", "path"]
                            }
                        },
                        "max_results": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 100
                        }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ))
            .await;

        let mut ctx = backing.turn_processing_context();

        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        snapshot.capabilities.tools = false;
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("text-only request should build");

        assert!(!built.has_tools);
        assert!(built.request.tools.is_none());
        assert!(built.request.tool_choice.is_none());

        let system_prompt = built.request.system_prompt.as_ref().expect("system prompt").as_str();
        assert!(!system_prompt.contains("[Runtime Tool Catalog]"));
    }

    #[tokio::test]
    async fn copilot_request_keeps_runtime_tools_out_of_band() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing
            .add_tool_definition(ToolDefinition::function(
                "code_search".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "path": { "type": "string" },
                        "file_types": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "result_types": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["definition", "usage", "text", "path"]
                            }
                        },
                        "max_results": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 100
                        }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ))
            .await;

        let mut ctx = backing.turn_processing_context();
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "copilot-gpt-5.4", false);
        snapshot.provider_name = vtcode_core::copilot::COPILOT_PROVIDER_KEY.to_string();
        snapshot.capabilities.tools = true;
        let built =
            build_turn_request(&mut ctx, 1, "copilot-gpt-5.4", &snapshot, Some(320), None, true)
                .await
                .expect("copilot request should build");

        assert!(built.has_tools);
        assert!(built.request.tools.is_none());
        assert!(built.request.tool_choice.is_none());
        assert_eq!(built.runtime_tools.as_ref().map(|tools| tools.len()), Some(1));

        let system_prompt = built.request.system_prompt.as_ref().expect("system prompt").as_str();
        assert!(system_prompt.contains("[GitHub Copilot Client Tools]"));
        assert!(system_prompt.contains("emit the actual client tool call"));
    }

    #[tokio::test]
    async fn client_local_tool_deferral_omits_deferred_tools_from_wire_payload() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.add_tool_definition(named_tool("read_file")).await;
        backing
            .add_tool_definition(
                ToolDefinition::function(
                    "context7_lookup".to_string(),
                    "Look up documentation via context7".to_string(),
                    json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" }
                        }
                    }),
                )
                .with_defer_loading(true),
            )
            .await;

        let mut ctx = backing.turn_processing_context();
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        // Simulate the ClientLocal policy being active for this turn (no
        // provider-hosted tool search, `client_tool_search` enabled) without
        // wiring the full config/provider plumbing that would normally
        // compute this flag -- see `capture_turn_request_snapshot` above for
        // how it is derived from `active_deferred_tool_policy` in production.
        snapshot.client_local_tool_deferral = true;

        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("client-local request should build");

        let tool_names = request_tool_names(&built.request);
        assert!(tool_names.contains(&"read_file".to_string()));
        assert!(!tool_names.contains(&"context7_lookup".to_string()));

        // `runtime_tools` must stay unfiltered: Copilot's out-of-band tool
        // exposure and stats consumers need the full catalog even when the
        // wire payload omits deferred definitions.
        assert_eq!(built.runtime_tools.as_ref().map(|tools| tools.len()), Some(2));
    }

    #[tokio::test]
    async fn hosted_tool_search_keeps_deferred_tools_on_the_wire() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.add_tool_definition(named_tool("read_file")).await;
        backing
            .add_tool_definition(
                ToolDefinition::function(
                    "context7_lookup".to_string(),
                    "Look up documentation via context7".to_string(),
                    json!({
                        "type": "object",
                        "properties": {
                            "query": { "type": "string" }
                        }
                    }),
                )
                .with_defer_loading(true),
            )
            .await;

        let mut ctx = backing.turn_processing_context();
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        snapshot.provider_name = "anthropic".to_string();
        // Provider-hosted tool search (Anthropic/OpenAI) never sets this
        // flag -- `deferred_tool_policy_for_runtime` only returns
        // `ClientLocal` on the no-hosted-search fallthrough arm. Asserting
        // it is false here pins the safety requirement that hosted payloads
        // stay byte-identical: every deferred tool remains on the wire.
        assert!(!snapshot.client_local_tool_deferral);

        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("hosted request should build");

        let tool_names = request_tool_names(&built.request);
        assert!(tool_names.contains(&"read_file".to_string()));
        assert!(tool_names.contains(&"context7_lookup".to_string()));
    }

    #[tokio::test]
    async fn openai_responses_replays_full_structured_history_without_suffixing() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let prior_messages = vec![
            uni::Message::user("hello".to_string()),
            uni::Message::assistant("hi".to_string()),
        ];
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.extend(prior_messages.clone());
        ctx.working_history.push(uni::Message::user("continue".to_string()));
        ctx.session_stats.set_previous_response_chain(
            "openai",
            "noop-model",
            Some("resp_123"),
            &prior_messages,
        );

        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("openai request should build");

        assert_eq!(built.request.previous_response_id, None);
        assert_eq!(
            non_runtime_request_messages(&built.request),
            vec![
                uni::Message::user("hello".to_string()),
                uni::Message::assistant("hi".to_string()),
                uni::Message::user("continue".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn compatible_provider_responses_keeps_full_history_without_previous_response_id() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let prior_messages = vec![uni::Message::user("hello".to_string())];
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.extend(prior_messages.clone());
        ctx.working_history.push(uni::Message::user("continue".to_string()));
        ctx.session_stats.set_previous_response_chain(
            "mycorp",
            "noop-model",
            Some("resp_123"),
            &prior_messages,
        );

        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        snapshot.provider_name = "mycorp".to_string();
        snapshot.capabilities.responses_compaction = true;
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("compatible provider request should build");

        assert_eq!(built.request.previous_response_id, None);
        assert_eq!(
            non_runtime_request_messages(&built.request),
            vec![
                uni::Message::user("hello".to_string()),
                uni::Message::user("continue".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn non_openai_responses_chain_keeps_full_history() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let prior_messages = vec![uni::Message::user("hello".to_string())];
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.extend(prior_messages.clone());
        ctx.working_history.push(uni::Message::user("continue".to_string()));
        ctx.session_stats.set_previous_response_chain(
            "gemini",
            "noop-model",
            Some("resp_123"),
            &prior_messages,
        );

        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        snapshot.provider_name = "gemini".to_string();
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("gemini request should build");

        assert_eq!(built.request.previous_response_id.as_deref(), Some("resp_123"));
        assert_eq!(
            non_runtime_request_messages(&built.request),
            vec![
                uni::Message::user("hello".to_string()),
                uni::Message::user("continue".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn request_build_moves_editor_context_out_of_system_prompt() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.context_manager.set_workspace_root(workspace.path());
        ctx.context_manager.set_editor_context_snapshot(
            Some(EditorContextSnapshot {
                workspace_root: Some(PathBuf::from(workspace.path())),
                active_file: Some(EditorFileContext {
                    path: workspace.path().join("src/main.rs").display().to_string(),
                    language_id: Some("rust".to_string()),
                    line_range: None,
                    dirty: false,
                    truncated: false,
                    selection: None,
                }),
                ..EditorContextSnapshot::default()
            }),
            Some(&vtcode_config::IdeContextConfig::default()),
        );
        ctx.working_history.push(uni::Message::user("hello".to_string()));

        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        let built =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build");

        let system_prompt = built.request.system_prompt.as_ref().expect("system prompt").as_str();
        assert!(!system_prompt.contains("## Active Editor Context"));
        let non_runtime_messages = non_runtime_request_messages(&built.request);
        assert_eq!(non_runtime_messages.len(), 2);
        assert_eq!(non_runtime_messages[0].role, uni::MessageRole::User);
        assert!(non_runtime_messages[0].content.as_text().contains("## Active Editor Context"));
        assert!(non_runtime_messages[0].content.as_text().contains("- Active file: src/main.rs"));
        assert!(non_runtime_messages[0].content.as_text().contains("- Language: Rust"));
        assert_eq!(non_runtime_messages[1], uni::Message::user("hello".to_string()));
        assert_eq!(built.continuation_messages, vec![uni::Message::user("hello".to_string())]);
    }

    #[tokio::test]
    async fn active_primary_agent_runtime_state_is_system_prompt_context() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing
            .add_tool_definition(ToolDefinition::function(
                "code_search".to_string(),
                "Search project files".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "path": { "type": "string" },
                        "file_types": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "result_types": {
                            "type": "array",
                            "items": {
                                "type": "string",
                                "enum": ["definition", "usage", "text", "path"]
                            }
                        },
                        "max_results": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 100
                        }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }),
            ))
            .await;
        let spec = test_primary_agent_spec("planner", "Plan carefully before editing.");
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(built.request.messages.len(), 1);
        assert_eq!(built.request.messages[0], uni::Message::user("hello".to_string()));
        let runtime_context = built.request.system_prompt.as_ref().expect("system prompt").as_str();
        assert!(runtime_context.contains("## Active Primary Agent Runtime State"));
        assert!(runtime_context.contains("- Active agent: planner"));
        assert!(runtime_context.contains("- Effective request tools: code_search"));
        assert!(runtime_context.contains(
            "- Session state: planning_workflow=false, auto_permission=false, full_auto=false"
        ));
        assert!(runtime_context.contains("- Active primary permission default: deny"));
        assert!(runtime_context.contains("Plan carefully before editing."));
        assert_eq!(built.continuation_messages, vec![uni::Message::user("hello".to_string())]);
    }

    #[tokio::test]
    async fn active_primary_agent_memory_appendix_uses_canonical_name_for_alias_selection() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let workspace = backing.workspace_path().to_path_buf();
        std::fs::create_dir_all(workspace.join(".vtcode/agent-memory/reviewer"))
            .expect("canonical memory dir");
        std::fs::write(
            workspace.join(".vtcode/agent-memory/reviewer/MEMORY.md"),
            "# Reviewer Memory\n\n- Canonical reviewer memory.\n",
        )
        .expect("canonical memory");
        std::fs::create_dir_all(workspace.join(".vtcode/agent-memory/critic"))
            .expect("alias memory dir");
        std::fs::write(
            workspace.join(".vtcode/agent-memory/critic/MEMORY.md"),
            "# Critic Memory\n\n- Alias memory must not load.\n",
        )
        .expect("alias memory");

        let mut spec = test_primary_agent_spec("reviewer", "Review carefully.");
        spec.memory = Some(SubagentMemoryScope::Project);
        spec.aliases = vec!["critic".to_string()];
        backing.select_primary_agent_from_specs(&[spec], "critic");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        let runtime_context = system_prompt_text(&built.request);
        assert!(runtime_context.contains("### Memory Appendix"));
        assert!(runtime_context.contains("Primary-agent memory file:"));
        assert!(runtime_context.contains(".vtcode/agent-memory/reviewer/MEMORY.md"));
        assert!(runtime_context.contains("Canonical reviewer memory."));
        assert!(!runtime_context.contains("Alias memory must not load."));
        assert!(!runtime_context.contains("Create or update `MEMORY.md`"));
        assert!(!runtime_context.contains("Read and maintain `MEMORY.md`"));
    }

    #[tokio::test]
    async fn active_primary_agent_missing_memory_is_noop_and_does_not_expand_tools() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.add_tool_definition(named_tool("code_search")).await;
        backing.add_tool_definition(named_tool("apply_patch")).await;
        let workspace = backing.workspace_path().to_path_buf();
        let memory_dir = workspace.join(".vtcode/agent-memory/planner");

        let mut spec = test_primary_agent_spec("planner", "Plan carefully.");
        spec.memory = Some(SubagentMemoryScope::Project);
        spec.tools = Some(vec!["code_search".to_string()]);
        spec.disallowed_tools = Vec::new();
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        let runtime_context = system_prompt_text(&built.request);
        assert!(!runtime_context.contains("### Memory Appendix"));
        assert!(!runtime_context.contains("Create or update `MEMORY.md`"));
        assert!(!memory_dir.exists());
        assert_eq!(request_tool_names(&built.request), vec!["code_search"]);
    }

    #[tokio::test]
    async fn active_primary_agent_memory_appendix_is_replaced_on_switch() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let workspace = backing.workspace_path().to_path_buf();
        for (agent, memory) in [
            ("planner", "Planner-only durable note."),
            ("reviewer", "Reviewer-only durable note."),
        ] {
            let memory_dir = workspace.join(".vtcode/agent-memory").join(agent);
            std::fs::create_dir_all(&memory_dir).expect("memory dir");
            std::fs::write(memory_dir.join("MEMORY.md"), format!("- {memory}\n")).expect("memory");
        }

        let mut planner = test_primary_agent_spec("planner", "Planner instructions.");
        planner.memory = Some(SubagentMemoryScope::Project);
        let mut reviewer = test_primary_agent_spec("reviewer", "Reviewer instructions.");
        reviewer.memory = Some(SubagentMemoryScope::Project);

        backing.select_primary_agent_from_specs(std::slice::from_ref(&planner), "planner");
        let first_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("first request should build")
        };

        backing.select_primary_agent_from_specs(std::slice::from_ref(&reviewer), "reviewer");
        let second_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.clear();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("second request should build")
        };

        let first_runtime = system_prompt_text(&first_built.request);
        let second_runtime = system_prompt_text(&second_built.request);
        assert!(first_runtime.contains("Planner-only durable note."));
        assert!(!first_runtime.contains("Reviewer-only durable note."));
        assert!(second_runtime.contains("Reviewer-only durable note."));
        assert!(!second_runtime.contains("Planner-only durable note."));
    }

    #[tokio::test]
    async fn active_primary_agent_tool_allow_list_intersects_baseline_tools() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.add_tool_definition(named_tool("code_search")).await;
        backing.add_tool_definition(named_tool("apply_patch")).await;
        backing.add_tool_definition(named_tool("exec_command")).await;
        let mut spec = test_primary_agent_spec("planner", "Use limited tools.");
        spec.tools = Some(vec!["code_search".to_string(), "missing_tool".to_string()]);
        spec.disallowed_tools = Vec::new();
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(request_tool_names(&built.request), vec!["code_search"]);
        assert!(built.has_tools);
    }

    #[tokio::test]
    async fn active_primary_agent_deny_list_applies_after_allow_list() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.add_tool_definition(named_tool("code_search")).await;
        backing.add_tool_definition(named_tool("apply_patch")).await;
        let mut spec = test_primary_agent_spec("planner", "Use deterministic tools.");
        spec.tools = Some(vec!["code_search".to_string(), "apply_patch".to_string()]);
        spec.disallowed_tools = vec!["code_search".to_string()];
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(request_tool_names(&built.request), vec!["apply_patch"]);
        assert!(
            system_prompt_text(&built.request).contains("- Effective request tools: apply_patch")
        );
    }

    #[tokio::test]
    async fn unconstrained_primary_agent_falls_back_to_baseline_tools() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        backing.select_primary_agent_from_specs(
            &[vtcode_config::builtin_primary_build_agent()],
            "build",
        );
        backing.add_tool_definition(named_tool("code_search")).await;
        backing.add_tool_definition(named_tool("apply_patch")).await;

        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(request_tool_names(&built.request), vec!["code_search", "apply_patch"]);
        assert_eq!(built.continuation_messages, vec![uni::Message::user("hello".to_string())]);
    }

    #[tokio::test]
    async fn active_primary_agent_runtime_state_ignores_openai_response_chain() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let spec = test_primary_agent_spec("planner", "Use the active primary agent.");
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let prior_messages = vec![uni::Message::user("hello".to_string())];
        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.extend(prior_messages.clone());
            ctx.working_history.push(uni::Message::user("continue".to_string()));
            ctx.session_stats.set_previous_response_chain(
                "openai",
                "noop-model",
                Some("resp_123"),
                &prior_messages,
            );
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(built.request.previous_response_id, None);
        assert_eq!(
            non_runtime_request_messages(&built.request),
            vec![
                uni::Message::user("hello".to_string()),
                uni::Message::user("continue".to_string())
            ]
        );
        assert!(
            system_prompt_text(&built.request).contains("## Active Primary Agent Runtime State")
        );
        assert_eq!(
            built.continuation_messages,
            vec![
                uni::Message::user("hello".to_string()),
                uni::Message::user("continue".to_string())
            ]
        );
    }

    #[tokio::test]
    async fn active_primary_agent_runtime_state_keeps_stable_prompt_cache_friendly() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut cfg = VTCodeConfig::default();
        cfg.agent.include_temporal_context = true;
        cfg.prompt_cache.cache_friendly_prompt_shaping = true;
        let cfg = Box::leak(Box::new(cfg));
        let first = test_primary_agent_spec("planner", "Planner instructions.");
        let second = test_primary_agent_spec("reviewer", "Reviewer instructions.");

        backing.select_primary_agent_from_specs(std::slice::from_ref(&first), "planner");
        let first_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.vt_cfg = Some(cfg);
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("first request should build")
        };

        backing.select_primary_agent_from_specs(std::slice::from_ref(&second), "reviewer");
        let second_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.vt_cfg = Some(cfg);
            ctx.working_history.clear();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("second request should build")
        };

        let first_system = first_built.request.system_prompt.as_ref().expect("system");
        let second_system = second_built.request.system_prompt.as_ref().expect("system");
        assert_ne!(first_system, second_system);
        assert_eq!(
            stable_system_prefix_hash(first_system),
            stable_system_prefix_hash(second_system)
        );
        assert!(first_system.contains("Planner instructions."));
        assert!(second_system.contains("Reviewer instructions."));
        assert!(first_system.contains("Current date and time"));
        assert!(second_system.contains("Current date and time"));
    }

    #[tokio::test]
    async fn active_primary_agent_skills_are_request_scoped_on_switch() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut first = test_primary_agent_spec("planner", "Planner instructions.");
        first.skills = vec!["alpha".to_string()];
        let mut second = test_primary_agent_spec("reviewer", "Reviewer instructions.");
        second.skills = vec!["beta".to_string()];

        backing.select_primary_agent_from_specs(std::slice::from_ref(&first), "planner");
        let first_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("first request should build")
        };

        backing.select_primary_agent_from_specs(std::slice::from_ref(&second), "reviewer");
        let second_built = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.clear();
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
            build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("second request should build")
        };

        let first_system = first_built.request.system_prompt.as_ref().expect("system");
        let second_system = second_built.request.system_prompt.as_ref().expect("system");
        assert!(first_system.contains("## Active Primary Agent Skills"));
        assert!(first_system.contains("- alpha"));
        assert!(!first_system.contains("- beta"));
        assert!(second_system.contains("## Active Primary Agent Skills"));
        assert!(second_system.contains("- beta"));
        assert!(!second_system.contains("- alpha"));

        assert!(first_system.contains("- Active primary skills: alpha"));
        assert!(second_system.contains("- Active primary skills: beta"));
    }

    #[tokio::test]
    async fn primary_agent_model_and_reasoning_feed_request_metadata() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut spec = test_primary_agent_spec("planner", "Use agent metadata.");
        spec.model = Some("overlay-model".to_string());
        spec.reasoning_effort = Some(ReasoningEffortLevel::High);
        backing.select_primary_agent_from_specs(&[spec], "planner");

        let mut cfg = VTCodeConfig::default();
        cfg.agent.reasoning_effort = ReasoningEffortLevel::Medium;
        let cfg = Box::leak(Box::new(cfg));
        let built = {
            let mut ctx = backing.turn_processing_context();
            ctx.vt_cfg = Some(cfg);
            ctx.working_history.push(uni::Message::user("hello".to_string()));
            let mut snapshot = capture_turn_request_snapshot(&mut ctx, "base-model", false);
            assert_eq!(snapshot.active_model, "overlay-model");
            snapshot.capabilities.reasoning_effort = true;
            build_turn_request(&mut ctx, 1, "base-model", &snapshot, Some(320), None, false)
                .await
                .expect("request should build")
        };

        assert_eq!(built.request.model, "overlay-model");
        assert_eq!(built.request.reasoning_effort, Some(ReasoningEffortLevel::High));
        let runtime_context = system_prompt_text(&built.request);
        assert!(runtime_context.contains("- Request model: overlay-model"));
        assert!(runtime_context.contains("- Request reasoning effort: high"));
    }

    #[tokio::test]
    async fn editor_context_replays_full_openai_history_without_previous_response_chain() {
        let workspace = tempfile::TempDir::new().expect("workspace");
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut ctx = backing.turn_processing_context();
        ctx.context_manager.set_workspace_root(workspace.path());
        ctx.context_manager.set_editor_context_snapshot(
            Some(EditorContextSnapshot {
                workspace_root: Some(PathBuf::from(workspace.path())),
                active_file: Some(EditorFileContext {
                    path: workspace.path().join("src/main.rs").display().to_string(),
                    language_id: Some("rust".to_string()),
                    line_range: None,
                    dirty: false,
                    truncated: false,
                    selection: None,
                }),
                ..EditorContextSnapshot::default()
            }),
            Some(&vtcode_config::IdeContextConfig::default()),
        );
        ctx.working_history.push(uni::Message::user("hello".to_string()));

        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        let first =
            build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("first request should build");
        update_previous_response_chain_after_success(
            ctx.session_stats,
            "openai",
            false,
            "noop-model",
            Some("resp_123"),
            &first.continuation_messages,
        );

        ctx.working_history.push(uni::Message::user("continue".to_string()));
        ctx.context_manager.set_editor_context_snapshot(
            Some(EditorContextSnapshot {
                workspace_root: Some(PathBuf::from(workspace.path())),
                active_file: Some(EditorFileContext {
                    path: workspace.path().join("src/lib.rs").display().to_string(),
                    language_id: Some("rust".to_string()),
                    line_range: None,
                    dirty: false,
                    truncated: false,
                    selection: None,
                }),
                ..EditorContextSnapshot::default()
            }),
            Some(&vtcode_config::IdeContextConfig::default()),
        );

        let second =
            build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
                .await
                .expect("second request should build");

        assert_eq!(second.request.previous_response_id, None);
        let non_runtime_messages = non_runtime_request_messages(&second.request);
        assert_eq!(non_runtime_messages.len(), 3);
        assert_eq!(non_runtime_messages[0].role, uni::MessageRole::User);
        assert!(non_runtime_messages[0].content.as_text().contains("## Active Editor Context"));
        assert!(non_runtime_messages[0].content.as_text().contains("- Active file: src/lib.rs"));
        assert_eq!(non_runtime_messages[1], uni::Message::user("hello".to_string()));
        assert_eq!(non_runtime_messages[2], uni::Message::user("continue".to_string()));
        assert_eq!(
            second.continuation_messages,
            vec![
                uni::Message::user("hello".to_string()),
                uni::Message::user("continue".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn anthropic_request_build_combines_clearing_and_compaction_when_enabled() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut cfg = VTCodeConfig::default();
        cfg.agent.provider = "anthropic".to_string();
        cfg.agent.harness.auto_compaction_enabled = true;
        cfg.agent.harness.auto_compaction_threshold_tokens = Some(100_000);
        cfg.agent.harness.tool_result_clearing.enabled = true;
        cfg.agent.harness.tool_result_clearing.trigger_tokens = 120_000;
        cfg.agent.harness.tool_result_clearing.keep_tool_uses = 5;
        cfg.agent.harness.tool_result_clearing.clear_at_least_tokens = 40_000;
        cfg.provider.anthropic.memory.enabled = true;
        let cfg = Box::leak(Box::new(cfg));

        let mut ctx = backing.turn_processing_context();
        ctx.vt_cfg = Some(cfg);
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "claude-sonnet-4-6", false);
        snapshot.provider_name = "anthropic".to_string();
        snapshot.capabilities.context_edits = true;

        let built =
            build_turn_request(&mut ctx, 1, "claude-sonnet-4-6", &snapshot, Some(320), None, false)
                .await
                .expect("anthropic request should build");

        assert_eq!(
            built.request.context_management,
            Some(json!({
                "edits": [{
                    "type": "clear_tool_uses_20250919",
                    "trigger": { "type": "input_tokens", "value": 120000 },
                    "keep": { "type": "tool_uses", "value": 5 },
                    "clear_at_least": { "type": "input_tokens", "value": 40000 },
                    "clear_tool_inputs": false,
                    "exclude_tools": ["memory"],
                }, {
                    "type": "compact_20260112",
                    "trigger": { "type": "input_tokens", "value": 100000 },
                }]
            }))
        );

        let mut compaction_only_cfg = VTCodeConfig::default();
        compaction_only_cfg.agent.provider = "anthropic".to_string();
        compaction_only_cfg.agent.harness.auto_compaction_enabled = true;
        compaction_only_cfg.agent.harness.auto_compaction_threshold_tokens = Some(90_000);
        // `tool_result_clearing` defaults to enabled; disable it here so this
        // scenario exercises the "compaction only" path (clearing off).
        compaction_only_cfg.agent.harness.tool_result_clearing.enabled = false;
        ctx.vt_cfg = Some(Box::leak(Box::new(compaction_only_cfg)));
        let built =
            build_turn_request(&mut ctx, 1, "claude-sonnet-4-6", &snapshot, Some(320), None, false)
                .await
                .expect("compaction-only anthropic request should build");
        assert_eq!(
            built.request.context_management,
            Some(json!({
                "edits": [{
                    "type": "compact_20260112",
                    "trigger": { "type": "input_tokens", "value": 90000 },
                }]
            }))
        );

        // Default now enables auto-compaction, so explicitly disable it here to
        // assert the "no context management payload" (disabled) path.
        let mut disabled_cfg = VTCodeConfig::default();
        disabled_cfg.agent.harness.auto_compaction_enabled = false;
        // `tool_result_clearing` defaults to enabled; disable it here so the
        // "no context management payload" (fully disabled) path is exercised.
        disabled_cfg.agent.harness.tool_result_clearing.enabled = false;
        ctx.vt_cfg = Some(Box::leak(Box::new(disabled_cfg)));
        let built =
            build_turn_request(&mut ctx, 1, "claude-sonnet-4-6", &snapshot, Some(320), None, false)
                .await
                .expect("disabled anthropic request should build");
        assert!(built.request.context_management.is_none());
    }

    #[tokio::test]
    async fn openai_request_build_keeps_existing_compaction_payload() {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.auto_compaction_enabled = true;
        cfg.agent.harness.auto_compaction_threshold_tokens = Some(512);
        let cfg = Box::leak(Box::new(cfg));

        let mut ctx = backing.turn_processing_context();
        ctx.vt_cfg = Some(cfg);
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "gpt-5", false);
        snapshot.capabilities.responses_compaction = true;

        let built = build_turn_request(&mut ctx, 1, "gpt-5", &snapshot, Some(320), None, false)
            .await
            .expect("openai request should build");

        assert_eq!(
            built.request.context_management,
            Some(json!([{
                "type": "compaction",
                "compact_threshold": 512,
            }]))
        );
    }

    #[test]
    fn stable_prefix_hash_ignores_runtime_only_changes() {
        let first = "Static prefix\n## Skills\n- rust-skills\n[Runtime Context]\n- Time (UTC): 2026-03-22T00:00:00Z\n- retries: 1";
        let second = "Static prefix\n## Skills\n- rust-skills\n[Runtime Context]\n- Time (UTC): 2026-03-23T00:00:00Z\n- retries: 4";

        assert_eq!(stable_system_prefix_hash(first), stable_system_prefix_hash(second));
    }
}
