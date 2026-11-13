use super::*;

#[tokio::test]
async fn recovery_request_keeps_tools_for_cache_and_disables_tool_choice() {
    let mut backing = TestTurnProcessingBacking::new().await;
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
    vt_cfg.agent.verbosity = vtcode_core::config::types::VerbosityLevel::High;
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
    assert_eq!(normal_built.request.verbosity, Some(vtcode_core::config::types::VerbosityLevel::High));
    assert_eq!(built.request.verbosity, normal_built.request.verbosity);
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
    let mut backing = TestTurnProcessingBacking::new().await;
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
    let mut backing = TestTurnProcessingBacking::new().await;
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
    let mut backing = TestTurnProcessingBacking::new().await;
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
    let mut backing = TestTurnProcessingBacking::new().await;
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
    let mut backing = TestTurnProcessingBacking::new().await;
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
    let mut backing = TestTurnProcessingBacking::new().await;
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
