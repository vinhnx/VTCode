use super::*;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider::{FinishReason, LLMResponse, ToolCall, Usage};

#[test]
fn attempted_draft_detection_preserves_questions_native_tools_and_execution_prose() {
    let draft = "## Implementation Steps\n1. Inspect README\n2. Refine wording";
    for (planning, native_tools, question) in [(true, true, false), (true, false, true), (false, false, false)] {
        let text = if question {
            format!("{draft}\nWhich audience should this target?")
        } else {
            draft.to_string()
        };
        let mut response = LLMResponse::new("test", &text);
        if native_tools {
            response.tool_calls = Some(vec![ToolCall::function(
                "read".to_string(),
                "read_file".to_string(),
                r#"{"path":"README.md"}"#.to_string(),
            )]);
        }
        let mut renderer = AnsiRenderer::stdout();
        let result =
            process_llm_response(&response, &mut renderer, 0, planning, false, false, true, None, None).unwrap();
        if native_tools {
            let TurnProcessingResult::ToolCalls { tool_calls, .. } = result else {
                panic!("native inspection must be preserved");
            };
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].tool_name(), "read_file");
        } else {
            assert!(
                matches!(result, TurnProcessingResult::TextResponse { proposed_plan: None, text: actual, .. } if actual == text)
            );
        }
    }
}

#[test]
fn prepare_tool_calls_reuses_parsed_arguments_and_metadata() {
    let tool_calls = vec![
        ToolCall::function(
            "call_search".to_string(),
            tools::CODE_SEARCH.to_string(),
            r#"{"query":"TurnProcessingResult"}"#.to_string(),
        ),
        ToolCall::function(
            "call_exec".to_string(),
            tools::UNIFIED_EXEC.to_string(),
            r#"{"action":"run","command":["cargo","check"]}"#.to_string(),
        ),
    ];

    let prepared = prepare_tool_calls(tool_calls);

    assert_eq!(prepared.len(), 2);
    assert_eq!(prepared[0].call_id(), "call_search");
    assert_eq!(prepared[0].tool_name(), tools::CODE_SEARCH);
    assert_eq!(prepared[0].args(), Some(&serde_json::json!({"query":"TurnProcessingResult"})));
    assert!(prepared[0].is_parallel_safe());
    assert!(!prepared[0].is_command_execution());

    assert_eq!(prepared[1].call_id(), "call_exec");
    assert_eq!(prepared[1].tool_name(), tools::UNIFIED_EXEC);
    assert_eq!(prepared[1].args(), Some(&serde_json::json!({"action":"run","command":["cargo","check"]})));
    assert!(!prepared[1].is_parallel_safe());
    assert!(prepared[1].is_command_execution());
}

#[test]
fn prepare_tool_calls_records_invalid_json_without_reparsing() {
    let tool_calls = vec![ToolCall::function(
        "call_invalid".to_string(),
        tools::CODE_SEARCH.to_string(),
        "{not-json}".to_string(),
    )];

    let prepared = prepare_tool_calls(tool_calls);

    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].call_id(), "call_invalid");
    assert_eq!(prepared[0].tool_name(), tools::CODE_SEARCH);
    assert!(prepared[0].args().is_none());
    assert!(prepared[0].args_error().is_some());
    assert!(!prepared[0].is_parallel_safe());
    assert!(!prepared[0].is_command_execution());
}

#[test]
fn prepare_tool_calls_keeps_custom_tool_payload_as_raw_string() {
    let tool_calls = vec![ToolCall::custom(
        "call_patch".to_string(),
        tools::APPLY_PATCH.to_string(),
        "*** Begin Patch\n*** End Patch\n".to_string(),
    )];

    let prepared = prepare_tool_calls(tool_calls);

    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].call_id(), "call_patch");
    assert_eq!(prepared[0].tool_name(), tools::APPLY_PATCH);
    assert_eq!(prepared[0].args(), Some(&serde_json::json!("*** Begin Patch\n*** End Patch\n")));
    assert!(prepared[0].args_error().is_none());
    assert!(!prepared[0].is_parallel_safe());
    assert!(!prepared[0].is_command_execution());
}

#[test]
fn extract_interview_questions_from_numbered_lines() {
    let text = "1. First question?\n2) Second question?\n3. Third question?";
    let questions = extract_interview_questions(text);
    assert_eq!(questions.len(), 3);
    assert_eq!(questions[0], "First question?");
    assert_eq!(questions[1], "Second question?");
    assert_eq!(questions[2], "Third question?");
}

#[test]
fn extract_interview_questions_from_bullets() {
    let text = "- Should we do X?\n- Should we do Y?";
    let questions = extract_interview_questions(text);
    assert_eq!(questions.len(), 2);
    assert_eq!(questions[0], "Should we do X?");
}

#[test]
fn process_llm_response_turns_questions_into_tool_call() {
    let response = LLMResponse {
        content: Some("1. First question?\n2. Second question?".to_string()),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, true, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
        }
        _ => panic!("Expected tool calls"),
    }
}

fn refusal_response(content: Option<&str>, reasoning_details: Option<Vec<String>>) -> LLMResponse {
    LLMResponse {
        content: content.map(str::to_string),
        finish_reason: FinishReason::Refusal,
        reasoning_details,
        ..Default::default()
    }
}

fn usage_with_completion_tokens(completion_tokens: u32) -> Usage {
    Usage {
        prompt_tokens: 1,
        completion_tokens,
        total_tokens: completion_tokens + 1,
        ..Default::default()
    }
}

#[test]
fn process_llm_response_captures_completion_tokens_without_content() {
    // The observed failure shape: the provider billed tokens but surfaced
    // no final content, so the turn must record that evidence instead of
    // claiming a plain empty response.
    let response = LLMResponse {
        usage: Some(usage_with_completion_tokens(477)),
        ..Default::default()
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, true, false, None, None)
        .expect("processing should succeed");

    let TurnProcessingResult::Empty { facts } = result else {
        panic!("tokens without content must classify as empty with captured facts");
    };
    assert_eq!(facts.completion_tokens, 477);
    assert!(facts.has_completion_tokens());
    assert!(!facts.dropped_tool_calls);
    assert!(!facts.reasoning_only);
}

#[test]
fn process_llm_response_captures_dropped_tool_calls_and_keeps_reasoning() {
    // A tool-free pass that still emits native tool calls drops them; the
    // reasoning text those calls displaced must survive for repair instead
    // of being reported as an empty synthesis. Reasoning models deliver
    // that text inside `<thinking>` tags on the wire.
    let plan_body = "## Summary\nPlan to change the parser.\n## Implementation Steps\n1. Edit the parser -> files: [src/a.rs] -> verify: [cargo check]";
    let response = LLMResponse {
        content: None,
        reasoning: Some(format!("<thinking>{plan_body}</thinking>")),
        tool_calls: Some(vec![ToolCall::function(
            "call_1".to_string(),
            "grep".to_string(),
            "{}".to_string(),
        )]),
        usage: Some(usage_with_completion_tokens(210)),
        ..Default::default()
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, true, false, None, None)
        .expect("processing should succeed");

    let TurnProcessingResult::Empty { facts } = result else {
        panic!("dropped tool calls must classify as empty with captured facts");
    };
    assert!(facts.dropped_tool_calls);
    assert_eq!(facts.completion_tokens, 210);
    assert!(facts.reasoning_only);
}

#[test]
fn process_llm_response_total_absence_captures_no_evidence() {
    // Asymmetric pair for the tokens case: a genuinely empty response must
    // not fabricate salvage or a token count it has no evidence for.
    let response = LLMResponse::default();

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, true, false, None, None)
        .expect("processing should succeed");

    let TurnProcessingResult::Empty { facts } = result else {
        panic!("a response with nothing in it must classify as empty");
    };
    assert_eq!(facts.completion_tokens, 0);
    assert!(!facts.has_completion_tokens());
    assert!(!facts.dropped_tool_calls);
    assert!(!facts.reasoning_only);
}

#[test]
fn process_llm_response_ends_refused_turn_with_category_and_explanation() {
    let detail = serde_json::json!({
        "type": "stop_details",
        "category": "cyber",
        "explanation": "Request resembles malware development",
    })
    .to_string();
    // Partial output cut off by the refusal must not become the answer.
    let response = refusal_response(Some("Sure, here is"), Some(vec![detail]));

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, true, true, true, None, None)
        .expect("processing should succeed");

    let TurnProcessingResult::Refusal { reason } = result else {
        panic!("refusal should end the turn");
    };
    assert_eq!(
        reason,
        "The model declined this request (category: cyber): request resembles malware development. \
         The request was not retried; rephrase it or switch models."
    );
    assert!(!reason.contains("Sure, here is"), "{reason}");
    assert!(!reason.contains("fallback"), "no fallback ran, so none is claimed: {reason}");
}

#[test]
fn process_llm_response_uses_refusal_content_when_stop_details_are_absent() {
    let response = refusal_response(Some("  I can't help with that.  "), None);

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, true, true, true, None, None)
        .expect("processing should succeed");

    let TurnProcessingResult::Refusal { reason } = result else {
        panic!("refusal should end the turn");
    };
    assert_eq!(
        reason,
        "The model declined this request: I can't help with that. \
         The request was not retried; rephrase it or switch models."
    );
}

#[test]
fn process_llm_response_reports_refusal_without_stop_details() {
    let response = refusal_response(None, None);

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, true, true, true, None, None)
        .expect("processing should succeed");

    let TurnProcessingResult::Refusal { reason } = result else {
        panic!("refusal should not fall through to empty-response recovery");
    };
    assert!(reason.starts_with("The model declined this request."), "{reason}");
    assert!(!reason.contains("category"), "{reason}");
    assert!(!reason.contains("fallback"), "{reason}");
}

#[tokio::test]
async fn process_llm_response_rejects_textual_exec_command_without_command() {
    let temp = tempfile::tempdir().expect("temp workspace");
    let tool_registry = vtcode_core::tools::ToolRegistry::new(temp.path().to_path_buf()).await;
    let response = LLMResponse {
        content: Some("run()".to_string()),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result =
        process_llm_response(&response, &mut renderer, 7, false, false, true, true, None, Some(&tool_registry))
            .expect("processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text, "run()");
        }
        _ => panic!("Expected textual invalid exec_command to stay a text response"),
    }
}

#[test]
fn process_llm_response_extracts_execution_replan_block() {
    let response = LLMResponse {
        content: Some(
            "Replanning: the approved paths no longer exist.\n<proposed_plan>\n## Summary\nRevised scope.\n</proposed_plan>"
                .to_string(),
        ),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    // planning_active=false: execution turns must still surface an
    // explicit `<proposed_plan>` replan instead of leaking it as text.
    let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
            assert!(proposed_plan.is_some_and(|plan| plan.contains("Revised scope")));
            assert!(!text.contains("<proposed_plan>"));
            assert!(text.contains("Replanning: the approved paths no longer exist."));
        }
        _ => panic!("Expected text response with extracted execution replan"),
    }
}

#[test]
fn process_llm_response_keeps_execution_replan_with_tool_calls_unextracted() {
    let response = LLMResponse {
        content: Some("Continuing work.\n<proposed_plan>\n## Summary\nDraft only.\n</proposed_plan>".to_string()),
        tool_calls: Some(vec![ToolCall::function(
            "call_1".to_string(),
            "code_search".to_string(),
            r#"{"query":"x"}"#.to_string(),
        )]),
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, assistant_text, .. } => {
            assert_eq!(tool_calls.len(), 1);
            assert!(assistant_text.contains("<proposed_plan>"));
        }
        _ => panic!("Expected tool-call dispatch when the response carries native calls"),
    }
}

#[test]
fn process_llm_response_parses_spaced_dsml_before_stripping_markup() {
    let response = LLMResponse {
        content: Some(concat!(
            "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke name=\"exec_command\">\n",
            "<\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter name=\"cmd\" string=\"true\">true</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} parameter>\n",
            "</\u{ff5c} \u{ff5c} DSML\u{ff5c} \u{ff5c} invoke>",
        ).to_string()),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].tool_name(), tools::EXEC_COMMAND);
            assert_eq!(tool_calls[0].args(), Some(&serde_json::json!({"cmd": "true", "action": "run"})));
        }
        TurnProcessingResult::TextResponse { text, .. } => {
            panic!("spaced DSML leaked as text: {text}");
        }
        TurnProcessingResult::Empty { .. } | TurnProcessingResult::Refusal { .. } => {
            panic!("spaced DSML should produce a tool call")
        }
    }
}

#[test]
fn process_llm_response_skips_questions_when_interview_not_ready() {
    let response = LLMResponse {
        content: Some("1. First question?\n2. Second question?".to_string()),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert!(text.contains("First question"));
        }
        _ => panic!("Expected text response without tool calls"),
    }
}

#[test]
fn process_llm_response_removes_plan_from_duplicate_assistant_text() {
    let response = LLMResponse {
        content: Some("Intro\n<proposed_plan>\n- Step 1\n</proposed_plan>\nOutro".to_string()),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
            assert_eq!(text, "Intro\n\nOutro");
            assert!(!text.contains("<proposed_plan>"));
            assert_eq!(proposed_plan.as_deref(), Some("- Step 1"));
        }
        _ => panic!("Expected text response with visible proposed plan"),
    }
}

#[test]
fn process_llm_response_extracts_plan_block_in_planning_workflow() {
    let response = LLMResponse {
        content: Some("Intro\n<plan>\n- Step 1\n</plan>\nOutro".to_string()),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
            assert_eq!(proposed_plan.as_deref(), Some("- Step 1"));
            assert!(!text.contains("<plan>"));
            assert!(!text.contains("</plan>"));
        }
        _ => panic!("Expected text response with extracted <plan> block"),
    }
}

#[test]
fn process_llm_response_removes_echoed_plan_policy_text() {
    let policy = vtcode_core::prompts::system::PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE;
    let response = LLMResponse {
        content: Some(format!("{policy}\n\n<plan>\n- Step 1\n</plan>")),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
        .expect("response processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
            assert!(text.trim().is_empty(), "policy echo must not remain visible: {text:?}");
            assert_eq!(proposed_plan.as_deref(), Some("- Step 1"));
        }
        _ => panic!("expected the alternate plan block to remain actionable"),
    }
}

#[test]
fn process_llm_response_extracts_canonical_stream_handoff_plan() {
    let response = LLMResponse {
        content: Some("Intro\n\nOutro\n\n<proposed_plan>\n- Streamed step\n</proposed_plan>".to_string()),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
        .expect("response processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
            assert_eq!(proposed_plan.as_deref(), Some("- Streamed step"));
            assert_eq!(text.trim_end(), "Intro\n\nOutro");
            assert!(!text.contains("<proposed_plan>"));
        }
        _ => panic!("expected the canonical stream handoff to produce a text response"),
    }
}

#[test]
fn process_llm_response_drops_tool_calls_attached_to_a_complete_plan() {
    let response = LLMResponse {
        content: Some(
            "# Attached plan\n\n## Summary\nKeep approval ahead of tool execution.\n\n## Steps\n1. Gate the plan -> files: [src/agent/runloop/unified/turn/turn_processing/response_processing.rs] -> verify: cargo check\n\n## Validation\n1. Run the planning regression tests.\n\n## Assumptions\n1. Preserve the existing planning policy.\n".to_string(),
        ),
        tool_calls: Some(vec![ToolCall::function(
            "call_after_plan".to_string(),
            "code_search".to_string(),
            r#"{"query":"should-not-run"}"#.to_string(),
        )]),
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::ToolCalls,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
            assert!(text.is_empty());
            assert!(proposed_plan.is_some());
        }
        TurnProcessingResult::ToolCalls { .. } => panic!("attached calls must not bypass plan approval"),
        TurnProcessingResult::Empty { .. } | TurnProcessingResult::Refusal { .. } => {
            panic!("complete plan should remain actionable")
        }
    }
}

#[test]
fn process_llm_response_extracts_sparse_proposed_plan_blocks() {
    let response = LLMResponse {
        content: Some(
            r#"Intro
Next open decision: none.
<proposed_plan>
# Apply Slate-Style Prompting

## Summary
Keep the default runtime prompt sparse and consistent with Planning workflow.

## Implementation Steps
1. Update prompt constants -> files: [crates/codegen/vtcode-core/src/prompts/system.rs] -> verify: [cargo check]
2. Update plan scaffold -> files: [crates/codegen/vtcode-core/src/tools/handlers/planning_workflow.rs] -> verify: [cargo test -p vtcode-core test_start_planning -- --nocapture]
3. Update parser tests -> files: [src/agent/runloop/unified/turn/turn_processing/response_processing.rs] -> verify: [cargo test -p vtcode process_llm_response_extracts_sparse_proposed_plan_blocks -- --nocapture]

## Test Cases and Validation
1. Build and lint: [project build and lint command(s) based on detected toolchain]
2. Tests: [project test command(s) based on detected toolchain]
3. Targeted behavior checks: planning workflow transcript extraction

## Assumptions and Defaults
1. `Next open decision` remains the only explicit reopen marker.
2. The proposed plan stays decision-complete without rigid extra sections.
</proposed_plan>
Outro"#
                .to_string(),
        ),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, proposed_plan, .. } => {
            let plan = proposed_plan.expect("expected extracted proposed plan");
            for required_section in [
                "## Summary",
                "## Implementation Steps",
                "## Test Cases and Validation",
                "## Assumptions and Defaults",
            ] {
                assert!(plan.contains(required_section), "proposed_plan missing required section: {required_section}");
            }
            assert!(!plan.contains("## Scope Locked"));
            assert!(!plan.contains("## Public API / Interface Changes"));
            assert!(!text.contains("<proposed_plan>"));
            assert!(text.contains("Intro"));
            assert!(text.contains("Outro"));
        }
        _ => panic!("Expected text response with extracted proposed plan"),
    }
}

#[test]
fn tool_free_planning_recovery_accepts_only_one_canonical_plan_block() {
    let valid_plan = "<proposed_plan>\n## Summary\nKeep recovery deterministic.\n\n## Implementation Steps\n1. Update src/lib.rs -> files: [src/lib.rs] -> verify: [cargo check --locked]\n\n## Test Cases and Validation\n- Run the focused nextest suite.\n\n## Assumptions and Defaults\n- Preserve the existing event contract.\n</proposed_plan>";
    let response = |content: &str| LLMResponse::new("test", content);

    let mut renderer = AnsiRenderer::stdout();
    let valid = process_llm_response(&response(valid_plan), &mut renderer, 0, true, false, false, false, None, None)
        .expect("valid recovery plan should process");
    assert!(matches!(
        valid,
        TurnProcessingResult::TextResponse { proposed_plan: Some(_), text, .. } if text.trim().is_empty()
    ));

    let mut renderer = AnsiRenderer::stdout();
    let alternate = process_llm_response(
        &response(&valid_plan.replace("proposed_plan", "plan")),
        &mut renderer,
        0,
        true,
        false,
        false,
        false,
        None,
        None,
    )
    .expect("alternate marker should be handled");
    assert!(matches!(
        alternate,
        TurnProcessingResult::TextResponse { proposed_plan: None, text, .. } if text.contains("<plan>")
    ));

    let mut renderer = AnsiRenderer::stdout();
    let duplicate = process_llm_response(
        &response(&format!("{valid_plan}\n{valid_plan}")),
        &mut renderer,
        0,
        true,
        false,
        false,
        false,
        None,
        None,
    )
    .expect("duplicate marker should be handled");
    assert!(matches!(
        duplicate,
        TurnProcessingResult::TextResponse { proposed_plan: None, text, .. } if text.matches("<proposed_plan>").count() == 2
    ));
}

#[test]
fn tool_free_planning_recovery_accepts_complete_unwrapped_plans_only() {
    let valid = "## Summary\nKeep recovery deterministic.\n\n## Implementation Steps\n1. Update src/lib.rs -> files: [src/lib.rs] -> verify: [cargo check --locked]\n\n## Test Cases and Validation\n- Run the focused nextest suite.\n\n## Assumptions and Defaults\n- Preserve the existing event contract.";
    assert!(validate_plan_content(valid).is_ready());
    for (content, accepted) in [(valid, true), ("The plan is ready; please approve it.", false)] {
        let response = LLMResponse::new("test", content);
        let result =
            process_llm_response(&response, &mut AnsiRenderer::stdout(), 0, true, false, false, false, None, None)
                .expect("public plan response");
        assert!(
            matches!(result, TurnProcessingResult::TextResponse { proposed_plan, .. } if proposed_plan.is_some() == accepted)
        );
    }
}

#[test]
fn tool_free_planning_recovery_rejects_native_tool_calls_even_with_a_plan() {
    let mut response = LLMResponse::new(
        "test",
        "<proposed_plan>\n## Summary\nDo the work.\n\n## Implementation Steps\n1. Update src/lib.rs -> files: [src/lib.rs] -> verify: [cargo check --locked]\n\n## Test Cases and Validation\n- Run nextest.\n\n## Assumptions and Defaults\n- Preserve the contract.\n</proposed_plan>",
    );
    response.tool_calls = Some(vec![ToolCall::function(
        "call-forbidden".to_string(),
        "code_search".to_string(),
        r#"{"query":"must not execute"}"#.to_string(),
    )]);

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, true, false, false, false, None, None)
        .expect("forbidden tool call should be handled as an empty recovery result");
    assert!(matches!(result, TurnProcessingResult::Empty { .. }));
}

#[test]
fn extract_interview_questions_strips_markdown_wrapping() {
    let text = "**How should we proceed?**";
    let questions = extract_interview_questions(text);
    assert_eq!(questions, vec!["How should we proceed?".to_string()]);
}

#[test]
fn extract_interview_questions_handles_bold_bullets() {
    let text = "- **Should we do X?**";
    let questions = extract_interview_questions(text);
    assert_eq!(questions, vec!["Should we do X?".to_string()]);
}

#[test]
fn build_interview_args_synthesizes_alignment_question_with_hints() {
    let text = r#"
I've analyzed the current system prompt architecture.
The plan is drafted. I need clarification on 3 key decisions before finalizing the implementation approach.
Key findings:
• Redundancy exists between prompt variants (tool guidance, bias for action warnings)
• Missing explicit guidance for common failure patterns (patch failures, circular deps)
• Harness integration is good but could be strengthened with more specific doc refs
Open questions for alignment:
"#;

    let args = build_interview_args_from_text(text).expect("expected synthesized interview args");
    let questions = args["questions"].as_array().expect("questions should be an array");
    assert_eq!(questions.len(), 1);

    let first = &questions[0];
    let question_text = first["question"].as_str().expect("question should be a string");
    assert!(question_text.contains("prioritize"));
    assert_eq!(first["focus_area"].as_str(), Some("system_prompt"));

    let hints = first["analysis_hints"].as_array().expect("analysis_hints should exist");
    assert!(!hints.is_empty(), "expected extracted hints");
}

#[test]
fn process_llm_response_turns_alignment_request_into_tool_call() {
    let response = LLMResponse {
        content: Some("Need clarification before finalizing.\nOpen questions for alignment:".to_string()),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 1, true, true, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, .. } => {
            let name = tool_calls.first().map(|call| call.tool_name()).expect("function name expected");
            assert_eq!(name, tools::REQUEST_USER_INPUT);
        }
        _ => panic!("Expected tool calls"),
    }
}

#[test]
fn process_llm_response_preserves_reasoning_details_for_tool_calls() {
    let response = LLMResponse {
        content: Some("".to_string()),
        tool_calls: Some(vec![ToolCall::function(
            "call_1".to_string(),
            "code_search".to_string(),
            r#"{"query":"x"}"#.to_string(),
        )]),
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::ToolCalls,
        reasoning: None,
        reasoning_details: Some(vec![r#"{"type":"reasoning_content","text":"trace"}"#.to_string()]),
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, false, true, true, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::ToolCalls { tool_calls, reasoning_details, .. } => {
            assert_eq!(reasoning_details, Some(vec![r#"{"type":"reasoning_content","text":"trace"}"#.to_string()]));
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].call_id(), "call_1");
            assert!(tool_calls[0].is_parallel_safe());
            assert!(!tool_calls[0].is_command_execution());
        }
        _ => panic!("Expected tool calls"),
    }
}

#[test]
fn process_llm_response_keeps_textual_tool_request_as_text_when_tool_calls_disabled() {
    let response = LLMResponse {
        content: Some("code_search({\"query\":\"Widget\",\"path\":\"src\"})".to_string()),
        tool_calls: None,
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::Stop,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, false, true, false, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text, "code_search({\"query\":\"Widget\",\"path\":\"src\"})");
        }
        _ => panic!("Expected text response when tool calls are disabled"),
    }
}

#[test]
fn process_llm_response_ignores_structured_tool_calls_when_tool_calls_disabled() {
    let response = LLMResponse {
        content: Some("Final synthesis only.".to_string()),
        tool_calls: Some(vec![ToolCall::function(
            "call_1".to_string(),
            "code_search".to_string(),
            r#"{"query":"Widget","path":"src","result_types":["path"]}"#.to_string(),
        )]),
        model: "test".to_string(),
        usage: None,
        finish_reason: FinishReason::ToolCalls,
        reasoning: None,
        reasoning_details: None,
        tool_references: Vec::new(),
        compaction: None,
        request_id: None,
        organization_id: None,
    };

    let mut renderer = AnsiRenderer::stdout();
    let result = process_llm_response(&response, &mut renderer, 0, false, false, true, false, None, None)
        .expect("processing should succeed");

    match result {
        TurnProcessingResult::TextResponse { text, .. } => {
            assert_eq!(text, "Final synthesis only.");
        }
        _ => panic!("Expected text response when tool calls are disabled"),
    }
}

#[test]
fn looks_like_structured_plan_detects_plain_text_plan_without_tags() {
    let text = r#"
 •   Summary
     Focus on the startup configuration/validation path.

     1. Measure baseline
       - Action: Add a high-resolution startup timer.
       - Files/symbols: src/main.rs:main.
       - Verify: `cargo run --release` prints phase durations.

     2. Trim binary size
       - Action: Audit Cargo.toml for large crates.
       - Files/symbols: Cargo.toml.
       - Verify: `cargo build --locked --release` size drops.

      Validation

      •   Build: cargo check --locked
      •   Tests: targeted tests in src/startup/.

      Assumptions

      •   Keep the existing startup entry point and output format.
        "#;
    assert!(looks_like_structured_plan(text));
}

#[test]
fn looks_like_structured_plan_rejects_plain_conversation() {
    let text = "Here is a quick update: I searched the codebase and found nothing relevant.";
    assert!(!looks_like_structured_plan(text));

    let generic_fallback = r#"Summary
The current context is sufficient.

Steps
1. Review the relevant code.
2. Make the required changes.

Validation
Run the usual checks.
"#;
    assert!(!looks_like_structured_plan(generic_fallback));
}
