//! Open Responses Specification Compliance Tests.
//!
//! This test suite validates that VT Code's Open Responses implementation
//! conforms to the requirements defined in the specification.

use serde_json::json;
use vtcode_core::open_responses::{
    IncompleteReason, ItemStatus, MessageRole, OpenResponseError, OpenResponseErrorCode,
    OutputItem, Request, Response, ResponseBuilder, ResponseStatus, VecStreamEmitter,
    is_valid_extension_type,
};
use vtcode_core::{
    AgentMessageItem, ItemCompletedEvent, ItemStartedEvent, ItemUpdatedEvent, McpToolCallItem,
    McpToolCallStatus, ReasoningItem as ExecReasoningItem, ThreadEvent, ThreadItem,
    ThreadItemDetails, ThreadStartedEvent, TurnCompletedEvent, Usage,
};

#[test]
fn test_response_object_compliance() {
    let mut response = Response::new("resp_test", "gpt-5");

    // 1. Check mandatory fields
    assert_eq!(response.object, "response");
    assert_eq!(response.model, "gpt-5");
    assert_eq!(response.status, ResponseStatus::InProgress);
    assert!(response.created_at > 0);

    // 2. Simulate success
    response.complete();
    assert_eq!(response.status, ResponseStatus::Completed);
    assert!(response.completed_at.is_some());

    // 3. Serialize and check structure
    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["object"], "response");
    assert_eq!(json["status"], "completed");
    assert!(json.get("output").is_some(), "output field must be present");
}

#[test]
fn test_item_state_machine_compliance() {
    // Items MUST follow the state machine: in_progress -> (completed | failed | incomplete)

    // Message Item
    let mut msg = OutputItem::message("m1", MessageRole::Assistant, vec![]);
    assert_eq!(msg.status(), ItemStatus::InProgress);

    // Complete it
    if let OutputItem::Message(ref mut m) = msg {
        m.status = ItemStatus::Completed;
    }
    assert!(msg.status().is_terminal());
    assert!(msg.status().is_success());

    // Function Call Item
    let fc = OutputItem::function_call("fc1", "read_file", json!({"path": "test.txt"}));
    assert_eq!(fc.status(), ItemStatus::InProgress);
}

#[test]
fn test_streaming_event_sequence_compliance() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    // 1. Response Created
    builder.process_event(
        &ThreadEvent::ThreadStarted(ThreadStartedEvent {
            thread_id: "t1".to_string(),
        }),
        &mut emitter,
    );

    // 2. Item Started
    let item = ThreadItem {
        id: "msg_1".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
            text: "Hello".to_string(),
        }),
    };
    builder.process_event(
        &ThreadEvent::ItemStarted(ItemStartedEvent { item: item.clone() }),
        &mut emitter,
    );

    // 3. Text Delta
    let updated_item = ThreadItem {
        id: "msg_1".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
            text: "Hello world".to_string(),
        }),
    };
    builder.process_event(
        &ThreadEvent::ItemUpdated(ItemUpdatedEvent { item: updated_item }),
        &mut emitter,
    );

    // 4. Item Completed
    let final_item = ThreadItem {
        id: "msg_1".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem {
            text: "Hello world!".to_string(),
        }),
    };
    builder.process_event(
        &ThreadEvent::ItemCompleted(ItemCompletedEvent { item: final_item }),
        &mut emitter,
    );

    // 5. Response Completed
    builder.process_event(
        &ThreadEvent::TurnCompleted(TurnCompletedEvent {
            usage: Usage::default(),
        }),
        &mut emitter,
    );

    let events = emitter.into_events();

    // Validate sequence
    let types: Vec<&str> = events.iter().map(|e| e.event_type()).collect();

    // Minimum expected sequence for a single message response
    assert!(types.contains(&"response.created"));
    assert!(types.contains(&"response.in_progress"));
    assert!(types.contains(&"response.output_item.added"));
    assert!(types.contains(&"response.output_text.delta"));
    assert!(types.contains(&"response.output_text.done"));
    assert!(types.contains(&"response.output_item.done"));
    assert!(types.contains(&"response.completed"));

    // Check ordering (simplified)
    let created_idx = types.iter().position(|&t| t == "response.created").unwrap();
    let completed_idx = types
        .iter()
        .position(|&t| t == "response.completed")
        .unwrap();
    assert!(created_idx < completed_idx);
}

#[test]
fn test_request_object_compliance() {
    let mut req = Request::from_message("gpt-5", "Hello");
    req.temperature = Some(0.7);
    req.stream = true;

    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["model"], "gpt-5");
    assert_eq!(json["temperature"], 0.7);
    assert_eq!(json["stream"], true);
    assert!(json["input"].is_array());
    assert_eq!(json["input"][0]["type"], "message");
}

#[test]
fn test_incomplete_response_compliance() {
    let mut response = Response::new("resp_inc", "gpt-5");

    response.incomplete(IncompleteReason::MaxOutputTokens);

    assert_eq!(response.status, ResponseStatus::Incomplete);
    assert!(response.incomplete_details.is_some());
    assert_eq!(
        response.incomplete_details.as_ref().unwrap().reason,
        IncompleteReason::MaxOutputTokens
    );

    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["status"], "incomplete");
    assert_eq!(json["incomplete_details"]["reason"], "max_output_tokens");
}

#[test]
fn test_custom_extension_compliance() {
    // Extensions MUST be prefixed with "slug:"
    assert!(is_valid_extension_type("vtcode:file_change"));
    assert!(is_valid_extension_type("acme:search"));
    assert!(!is_valid_extension_type("invalid_type")); // No prefix
}

#[test]
fn test_error_object_compliance() {
    let err = OpenResponseError::invalid_param("model", "Model not found")
        .with_code(OpenResponseErrorCode::InvalidModel);

    let json = serde_json::to_value(&err).unwrap();
    assert_eq!(json["type"], "invalid_request");
    assert_eq!(json["code"], "invalid_model");
    assert_eq!(json["param"], "model");
    assert_eq!(json["message"], "Model not found");
}

#[test]
fn test_reasoning_item_compliance() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    let item = ThreadItem {
        id: "r1".to_string(),
        details: ThreadItemDetails::Reasoning(ExecReasoningItem {
            text: "Thinking...".to_string(),
            stage: None,
        }),
    };

    builder.process_event(
        &ThreadEvent::ItemStarted(ItemStartedEvent { item: item.clone() }),
        &mut emitter,
    );

    builder.process_event(
        &ThreadEvent::ItemCompleted(ItemCompletedEvent { item }),
        &mut emitter,
    );

    let response = builder.build();
    assert_eq!(response.output.len(), 1);
    assert!(matches!(response.output[0], OutputItem::Reasoning(_)));

    let events = emitter.into_events();
    assert!(
        events
            .iter()
            .any(|e| e.event_type() == "response.reasoning.done")
    );
}

#[test]
fn test_tool_call_compliance() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    let item = ThreadItem {
        id: "fc1".to_string(),
        details: ThreadItemDetails::McpToolCall(McpToolCallItem {
            tool_name: "read_file".to_string(),
            arguments: Some(json!({"path": "README.md"})),
            result: None,
            status: Some(McpToolCallStatus::Completed),
        }),
    };

    builder.process_event(
        &ThreadEvent::ItemCompleted(ItemCompletedEvent { item }),
        &mut emitter,
    );

    let response = builder.build();
    if let OutputItem::FunctionCall(fc) = &response.output[0] {
        assert_eq!(fc.name, "read_file");
        assert_eq!(fc.arguments["path"], "README.md");
    } else {
        panic!("Expected FunctionCall item");
    }
}
