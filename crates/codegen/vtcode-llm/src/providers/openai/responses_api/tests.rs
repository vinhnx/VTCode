use super::{build_standard_responses_payload, parse_responses_payload};
use crate::provider::{LLMRequest, Message, ToolCall};
use crate::providers::shared::{parse_cache_write_tokens_from_usage, parse_cached_prompt_tokens_from_usage};
use serde_json::{Value, json};

fn assert_multimodal_tool_result(payload: super::OpenAIResponsesPayload) {
    let tool_msg = payload
        .input
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("function_call_output"))
        .expect("function_call_output should exist");
    let tool_result_content = tool_msg
        .get("output")
        .and_then(Value::as_array)
        .expect("function_call_output output should be an array");

    assert_eq!(tool_result_content.len(), 2);
    assert_eq!(tool_result_content[0]["type"], "input_text");
    assert_eq!(tool_result_content[0]["text"], "inline image note");
    assert_eq!(tool_result_content[1]["type"], "input_image");
    assert_eq!(tool_result_content[1]["image_url"], "data:image/png;base64,abc");
}

fn parsed_reasoning_details(response: &crate::provider::LLMResponse) -> Vec<Value> {
    response
        .reasoning_details
        .as_ref()
        .expect("reasoning details should exist")
        .iter()
        .map(|item| serde_json::from_str(item))
        .collect::<Result<Vec<_>, _>>()
        .expect("reasoning detail should be valid JSON")
}

#[test]
fn standard_payload_normalizes_stringified_reasoning_details_items() {
    let request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![
            Message::assistant("answer".to_string()).with_reasoning_details(Some(vec![
                json!(r#"{"type":"compaction","id":"cmp_1","encrypted_content":"opaque"}"#),
                json!("plain-text"),
            ])),
        ]
        .into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, true).expect("payload should build");
    assert_eq!(payload.input.len(), 2);
    assert_eq!(payload.input[0]["type"], "compaction");
}

#[test]
fn standard_payload_preserves_multimodal_tool_result_content() {
    let request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_1".to_string(),
                    "view_image".to_string(),
                    "{\"path\":\"./img.png\"}".to_string(),
                )],
            ),
            Message::tool_response(
                "call_1".to_string(),
                r#"[{"type":"input_text","text":"inline image note"},{"type":"input_image","image_url":"data:image/png;base64,abc"}]"#
                    .to_string(),
            ),
        ].into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, true).expect("payload should build");
    assert_multimodal_tool_result(payload);
}

#[test]
fn standard_payload_drops_unsupported_svg_image_parts() {
    // Regression: an SVG auto-attached from a quoted path in a WebMCP diff
    // must not reach the wire — providers fail the whole request with 400
    // `invalid_value` when any `input_image` carries an unsupported type.
    let request = LLMRequest {
        model: "gpt-5".to_string(),
        messages: vec![Message::user_with_parts(vec![
            crate::provider::ContentPart::text("see the logo".to_string()),
            crate::provider::ContentPart::image("PHN2Zz48L3N2Zz4=".to_string(), "image/svg+xml".to_string()),
        ])]
        .into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, true).expect("payload should build");
    let serialized = serde_json::to_string(&payload.input).expect("payload input should serialize");
    assert!(!serialized.contains("image/svg+xml"), "SVG image must be dropped from the wire payload");
    assert!(!serialized.contains("input_image"), "no image part should remain");
    assert!(serialized.contains("see the logo"), "the text part must survive");
}

#[test]
fn grown_history_keeps_wire_prefix_byte_stable() {
    // Root-cause guard for low prompt-cache hit rates: provider-side
    // prefix caching only hits when the rendered wire prefix is
    // append-stable. A second turn that only appends one assistant+tool
    // round must keep `instructions` identical and extend `input`
    // without rewriting earlier items. Asymmetric pair: grown history
    // must contain the first turn's rendering as an exact prefix.
    use std::sync::Arc;

    let history = vec![
        Message::user("list files".to_string()),
        Message::assistant_with_tools(
            "checking".to_string(),
            vec![ToolCall::function(
                "call_1".to_string(),
                "exec_command".to_string(),
                "{\"command\":\"ls\"}".to_string(),
            )],
        ),
        Message::tool_response("call_1".to_string(), "a.txt\nb.txt".to_string()),
    ];
    let first = LLMRequest {
        model: "gpt-5.6-luna".to_string(),
        system_prompt: Some(Arc::from("stable system instructions")),
        messages: history.clone().into(),
        ..Default::default()
    };
    let mut grown = history;
    grown.push(Message::assistant_with_tools(
        "reading".to_string(),
        vec![ToolCall::function(
            "call_2".to_string(),
            "exec_command".to_string(),
            "{\"command\":\"cat a.txt\"}".to_string(),
        )],
    ));
    grown.push(Message::tool_response("call_2".to_string(), "hello".to_string()));
    let second = LLMRequest {
        model: "gpt-5.6-luna".to_string(),
        system_prompt: Some(Arc::from("stable system instructions")),
        messages: grown.into(),
        ..Default::default()
    };

    let a = build_standard_responses_payload(&first, true).expect("payload should build");
    let b = build_standard_responses_payload(&second, true).expect("payload should build");
    assert_eq!(a.instructions, b.instructions, "appended history must not rewrite instructions");
    assert!(b.input.len() > a.input.len(), "grown history must extend input");
    assert_eq!(&b.input[..a.input.len()], &a.input[..], "input prefix must be append-stable across turns");
}

#[test]
fn standard_payload_uses_responses_function_call_items_for_structured_tool_history() {
    let request = LLMRequest {
        model: "gpt-5-codex".to_string(),
        messages: vec![
            Message::user("run cargo fmt".to_string()),
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "direct_exec_command_1".to_string(),
                    "exec_command".to_string(),
                    "{\"command\":\"cargo fmt\"}".to_string(),
                )],
            ),
            Message::tool_response(
                "direct_exec_command_1".to_string(),
                "{\"output\":\"\",\"exit_code\":0,\"backend\":\"pipe\"}".to_string(),
            ),
            Message::assistant("cargo fmt completed successfully.".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, true).expect("payload should build");

    assert_eq!(payload.input.len(), 4);
    assert_eq!(payload.input[0]["role"], "user");
    assert_eq!(payload.input[1]["type"], "function_call");
    assert!(payload.input[1].get("id").is_none());
    assert_eq!(payload.input[1]["call_id"], "direct_exec_command_1");
    assert_eq!(payload.input[2]["type"], "function_call_output");
    assert_eq!(payload.input[2]["call_id"], "direct_exec_command_1");
    assert_eq!(payload.input[2]["output"], "{\"output\":\"\",\"exit_code\":0,\"backend\":\"pipe\"}");
    assert_eq!(payload.input[3]["role"], "assistant");
}

#[test]
fn standard_payload_synthesizes_missing_function_call_output_for_orphan_call() {
    let request = LLMRequest {
        model: "gpt-5-codex".to_string(),
        messages: vec![
            Message::user("run cargo fmt".to_string()),
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_orphan".to_string(),
                    "exec_command".to_string(),
                    "{\"command\":\"cargo fmt\"}".to_string(),
                )],
            ),
            Message::user("continue".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, true).expect("payload should build");

    assert!(payload.input.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("function_call")
            && item.get("call_id").and_then(Value::as_str) == Some("call_orphan")
    }));
    assert!(payload.input.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("function_call_output")
            && item.get("call_id").and_then(Value::as_str) == Some("call_orphan")
            && item.get("output").and_then(Value::as_str) == Some("aborted")
    }));
}

#[test]
fn standard_payload_pairs_deferred_tool_output_when_output_precedes_call() {
    let request = LLMRequest {
        model: "gpt-5-codex".to_string(),
        messages: vec![
            Message::user("continue".to_string()),
            Message::tool_response("call_1".to_string(), "{\"output\":\"late\"}".to_string()),
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_1".to_string(),
                    "exec_command".to_string(),
                    "{\"command\":\"echo late\"}".to_string(),
                )],
            ),
        ]
        .into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, true).expect("payload should build");

    let call_index = payload
        .input
        .iter()
        .position(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call")
                && item.get("call_id").and_then(Value::as_str) == Some("call_1")
        })
        .expect("function_call should exist");
    let output_index = payload
        .input
        .iter()
        .position(|item| {
            item.get("type").and_then(Value::as_str) == Some("function_call_output")
                && item.get("call_id").and_then(Value::as_str) == Some("call_1")
        })
        .expect("function_call_output should exist");

    assert!(output_index > call_index);
    assert_eq!(payload.input[output_index]["output"], "{\"output\":\"late\"}");
    assert_ne!(payload.input[output_index]["output"], "aborted");
}

#[test]
fn standard_payload_omits_function_call_id_for_codex_replay_shape() {
    let request = LLMRequest {
        model: "gpt-5.1-codex".to_string(),
        messages: vec![
            Message::user("run cargo fmt and report".to_string()),
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_T4IsdQtJifUHQUXutDlwoFLd".to_string(),
                    "exec_command".to_string(),
                    r#"{"command":"cd /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode && cargo fmt","workdir":"/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode","sandbox_permissions":"use_default","additional_permissions":{"fs_read":[],"fs_write":[]}}"#.to_string(),
                )],
            ),
            Message::tool_response(
                "call_T4IsdQtJifUHQUXutDlwoFLd".to_string(),
                r#"{"output":"","exit_code":0,"backend":"pipe"}"#.to_string(),
            ),
            Message::system(
                "Previous turn already completed tool execution. Reuse the latest tool outputs in history instead of rerunning the same exploration.".to_string(),
            ),
            Message::user("ok".to_string()),
        ].into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, true).expect("payload should build");
    let function_call = payload
        .input
        .iter()
        .find(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
        .expect("function_call item should exist");

    assert_eq!(function_call.get("call_id").and_then(Value::as_str), Some("call_T4IsdQtJifUHQUXutDlwoFLd"));
    assert!(function_call.get("id").is_none(), "function_call replay items should omit id");
}

#[test]
fn parse_responses_payload_prefers_call_id_for_tool_correlation() {
    let response = json!({
        "output": [
            {
                "type": "function_call",
                "id": "fc_123",
                "call_id": "call_123",
                "name": "exec_command",
                "arguments": "{\"command\":\"cargo fmt\"}"
            }
        ]
    });

    let parsed = parse_responses_payload(response, "gpt-5-codex".to_string(), false).expect("payload should parse");

    let tool_calls = parsed.tool_calls.expect("tool calls should exist");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id, "call_123");
    assert_eq!(tool_calls[0].function.as_ref().map(|function| function.name.as_str()), Some("exec_command"));
}

#[test]
fn parse_responses_payload_preserves_function_namespace() {
    let response = json!({
        "output": [
            {
                "type": "function_call",
                "id": "fc_456",
                "call_id": "call_456",
                "namespace": "repo_browser",
                "name": "code_search",
                "arguments": "{\"query\":\"Widget\",\"path\":\".\"}"
            }
        ]
    });

    let parsed = parse_responses_payload(response, "gpt-5-codex".to_string(), false).expect("payload should parse");

    let tool_calls = parsed.tool_calls.expect("tool calls should exist");
    let namespace = tool_calls[0]
        .function
        .as_ref()
        .and_then(|function| function.namespace.as_deref());

    assert_eq!(namespace, Some("repo_browser"));
}

#[test]
fn parse_responses_payload_parses_custom_tool_calls() {
    let response = json!({
        "output": [
            {
                "type": "custom_tool_call",
                "id": "ct_123",
                "call_id": "call_patch_1",
                "name": "apply_patch",
                "input": "*** Begin Patch\n*** End Patch\n"
            }
        ]
    });

    let parsed = parse_responses_payload(response, "gpt-5-codex".to_string(), false).expect("payload should parse");

    let tool_calls = parsed.tool_calls.expect("tool calls should exist");
    assert_eq!(tool_calls.len(), 1);
    assert!(tool_calls[0].is_custom());
    assert_eq!(tool_calls[0].id, "call_patch_1");
    assert_eq!(tool_calls[0].tool_name(), Some("apply_patch"));
    assert_eq!(tool_calls[0].raw_input(), Some("*** Begin Patch\n*** End Patch\n"));
}

#[test]
fn parse_responses_payload_preserves_nested_encrypted_reasoning_for_replay() {
    let encrypted_reasoning = json!({
        "type": "reasoning",
        "id": "rs_msg_1",
        "text": "checked constraints",
        "encrypted_content": "opaque_reasoning_blob",
        "future_state": {"provider": "openai"}
    });
    let response = json!({
        "output": [
            {
                "type": "message",
                "content": [
                    encrypted_reasoning.clone(),
                    {"type": "output_text", "text": "done"}
                ]
            }
        ]
    });

    let parsed = parse_responses_payload(response, "gpt-5-codex".to_string(), false).expect("payload should parse");

    assert_eq!(parsed.content.as_deref(), Some("done"));
    assert_eq!(parsed.reasoning.as_deref(), Some("checked constraints"));

    let reasoning_details = parsed_reasoning_details(&parsed);
    assert!(
        reasoning_details.iter().any(|item| item == &encrypted_reasoning),
        "encrypted reasoning item should be preserved verbatim"
    );

    let payload = build_standard_responses_payload(
        &LLMRequest {
            model: "gpt-5-codex".to_string(),
            messages: vec![
                Message::assistant(parsed.content.unwrap_or_default()).with_reasoning_details(Some(reasoning_details)),
            ]
            .into(),
            ..Default::default()
        },
        true,
    )
    .expect("payload should replay encrypted reasoning");

    assert_eq!(payload.input[0], encrypted_reasoning);
    assert_eq!(payload.input[1]["role"], "assistant");
}

#[test]
fn parse_responses_payload_preserves_unknown_replay_items() {
    let unknown_replay_item = json!({
        "type": "provider_replay_state",
        "id": "prs_1",
        "encrypted_content": "opaque_provider_state",
        "future_field": {"preserve": true}
    });
    let response = json!({
        "output": [
            unknown_replay_item.clone(),
            {
                "type": "message",
                "content": [{"type": "output_text", "text": "done"}]
            }
        ]
    });

    let parsed = parse_responses_payload(response, "gpt-5-codex".to_string(), false).expect("payload should parse");

    let preserved_items = parsed_reasoning_details(&parsed);
    assert!(preserved_items.iter().any(|item| item == &unknown_replay_item));

    let payload = build_standard_responses_payload(
        &LLMRequest {
            model: "gpt-5-codex".to_string(),
            messages: vec![
                Message::assistant(parsed.content.unwrap_or_default()).with_reasoning_details(Some(preserved_items)),
            ]
            .into(),
            ..Default::default()
        },
        true,
    )
    .expect("payload should replay unknown item");

    assert_eq!(payload.input[0], unknown_replay_item);
    assert_eq!(payload.input[1]["role"], "assistant");
}

#[test]
fn parse_responses_payload_preserves_compaction_item_for_replay() {
    let compaction_item = json!({
        "type": "compaction",
        "id": "cmp_1",
        "encrypted_content": "opaque_state"
    });
    let response = json!({
        "output": [
            compaction_item.clone(),
            {
                "type": "message",
                "content": [{"type": "output_text", "text": "continued"}]
            }
        ]
    });

    let parsed = parse_responses_payload(response, "gpt-5".to_string(), false).expect("payload should parse");
    let reasoning_details = parsed_reasoning_details(&parsed);
    assert!(reasoning_details.iter().any(|item| item == &compaction_item));

    let payload = build_standard_responses_payload(
        &LLMRequest {
            model: "gpt-5".to_string(),
            messages: vec![
                Message::assistant(parsed.content.unwrap_or_default()).with_reasoning_details(Some(reasoning_details)),
            ]
            .into(),
            ..Default::default()
        },
        true,
    )
    .expect("payload should replay compaction item");

    assert_eq!(payload.input[0], compaction_item);
    assert_eq!(payload.input[1]["role"], "assistant");
}

#[test]
fn parse_responses_payload_tool_calls_replay_with_result_linkage() {
    let response = json!({
        "output": [
            {
                "type": "function_call",
                "id": "fc_789",
                "call_id": "call_789",
                "name": "exec_command",
                "arguments": "{\"command\":\"cargo test\"}"
            }
        ]
    });

    let parsed = parse_responses_payload(response, "gpt-5-codex".to_string(), false).expect("payload should parse");
    let tool_calls = parsed.tool_calls.expect("tool calls should exist");

    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id, "call_789");
    let function = tool_calls[0].function.as_ref().expect("function call");
    assert_eq!(function.name, "exec_command");
    assert_eq!(function.arguments, "{\"command\":\"cargo test\"}");

    let payload = build_standard_responses_payload(
        &LLMRequest {
            model: "gpt-5-codex".to_string(),
            messages: vec![
                Message::assistant_with_tools(String::new(), tool_calls),
                Message::tool_response("call_789".to_string(), "{\"exit_code\":0}".to_string()),
            ]
            .into(),
            ..Default::default()
        },
        true,
    )
    .expect("payload should replay tool call and output");

    assert_eq!(payload.input[0]["type"], "function_call");
    assert_eq!(payload.input[0]["call_id"], "call_789");
    assert_eq!(payload.input[0]["name"], "exec_command");
    assert_eq!(payload.input[0]["arguments"], "{\"command\":\"cargo test\"}");
    assert_eq!(payload.input[1]["type"], "function_call_output");
    assert_eq!(payload.input[1]["call_id"], "call_789");
    assert_eq!(payload.input[1]["output"], "{\"exit_code\":0}");
}

#[test]
fn standard_payload_replays_custom_tool_turns_with_custom_items() {
    let request = LLMRequest {
        messages: vec![
            Message::user("Apply patch".to_string()),
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::custom(
                    "call_patch_1".to_string(),
                    "apply_patch".to_string(),
                    "*** Begin Patch\n*** End Patch\n".to_string(),
                )],
            ),
            Message::tool_response("call_patch_1".to_string(), "patched".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, true).expect("payload should build");

    assert_eq!(payload.input.len(), 3);
    assert_eq!(payload.input[1]["type"], "custom_tool_call");
    assert_eq!(payload.input[1]["call_id"], "call_patch_1");
    assert_eq!(payload.input[1]["name"], "apply_patch");
    assert_eq!(payload.input[1]["input"], "*** Begin Patch\n*** End Patch\n");
    assert_eq!(payload.input[2]["type"], "custom_tool_call_output");
    assert_eq!(payload.input[2]["call_id"], "call_patch_1");
    assert_eq!(payload.input[2]["output"], "patched");
}

#[test]
fn parse_responses_payload_extracts_tool_search_references() {
    let response = json!({
        "output": [
            {
                "type": "tool_search_output",
                "execution": "client",
                "status": "completed",
                "tools": [
                    {
                        "name": "exec_command",
                        "description": "Read a file"
                    },
                    {
                        "name": "namespace_group",
                        "tools": [
                            {
                                "name": "write_file",
                                "description": "Write a file"
                            }
                        ]
                    }
                ]
            }
        ]
    });

    let parsed = parse_responses_payload(response, "gpt-5-codex".to_string(), false).expect("payload should parse");

    assert_eq!(parsed.tool_references, vec!["exec_command".to_string(), "write_file".to_string()]);
}

#[test]
fn standard_payload_can_move_assistant_text_history_into_instructions() {
    let request = LLMRequest {
        model: "gpt-5.2-codex".to_string(),
        messages: vec![
            Message::user("What is this project?".to_string()),
            Message::assistant("VT Code is a Rust Cargo workspace.".to_string()),
            Message::user("Tell me more.".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, false).expect("payload should build");

    assert_eq!(payload.input.len(), 2);
    assert_eq!(payload.input[0]["role"], "user");
    assert_eq!(payload.input[1]["role"], "user");
    assert_eq!(
        payload.instructions.as_deref(),
        Some("Previous assistant response:\nVT Code is a Rust Cargo workspace.")
    );
}

#[test]
fn standard_payload_can_omit_reasoning_details_from_input() {
    let request = LLMRequest {
        model: "gpt-5.2-codex".to_string(),
        messages: vec![
            Message::assistant("answer".to_string()).with_reasoning_details(Some(vec![json!({
                "type": "reasoning",
                "id": "rs_1",
                "summary": [{"type":"summary_text","text":"opaque"}]
            })])),
            Message::user("next".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, false).expect("payload should build");

    assert_eq!(payload.input.len(), 1);
    assert_eq!(payload.input[0]["role"], "user");
}

#[test]
fn standard_payload_can_move_tool_turn_history_into_instructions() {
    let request = LLMRequest {
        model: "gpt-5.2-codex".to_string(),
        messages: vec![
            Message::user("run cargo check".to_string()),
            Message::assistant_with_tools(
                String::new(),
                vec![ToolCall::function(
                    "call_1".to_string(),
                    "exec_command".to_string(),
                    "{\"command\":\"cargo check\"}".to_string(),
                )],
            ),
            Message::tool_response(
                "call_1".to_string(),
                "{\"output\":\"Finished `dev` profile\",\"exit_code\":0}".to_string(),
            ),
            Message::assistant("cargo check completed successfully.".to_string()),
            Message::user("tell me more".to_string()),
        ]
        .into(),
        ..Default::default()
    };

    let payload = build_standard_responses_payload(&request, false).expect("payload should build");

    assert_eq!(payload.input.len(), 2);
    assert_eq!(payload.input[0]["role"], "user");
    assert_eq!(payload.input[1]["role"], "user");
    let instructions = payload.instructions.expect("instructions should exist");
    assert!(instructions.contains("Previous tool result (call_1):"));
    assert!(instructions.contains("Finished `dev` profile"));
    assert!(instructions.contains("Previous assistant response:\ncargo check completed successfully."));
}

#[test]
fn parse_responses_payload_ignores_hosted_shell_trace_items() {
    let response = json!({
        "output": [
            {
                "type": "shell_call",
                "id": "sh_1",
                "status": "completed",
                "action": { "type": "command", "command": ["pwd"] }
            },
            {
                "type": "shell_call_output",
                "id": "sho_1",
                "call_id": "sh_1",
                "output": "workspace\n"
            },
            {
                "type": "message",
                "content": [
                    { "type": "output_text", "text": "Done." }
                ]
            }
        ]
    });

    let parsed = parse_responses_payload(response, "gpt-5".to_string(), false).expect("should parse");

    assert_eq!(parsed.content.as_deref(), Some("Done."));
    assert!(parsed.tool_calls.unwrap_or_default().is_empty());
}

#[test]
fn parse_responses_payload_extracts_cached_prompt_tokens_from_input_details() {
    let response = json!({
        "output": [
            {
                "type": "message",
                "content": [
                    {"type": "output_text", "text": "hello"}
                ]
            }
        ],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 5,
            "total_tokens": 105,
            "input_tokens_details": {
                "cached_tokens": 42
            }
        }
    });

    let parsed = parse_responses_payload(response, "gpt-5".to_string(), true).expect("payload should parse");

    assert_eq!(parsed.usage.and_then(|usage| usage.cached_prompt_tokens), Some(42));
}

#[test]
fn parse_responses_payload_treats_missing_cached_prompt_tokens_as_normal() {
    let response = json!({
        "output": [
            {
                "type": "message",
                "content": [
                    {"type": "output_text", "text": "hello"}
                ]
            }
        ],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 5,
            "total_tokens": 105
        }
    });

    let parsed = parse_responses_payload(response, "gpt-5".to_string(), true).expect("payload should parse");

    assert_eq!(parsed.usage.and_then(|usage| usage.cached_prompt_tokens), None);
}

#[test]
fn parse_cached_prompt_tokens_supports_responses_fallback_shapes() {
    assert_eq!(
        parse_cached_prompt_tokens_from_usage(&json!({"prompt_tokens_details": {"cached_tokens": 17}}), true),
        Some(17)
    );
    assert_eq!(parse_cached_prompt_tokens_from_usage(&json!({"prompt_cache_hit_tokens": 23}), true), Some(23));
    assert_eq!(parse_cached_prompt_tokens_from_usage(&json!({"cached_tokens": 29}), true), Some(29));
    assert_eq!(
        parse_cached_prompt_tokens_from_usage(&json!({"input_tokens_details": {"cached_tokens": 31}}), false),
        None
    );
}

#[test]
fn parse_responses_payload_reports_cache_write_tokens() {
    let response = json!({
        "output": [
            {
                "type": "message",
                "content": [
                    {"type": "output_text", "text": "hello"}
                ]
            }
        ],
        "usage": {
            "input_tokens": 100,
            "output_tokens": 5,
            "total_tokens": 105,
            "input_tokens_details": {
                "cached_tokens": 42,
                "cache_write_tokens": 58
            }
        }
    });

    let parsed = parse_responses_payload(response, "gpt-6-astra".to_string(), true).expect("payload should parse");
    let usage = parsed.usage.expect("usage should exist");

    assert_eq!(usage.cached_prompt_tokens, Some(42));
    assert_eq!(usage.cache_creation_tokens, Some(58));
}

#[test]
fn parse_cache_write_tokens_supports_chat_fallback_shape_and_metrics_gate() {
    assert_eq!(
        parse_cache_write_tokens_from_usage(&json!({"prompt_tokens_details": {"cache_write_tokens": 17}}), true),
        Some(17)
    );
    assert_eq!(
        parse_cache_write_tokens_from_usage(&json!({"input_tokens_details": {"cache_write_tokens": 31}}), false),
        None
    );
    assert_eq!(parse_cache_write_tokens_from_usage(&json!({"input_tokens_details": {}}), true), None);
}
