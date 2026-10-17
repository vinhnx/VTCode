use std::sync::Arc;

use serde_json::json;
use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
use vtcode_config::{SubagentMemoryScope, SubagentSource, SubagentSpec};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::config::{ToolDisplayMode, ToolOutputMode};
use vtcode_core::llm::provider::{self as uni, ToolDefinition};

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
    request.system_prompt.as_ref().expect("system prompt").as_ref()
}

#[tokio::test]
async fn recovery_request_keeps_tools_for_cache_and_disables_tool_choice() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.select_primary_agent_from_specs(&[vtcode_config::builtin_primary_build_agent()], "build");
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

    let normal_built = build_turn_request(&mut ctx, 1, "noop-model", &normal_snapshot, Some(320), None, false)
        .await
        .expect("normal request should build");
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("recovery request should build");

    assert_eq!(normal_built.request.reasoning_effort, Some(ReasoningEffortLevel::High));
    // Recovery keeps the same reasoning effort so the provider prefix
    // stays cache-stable (OpenAI: changing effort rewrites instructions).
    assert_eq!(built.request.reasoning_effort, Some(ReasoningEffortLevel::High));
    // Tool definitions stay on the wire so the provider prefix matches
    // tool-enabled turns; only the choice is disabled.
    assert!(built.has_tools);
    assert!(built.request.tools.as_ref().is_some_and(|tools| !tools.is_empty()));
    assert_eq!(
        request_tool_names(&built.request),
        request_tool_names(&normal_built.request),
        "recovery must keep the same ordered tool catalog as the prior turn"
    );
    assert!(matches!(built.request.tool_choice, Some(uni::ToolChoice::None)));
    assert_eq!(built.request.max_tokens, Some(320));

    let system_prompt = built.request.system_prompt.as_ref().expect("system prompt").as_ref();
    // The recovery contract rides as a request-only tail message, so the
    // system prompt stays byte-identical to the tool-enabled turn and the
    // provider prefix cache (tools -> system -> history) still hits.
    assert_eq!(
        built.request.system_prompt.as_deref(),
        normal_built.request.system_prompt.as_deref(),
        "recovery must not mutate the cached system prefix"
    );
    assert!(!system_prompt.contains("[Recovery Mode]"));
    assert!(!system_prompt.contains("recovery_reason: loop detector"));
    assert!(!system_prompt.contains("<budget:token_budget>"));

    let directive = built
        .request
        .messages
        .as_ref()
        .last()
        .expect("recovery request carries messages");
    let directive_text = directive.content.as_text();
    assert!(directive_text.as_ref().contains("[Recovery Mode]"));
    assert!(directive_text.as_ref().contains("do_not_request_more_tools: true"));
    assert!(directive_text.as_ref().contains("recovery_reason: loop detector"));
    assert!(
        !normal_built.request.messages.as_ref().iter().any(|message| message
            .content
            .as_text()
            .as_ref()
            .contains("[Recovery Mode]")),
        "non-recovery requests must not carry the recovery directive"
    );
}

#[tokio::test]
async fn recovery_prompt_reason_is_frozen_across_reason_updates() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.select_primary_agent_from_specs(&[vtcode_config::builtin_primary_build_agent()], "build");
    backing
        .add_tool_definition(ToolDefinition::function(
            "code_search".to_string(),
            "Search project files".to_string(),
            json!({"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}),
        ))
        .await;

    let mut ctx = backing.turn_processing_context();
    ctx.activate_recovery("loop detector");

    let first = capture_turn_request_snapshot(&mut ctx, "noop-model", true);
    // A later telemetry reason must not rewrite the frozen prompt block.
    ctx.harness_state.recovery_reason = Some("blocked tool-call fuse tripped".to_string());
    let second = capture_turn_request_snapshot(&mut ctx, "noop-model", true);

    assert_eq!(first.recovery_reason.as_deref(), Some("loop detector"), "prompt reason must reflect the activation");
    assert_eq!(
        second.recovery_reason.as_deref(),
        Some("loop detector"),
        "prompt reason must stay frozen while the activation is live"
    );
}

#[tokio::test]
async fn request_builder_omits_stale_effort_for_unsupported_route() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.set_provider(Box::new(vtcode_core::llm::providers::MinimaxProvider::from_config(
        Some("offline-fixture".to_string()),
        Some("MiniMax-M3".to_string()),
        None,
        None,
        None,
        None,
        None,
    )));

    let mut config = VTCodeConfig::default();
    config.agent.reasoning_effort = ReasoningEffortLevel::High;
    let mut ctx = backing.turn_processing_context();
    ctx.vt_cfg = Some(&config);
    ctx.working_history.push(uni::Message::user("hello".to_string()));

    let snapshot = capture_turn_request_snapshot(&mut ctx, "MiniMax-M3", false);
    let built = build_turn_request(&mut ctx, 1, "MiniMax-M3", &snapshot, Some(320), None, false)
        .await
        .expect("unsupported persisted effort must not block request assembly");

    assert!(built.request.reasoning_effort.is_none());
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
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("text-only request should build");

    assert!(!built.has_tools);
    assert!(built.request.tools.is_none());
    assert!(built.request.tool_choice.is_none());

    let system_prompt = built.request.system_prompt.as_ref().expect("system prompt").as_ref();
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
    let built = build_turn_request(&mut ctx, 1, "copilot-gpt-5.4", &snapshot, Some(320), None, true)
        .await
        .expect("copilot request should build");

    assert!(built.has_tools);
    assert!(built.request.tools.is_none());
    assert!(built.request.tool_choice.is_none());
    assert_eq!(built.runtime_tools.as_ref().map(|tools| tools.len()), Some(1));

    let system_prompt = built.request.system_prompt.as_ref().expect("system prompt").as_ref();
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

    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
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

    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
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
    ctx.session_stats
        .set_previous_response_chain("openai", "noop-model", Some("resp_123"), &prior_messages);

    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
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
    ctx.session_stats
        .set_previous_response_chain("mycorp", "noop-model", Some("resp_123"), &prior_messages);

    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    snapshot.provider_name = "mycorp".to_string();
    snapshot.capabilities.responses_compaction = true;
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
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
async fn clean_request_shares_messages_with_continuation_history() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.push(uni::Message::user("hello".to_string()));

    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("clean request should build");

    assert!(Arc::ptr_eq(&built.request.messages, &built.continuation_messages));
}

#[tokio::test]
async fn first_call_composition_captures_on_first_build_only() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.push(uni::Message::user("hello".to_string()));

    assert!(ctx.session_stats.first_call_composition().is_none());
    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("first request should build");

    let first = ctx
        .session_stats
        .first_call_composition()
        .expect("first build must capture composition");
    assert!(first.fixed_overhead_tokens() > 0, "first build must record a non-zero harness tax");
    // `first_call` is defined as "composition not yet captured", so the
    // first build is the only one that can observe it as true.
    let first_call_flag = ctx.session_stats.first_call_composition().is_none();
    assert!(!first_call_flag, "composition is already captured after the first build");

    ctx.working_history.push(uni::Message::user("again".to_string()));
    let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
        .await
        .expect("second request should build");
    assert_eq!(
        ctx.session_stats.first_call_composition(),
        Some(first),
        "later builds must not overwrite the first-call snapshot"
    );
}

#[tokio::test]
async fn non_openai_responses_chain_keeps_full_history() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let prior_messages = vec![uni::Message::user("hello".to_string())];
    let mut ctx = backing.turn_processing_context();
    ctx.working_history.extend(prior_messages.clone());
    ctx.working_history.push(uni::Message::user("continue".to_string()));
    ctx.session_stats
        .set_previous_response_chain("gemini", "noop-model", Some("resp_123"), &prior_messages);

    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
    snapshot.provider_name = "gemini".to_string();
    let built = build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
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
    let runtime_context = built.request.system_prompt.as_ref().expect("system prompt").as_ref();
    assert!(runtime_context.contains("## Active Primary Agent Runtime State"));
    assert!(runtime_context.contains("- Active agent: planner"));
    assert!(runtime_context.contains("- Effective request tools: code_search"));
    assert!(runtime_context.contains("- Session state: planning_workflow=false, full_auto=false"));
    assert!(!runtime_context.contains("auto_permission="));
    assert!(!runtime_context.contains("permission default"));
    assert!(runtime_context.contains("Plan carefully before editing."));
    assert_eq!(built.continuation_messages.as_slice(), [uni::Message::user("hello".to_string())]);
}

#[tokio::test]
async fn active_primary_agent_memory_appendix_uses_canonical_name_for_alias_selection() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let workspace = backing.workspace_path().to_path_buf();
    std::fs::create_dir_all(workspace.join(".vtcode/agent-memory/reviewer")).expect("canonical memory dir");
    std::fs::write(
        workspace.join(".vtcode/agent-memory/reviewer/MEMORY.md"),
        "# Reviewer Memory\n\n- Canonical reviewer memory.\n",
    )
    .expect("canonical memory");
    std::fs::create_dir_all(workspace.join(".vtcode/agent-memory/critic")).expect("alias memory dir");
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
async fn coordinator_selection_restricts_request_tools_and_restores_build_guidance() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    for name in [
        "matrix",
        "agent",
        "request_user_input",
        "code_search",
        "apply_patch",
        "exec_command",
    ] {
        backing.add_tool_definition(named_tool(name)).await;
    }
    let specs = [
        vtcode_config::builtin_primary_coordinator_agent(),
        vtcode_config::builtin_primary_build_agent(),
    ];
    backing.select_primary_agent_from_specs(&specs, "coordinator");
    let coordinated = {
        let mut ctx = backing.turn_processing_context();
        ctx.context_manager
            .set_base_system_prompt(vtcode_core::prompts::system::minimal_system_prompt().to_string());
        ctx.working_history
            .push(uni::Message::user("Inspect the asymmetric matrix".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("coordinator request")
    };
    let tool_names = request_tool_names(&coordinated.request);
    assert!(tool_names.contains(&"matrix".to_string()));
    for forbidden in ["code_search", "apply_patch", "exec_command"] {
        assert!(!tool_names.contains(&forbidden.to_string()), "{forbidden}");
    }
    let coordinator_prompt = system_prompt_text(&coordinated.request);
    assert!(coordinator_prompt.contains("## Coordinator Role"));
    assert!(!coordinator_prompt.contains("keep small tasks and verification in the main thread"));

    backing.select_primary_agent_from_specs(&specs, "build");
    let built = {
        let mut ctx = backing.turn_processing_context();
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("Build request")
    };
    assert!(request_tool_names(&built.request).contains(&"exec_command".to_string()));
    let build_prompt = system_prompt_text(&built.request);
    assert!(!build_prompt.contains("## Coordinator Role"));
    assert!(build_prompt.contains("keep small tasks and verification in the main thread"));
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
    assert!(system_prompt_text(&built.request).contains("- Effective request tools: apply_patch"));
}

#[tokio::test]
async fn unconstrained_primary_agent_falls_back_to_baseline_tools() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    backing.select_primary_agent_from_specs(&[vtcode_config::builtin_primary_build_agent()], "build");
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

    assert_eq!(request_tool_names(&built.request), vec!["apply_patch", "code_search"]);
    assert_eq!(built.continuation_messages.as_slice(), [uni::Message::user("hello".to_string())]);
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
        ctx.session_stats
            .set_previous_response_chain("openai", "noop-model", Some("resp_123"), &prior_messages);
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
    assert!(system_prompt_text(&built.request).contains("## Active Primary Agent Runtime State"));
    assert_eq!(
        built.continuation_messages.as_slice(),
        [
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
    assert_eq!(stable_system_prefix_hash(first_system), stable_system_prefix_hash(second_system));
    assert!(first_system.contains("Planner instructions."));
    assert!(second_system.contains("Reviewer instructions."));
    assert!(
        !first_system.contains("Current date and time") && !second_system.contains("Current date and time"),
        "temporal context must not be regenerated in the per-turn primary-agent appendix"
    );
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
async fn session_232419_read_evidence_survives_request_clearing() {
    struct EvidencePatchProvider(Arc<std::sync::atomic::AtomicUsize>);
    #[async_trait::async_trait]
    impl uni::LLMProvider for EvidencePatchProvider {
        fn name(&self) -> &str {
            "openai"
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        fn supported_models(&self) -> Vec<String> {
            vec!["noop-model".into()]
        }
        fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
            Ok(())
        }
        async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
            let count = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(count < 2, "edit and completion must not enter another recovery pass");
            let mut response = uni::LLMResponse::new(request.model, "Updated the Quick start wording.");
            if count == 0 {
                let source = request
                    .messages
                    .iter()
                    .find(|message| message.tool_call_id.as_deref() == Some("read_1"))
                    .unwrap();
                assert!(source.content.as_text().contains("default. To manage installation yourself, use Homebrew or Cargo; Cargo installs require Rust 1.98.1+:"));
                response.content = None;
                response.tool_calls = Some(vec![uni::ToolCall::function("edit".into(), "apply_patch".into(), json!({
                    "input": "*** Begin Patch\n*** Update File: README.md\n@@\n-default. To manage installation yourself, use Homebrew or Cargo; Cargo installs require Rust 1.98.1+:\n+default. To manage installation yourself, use Homebrew or Cargo:\n*** End Patch\n"
                }).to_string())]);
            }
            Ok(response)
        }
    }
    // Exact successful read outputs from the failed implementation turn,
    // excluding the transport's structured metadata. Six README pages were
    // followed by anchor searches and alignment reads before the intended edit.
    let reads: Vec<serde_json::Value> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/crates/codegen/vtcode-core/src/core/agent/state/fixtures/session_232419_reads.json"
    )))
    .unwrap();
    assert_eq!(reads.len(), 15);
    for parallel in [false, true] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        std::fs::write(backing.workspace_path().join("README.md"), reads[1]["output"].as_str().unwrap()).unwrap();
        let cfg = VTCodeConfig::default();
        backing.set_vt_cfg_for_test(cfg);
        let mut history = {
            let mut ctx = backing.turn_processing_context();
            ctx.working_history.push(uni::Message::user("Earlier inspection".into()));
            let mut old = uni::Message::assistant(String::new());
            old.tool_calls = Some(vec![uni::ToolCall::function(
                "old".into(),
                "exec_command".into(),
                json!({"cmd": "git status --short"}).to_string(),
            )]);
            ctx.working_history.push(old);
            ctx.working_history
                .push(uni::Message::tool_response("old".into(), "obsolete output ".repeat(20000)));
            ctx.working_history
                .push(uni::Message::user("Implement the approved README refinement".into()));
            // Replay both the actual parallel batches and one-call batches.
            let mut batch_start = 0;
            while batch_start < reads.len() {
                let batch_end = if parallel {
                    reads[batch_start..]
                        .iter()
                        .take_while(|read| read["batch"] == reads[batch_start]["batch"])
                        .count()
                        + batch_start
                } else {
                    batch_start + 1
                };
                let mut assistant = uni::Message::assistant(String::new());
                assistant.tool_calls = Some(
                    (batch_start..batch_end)
                        .map(|index| {
                            uni::ToolCall::function(
                                format!("read_{index}"),
                                "exec_command".into(),
                                json!({"cmd": reads[index]["cmd"]}).to_string(),
                            )
                        })
                        .collect(),
                );
                ctx.working_history.push(assistant);
                for (index, read) in reads.iter().enumerate().take(batch_end).skip(batch_start) {
                    ctx.working_history.push(uni::Message::tool_response(
                        format!("read_{index}"),
                        read["output"].as_str().unwrap().into(),
                    ));
                }
                batch_start = batch_end;
            }
            let original = ctx.working_history.clone();
            assert!(original.iter().map(uni::Message::estimate_tokens).sum::<usize>() > 40_000);
            let snapshot = capture_turn_request_snapshot(&mut ctx, "test-model", false);
            let built = build_turn_request(&mut ctx, 1, "test-model", &snapshot, Some(2000), None, false)
                .await
                .unwrap();
            let evidence = built
                .continuation_messages
                .iter()
                .find(|message| message.tool_call_id.as_deref() == Some("read_1"))
                .unwrap();
            assert_eq!(evidence.content.as_text(), reads[1]["output"].as_str().unwrap());
            assert!(evidence.content.as_text().contains("Rust 1.98.1+"));
            assert!(
                built
                    .continuation_messages
                    .iter()
                    .find(|message| message.tool_call_id.as_deref() == Some("old"))
                    .unwrap()
                    .content
                    .as_text()
                    .contains("tool_result_clearing")
            );
            for message in built.continuation_messages.iter() {
                for call in message.tool_calls.iter().flatten() {
                    let args: serde_json::Value =
                        serde_json::from_str(&call.function.as_ref().unwrap().arguments).unwrap();
                    assert!(args["cmd"].is_string(), "small executable arguments survive: {args}");
                }
            }
            assert!(vtcode_core::core::agent::state::validate_history_invariants(&built.request.messages).is_valid());
            assert!(ctx.working_history.starts_with(&original));
            original
        };
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        backing.set_provider(Box::new(EvidencePatchProvider(calls.clone())));
        let outcome =
            crate::agent::runloop::unified::turn::turn_loop::run_turn_loop(&mut history, backing.turn_loop_context())
                .await
                .unwrap();
        assert!(matches!(
            outcome.result,
            crate::agent::runloop::unified::turn::context::TurnLoopResult::Completed {
                plan_approved_execution_pending: false
            }
        ));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert!(!outcome.final_response_was_fallback);
        let edited = std::fs::read_to_string(backing.workspace_path().join("README.md")).unwrap();
        assert!(edited.contains("default. To manage installation yourself, use Homebrew or Cargo:\n"));
        assert!(!edited.contains("Cargo; Cargo installs require"));
    }
}

#[tokio::test]
async fn local_clearing_uses_session_budget_and_preserves_recent_batch() {
    for (enabled, recent_batches, native) in [
        (true, 0, false),
        (true, 1, false),
        (true, 2, false),
        (false, 2, false),
        (true, 0, true),
        (true, 2, true),
    ] {
        let mut backing = TestTurnProcessingBacking::new(4).await;
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.inspection.mode = vtcode_core::config::core::agent::InspectionMode::Enforce;
        cfg.context.max_context_tokens = 16_000;
        cfg.agent.harness.tool_result_clearing.trigger_tokens = 40_000;
        cfg.agent.harness.tool_result_clearing.enabled = enabled;
        cfg.agent.harness.tool_result_clearing.keep_recent_tool_batches = recent_batches;
        backing.set_vt_cfg_for_test(cfg);
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("Inspect evidence".into()));
        for (batch, count) in [1, 4, 3].into_iter().enumerate() {
            let mut assistant = uni::Message::assistant("Inspect batch".into());
            assistant.tool_calls = Some(
                (0..count)
                    .map(|call| {
                        uni::ToolCall::function(
                            format!("call_{batch}_{call}"),
                            "read_file".into(),
                            "{\"path\":\"source.rs\"}".into(),
                        )
                    })
                    .collect(),
            );
            ctx.working_history.push(assistant);
            for call in (0..count).rev() {
                ctx.working_history.push(uni::Message::tool_response(
                    format!("call_{batch}_{call}"),
                    format!("evidence_{batch}_{call} ").repeat(600),
                ));
            }
        }
        let original = ctx.working_history.clone();
        let tokens = original.iter().map(uni::Message::estimate_tokens).sum::<usize>();
        assert!(tokens < 40_000 && tokens > 9_600, "fixture lies between capped and configured triggers");
        let mut snapshot = capture_turn_request_snapshot(&mut ctx, "test-model", false);
        if native {
            snapshot.provider_name = "anthropic".into();
            snapshot.capabilities.context_edits = true;
        }
        let built = build_turn_request(&mut ctx, 1, "test-model", &snapshot, Some(2_000), None, false)
            .await
            .unwrap();
        let results: Vec<_> = built
            .continuation_messages
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .collect();
        assert_eq!(results.len(), 8);
        assert_eq!(results[0].content.as_text().contains("\"cleared\":\"tool_result\""), enabled);
        for result in &results[1..5] {
            assert_eq!(result.content.as_text().contains("\"cleared\":\"tool_result\""), enabled && recent_batches < 2);
        }
        for result in &results[5..] {
            assert!(result.content.as_text().contains("evidence_2_"));
        }
        assert!(vtcode_core::core::agent::state::validate_history_invariants(&built.request.messages).is_valid());
        assert!(ctx.working_history.starts_with(&original), "durable history retains full evidence");
    }
}

#[tokio::test]
async fn anthropic_request_build_combines_clearing_and_compaction_when_enabled() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut cfg = VTCodeConfig::default();
    cfg.agent.provider = "anthropic".to_string();
    cfg.agent.harness.auto_compaction_enabled = true;
    cfg.agent.harness.auto_compaction_threshold_tokens = Some(100_000);
    cfg.agent.harness.tool_result_clearing.enabled = true;
    // The default advisory policy preserves supported native clearing.
    assert_eq!(cfg.agent.harness.inspection.mode, vtcode_core::config::core::agent::InspectionMode::Warn);
    cfg.agent.harness.tool_result_clearing.trigger_tokens = 120_000;
    cfg.agent.harness.tool_result_clearing.keep_tool_uses = 5;
    cfg.agent.harness.tool_result_clearing.clear_at_least_tokens = 40_000;
    cfg.provider.anthropic.memory.enabled = true;
    let cfg = Box::leak(Box::new(cfg));

    let mut ctx = backing.turn_processing_context();
    ctx.vt_cfg = Some(cfg);
    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "claude-sonnet-5", false);
    snapshot.provider_name = "anthropic".to_string();
    snapshot.capabilities.context_edits = true;

    let built = build_turn_request(&mut ctx, 1, "claude-sonnet-5", &snapshot, Some(320), None, false)
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
                "clear_tool_inputs": true,
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
    let built = build_turn_request(&mut ctx, 1, "claude-sonnet-5", &snapshot, Some(320), None, false)
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
    let built = build_turn_request(&mut ctx, 1, "claude-sonnet-5", &snapshot, Some(320), None, false)
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
fn preflight_projection_preserves_other_planning_synthesis_directives() {
    let planning = crate::agent::runloop::unified::turn::tool_outcomes::handlers::PLANNING_TOOL_FREE_RECOVERY_DIRECTIVE;
    let mut history = vec![
        uni::Message::tool_response("policy".into(), r#"{"failure_kind":"policy"}"#.into()),
        uni::Message::system(planning.into()),
        uni::Message::assistant("new batch".into()),
        uni::Message::tool_response("preflight".into(), r#"{"failure_kind":"preflight_circuit_breaker"}"#.into()),
        uni::Message::system(planning.into()),
    ];
    let canonical = history.clone();
    super::omit_expired_preflight_directives(&mut history);
    assert_eq!(history, canonical[..4]);
    assert_eq!(canonical.len(), 5);
}

#[test]
fn preflight_fixture_projection_removes_expired_bytes_without_changing_archive() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tool_outcomes/handlers/tests/fixtures/tracker_preflight_session.json"
    ))
    .unwrap();
    let canonical: Vec<_> = fixture["legacy_preflight_directives"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| uni::Message::system(value.as_str().unwrap().to_string()))
        .collect();
    assert_eq!(canonical.len(), 6);
    let bytes_before: usize = canonical.iter().map(|message| message.content.as_text().len()).sum();
    let mut projected = canonical.clone();
    super::omit_expired_preflight_directives(&mut projected);
    assert!(projected.is_empty());
    assert_eq!(canonical.len(), 6);
    assert_eq!(bytes_before, 2298);
}

#[test]
fn preflight_projection_omits_only_identifiable_legacy_system_directives() {
    let legacy = crate::agent::runloop::unified::turn::tool_outcomes::handlers::PREFLIGHT_CIRCUIT_RECOVERY_DIRECTIVE;
    let canonical = vec![
        uni::Message::system(legacy.to_string()),
        uni::Message::user(legacy.to_string()),
        uni::Message::system("Policy: tools are disabled until approval".to_string()),
    ];
    let archive = canonical.clone();
    let mut projected = canonical.clone();
    super::omit_expired_preflight_directives(&mut projected);
    assert_eq!(projected, canonical[1..]);
    assert_eq!(canonical, archive);
    projected.push(uni::Message::turn_scoped_system(super::recovery_mode_directive(
        false,
        Some("preflight validation circuit breaker"),
    )));
    assert!(
        projected
            .last()
            .unwrap()
            .content
            .as_text()
            .contains("do_not_emit_tool_calls: true")
    );
    super::omit_expired_preflight_directives(&mut projected);
    assert_eq!(projected.len(), 3, "active request-only synthesis must survive");
}

#[test]
fn stable_prefix_hash_ignores_runtime_only_changes() {
    let first =
        "Static prefix\n## Skills\n- rust-skills\n[Runtime Context]\n- Time (UTC): 2026-03-22T00:00:00Z\n- retries: 1";
    let second =
        "Static prefix\n## Skills\n- rust-skills\n[Runtime Context]\n- Time (UTC): 2026-03-23T00:00:00Z\n- retries: 4";

    assert_eq!(stable_system_prefix_hash(first), stable_system_prefix_hash(second));
}

#[test]
fn collapsed_output_notice_is_provider_neutral_for_collapsed_tool_turns() {
    let history = vec![uni::Message::tool_response("toolu_1".to_string(), "output".to_string())];

    assert!(super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Compact,
        Some(ToolOutputMode::Full),
        &history,
    ));
    assert!(super::should_add_collapsed_tool_output_notice(
        false,
        ToolDisplayMode::Expanded,
        Some(ToolOutputMode::Compact),
        &history,
    ));
    assert!(!super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Expanded,
        Some(ToolOutputMode::Full),
        &history,
    ));
    let command_history = vec![uni::Message::tool_response_with_origin(
        "toolu_2".to_string(),
        "output".to_string(),
        "exec_command".to_string(),
    )];
    assert!(super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Expanded,
        Some(ToolOutputMode::Full),
        &command_history,
    ));
    assert!(super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Compact,
        Some(ToolOutputMode::Compact),
        &history,
    ));

    let history_with_system_directive = vec![
        history[0].clone(),
        uni::Message::system("auto-permission review warning".to_string()),
    ];
    assert!(super::should_add_collapsed_tool_output_notice(
        true,
        ToolDisplayMode::Expanded,
        Some(ToolOutputMode::Compact),
        &history_with_system_directive,
    ));
}

fn write_patch_edit_few_shot_example(workspace: &std::path::Path) {
    let examples_dir = workspace.join(".vtcode/prompts/examples");
    std::fs::create_dir_all(&examples_dir).expect("examples dir");
    std::fs::write(
        examples_dir.join("patch-edit.md"),
        "---\nid: patch-edit\ntags: [patch, edit]\nsummary: Use apply_patch for edits.\n---\n# User\nedit the file\n\n# Assistant\nRead it, then apply_patch.\n",
    )
    .expect("few-shot example");
}

fn is_few_shot_block(message: &uni::Message) -> bool {
    message
        .content
        .as_text()
        .starts_with(vtcode_core::prompts::FEW_SHOT_SECTION_HEADER)
}

#[tokio::test]
async fn few_shot_context_is_persisted_once_and_replayed_append_only() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    write_patch_edit_few_shot_example(backing.workspace_path());
    let mut ctx = backing.turn_processing_context();
    ctx.working_history
        .push(uni::Message::user("please patch and edit the parser".to_string()));

    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "claude-opus-5-5", false);
    snapshot.provider_name = "anthropic".to_string();
    snapshot.capabilities.turn_scoped_system_messages = true;

    let first = build_turn_request(&mut ctx, 1, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("first request should build");
    let first_messages = non_runtime_request_messages(&first.request);
    assert_eq!(first_messages.len(), 2);
    assert_eq!(first_messages[0], uni::Message::user("please patch and edit the parser".to_string()));
    assert_eq!(first_messages[1].role, uni::MessageRole::System);
    assert_eq!(first_messages[1].clear_at, Some(uni::MessageClearAt::NextUserMessage));
    assert!(is_few_shot_block(&first_messages[1]));
    assert!(first_messages[1].content.as_text().contains("### patch-edit"));
    assert!(!system_prompt_text(&first.request).contains(vtcode_core::prompts::FEW_SHOT_SECTION_HEADER));

    ctx.working_history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            "call_1".to_string(),
            "read_file".to_string(),
            "{}".to_string(),
        )],
    ));
    ctx.working_history
        .push(uni::Message::tool_response("call_1".to_string(), "fn parse() {}".to_string()));

    let second = build_turn_request(&mut ctx, 2, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("second request should build");
    let second_messages = non_runtime_request_messages(&second.request);
    assert_eq!(
        &second_messages[..first_messages.len()],
        first_messages.as_slice(),
        "later requests of the turn must only append to the earlier request"
    );
    assert_eq!(second_messages.iter().filter(|message| is_few_shot_block(message)).count(), 1);

    let mut fold_snapshot = snapshot.clone();
    fold_snapshot.capabilities.turn_scoped_system_messages = false;
    let folded = build_turn_request(&mut ctx, 3, "claude-sonnet-5", &fold_snapshot, Some(320), None, false)
        .await
        .expect("fold-route request should build");
    let folded_messages = non_runtime_request_messages(&folded.request);
    assert_eq!(folded_messages[1].role, uni::MessageRole::User);
    assert!(is_few_shot_block(&folded_messages[1]));
    assert!(folded_messages.iter().all(|message| message.clear_at.is_none()));
    assert!(
        !folded_messages
            .iter()
            .any(|message| message.role == uni::MessageRole::System && is_few_shot_block(message)),
        "fold routes must not receive few-shot context as a system message"
    );
    assert_eq!(ctx.working_history.iter().filter(|message| is_few_shot_block(message)).count(), 1);
}

fn exec_round(ctx: &mut crate::agent::runloop::unified::turn::context::TurnProcessingContext<'_>, id: &str) {
    ctx.working_history.push(uni::Message::assistant_with_tools(
        String::new(),
        vec![uni::ToolCall::function(
            id.to_string(),
            "exec_command".to_string(),
            "{}".to_string(),
        )],
    ));
    ctx.working_history
        .push(uni::Message::tool_response(id.to_string(), "exit 0".to_string()));
}

fn collapsed_notice_count(messages: &[uni::Message]) -> usize {
    messages
        .iter()
        .filter(|message| {
            message.role == uni::MessageRole::System
                && message.content.as_text().as_ref() == super::COLLAPSED_TOOL_OUTPUT_NOTICE
        })
        .count()
}

#[tokio::test]
async fn collapsed_output_notice_is_append_only_across_tool_rounds() {
    let mut backing = TestTurnProcessingBacking::new(4).await;
    let mut config = VTCodeConfig::default();
    config.agent.provider = "anthropic".to_string();
    let config = Box::leak(Box::new(config));

    let mut ctx = backing.turn_processing_context();
    ctx.vt_cfg = Some(config);
    ctx.working_history.push(uni::Message::user("run the checks".to_string()));
    exec_round(&mut ctx, "toolu_1");

    let mut snapshot = capture_turn_request_snapshot(&mut ctx, "claude-opus-5-5", false);
    snapshot.provider_name = "anthropic".to_string();
    snapshot.capabilities.turn_scoped_system_messages = true;

    let first = build_turn_request(&mut ctx, 1, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("first request should build");
    let first_messages = non_runtime_request_messages(&first.request);
    assert_eq!(collapsed_notice_count(&first_messages), 1);
    assert!(first_messages.iter().any(|message| {
        message.clear_at == Some(uni::MessageClearAt::NextUserMessage)
            && message.content.as_text().as_ref() == super::COLLAPSED_TOOL_OUTPUT_NOTICE
    }));

    let repeated = build_turn_request(&mut ctx, 2, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("repeated request should build");
    assert_eq!(non_runtime_request_messages(&repeated.request), first_messages);

    exec_round(&mut ctx, "toolu_2");
    let second = build_turn_request(&mut ctx, 3, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("next tool round should build");
    let second_messages = non_runtime_request_messages(&second.request);
    assert_eq!(
        &second_messages[..first_messages.len()],
        first_messages.as_slice(),
        "a later tool round must not move or delete the notice already sent"
    );
    assert_eq!(collapsed_notice_count(&second_messages), 1, "one notice per user turn");

    ctx.working_history.push(uni::Message::assistant("checks pass".to_string()));
    ctx.working_history.push(uni::Message::user("run them again".to_string()));
    exec_round(&mut ctx, "toolu_3");
    let next_turn = build_turn_request(&mut ctx, 4, "claude-opus-5-5", &snapshot, Some(320), None, false)
        .await
        .expect("next user turn should build");
    let next_turn_messages = non_runtime_request_messages(&next_turn.request);
    assert!(next_turn_messages.starts_with(&second_messages));
    assert_eq!(collapsed_notice_count(&next_turn_messages), 2, "each user turn gets its own turn-scoped copy");

    let mut non_anthropic_snapshot = snapshot.clone();
    non_anthropic_snapshot.provider_name = "openai".to_string();
    non_anthropic_snapshot.capabilities.turn_scoped_system_messages = false;
    let switched = build_turn_request(&mut ctx, 5, "gpt-5", &non_anthropic_snapshot, Some(320), None, false)
        .await
        .expect("provider-switched request should build");
    assert!(switched.request.messages.iter().all(|message| message.clear_at.is_none()));
    assert_eq!(
        collapsed_notice_count(switched.request.messages.as_slice()),
        1,
        "routes without turn-scoped messages receive only the latest copy"
    );
    let latest_notice = switched
        .request
        .messages
        .iter()
        .rposition(|message| message.content.as_text().as_ref() == super::COLLAPSED_TOOL_OUTPUT_NOTICE)
        .expect("notice");
    assert_eq!(switched.request.messages[latest_notice - 1].tool_call_id.as_deref(), Some("toolu_3"));
    assert_eq!(collapsed_notice_count(switched.continuation_messages.as_slice()), 2);
}
