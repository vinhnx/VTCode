use super::{ResponsesLifecycleEvent, ResponsesStreamAdapter, ResponsesStreamEvent};
use crate::provider::ReasoningSource;
use serde_json::{Value, json};

fn event_fixture(payload: Value) -> ResponsesStreamEvent {
    ResponsesStreamAdapter::parse_sse_data(&payload.to_string()).expect("fixture should parse")
}

fn assert_invalid_stream_payload(payload: Value) {
    let err = ResponsesStreamAdapter::parse_sse_data(&payload.to_string())
        .expect_err("fixture should be rejected as invalid stream payload");
    assert!(err.to_string().contains("invalid stream payload"), "unexpected error: {err}");
}

fn assert_provider_value_bearing_rig_gap(
    payload: Value,
    expected_event_type: &str,
    expected_call_id: Option<&str>,
) -> Value {
    let event = event_fixture(payload);
    let ResponsesStreamEvent::ProviderValueBearingRigGap {
        event_type,
        item_id,
        call_id,
        output_index,
        sequence_number,
        payload,
    } = event
    else {
        panic!("expected provider value-bearing Rig-gap event");
    };

    assert_eq!(event_type, expected_event_type);
    assert_eq!(item_id.as_deref(), Some("item_1"));
    assert_eq!(call_id.as_deref(), expected_call_id);
    assert_eq!(output_index, Some(2));
    assert_eq!(sequence_number, Some(10));
    payload
}

#[test]
fn stream_event_policy_separates_rig_overlay_noop_value_and_unsupported() {
    use super::ResponsesStreamEventPolicy as Policy;

    assert_eq!(
        super::response_stream_event_policy_for_type("response.output_text.delta"),
        Policy::RigSupportedTyped
    );
    assert_eq!(
        super::response_stream_event_policy_for_type("response.reasoning_text.delta"),
        Policy::RigSupportedTyped
    );
    assert_eq!(
        super::response_stream_event_policy_for_type("response.reasoning_content.delta"),
        Policy::VtcodeOverlayConversion
    );
    assert_eq!(super::response_stream_event_policy_for_type("response.queued"), Policy::DocumentedStatusMarkerNoop);
    assert_eq!(super::response_stream_event_policy_for_type("keepalive"), Policy::DocumentedStatusMarkerNoop);
    // StepFun emits the non-summary reasoning part boundaries; they are
    // status markers because the text arrives via `reasoning_text.delta`.
    assert_eq!(
        super::response_stream_event_policy_for_type("response.reasoning_part.added"),
        Policy::DocumentedStatusMarkerNoop
    );
    assert_eq!(
        super::response_stream_event_policy_for_type("response.reasoning_part.done"),
        Policy::DocumentedStatusMarkerNoop
    );

    for event_type in [
        "response.code_interpreter_call_code.delta",
        "response.code_interpreter_call_code.done",
        "response.mcp_call_arguments.delta",
        "response.mcp_call_arguments.done",
        "response.image_generation_call.partial_image",
        "response.custom_tool_call_input.delta",
        "response.custom_tool_call_input.done",
        "response.output_text.annotation.added",
        "response.reasoning_text.done",
    ] {
        assert_ne!(
            super::response_stream_event_policy_for_type(event_type),
            Policy::DocumentedStatusMarkerNoop,
            "{event_type} must not be classified as a status-only no-op"
        );
    }

    assert_eq!(
        super::response_stream_event_policy_for_type("response.code_interpreter_call_code.delta"),
        Policy::DocumentedValueBearingRigGap
    );
    assert_eq!(super::response_stream_event_policy_for_type("response.not_a_real_event"), Policy::Unsupported);
}

#[test]
fn completed_response_with_custom_tool_call_uses_raw_envelope_fallback() {
    let raw_response = json!({
        "id": "resp_custom_tool",
        "object": "response",
        "created_at": 1,
        "status": "completed",
        "error": null,
        "incomplete_details": null,
        "instructions": null,
        "max_output_tokens": null,
        "model": "gpt-5",
        "usage": {
            "input_tokens": 11,
            "output_tokens": 7,
            "total_tokens": 18
        },
        "output": [{
            "type": "custom_tool_call",
            "id": "ctc_1",
            "call_id": "call_custom_1",
            "name": "apply_patch",
            "input": "*** Begin Patch\n*** End Patch\n",
            "status": "completed"
        }],
        "tools": [],
        "vtcode_overlay": {"preserved": true}
    });

    let event = event_fixture(json!({
        "type": "response.completed",
        "sequence_number": 20,
        "response": raw_response.clone()
    }));

    let ResponsesStreamEvent::CompletedResponse { response } = event else {
        panic!("expected completed response fallback");
    };
    assert_eq!(response, raw_response);
}

#[test]
fn output_item_added_with_mcp_call_uses_raw_envelope_fallback() {
    let payload = json!({
        "type": "response.output_item.added",
        "sequence_number": 21,
        "item_id": "mcp_1",
        "output_index": 3,
        "item": {
            "type": "mcp_call",
            "id": "mcp_1",
            "call_id": "call_mcp_1",
            "server_label": "workspace",
            "name": "exec_command",
            "arguments": "{\"path\":\"src/main.rs\"}",
            "status": "in_progress"
        }
    });

    let event = event_fixture(payload.clone());
    let ResponsesStreamEvent::ProviderValueBearingRigGap {
        event_type,
        item_id,
        call_id,
        output_index,
        sequence_number,
        payload: preserved_payload,
    } = event
    else {
        panic!("expected provider value-bearing Rig-gap fallback");
    };

    assert_eq!(event_type, "response.output_item.added");
    assert_eq!(item_id.as_deref(), Some("mcp_1"));
    assert_eq!(call_id.as_deref(), Some("call_mcp_1"));
    assert_eq!(output_index, Some(3));
    assert_eq!(sequence_number, Some(21));
    assert_eq!(preserved_payload, payload);
}

#[test]
fn output_item_done_with_code_interpreter_call_uses_raw_envelope_fallback() {
    let payload = json!({
        "type": "response.output_item.done",
        "sequence_number": 22,
        "item_id": "ci_1",
        "output_index": 4,
        "item": {
            "type": "code_interpreter_call",
            "id": "ci_1",
            "call_id": "call_ci_1",
            "code": "print('hello')",
            "status": "completed",
            "outputs": [{
                "type": "logs",
                "logs": "hello\n"
            }]
        }
    });

    let event = event_fixture(payload.clone());
    let ResponsesStreamEvent::ProviderValueBearingRigGap {
        event_type,
        item_id,
        call_id,
        output_index,
        sequence_number,
        payload: preserved_payload,
    } = event
    else {
        panic!("expected provider value-bearing Rig-gap fallback");
    };

    assert_eq!(event_type, "response.output_item.done");
    assert_eq!(item_id.as_deref(), Some("ci_1"));
    assert_eq!(call_id.as_deref(), Some("call_ci_1"));
    assert_eq!(output_index, Some(4));
    assert_eq!(sequence_number, Some(22));
    assert_eq!(preserved_payload, payload);
}

#[test]
fn malformed_known_rig_stream_events_remain_invalid_payload() {
    assert_invalid_stream_payload(json!({
        "type": "response.output_text.delta",
        "item_id": "msg_1",
        "output_index": 0,
        "content_index": 0,
        "sequence_number": 23,
        "delta": 42
    }));

    assert_invalid_stream_payload(json!({
        "type": "response.output_item.done",
        "item_id": "fc_1",
        "output_index": 0,
        "sequence_number": 24,
        "item": {
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_1",
            "arguments": "{}",
            "status": "completed"
        }
    }));
}

#[test]
fn raw_envelope_fallback_requires_nested_unknown_output_item_evidence() {
    for payload in [
        json!({
            "type": "response.completed",
            "sequence_number": 25,
            "output": [{
                "type": "custom_tool_call",
                "id": "ctc_missing_response"
            }]
        }),
        json!({
            "type": "response.output_item.added",
            "sequence_number": 26,
            "item_id": "item_missing_type",
            "output_index": 0,
            "item": {
                "id": "item_missing_type"
            }
        }),
        json!({
            "type": "response.output_item.done",
            "sequence_number": 27,
            "item_id": "item_non_string_type",
            "output_index": 0,
            "item": {
                "type": 42,
                "id": "item_non_string_type"
            }
        }),
        json!({
            "type": "response.output_item.added",
            "sequence_number": 28,
            "item_id": "fc_2",
            "output_index": 0,
            "item": {
                "type": "function_call",
                "id": "fc_2",
                "call_id": "call_2",
                "arguments": "{}",
                "status": "in_progress"
            }
        }),
    ] {
        assert_invalid_stream_payload(payload);
    }
}

#[test]
fn documented_status_marker_noop_fixtures_do_not_emit_runtime_events() {
    for (event_type, payload) in [
        (
            "response.queued",
            json!({
                "type": "response.queued",
                "sequence_number": 0,
                "response": {
                    "id": "resp_queued",
                    "status": "queued"
                }
            }),
        ),
        (
            "response.file_search_call.in_progress",
            json!({
                "type": "response.file_search_call.in_progress",
                "sequence_number": 1,
                "item_id": "fs_1",
                "output_index": 0
            }),
        ),
        (
            "response.file_search_call.searching",
            json!({
                "type": "response.file_search_call.searching",
                "sequence_number": 2,
                "item_id": "fs_1",
                "output_index": 0
            }),
        ),
        (
            "response.file_search_call.completed",
            json!({
                "type": "response.file_search_call.completed",
                "sequence_number": 3,
                "item_id": "fs_1",
                "output_index": 0
            }),
        ),
        (
            "response.web_search_call.in_progress",
            json!({
                "type": "response.web_search_call.in_progress",
                "sequence_number": 4,
                "item_id": "ws_1",
                "output_index": 1
            }),
        ),
        (
            "response.web_search_call.searching",
            json!({
                "type": "response.web_search_call.searching",
                "sequence_number": 5,
                "item_id": "ws_1",
                "output_index": 1
            }),
        ),
        (
            "response.web_search_call.completed",
            json!({
                "type": "response.web_search_call.completed",
                "sequence_number": 6,
                "item_id": "ws_1",
                "output_index": 1
            }),
        ),
        (
            "response.image_generation_call.in_progress",
            json!({
                "type": "response.image_generation_call.in_progress",
                "sequence_number": 7,
                "item_id": "ig_1",
                "output_index": 2
            }),
        ),
        (
            "response.image_generation_call.generating",
            json!({
                "type": "response.image_generation_call.generating",
                "sequence_number": 8,
                "item_id": "ig_1",
                "output_index": 2
            }),
        ),
        (
            "response.image_generation_call.completed",
            json!({
                "type": "response.image_generation_call.completed",
                "sequence_number": 9,
                "item_id": "ig_1",
                "output_index": 2
            }),
        ),
        (
            "response.mcp_call.in_progress",
            json!({
                "type": "response.mcp_call.in_progress",
                "sequence_number": 10,
                "item_id": "mcp_1",
                "output_index": 3
            }),
        ),
        (
            "response.mcp_call.completed",
            json!({
                "type": "response.mcp_call.completed",
                "sequence_number": 11,
                "item_id": "mcp_1",
                "output_index": 3
            }),
        ),
        (
            "response.mcp_list_tools.in_progress",
            json!({
                "type": "response.mcp_list_tools.in_progress",
                "sequence_number": 12,
                "item_id": "mcp_tools_1",
                "output_index": 4
            }),
        ),
        (
            "response.mcp_list_tools.completed",
            json!({
                "type": "response.mcp_list_tools.completed",
                "sequence_number": 13,
                "item_id": "mcp_tools_1",
                "output_index": 4
            }),
        ),
        (
            "response.code_interpreter_call.in_progress",
            json!({
                "type": "response.code_interpreter_call.in_progress",
                "sequence_number": 14,
                "item_id": "ci_1",
                "output_index": 5
            }),
        ),
        (
            "response.code_interpreter_call.interpreting",
            json!({
                "type": "response.code_interpreter_call.interpreting",
                "sequence_number": 15,
                "item_id": "ci_1",
                "output_index": 5
            }),
        ),
        (
            "response.code_interpreter_call.completed",
            json!({
                "type": "response.code_interpreter_call.completed",
                "sequence_number": 16,
                "item_id": "ci_1",
                "output_index": 5
            }),
        ),
    ] {
        assert_eq!(
            event_fixture(payload),
            ResponsesStreamEvent::Unknown,
            "{event_type} should be an explicit status/marker no-op"
        );
    }
}

#[test]
fn value_bearing_code_interpreter_code_events_preserve_payload_identity_and_sequence() {
    let delta_payload = assert_provider_value_bearing_rig_gap(
        json!({
            "type": "response.code_interpreter_call_code.delta",
            "sequence_number": 10,
            "item_id": "item_1",
            "call_id": "call_1",
            "output_index": 2,
            "code_index": 0,
            "delta": "print('hello')\n"
        }),
        "response.code_interpreter_call_code.delta",
        Some("call_1"),
    );
    assert_eq!(delta_payload["delta"], "print('hello')\n");
    assert_eq!(delta_payload["code_index"], 0);

    let done_payload = assert_provider_value_bearing_rig_gap(
        json!({
            "type": "response.code_interpreter_call_code.done",
            "sequence_number": 10,
            "item_id": "item_1",
            "output_index": 2,
            "code": "print('hello')\n"
        }),
        "response.code_interpreter_call_code.done",
        None,
    );
    assert_eq!(done_payload["code"], "print('hello')\n");
}

#[test]
fn value_bearing_mcp_argument_events_preserve_payload_identity_and_sequence() {
    let delta_payload = assert_provider_value_bearing_rig_gap(
        json!({
            "type": "response.mcp_call_arguments.delta",
            "sequence_number": 10,
            "item_id": "item_1",
            "output_index": 2,
            "delta": "{\"path\":\"src"
        }),
        "response.mcp_call_arguments.delta",
        None,
    );
    assert_eq!(delta_payload["delta"], "{\"path\":\"src");

    let done_payload = assert_provider_value_bearing_rig_gap(
        json!({
            "type": "response.mcp_call_arguments.done",
            "sequence_number": 10,
            "item_id": "item_1",
            "output_index": 2,
            "arguments": "{\"path\":\"src/main.rs\"}"
        }),
        "response.mcp_call_arguments.done",
        None,
    );
    assert_eq!(done_payload["arguments"], "{\"path\":\"src/main.rs\"}");
}

#[test]
fn value_bearing_image_partial_event_preserves_payload_identity_and_sequence() {
    let payload = assert_provider_value_bearing_rig_gap(
        json!({
            "type": "response.image_generation_call.partial_image",
            "sequence_number": 10,
            "item_id": "item_1",
            "output_index": 2,
            "partial_image_index": 0,
            "partial_image_b64": "iVBORw0KGgo="
        }),
        "response.image_generation_call.partial_image",
        None,
    );
    assert_eq!(payload["partial_image_index"], 0);
    assert_eq!(payload["partial_image_b64"], "iVBORw0KGgo=");
}

#[test]
fn value_bearing_output_text_annotation_event_preserves_metadata_identity_and_sequence() {
    let payload = assert_provider_value_bearing_rig_gap(
        json!({
            "type": "response.output_text.annotation.added",
            "sequence_number": 10,
            "item_id": "item_1",
            "output_index": 2,
            "content_index": 0,
            "annotation_index": 0,
            "annotation": {
                "type": "text_annotation",
                "text": "see docs",
                "start": 0,
                "end": 8
            }
        }),
        "response.output_text.annotation.added",
        None,
    );
    assert_eq!(payload["annotation_index"], 0);
    assert_eq!(payload["annotation"]["text"], "see docs");
}

#[test]
fn custom_tool_input_events_preserve_payload_identity_without_runtime_dispatch() {
    let delta_payload = assert_provider_value_bearing_rig_gap(
        json!({
            "type": "response.custom_tool_call_input.delta",
            "sequence_number": 10,
            "item_id": "item_1",
            "call_id": "call_custom_1",
            "output_index": 2,
            "delta": "*** Begin"
        }),
        "response.custom_tool_call_input.delta",
        Some("call_custom_1"),
    );
    assert_eq!(delta_payload["delta"], "*** Begin");

    let done_payload = assert_provider_value_bearing_rig_gap(
        json!({
            "type": "response.custom_tool_call_input.done",
            "sequence_number": 10,
            "item_id": "item_1",
            "call_id": "call_custom_1",
            "output_index": 2,
            "input": "*** Begin Patch\n*** End Patch\n"
        }),
        "response.custom_tool_call_input.done",
        Some("call_custom_1"),
    );
    assert_eq!(done_payload["input"], "*** Begin Patch\n*** End Patch\n");
}

#[test]
fn documented_overlay_and_noop_rig_failures_do_not_become_invalid_payload() {
    assert_eq!(
        event_fixture(json!({
            "type": "response.reasoning_text.delta",
            "sequence_number": 1,
            "item_id": "rs_1",
            "output_index": 0,
            "delta": "private chain summary"
        })),
        ResponsesStreamEvent::ReasoningDelta {
            delta: "private chain summary".to_string(),
            source: ReasoningSource::Continuation,
        }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.reasoning_content.delta",
            "sequence_number": 2,
            "item_id": "rs_1",
            "output_index": 0,
            "delta": "provider reasoning content"
        })),
        ResponsesStreamEvent::ReasoningDelta {
            delta: "provider reasoning content".to_string(),
            source: ReasoningSource::Continuation,
        }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.reasoning_text.done",
            "sequence_number": 3,
            "item_id": "rs_1",
            "output_index": 0
        })),
        ResponsesStreamEvent::Unknown
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.reasoning_text.done",
            "sequence_number": 4,
            "item_id": "rs_1",
            "output_index": 0,
            "text": "final reasoning text"
        })),
        ResponsesStreamEvent::ReasoningDelta {
            delta: "final reasoning text".to_string(),
            source: ReasoningSource::Continuation,
        }
    );
}

#[test]
fn overlay_conversion_shapes_remain_strict() {
    assert!(
        ResponsesStreamAdapter::parse_sse_data(
            &json!({
                "type": "response.reasoning_text.delta",
                "sequence_number": 1,
                "delta": 42
            })
            .to_string()
        )
        .is_err()
    );

    assert!(
        ResponsesStreamAdapter::parse_sse_data(
            &json!({
                "type": "response.not_a_real_event",
                "sequence_number": 1
            })
            .to_string()
        )
        .is_err()
    );
}

#[test]
fn stream_adapter_parses_lifecycle_text_refusal_reasoning_and_usage_fixtures() {
    assert_eq!(
        event_fixture(json!({
            "type": "response.created",
            "sequence_number": 0,
            "response": {
                "id": "resp_1",
                "object": "response",
                "created_at": 1,
                "status": "in_progress",
                "error": null,
                "incomplete_details": null,
                "instructions": null,
                "max_output_tokens": null,
                "model": "gpt-5",
                "usage": null,
                "output": [],
                "tools": []
            }
        })),
        ResponsesStreamEvent::Lifecycle { kind: ResponsesLifecycleEvent::Created }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.output_text.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 1,
            "delta": "hello"
        })),
        ResponsesStreamEvent::TextDelta { delta: "hello".to_string() }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.refusal.delta",
            "item_id": "msg_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 2,
            "delta": "no"
        })),
        ResponsesStreamEvent::RefusalDelta { delta: "no".to_string() }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.reasoning_summary_text.delta",
            "item_id": "rs_1",
            "output_index": 1,
            "summary_index": 0,
            "sequence_number": 3,
            "delta": "thinking"
        })),
        ResponsesStreamEvent::ReasoningDelta {
            delta: "thinking".to_string(),
            source: ReasoningSource::ProviderSummary,
        }
    );

    let completed = event_fixture(json!({
        "type": "response.completed",
        "sequence_number": 4,
        "response": {
            "id": "resp_1",
            "object": "response",
            "created_at": 1,
            "status": "completed",
            "error": null,
            "incomplete_details": null,
            "instructions": null,
            "max_output_tokens": null,
            "model": "gpt-5",
            "usage": {
                "input_tokens": 10,
                "output_tokens": 5,
                "total_tokens": 15
            },
            "output": [],
            "tools": [],
            "vtcode_overlay": "preserved"
        }
    }));

    let ResponsesStreamEvent::CompletedResponse { response } = completed else {
        panic!("expected completed response event");
    };
    assert_eq!(response["id"], "resp_1");
    assert_eq!(response["status"], "completed");
    assert_eq!(response["usage"]["input_tokens"], 10);
    assert_eq!(response["usage"]["output_tokens"], 5);
    assert_eq!(response["usage"]["total_tokens"], 15);
    assert_eq!(response["vtcode_overlay"], "preserved");
}

#[test]
fn provider_error_events_surface_messages() {
    assert_eq!(
        event_fixture(json!({
            "type": "response.failed",
            "sequence_number": 1,
            "response": {
                "id": "resp_failed",
                "object": "response",
                "created_at": 1,
                "status": "failed",
                "error": {
                    "code": "server_error",
                    "message": "backend failed"
                },
                "incomplete_details": null,
                "instructions": null,
                "max_output_tokens": null,
                "model": "gpt-5",
                "usage": null,
                "output": [],
                "tools": []
            }
        })),
        ResponsesStreamEvent::Error { message: "backend failed".to_string() }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.incomplete",
            "sequence_number": 2,
            "response": {
                "id": "resp_incomplete",
                "object": "response",
                "created_at": 1,
                "status": "incomplete",
                "error": {
                    "code": "max_output_tokens",
                    "message": "max output tokens reached"
                },
                "incomplete_details": {"reason": "max_output_tokens"},
                "instructions": null,
                "max_output_tokens": 100,
                "model": "gpt-5",
                "usage": null,
                "output": [],
                "tools": []
            }
        })),
        ResponsesStreamEvent::Error { message: "max output tokens reached".to_string() }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "error",
            "error": {"message": "rate limited"}
        })),
        ResponsesStreamEvent::Error { message: "rate limited".to_string() }
    );
}

#[test]
fn response_failed_minimal_gateway_frame_surfaces_backend_message() {
    assert_eq!(
        event_fixture(json!({
            "type": "response.failed",
            "response": {
                "id": "resp_XXX",
                "object": "response",
                "model": "gpt-5.6-luna",
                "status": "failed",
                "output": [],
                "error": {
                    "code": "rate_limit_exceeded",
                    "message": "Concurrency limit exceeded for account, please retry later"
                }
            }
        })),
        ResponsesStreamEvent::Error {
            message: "Concurrency limit exceeded for account, please retry later".to_string()
        }
    );
}

#[test]
fn response_incomplete_minimal_frame_surfaces_backend_message() {
    assert_eq!(
        event_fixture(json!({
            "type": "response.incomplete",
            "response": {
                "id": "resp_incomplete",
                "object": "response",
                "model": "gpt-5.6-luna",
                "status": "incomplete",
                "output": [],
                "error": {
                    "code": "max_output_tokens",
                    "message": "max output tokens reached"
                }
            }
        })),
        ResponsesStreamEvent::Error { message: "max output tokens reached".to_string() }
    );
}

#[test]
fn lifecycle_minimal_frames_surface_lifecycle_events() {
    assert_eq!(
        event_fixture(json!({
            "type": "response.created",
            "response": {
                "id": "resp_created",
                "object": "response",
                "model": "gpt-5.6-luna",
                "status": "in_progress",
                "output": []
            }
        })),
        ResponsesStreamEvent::Lifecycle { kind: ResponsesLifecycleEvent::Created }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.in_progress",
            "response": {
                "id": "resp_in_progress",
                "object": "response",
                "model": "gpt-5.6-luna",
                "status": "in_progress",
                "output": []
            }
        })),
        ResponsesStreamEvent::Lifecycle { kind: ResponsesLifecycleEvent::InProgress }
    );
}

#[test]
fn response_failed_without_error_falls_back_to_unknown_message() {
    assert_eq!(
        event_fixture(json!({
            "type": "response.failed",
            "response": {
                "id": "resp_failed",
                "object": "response",
                "model": "gpt-5.6-luna",
                "status": "failed",
                "output": []
            }
        })),
        ResponsesStreamEvent::Error {
            message: "Unknown error from Responses API".to_string()
        }
    );
}

#[test]
fn function_call_stream_preserves_call_id_across_start_delta_and_completion() {
    assert_eq!(
        event_fixture(json!({
            "type": "response.output_item.added",
            "item_id": "fc_1",
            "output_index": 0,
            "sequence_number": 1,
            "item": {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "search_workspace",
                "arguments": "",
                "status": "in_progress"
            }
        })),
        ResponsesStreamEvent::FunctionCallNameDelta {
            call_id: "call_1".to_string(),
            item_id: Some("fc_1".to_string()),
            name: "search_workspace".to_string(),
            output_index: Some(0)
        }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_1",
            "output_index": 0,
            "content_index": 0,
            "sequence_number": 2,
            "call_id": "call_1",
            "delta": "{\"query\":\"vtcode\"}"
        })),
        ResponsesStreamEvent::FunctionCallArgumentsDelta {
            call_id: "call_1".to_string(),
            item_id: Some("fc_1".to_string()),
            delta: "{\"query\":\"vtcode\"}".to_string(),
            output_index: Some(0)
        }
    );

    assert_eq!(
        event_fixture(json!({
            "type": "response.output_item.done",
            "item_id": "fc_1",
            "output_index": 0,
            "sequence_number": 3,
            "item": {
                "type": "function_call",
                "id": "fc_1",
                "call_id": "call_1",
                "name": "search_workspace",
                "arguments": "{\"query\":\"vtcode\"}",
                "status": "completed"
            }
        })),
        ResponsesStreamEvent::CompletedToolCall {
            call_id: "call_1".to_string(),
            item_id: Some("fc_1".to_string()),
            name: "search_workspace".to_string(),
            arguments: String::new(),
            output_index: Some(0)
        }
    );
}
