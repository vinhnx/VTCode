use super::*;

#[test]
fn api_key_and_chatgpt_subscription_share_responses_item_history_builder() {
    let native_provider = native_openai_provider(models::openai::GPT_5_6);
    let chatgpt_provider = chatgpt_backend_provider(models::openai::GPT_5_6);
    let mut request = provider::LLMRequest {
        messages: vec![
            provider::Message::system("Follow the local policy.".to_owned()),
            provider::Message::user("Summarise the repository state.".to_owned()),
            provider::Message::assistant("There is one modified file.".to_owned()),
            provider::Message::user("Continue.".to_owned()),
        ]
        .into(),
        model: models::openai::GPT_5_6.to_string(),
        stream: true,
        tools: Some(Arc::new(vec![sample_tool()])),
        ..Default::default()
    };
    request.previous_response_id = Some("resp_previous_123".to_owned());
    request.response_store = Some(true);
    request.responses_include = Some(vec!["output_text.annotations".to_owned()]);

    let native_payload = native_provider
        .convert_to_openai_responses_format(&request)
        .expect("native payload should build");
    let chatgpt_payload = chatgpt_provider
        .convert_to_openai_responses_format(&request)
        .expect("chatgpt payload should build");

    for (payload, expected_role) in [(&native_payload, "developer"), (&chatgpt_payload, "user")] {
        let reminder = payload["input"].as_array().unwrap().last().unwrap();
        assert_eq!(reminder["role"], expected_role);
        assert!(
            reminder["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Follow the local policy.")
        );
    }
    assert_eq!(
        without_native_cache_projection(native_payload.clone()).get("input"),
        without_native_cache_projection(chatgpt_payload.clone()).get("input"),
        "API-key and ChatGPT paths must share the same Responses item/history builder"
    );
    assert!(
        count_input_breakpoints(&native_payload) > 0,
        "native API-key requests keep explicit cache breakpoints"
    );
    assert_eq!(
        count_input_breakpoints(&chatgpt_payload),
        0,
        "ChatGPT subscription backend rejects prompt_cache_breakpoint (400), so it must be omitted"
    );
    assert_eq!(native_payload.get("instructions"), chatgpt_payload.get("instructions"));
    assert_eq!(
        without_responses_backend_fields(without_native_cache_projection(native_payload.clone())),
        without_responses_backend_fields(without_native_cache_projection(chatgpt_payload.clone()))
    );

    assert_absent(&native_payload, "previous_response_id");
    assert_absent(&chatgpt_payload, "previous_response_id");
    assert_eq!(native_payload.get("store").and_then(Value::as_bool), Some(false));
    assert_eq!(
        chatgpt_payload.get("store").and_then(Value::as_bool),
        Some(false),
        "ChatGPT subscription requests must force store=false"
    );
}

#[test]
fn openai_and_chatgpt_replay_structured_history_on_continuation_turns() {
    let native_provider = native_openai_provider(models::openai::GPT_5_CODEX);
    let chatgpt_provider = chatgpt_backend_provider(models::openai::GPT_5_CODEX);
    let mut request = provider::LLMRequest {
        messages: vec![
            provider::Message::user("Inspect the workspace.".to_owned()),
            provider::Message::assistant_with_tools(
                "Running the requested check.".to_owned(),
                vec![provider::ToolCall::function(
                    "call_1".to_string(),
                    "exec_command".to_string(),
                    "{\"command\":\"cargo check\"}".to_string(),
                )],
            )
            .with_reasoning_details(Some(vec![json!({
                "type": "reasoning",
                "id": "rs_1",
                "encrypted_content": "opaque_reasoning",
                "summary": [{"type": "summary_text", "text": "checked command"}],
            })]))
            .with_phase(Some(provider::AssistantPhase::Commentary)),
            provider::Message::tool_response(
                "call_1".to_string(),
                "{\"output\":\"Finished `dev` profile\",\"exit_code\":0}".to_string(),
            ),
            provider::Message::assistant("The check completed successfully.".to_owned())
                .with_phase(Some(provider::AssistantPhase::FinalAnswer)),
            provider::Message::user("Continue with the next step.".to_owned()),
        ]
        .into(),
        model: models::openai::GPT_5_CODEX.to_string(),
        stream: true,
        ..Default::default()
    };
    request.previous_response_id = Some("resp_previous_123".to_owned());

    for payload in [
        native_provider
            .convert_to_openai_responses_format(&request)
            .expect("native payload should build"),
        chatgpt_provider
            .convert_to_openai_responses_format(&request)
            .expect("chatgpt payload should build"),
    ] {
        assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
        assert_absent(&payload, "previous_response_id");
        let input = get_input_array(&payload);
        assert!(
            input.iter().any(|item| item.get("role").and_then(Value::as_str) == Some("user")
                && item.to_string().contains("Inspect the workspace.")),
            "continuation replay must keep the first user turn, not only the suffix"
        );
        assert!(
            input.iter().any(|item| item.get("role").and_then(Value::as_str) == Some("user")
                && item.to_string().contains("Continue with the next step.")),
            "continuation replay must keep the latest user turn"
        );
        assert!(input.iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("reasoning")
                && item.get("encrypted_content").and_then(Value::as_str) == Some("opaque_reasoning")
        }));
        assert!(input.iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call")
                && item.get("call_id").and_then(Value::as_str) == Some("call_1")
        }));
        assert!(input.iter().any(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(Value::as_str) == Some("call_1")
        }));
    }
}

#[test]
fn chat_payload_omits_assistant_phase_metadata() {
    let provider = native_openai_provider(models::openai::DEFAULT_MODEL);
    let request = provider::LLMRequest {
        messages: vec![
            provider::Message::user("Start".to_owned()),
            provider::Message::assistant("Working".to_owned()).with_phase(Some(provider::AssistantPhase::Commentary)),
        ]
        .into(),
        model: models::openai::DEFAULT_MODEL.to_string(),
        ..Default::default()
    };
    let payload = provider.convert_to_openai_format(&request).expect("conversion should succeed");
    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .expect("messages should exist");
    assert!(messages[1].get("phase").is_none());
}

#[test]
fn chatgpt_backend_keeps_plain_assistant_history_structured_for_codex() {
    let payload = chatgpt_codex_payload(
        vec![
            provider::Message::user("What is this project?".to_owned()),
            provider::Message::assistant("VT Code is a Rust Cargo workspace.".to_owned())
                .with_phase(Some(provider::AssistantPhase::FinalAnswer)),
            provider::Message::user("Tell me more.".to_owned()),
        ],
        models::openai::GPT_5_CODEX,
    );
    let input = get_input_array(&payload);
    assert_eq!(input.len(), 3);
    assert_eq!(input_role_at(&payload, 0), Some("user"));
    assert_eq!(input_role_at(&payload, 1), Some("assistant"));
    assert_absent(&input[1], "phase");
    assert_eq!(input_role_at(&payload, 2), Some("user"));
    assert!(
        payload["instructions"]
            .as_str()
            .unwrap()
            .contains("You are Codex, based on GPT-5.")
    );
}

#[test]
fn chatgpt_backend_preserves_reasoning_detail_items_for_codex_follow_up() {
    let payload = chatgpt_codex_payload(
        vec![
            provider::Message::assistant("Hello. What would you like me to do?".to_owned())
                .with_reasoning_details(Some(vec![json!({
                    "type": "reasoning",
                    "id": "rs_1",
                    "summary": [{"type":"summary_text","text":"task prompt"}]
                })]))
                .with_phase(Some(provider::AssistantPhase::FinalAnswer)),
            provider::Message::user("tell me more".to_owned()),
        ],
        models::openai::GPT_5_CODEX,
    );
    let input = get_input_array(&payload);
    assert_eq!(input.len(), 3);
    assert_eq!(input_type_at(&payload, 0), Some("reasoning"));
    assert_eq!(input_role_at(&payload, 1), Some("assistant"));
    assert_absent(&input[1], "phase");
    assert_eq!(input_role_at(&payload, 2), Some("user"));
}

#[test]
fn chatgpt_backend_keeps_tool_turn_history_structured_for_codex() {
    let payload = chatgpt_codex_payload(
        vec![
            provider::Message::user("run cargo check".to_owned()),
            provider::Message::assistant_with_tools(
                String::new(),
                vec![provider::ToolCall::function(
                    "call_1".to_string(),
                    "exec_command".to_string(),
                    "{\"command\":\"cargo check\"}".to_string(),
                )],
            ),
            provider::Message::tool_response(
                "call_1".to_string(),
                "{\"output\":\"Finished `dev` profile\",\"exit_code\":0}".to_string(),
            ),
            provider::Message::assistant("cargo check completed successfully.".to_owned())
                .with_phase(Some(provider::AssistantPhase::FinalAnswer)),
            provider::Message::user("who are you".to_owned()),
        ],
        models::openai::GPT_5_CODEX,
    );
    let input = get_input_array(&payload);
    assert_eq!(input.len(), 5);
    assert_eq!(input_role_at(&payload, 0), Some("user"));
    assert_eq!(input_type_at(&payload, 1), Some("function_call"));
    assert_eq!(input_call_id_at(&payload, 1), Some("call_1"));
    assert_eq!(input_type_at(&payload, 2), Some("function_call_output"));
    assert_eq!(input_call_id_at(&payload, 2), Some("call_1"));
    assert_eq!(input_role_at(&payload, 3), Some("assistant"));
    assert_eq!(input_role_at(&payload, 4), Some("user"));
    assert_absent(&input[3], "phase");
}

#[test]
fn chatgpt_backend_omits_assistant_phase_for_codex_models() {
    for model in [models::openai::GPT_5_CODEX, models::openai::GPT_5_6_SOL] {
        let payload = chatgpt_codex_payload(
            vec![
                provider::Message::user("Run the next check.".to_owned()),
                provider::Message::assistant("Checking.".to_owned())
                    .with_phase(Some(provider::AssistantPhase::Commentary)),
                provider::Message::assistant("Done.".to_owned())
                    .with_phase(Some(provider::AssistantPhase::FinalAnswer)),
                provider::Message::user("Continue.".to_owned()),
            ],
            model,
        );
        let input = get_input_array(&payload);
        assert_eq!(input.len(), 4);
        assert_absent(&input[1], "phase");
        assert_absent(&input[2], "phase");
    }
}

#[test]
fn chatgpt_backend_preserves_structured_tool_turns_with_paired_function_calls() {
    for model in [models::openai::GPT_5_CODEX, models::openai::GPT_5_6_SOL] {
        let payload = chatgpt_codex_payload(
            vec![
                provider::Message::user("Investigate the failing check.".to_owned()),
                provider::Message::assistant_with_tools(
                    "Checking the first command output.".to_owned(),
                    vec![provider::ToolCall::function(
                        "call_1".to_string(),
                        "exec_command".to_string(),
                        "{\"command\":\"cargo check -p vtcode-core\"}".to_string(),
                    )],
                )
                .with_phase(Some(provider::AssistantPhase::Commentary)),
                provider::Message::tool_response(
                    "call_1".to_string(),
                    "{\"output\":\"warning: example\",\"exit_code\":0}".to_string(),
                ),
                provider::Message::assistant("Need one more inspection step.".to_owned())
                    .with_phase(Some(provider::AssistantPhase::Commentary)),
                provider::Message::user("Continue.".to_owned()),
            ],
            model,
        );
        let input = get_input_array(&payload);
        assert_eq!(input.len(), 6);
        assert_eq!(input_role_at(&payload, 0), Some("user"));
        assert_eq!(input_role_at(&payload, 1), Some("assistant"));
        assert_absent(&input[1], "phase");
        assert_eq!(input_type_at(&payload, 2), Some("function_call"));
        assert_eq!(input_call_id_at(&payload, 2), Some("call_1"));
        assert_eq!(input_type_at(&payload, 3), Some("function_call_output"));
        assert_eq!(input_call_id_at(&payload, 3), Some("call_1"));
        assert_eq!(input_role_at(&payload, 4), Some("assistant"));
        assert_absent(&input[4], "phase");
        assert_eq!(input_role_at(&payload, 5), Some("user"));
    }
}

#[test]
fn chatgpt_backend_replays_prior_direct_tool_turns() {
    let payload = chatgpt_codex_payload(
        vec![
            provider::Message::user("run cargo fmt".to_owned()),
            provider::Message::assistant_with_tools(
                String::new(),
                vec![provider::ToolCall::function(
                    "direct_exec_command_1".to_string(),
                    "exec_command".to_string(),
                    "{\"command\":\"cargo fmt\"}".to_string(),
                )],
            ),
            provider::Message::tool_response(
                "direct_exec_command_1".to_string(),
                "{\"output\":\"\",\"exit_code\":0,\"backend\":\"pipe\"}".to_string(),
            ),
            provider::Message::assistant("cargo fmt completed successfully.".to_owned())
                .with_phase(Some(provider::AssistantPhase::FinalAnswer)),
            provider::Message::user("continue".to_owned()),
        ],
        models::openai::GPT_5_CODEX,
    );
    let input = get_input_array(&payload);
    assert_eq!(input.len(), 5);
    assert_eq!(input_type_at(&payload, 1), Some("function_call"));
    assert_eq!(input_type_at(&payload, 2), Some("function_call_output"));
    assert_eq!(input_call_id_at(&payload, 1), Some("direct_exec_command_1"));
    assert_eq!(input_call_id_at(&payload, 2), Some("direct_exec_command_1"));
    assert!(input.iter().all(|item| {
        let t = item.get("type").and_then(Value::as_str);
        t != Some("tool_call") && t != Some("tool_result")
    }));
}

#[test]
fn chatgpt_backend_synthesizes_missing_function_call_outputs_for_orphan_calls() {
    let payload = chatgpt_codex_payload(
        vec![
            provider::Message::user("Run commands".to_owned()),
            provider::Message::assistant_with_tools(
                String::new(),
                vec![provider::ToolCall::function(
                    "call_orphan".to_string(),
                    "exec_command".to_string(),
                    "{\"command\":\"echo orphan\"}".to_string(),
                )],
            ),
            provider::Message::assistant_with_tools(
                String::new(),
                vec![provider::ToolCall::function(
                    "call_paired".to_string(),
                    "exec_command".to_string(),
                    "{\"command\":\"echo paired\"}".to_string(),
                )],
            ),
            provider::Message::tool_response(
                "call_paired".to_string(),
                "{\"output\":\"paired\",\"exit_code\":0}".to_string(),
            ),
            provider::Message::user("continue".to_owned()),
        ],
        models::openai::GPT_5_CODEX,
    );
    let input = get_input_array(&payload);
    assert!(input.iter().any(|i| {
        i.get("type").and_then(Value::as_str) == Some("function_call")
            && i.get("call_id").and_then(Value::as_str) == Some("call_orphan")
    }));
    assert!(input.iter().any(|i| {
        i.get("type").and_then(Value::as_str) == Some("function_call_output")
            && i.get("call_id").and_then(Value::as_str) == Some("call_orphan")
            && i.get("output").and_then(Value::as_str) == Some("aborted")
    }));
    assert!(input.iter().any(|i| {
        i.get("type").and_then(Value::as_str) == Some("function_call_output")
            && i.get("call_id").and_then(Value::as_str) == Some("call_paired")
    }));
}

#[test]
fn responses_payload_phase_behavior() {
    // Native: includes phase for assistant, omits for user/tool
    let native = native_openai_provider(models::openai::GPT_5_6_SOL);
    let request = provider::LLMRequest {
        messages: vec![
            provider::Message::user("Start".to_owned()).with_phase(Some(provider::AssistantPhase::Commentary)),
            provider::Message::assistant("Checking.".to_owned()).with_phase(Some(provider::AssistantPhase::Commentary)),
            provider::Message::assistant_with_tools(
                "Looking up.".to_owned(),
                vec![provider::ToolCall::function(
                    "call_1".to_string(),
                    "search_workspace".to_string(),
                    r#"{"query":"x"}"#.to_string(),
                )],
            )
            .with_phase(Some(provider::AssistantPhase::Commentary)),
            provider::Message::tool_response("call_1".to_string(), "{\"ok\":true}".to_string())
                .with_phase(Some(provider::AssistantPhase::FinalAnswer)),
        ]
        .into(),
        model: models::openai::GPT_5_6_SOL.to_string(),
        ..Default::default()
    };
    let payload = native.convert_to_openai_responses_format(&request).expect("should succeed");
    let input = get_input_array(&payload);
    assert!(input[0].get("phase").is_none(), "user omits phase");
    assert_eq!(input[1].get("phase").and_then(Value::as_str), Some("commentary"));
    assert_eq!(input[2].get("phase").and_then(Value::as_str), Some("commentary"));
    assert!(input[3].get("phase").is_none(), "tool response omits phase");

    // Non-native: omits phase entirely
    let non_native = compatible_endpoint_provider(models::openai::GPT_5_6_SOL, "https://example.local/v1");
    let request2 = provider::LLMRequest {
        messages: vec![
            provider::Message::user("Start".to_owned()),
            provider::Message::assistant("Checking.".to_owned()).with_phase(Some(provider::AssistantPhase::Commentary)),
        ]
        .into(),
        model: models::openai::GPT_5_6_SOL.to_string(),
        ..Default::default()
    };
    let payload2 = non_native
        .convert_to_openai_responses_format(&request2)
        .expect("should succeed");
    assert!(get_input_array(&payload2)[1].get("phase").is_none());
}
