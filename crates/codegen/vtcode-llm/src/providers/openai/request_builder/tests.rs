use super::history::augment_openai_instructions;
use super::{ChatRequestContext, ResponsesRequestContext, build_chat_request, build_responses_request};
use crate::provider;
use serde_json::{Value, json};
use vtcode_config::constants::models;

fn base_context<'a>(default_responses_include: Option<&'a [String]>) -> ResponsesRequestContext<'a> {
    ResponsesRequestContext {
        supports_tools: false,
        supports_allowed_tools: false,
        supports_parallel_tool_config: false,
        supports_temperature: true,
        supports_reasoning_effort: true,
        supported_reasoning_efforts: &["low", "medium", "high", "xhigh", "max"],
        supports_reasoning: true,
        is_responses_api_model: true,
        include_max_output_tokens: true,
        include_output_types: true,
        include_sampling_parameters: true,
        force_response_store_false: false,
        include_assistant_phase: true,
        prompt_cache_key: None,
        include_prompt_cache_retention: false,
        prompt_cache_retention: None,
        include_explicit_cache_breakpoints: true,
        default_service_tier: None,
        default_response_store: None,
        default_responses_include,
        include_encrypted_reasoning: false,
        hosted_shell: None,
        include_structured_history_in_input: true,
        preserve_structured_history_on_replay: false,
        preserve_assistant_phase_on_replay: false,
        reasoning_context: None,
        safety_identifier: None,
    }
}

fn request() -> provider::LLMRequest {
    provider::LLMRequest {
        messages: vec![provider::Message::user("Hello".to_owned())].into(),
        model: models::openai::GPT_5.to_string(),
        stream: true,
        ..Default::default()
    }
}

fn chat_context() -> ChatRequestContext<'static> {
    ChatRequestContext {
        model: "custom-chat-model",
        is_native_openai: true,
        supports_tools: false,
        supports_parallel_tool_config: false,
        supports_temperature: true,
        prompt_cache_key: None,
        default_service_tier: None,
    }
}

#[test]
fn chat_request_carries_top_p_and_penalties_with_compact_wire_form() {
    let mut request = request();
    request.model = "custom-chat-model".to_string();
    request.temperature = Some(0.7);
    request.top_p = Some(0.9);
    request.presence_penalty = Some(0.1);
    request.frequency_penalty = Some(-0.5);

    let payload = build_chat_request(&request, &chat_context()).expect("chat request should build");

    assert_eq!(payload.get("temperature"), Some(&json!(0.7)));
    assert_eq!(
        payload.get("top_p").expect("top_p should serialize").to_string(),
        "0.9",
        "wire form must be compact"
    );
    assert_eq!(payload.get("presence_penalty").and_then(Value::as_f64), Some(0.1));
    assert_eq!(payload.get("frequency_penalty").and_then(Value::as_f64), Some(-0.5));
}

#[test]
fn openai_request_builder_serialises_rig_typed_state_fields() {
    let mut request = request();
    request.previous_response_id = Some("resp_previous".to_owned());
    request.response_store = Some(true);
    let mut ctx = base_context(None);
    ctx.force_response_store_false = true;

    let payload = build_responses_request(&request, &ctx).expect("responses request should build");

    assert!(payload.get("previous_response_id").is_none());
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
}

#[test]
fn responses_output_token_limit_normalizes_probe_and_boundary_allowances() {
    let mut request = request();
    request.model = models::openai::GPT_6_LUNA.to_string();
    let mut ctx = base_context(None);
    for (requested, expected) in [(1, 16), (8, 16), (15, 16), (16, 16), (17, 17), (512, 512)] {
        request.max_tokens = Some(requested);
        let payload = build_responses_request(&request, &ctx).expect("responses request");
        assert_eq!(payload["max_output_tokens"], expected, "requested {requested}");
    }
    request.max_tokens = None;
    assert!(
        build_responses_request(&request, &ctx)
            .unwrap()
            .get("max_output_tokens")
            .is_none()
    );
    request.max_tokens = Some(8);
    ctx.include_max_output_tokens = false;
    assert!(
        build_responses_request(&request, &ctx)
            .unwrap()
            .get("max_output_tokens")
            .is_none()
    );
}

#[test]
fn openai_request_builder_preserves_custom_include_strings_around_typed_include() {
    let default_include = vec!["output_text.annotations".to_owned()];
    let mut ctx = base_context(Some(default_include.as_slice()));
    ctx.include_encrypted_reasoning = true;

    let payload = build_responses_request(&request(), &ctx).expect("responses request should build");

    assert_eq!(
        payload.get("include").and_then(Value::as_array),
        Some(&vec![json!("output_text.annotations"), json!("reasoning.encrypted_content"),])
    );
}

#[test]
fn prompt_cache_fields_serialize_through_rig_typed_parameters() {
    let mut ctx = base_context(None);
    ctx.prompt_cache_key = Some("vtcode:openai:session-123");
    ctx.include_prompt_cache_retention = true;
    ctx.prompt_cache_retention = Some("24h");

    let payload = build_responses_request(&request(), &ctx).expect("responses request should build");

    assert_eq!(payload.get("prompt_cache_key").and_then(Value::as_str), Some("vtcode:openai:session-123"));
    assert_eq!(payload.get("prompt_cache_retention").and_then(Value::as_str), Some("24h"));
}

#[test]
fn chatgpt_request_construction_uses_shared_normal_responses_builder() {
    let mut request = request();
    request.stream = false;
    request.max_tokens = Some(512);
    request.temperature = Some(0.7);
    request.top_p = Some(0.8);
    request.parallel_tool_calls = Some(true);
    request.previous_response_id = Some("resp_previous".to_owned());
    request.responses_include = Some(vec!["output_text.annotations".to_owned()]);

    let mut ctx = base_context(None);
    ctx.include_max_output_tokens = false;
    ctx.include_output_types = false;
    ctx.include_sampling_parameters = false;
    ctx.force_response_store_false = true;
    ctx.include_encrypted_reasoning = true;

    let payload = build_responses_request(&request, &ctx).expect("chatgpt responses request should build");

    assert_eq!(payload.get("stream").and_then(Value::as_bool), Some(false));
    assert_eq!(payload.get("store").and_then(Value::as_bool), Some(false));
    assert!(payload.get("max_output_tokens").is_none());
    assert!(payload.get("temperature").is_none());
    assert!(payload.get("top_p").is_none());
    assert!(payload.get("parallel_tool_calls").is_none());
    assert!(payload.get("previous_response_id").is_none());
    assert_eq!(
        payload.get("include").and_then(Value::as_array),
        Some(&vec![json!("output_text.annotations"), json!("reasoning.encrypted_content"),])
    );
}

#[test]
fn chatgpt_request_construction_preserves_requested_stream_mode() {
    for requested_stream in [false, true] {
        let mut request = request();
        request.stream = requested_stream;

        let payload =
            build_responses_request(&request, &base_context(None)).expect("chatgpt responses request should build");

        assert_eq!(payload.get("stream").and_then(Value::as_bool), Some(requested_stream));
    }
}

fn gpt6_astra_request() -> provider::LLMRequest {
    let mut request = request();
    request.model = models::openai::GPT_6_ASTRA.to_string();
    request
}

#[test]
fn gpt6_astra_defaults_to_high_reasoning_effort() {
    let payload = build_responses_request(&gpt6_astra_request(), &base_context(None))
        .expect("astra responses request should build");

    assert_eq!(payload.pointer("/reasoning/effort").and_then(Value::as_str), Some("high"));
}

#[test]
fn gpt6_astra_omits_sampling_parameters() {
    let mut request = gpt6_astra_request();
    request.temperature = Some(0.7);
    request.top_p = Some(0.9);
    request.presence_penalty = Some(0.1);
    request.frequency_penalty = Some(-0.5);

    let payload = build_responses_request(&request, &base_context(None)).expect("astra responses request should build");

    assert!(payload.get("sampling_parameters").is_none());
    assert!(payload.get("temperature").is_none());
    assert!(payload.get("top_p").is_none());
}

#[test]
fn gpt6_astra_uses_prompt_cache_options_ttl() {
    let mut ctx = base_context(None);
    ctx.include_prompt_cache_retention = true;
    ctx.prompt_cache_retention = Some("24h");

    let payload = build_responses_request(&gpt6_astra_request(), &ctx).expect("astra responses request should build");

    assert_eq!(payload.pointer("/prompt_cache_options/ttl").and_then(Value::as_str), Some("30m"));
    assert!(payload.get("prompt_cache_retention").is_none());
}

#[test]
fn gpt6_astra_strips_logprobs_from_include() {
    let mut request = gpt6_astra_request();
    request.responses_include = Some(vec![
        "message.output_text.logprobs".to_owned(),
        "reasoning.encrypted_content".to_owned(),
    ]);

    let payload = build_responses_request(&request, &base_context(None)).expect("astra responses request should build");

    assert_eq!(payload.get("include").and_then(Value::as_array), Some(&vec![json!("reasoning.encrypted_content")]));
}

#[test]
fn gpt6_astra_receives_dedicated_addendum() {
    let instructions = augment_openai_instructions(models::openai::GPT_6_ASTRA, "Be helpful.".to_string());

    assert!(instructions.contains("GPT-6 Astra"));
    assert!(!instructions.contains("GPT-5.6 model"));
}

fn gpt6_sol_request() -> provider::LLMRequest {
    let mut request = request();
    request.model = models::openai::GPT_6_SOL.to_string();
    request
}

fn gpt6_luna_request() -> provider::LLMRequest {
    let mut request = request();
    request.model = models::openai::GPT_6_LUNA.to_string();
    request
}

#[test]
fn gpt6_sol_defaults_to_medium_reasoning_effort() {
    let payload =
        build_responses_request(&gpt6_sol_request(), &base_context(None)).expect("sol responses request should build");

    assert_eq!(payload.pointer("/reasoning/effort").and_then(Value::as_str), Some("medium"));
}

#[test]
fn gpt6_sol_explicit_reasoning_effort_overrides_model_default() {
    let mut request = gpt6_sol_request();
    request.reasoning_effort = Some(vtcode_config::types::ReasoningEffortLevel::High);

    let payload = build_responses_request(&request, &base_context(None)).expect("sol responses request should build");

    assert_eq!(payload.pointer("/reasoning/effort").and_then(Value::as_str), Some("high"));
}

#[test]
fn gpt6_luna_defaults_to_medium_reasoning_effort() {
    let payload = build_responses_request(&gpt6_luna_request(), &base_context(None))
        .expect("luna responses request should build");

    assert_eq!(payload.pointer("/reasoning/effort").and_then(Value::as_str), Some("medium"));
}

#[test]
fn gpt6_sol_luna_omit_sampling_and_use_cache_ttl() {
    for mut request in [gpt6_sol_request(), gpt6_luna_request()] {
        request.temperature = Some(0.7);
        request.top_p = Some(0.9);
        let payload =
            build_responses_request(&request, &base_context(None)).expect("gpt-6 responses request should build");
        assert!(payload.get("sampling_parameters").is_none());
        assert!(payload.get("temperature").is_none());
    }

    for request in [gpt6_sol_request(), gpt6_luna_request()] {
        let mut ctx = base_context(None);
        ctx.include_prompt_cache_retention = true;
        ctx.prompt_cache_retention = Some("24h");
        let payload = build_responses_request(&request, &ctx).expect("gpt-6 responses request should build");
        assert_eq!(payload.pointer("/prompt_cache_options/ttl").and_then(Value::as_str), Some("30m"));
        assert!(payload.get("prompt_cache_retention").is_none());
    }
}

#[test]
fn gpt6_sol_luna_receive_gpt6_addendum() {
    // Sol/Luna reuse the shared GPT-6 contract addendum (currently
    // Astra-named). Assert the exact shared text so a future
    // model-specific addendum split updates this test deliberately.
    for model in [models::openai::GPT_6_SOL, models::openai::GPT_6_LUNA] {
        let instructions = augment_openai_instructions(model, "Be helpful.".to_string());
        assert!(instructions.contains("GPT-6 Astra"));
        assert!(!instructions.contains("GPT-5.6 model"));
    }
}

fn cache_ctx() -> ResponsesRequestContext<'static> {
    let mut ctx = base_context(None);
    ctx.include_prompt_cache_retention = true;
    ctx
}

fn gpt56_request(messages: Vec<provider::Message>) -> provider::LLMRequest {
    provider::LLMRequest {
        messages: messages.into(),
        model: models::openai::GPT_5_6_LUNA.to_string(),
        stream: true,
        ..Default::default()
    }
}

fn count_explicit_breakpoints(payload: &Value) -> usize {
    payload
        .get("input")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("content").and_then(Value::as_array))
                .flat_map(|blocks| blocks.iter())
                .filter(|block| block.get("prompt_cache_breakpoint").is_some())
                .count()
        })
        .unwrap_or(0)
}

fn breakpoint_modes(payload: &Value) -> Vec<String> {
    payload
        .get("input")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("content").and_then(Value::as_array))
                .flat_map(|blocks| blocks.iter())
                .filter_map(|block| {
                    block
                        .pointer("/prompt_cache_breakpoint/mode")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn gpt56_marks_stable_prefix_not_trailing_message() {
    let request = gpt56_request(vec![
        provider::Message::user("stable context".to_string()),
        provider::Message::user("new question".to_string()),
    ]);

    let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");
    let input = payload.get("input").and_then(Value::as_array).expect("input");

    assert_eq!(input.len(), 2);
    let first_blocks = input[0].get("content").and_then(Value::as_array).expect("blocks");
    assert_eq!(
        first_blocks
            .last()
            .and_then(|b| b.pointer("/prompt_cache_breakpoint/mode"))
            .and_then(Value::as_str),
        Some("explicit"),
        "stable prefix boundary must carry the explicit breakpoint"
    );
    let last_blocks = input[1].get("content").and_then(Value::as_array).expect("blocks");
    assert!(
        last_blocks.iter().all(|b| b.get("prompt_cache_breakpoint").is_none()),
        "trailing varying message stays implicit-only so the stable prefix can partial-match"
    );
}

#[test]
fn non_native_endpoints_omit_explicit_cache_breakpoints() {
    // Merge Gateway and other Responses-compatible endpoints reject
    // `prompt_cache_breakpoint` with 400 `invalid_parameter`; the provider
    // only sets the flag for first-party backends.
    let request = gpt56_request(vec![
        provider::Message::user("stable context".to_string()),
        provider::Message::user("new question".to_string()),
    ]);
    let mut ctx = cache_ctx();
    ctx.include_explicit_cache_breakpoints = false;

    let payload = build_responses_request(&request, &ctx).expect("request should build");
    assert_eq!(
        count_explicit_breakpoints(&payload),
        0,
        "compat endpoints must not receive prompt_cache_breakpoint markers"
    );
}

#[test]
fn gpt61_sol_is_explicit_cache_breakpoint_model() {
    let mut request = gpt56_request(vec![provider::Message::user("Cache this prompt.".to_string())]);
    request.model = models::openai::GPT_6_1_SOL.to_string();
    let mut ctx = cache_ctx();
    let payload = build_responses_request(&request, &ctx).expect("native request builds");
    assert_eq!(count_explicit_breakpoints(&payload), 1);

    ctx.include_explicit_cache_breakpoints = false;
    let payload = build_responses_request(&request, &ctx).expect("compatible request builds");
    assert_eq!(count_explicit_breakpoints(&payload), 0);
}

#[test]
fn gpt56_grown_history_keeps_breakpoint_byte_stable() {
    let first = gpt56_request(vec![provider::Message::user("stable context".to_string())]);
    let second = gpt56_request(vec![
        provider::Message::user("stable context".to_string()),
        provider::Message::user("follow-up".to_string()),
    ]);

    let a = build_responses_request(&first, &cache_ctx()).expect("first should build");
    let b = build_responses_request(&second, &cache_ctx()).expect("second should build");

    let a_input = a.get("input").and_then(Value::as_array).expect("input");
    let b_input = b.get("input").and_then(Value::as_array).expect("input");
    assert_eq!(&b_input[..a_input.len()], &a_input[..], "breakpoint must not rewrite the stable prefix");
    assert!(count_explicit_breakpoints(&b) >= 1);
}

#[test]
fn gpt55_and_older_get_no_explicit_breakpoint() {
    let mut request = gpt56_request(vec![
        provider::Message::user("stable context".to_string()),
        provider::Message::user("new question".to_string()),
    ]);
    request.model = models::openai::GPT_5.to_string();

    let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

    assert_eq!(count_explicit_breakpoints(&payload), 0, "older models reject the breakpoint field");
    assert!(payload.get("prompt_cache_options").is_none());
}

#[test]
fn gpt56_reserves_implicit_write_slot() {
    let messages = (0..6).map(|i| provider::Message::user(format!("question {i}"))).collect();
    let request = gpt56_request(messages);

    let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

    assert_eq!(count_explicit_breakpoints(&payload), 3, "implicit mode leaves three explicit writes");
    assert!(breakpoint_modes(&payload).iter().all(|mode| mode == "explicit"));
}

#[test]
fn developer_anchor_survives_compaction_and_uses_one_explicit_write_slot() {
    let mut first = gpt56_request((0..7).map(|i| provider::Message::user(format!("history {i}"))).collect());
    first.system_prompt = Some(std::sync::Arc::from(system_with_dynamics()));
    let before = build_responses_request(&first, &cache_ctx()).unwrap();
    let mut compacted = first.clone();
    compacted.messages = vec![
        provider::Message::system("New compacted summary".to_string()),
        provider::Message::user("Continue".to_string()),
    ]
    .into();
    let after = build_responses_request(&compacted, &cache_ctx()).unwrap();
    assert_eq!(before.pointer("/input/0"), after.pointer("/input/0"));
    assert_eq!(before.pointer("/input/0/role"), Some(&json!("developer")));
    assert_eq!(before.pointer("/input/0/content/0/prompt_cache_breakpoint/mode"), Some(&json!("explicit")));
    assert!(before.get("instructions").is_none(), "instructions must appear only once");
    assert_eq!(count_explicit_breakpoints(&before), 3, "anchor plus two history writes");
    assert!(count_explicit_breakpoints(&after) <= 3);
    assert_eq!(first.messages.len(), 7, "wire projection leaves canonical history untouched");
    let mut compatible = cache_ctx();
    compatible.include_explicit_cache_breakpoints = false;
    let legacy = build_responses_request(&first, &compatible).unwrap();
    assert!(legacy.get("instructions").is_some());
    assert_eq!(legacy.pointer("/input/0/role"), Some(&json!("user")));
}

#[test]
fn gpt56_single_turn_still_writes_breakpoint() {
    let request = gpt56_request(vec![provider::Message::user("only question".to_string())]);

    let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

    assert_eq!(count_explicit_breakpoints(&payload), 1, "first request must write the prefix");
}

fn system_with_dynamics() -> String {
    "stable instructions\n\n[Harness Limits]\n- max_tool_calls_per_turn: 5\n\n[Runtime Tool Catalog]\n- epoch: 2"
        .to_string()
}

#[test]
fn native_runtime_reminders_preserve_implicit_tool_history_cache_boundaries() {
    for model in [models::openai::GPT_5_6_LUNA, models::openai::GPT_6_LUNA] {
        let mut request = gpt56_request(vec![provider::Message::user("Inspect the workspace".into())]);
        request.model = model.into();
        request.system_prompt = Some(system_with_dynamics().into());
        request.prompt_cache_key = Some("stable-session".into());
        request.tools = Some(std::sync::Arc::new(vec![provider::ToolDefinition::function(
            "inspect".into(),
            "Inspect workspace evidence".into(),
            json!({"type": "object", "properties": {}}),
        )]));
        let mut context = cache_ctx();
        context.prompt_cache_key = Some("stable-session");
        context.supports_tools = true;
        let original = request.messages.clone();
        let mut previous = build_responses_request(&request, &context).unwrap();
        assert_eq!(previous.pointer("/input/1/role"), Some(&json!("user")));
        assert_eq!(previous["prompt_cache_key"], "stable-session");
        assert_eq!(previous["tools"][0]["name"], "inspect");

        for step in 1..=3 {
            let mut messages = request.messages.as_ref().clone();
            let call_id = format!("call_{step}");
            messages.push(provider::Message::assistant_with_tools(
                String::new(),
                vec![provider::ToolCall::function(
                    call_id.clone(),
                    "inspect".into(),
                    "{}".into(),
                )],
            ));
            messages.push(provider::Message::tool_response(call_id.clone(), format!("Evidence for step {step}")));
            request.messages = messages.into();
            request.system_prompt = Some(format!("{}\n- retries: {step}", system_with_dynamics()).into());
            let current = build_responses_request(&request, &context).unwrap();
            let before = previous["input"].as_array().unwrap();
            let after = current["input"].as_array().unwrap();

            // Independent oracle from the documented native implicit boundaries:
            // user messages and the last tool result are eligible; developer
            // messages after the initial developer group are not. The previous
            // write must end before the replaced request-only reminder.
            let prior_boundary = before
                .iter()
                .rposition(|item| item["role"] == "user" || item["type"] == "function_call_output")
                .unwrap();
            assert_eq!(prior_boundary, before.len() - 2);
            assert_eq!(&before[..=prior_boundary], &after[..=prior_boundary], "reusable wire prefix, {model}");
            assert_eq!(after[after.len() - 2]["type"], "function_call_output");
            assert_eq!(after[after.len() - 2]["call_id"], call_id);
            let reminder = after.last().unwrap();
            assert_eq!(reminder["role"], "developer");
            assert!(reminder.pointer("/content/0/prompt_cache_breakpoint").is_none());
            assert_ne!(before.last(), after.last(), "exercise a changing runtime suffix");
            assert_eq!(previous["prompt_cache_key"], current["prompt_cache_key"]);
            assert_eq!(previous["tools"], current["tools"]);
            assert!(count_explicit_breakpoints(&current) <= 3);
            previous = current;
        }
        assert_eq!(original.len(), 1, "canonical history snapshots must remain unchanged");
        assert_eq!(request.messages.len(), 7, "reminders must remain request-only");
    }
}

#[test]
fn runtime_reminder_role_preserves_legacy_backend_and_model_shapes() {
    let cases = [
        (models::openai::GPT_5_6_LUNA, false, true),
        (models::openai::GPT_6_LUNA, false, true),
        (models::openai::GPT_6_LUNA, true, false),
        (models::openai::GPT_5, true, true),
    ];
    for (model, explicit_breakpoints, responses_model) in cases {
        let mut request = gpt56_request(vec![provider::Message::user("Continue".into())]);
        request.model = model.into();
        request.system_prompt = Some(system_with_dynamics().into());
        let mut context = cache_ctx();
        context.include_explicit_cache_breakpoints = explicit_breakpoints;
        context.is_responses_api_model = responses_model;
        let payload = build_responses_request(&request, &context).unwrap();
        let reminder = payload["input"].as_array().unwrap().last().unwrap();
        assert_eq!(reminder["role"], "user", "legacy reminder shape, {model}");
        assert!(
            reminder
                .pointer("/content/0/text")
                .unwrap()
                .as_str()
                .unwrap()
                .contains("[Harness Limits]")
        );
        assert_eq!(count_explicit_breakpoints(&payload), 0);
    }
}

#[test]
fn responses_splits_dynamic_suffix_into_trailing_reminder() {
    use std::sync::Arc;

    let mut request = gpt56_request(vec![
        provider::Message::system("Previous conversation summary:\n- did stuff".to_string()),
        provider::Message::user("continue".to_string()),
    ]);
    request.system_prompt = Some(Arc::from(system_with_dynamics()));

    let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

    let instructions = payload
        .pointer("/input/0/content/0/text")
        .and_then(Value::as_str)
        .expect("developer instructions");
    assert!(instructions.contains("stable instructions"), "stable prefix stays cached");
    assert!(!instructions.contains("[Harness Limits]"), "volatile sections leave instructions");
    assert!(!instructions.contains("Previous conversation summary"), "history system leaves instructions");

    let input = payload.get("input").and_then(Value::as_array).expect("input");
    let reminder = input.last().expect("trailing reminder");
    assert_eq!(reminder.get("role").and_then(Value::as_str), Some("developer"));
    let reminder_text = reminder
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .expect("reminder text");
    assert!(reminder_text.contains("[Harness Limits]"), "volatile sections ride the tail");
    assert!(reminder_text.contains("Previous conversation summary"), "history system rides the tail");
    assert!(reminder.pointer("/content/0/prompt_cache_breakpoint").is_none(), "varying tail stays unmarked");
}

#[test]
fn responses_static_prompts_anchor_developer_instructions() {
    use std::sync::Arc;

    let mut request = gpt56_request(vec![provider::Message::user("hello".to_string())]);
    request.system_prompt = Some(Arc::from("stable instructions"));

    let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

    let instructions = payload
        .pointer("/input/0/content/0/text")
        .and_then(Value::as_str)
        .expect("developer instructions");
    assert!(instructions.contains("stable instructions"));
    let input = payload.get("input").and_then(Value::as_array).expect("input");
    assert_eq!(input.len(), 2, "developer instructions and user input without a reminder");
    assert!(payload.get("instructions").is_none());
    assert_eq!(input[0]["role"], json!("developer"));
}

#[test]
fn responses_grown_history_keeps_stable_instructions() {
    use std::sync::Arc;

    let first = {
        let mut request = gpt56_request(vec![provider::Message::user("one".to_string())]);
        request.system_prompt = Some(Arc::from(system_with_dynamics()));
        request
    };
    let second = {
        let mut request = gpt56_request(vec![
            provider::Message::user("one".to_string()),
            provider::Message::user("two".to_string()),
        ]);
        request.system_prompt = Some(Arc::from(system_with_dynamics()));
        request
    };

    let a = build_responses_request(&first, &cache_ctx()).expect("first should build");
    let b = build_responses_request(&second, &cache_ctx()).expect("second should build");

    assert_eq!(a.pointer("/input/0"), b.pointer("/input/0"), "developer instructions repeat verbatim");
    let a_input = a.get("input").and_then(Value::as_array).expect("input");
    let b_input = b.get("input").and_then(Value::as_array).expect("input");
    assert_eq!(a_input[0], b_input[0], "stable history item keeps its bytes");
}

#[test]
fn responses_nonstructured_folds_move_to_reminder() {
    let mut ctx = cache_ctx();
    ctx.include_structured_history_in_input = false;
    let request = gpt56_request(vec![
        provider::Message::user("run it".to_string()),
        provider::Message::assistant("did it".to_string()),
        provider::Message::user("again".to_string()),
    ]);

    let payload = build_responses_request(&request, &ctx).expect("request should build");

    let instructions = payload.get("instructions").and_then(Value::as_str).unwrap_or("");
    assert!(
        !instructions.contains("Previous assistant response:"),
        "folded per-turn history leaves instructions"
    );
    let input = payload.get("input").and_then(Value::as_array).expect("input");
    let reminder = input.last().expect("trailing reminder");
    assert!(
        reminder
            .pointer("/content/0/text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("did it")),
        "folded history rides the tail"
    );
}

#[test]
fn chat_splits_dynamic_suffix_to_trailing_system_message() {
    use std::sync::Arc;

    let mut request = request();
    request.model = "custom-chat-model".to_string();
    request.system_prompt = Some(Arc::from(system_with_dynamics()));
    request.messages = vec![provider::Message::user("hello".to_string())].into();

    let payload = build_chat_request(&request, &chat_context()).expect("chat request should build");
    let messages = payload.get("messages").and_then(Value::as_array).expect("messages");

    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].get("role").and_then(Value::as_str), Some("system"));
    assert!(
        messages[0]
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("stable instructions"))
    );
    assert!(
        messages[0]
            .get("content")
            .and_then(Value::as_str)
            .is_none_or(|text| !text.contains("[Harness Limits]"))
    );
    assert_eq!(messages[1].get("role").and_then(Value::as_str), Some("user"));
    assert_eq!(messages[2].get("role").and_then(Value::as_str), Some("system"));
    assert!(
        messages[2]
            .get("content")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("[Harness Limits]"))
    );
}

#[test]
fn chat_without_dynamics_keeps_single_system_message() {
    use std::sync::Arc;

    let mut request = request();
    request.model = "custom-chat-model".to_string();
    request.system_prompt = Some(Arc::from("stable instructions"));
    request.messages = vec![provider::Message::user("hello".to_string())].into();

    let payload = build_chat_request(&request, &chat_context()).expect("chat request should build");
    let messages = payload.get("messages").and_then(Value::as_array).expect("messages");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].get("role").and_then(Value::as_str), Some("system"));
    // Fast path must be byte-identical to the legacy single system message.
    let legacy = augment_openai_instructions("custom-chat-model", "stable instructions".to_string());
    assert_eq!(messages[0].get("content").and_then(Value::as_str), Some(legacy.as_str()));
}

#[test]
fn responses_splits_mid_history_system_and_dynamics_without_panicking() {
    use std::sync::Arc;

    // Regression guard (review H1/M3): a System message positioned after an
    // assistant turn used to trip the prefix-strip heuristic / debug_assert.
    // Provenance-based segmentation must split every case deterministically.
    let mut request = gpt56_request(vec![
        provider::Message::assistant("did it".to_string()),
        provider::Message::system("resume note".to_string()),
        provider::Message::user("continue".to_string()),
    ]);
    request.system_prompt = Some(Arc::from(system_with_dynamics()));

    let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");
    let instructions = payload
        .pointer("/input/0/content/0/text")
        .and_then(Value::as_str)
        .expect("developer instructions");
    assert!(instructions.contains("stable instructions"));
    assert!(!instructions.contains("[Harness Limits]"), "dynamic sections leave instructions");
    assert!(!instructions.contains("resume note"), "mid-history system leaves instructions");

    let input = payload.get("input").and_then(Value::as_array).expect("input");
    let reminder = input.last().expect("trailing reminder");
    let reminder_text = reminder
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .expect("reminder text");
    assert!(reminder_text.contains("[Harness Limits]"));
    assert!(reminder_text.contains("resume note"));
    // History order before the tail is untouched: assistant then user.
    assert_eq!(input[0].get("role").and_then(Value::as_str), Some("developer"));
    assert_eq!(input[1].get("role").and_then(Value::as_str), Some("assistant"));
    assert_eq!(input[2].get("role").and_then(Value::as_str), Some("user"));
}

#[test]
fn responses_all_dynamic_system_prompt_keeps_contract_addendum() {
    use std::sync::Arc;

    // Regression guard (review L1): when the system prompt is entirely
    // dynamic, the GPT-5.6 contract addendum must still be emitted rather
    // than silently dropped.
    let mut request = gpt56_request(vec![provider::Message::user("hi".to_string())]);
    request.system_prompt = Some(Arc::from("[Harness Limits]\n- max_tool_calls_per_turn: 5"));

    let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");
    let instructions = payload
        .pointer("/input/0/content/0/text")
        .and_then(Value::as_str)
        .expect("developer instructions");
    assert!(
        instructions.contains("GPT-5.6"),
        "contract addendum must survive an all-dynamic prompt; got: {instructions:?}"
    );
    assert!(!instructions.contains("[Harness Limits]"), "got: {instructions:?}");
}

#[test]
fn reminder_only_input_receives_no_explicit_breakpoint() {
    // Regression guard (review L2): a history of only System messages leaves
    // the reminder as the sole input item; marking it would churn every turn.
    let request = gpt56_request(vec![provider::Message::system("resume note".to_string())]);

    let payload = build_responses_request(&request, &cache_ctx()).expect("request should build");

    assert_eq!(count_explicit_breakpoints(&payload), 0, "varying reminder must stay unmarked");
}

#[test]
fn ultrafast_service_tier_checks_models_for_explicit_and_provider_defaults() {
    for (model, allowed) in [
        ("gpt-6-astra", true),
        ("gpt-6.1-sol", true),
        ("gpt-5.6-sol", false),
        ("gpt-5", false),
        ("gpt-6-astra-pro", false),
    ] {
        for explicit in [false, true] {
            let mut request = request();
            request.model = model.to_string();
            let mut chat = chat_context();
            let mut responses = base_context(None);
            if explicit {
                request.service_tier = Some("ultrafast".to_string());
            } else {
                chat.default_service_tier = Some("ultrafast");
                responses.default_service_tier = Some("ultrafast");
            }
            for payload in [
                build_chat_request(&request, &chat).unwrap(),
                build_responses_request(&request, &responses).unwrap(),
            ] {
                assert_eq!(
                    payload.get("service_tier").and_then(Value::as_str),
                    allowed.then_some("ultrafast"),
                    "{model} explicit={explicit}"
                );
            }
        }
    }
}
