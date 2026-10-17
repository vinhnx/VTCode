use super::*;
use crate::open_responses::{ResponseStreamEvent, events::VecStreamEmitter};
use crate::provider::{FinishReason, LLMResponse, NormalizedStreamEvent, ReasoningSource, ToolCall};
use serde_json::json;
use vtcode_exec_events::{
    AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, ItemCompletedEvent, ItemStartedEvent,
    PlanApprovalDecision, PlanApprovalRequestedEvent, PlanApprovalResolvedEvent, PlanItem, ThreadStartedEvent,
    ToolCallStatus, ToolInvocationItem, ToolOutputItem, TurnCompletedEvent, Usage,
};

#[test]
fn test_response_builder_thread_lifecycle() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    // Thread started
    builder.process_event(
        &ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread_1".to_string() }),
        &mut emitter,
    );

    assert_eq!(builder.response().status, ResponseStatus::InProgress);

    // Turn completed
    builder.process_event(
        &ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage {
                input_tokens: 100,
                cached_input_tokens: 50,
                cache_creation_tokens: 0,
                output_tokens: 25,
            },
            in_progress_exec_sessions: Vec::new(),
        }),
        &mut emitter,
    );

    assert_eq!(builder.response().status, ResponseStatus::Completed);
    assert!(builder.response().usage.is_some());

    let events = emitter.into_events();
    assert!(events.iter().any(|e| matches!(e, ResponseStreamEvent::ResponseCreated { .. })));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ResponseStreamEvent::ResponseCompleted { .. }))
    );
}

#[test]
fn test_response_builder_message_item() {
    let mut builder = ResponseBuilder::new("claude-3");
    let mut emitter = VecStreamEmitter::new();

    // Item started
    let item = ThreadItem {
        context: None,
        id: "msg_1".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Hello".to_string() }),
    };
    builder.process_event(&ThreadEvent::ItemStarted(ItemStartedEvent { item: item.clone() }), &mut emitter);

    // Item completed
    let completed_item = ThreadItem {
        context: None,
        id: "msg_1".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Hello, world!".to_string() }),
    };
    builder.process_event(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item: completed_item }), &mut emitter);

    assert_eq!(builder.response().output.len(), 1);
    assert!(matches!(&builder.response().output[0], OutputItem::Message(_)));

    let events = emitter.into_events();
    assert!(events.iter().any(|e| matches!(e, ResponseStreamEvent::OutputItemAdded { .. })));
    assert!(events.iter().any(|e| matches!(e, ResponseStreamEvent::OutputItemDone { .. })));
    // Verify ContentPartAdded is emitted
    assert!(events.iter().any(|e| matches!(e, ResponseStreamEvent::ContentPartAdded { .. })));
    // Verify OutputTextDone is emitted
    assert!(events.iter().any(|e| matches!(e, ResponseStreamEvent::OutputTextDone { .. })));
}

#[test]
fn test_atomic_completion_emits_added_and_done() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    // Complete item without prior start (atomic)
    let item = ThreadItem {
        context: None,
        id: "msg_atomic".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Atomic message".to_string() }),
    };
    builder.process_event(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item }), &mut emitter);

    let events = emitter.into_events();
    // Must emit Added before Done for atomic completions
    let added_pos = events
        .iter()
        .position(|e| matches!(e, ResponseStreamEvent::OutputItemAdded { .. }));
    let done_pos = events
        .iter()
        .position(|e| matches!(e, ResponseStreamEvent::OutputItemDone { .. }));

    assert!(added_pos.is_some(), "OutputItemAdded should be emitted");
    assert!(done_pos.is_some(), "OutputItemDone should be emitted");
    assert!(added_pos.unwrap() < done_pos.unwrap(), "Added must come before Done");
}

#[test]
fn test_update_without_start_handles_implicit_start() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    // Update without prior start
    let item = ThreadItem {
        context: None,
        id: "msg_implicit".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Hello".to_string() }),
    };
    builder.process_event(&ThreadEvent::ItemUpdated(vtcode_exec_events::ItemUpdatedEvent { item }), &mut emitter);

    let events = emitter.into_events();
    // Should have implicitly started
    assert!(events.iter().any(|e| matches!(e, ResponseStreamEvent::OutputItemAdded { .. })));
}

#[test]
fn test_unicode_delta_safety() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    // Start with emoji
    let item1 = ThreadItem {
        context: None,
        id: "msg_unicode".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Hello 👋".to_string() }),
    };
    builder.process_event(&ThreadEvent::ItemStarted(ItemStartedEvent { item: item1 }), &mut emitter);

    // Update with more content
    let item2 = ThreadItem {
        context: None,
        id: "msg_unicode".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Hello 👋 World 🌍".to_string() }),
    };
    builder
        .process_event(&ThreadEvent::ItemUpdated(vtcode_exec_events::ItemUpdatedEvent { item: item2 }), &mut emitter);

    // Should not panic and should emit delta
    let events = emitter.into_events();
    assert!(events.iter().any(|e| matches!(e, ResponseStreamEvent::OutputTextDelta { .. })));
}

#[test]
fn test_non_append_update_fallback() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    // Start with some text
    let item1 = ThreadItem {
        context: None,
        id: "msg_edit".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Original text".to_string() }),
    };
    builder.process_event(&ThreadEvent::ItemStarted(ItemStartedEvent { item: item1 }), &mut emitter);

    // Update with completely different text (non-append)
    let item2 = ThreadItem {
        context: None,
        id: "msg_edit".to_string(),
        details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: "Completely different".to_string() }),
    };
    builder
        .process_event(&ThreadEvent::ItemUpdated(vtcode_exec_events::ItemUpdatedEvent { item: item2 }), &mut emitter);

    // Should fallback to emitting full text as delta
    let events = emitter.into_events();
    let delta_event = events.iter().find(|e| {
        matches!(
            e,
            ResponseStreamEvent::OutputTextDelta { delta, .. } if delta == "Completely different"
        )
    });
    assert!(delta_event.is_some(), "Should emit full text as delta for non-append updates");
}

#[test]
fn test_plan_item_maps_to_custom_output() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    let item = ThreadItem {
        context: None,
        id: "plan_1".to_string(),
        details: ThreadItemDetails::Plan(PlanItem { text: "- Step 1\n- Step 2".to_string() }),
    };
    builder.process_event(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item }), &mut emitter);

    assert_eq!(builder.response().output.len(), 1);
    match &builder.response().output[0] {
        OutputItem::Custom(custom) => {
            assert_eq!(custom.custom_type, "vtcode:plan");
            assert_eq!(custom.data["text"], "- Step 1\n- Step 2");
        }
        _ => panic!("expected custom output for plan item"),
    }
}

#[test]
fn test_plan_approval_events_map_to_custom_extensions() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    builder.process_event(
        &ThreadEvent::PlanApprovalRequested(PlanApprovalRequestedEvent {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-1".to_string(),
            plan_file: Some(".vtcode/plans/task.md".to_string()),
        }),
        &mut emitter,
    );
    builder.process_event(
        &ThreadEvent::PlanApprovalResolved(PlanApprovalResolvedEvent {
            thread_id: "thread-1".to_string(),
            turn_id: "turn-2".to_string(),
            decision: PlanApprovalDecision::Execute,
            automatic: false,
        }),
        &mut emitter,
    );

    let events = emitter.into_events();
    let custom_events = events
        .iter()
        .filter_map(|event| match event {
            ResponseStreamEvent::CustomEvent { event_type, data, .. } => Some((event_type.as_str(), data)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(custom_events.len(), 2);
    assert_eq!(custom_events[0].0, "vtcode.plan_approval_requested");
    assert_eq!(custom_events[0].1["plan_file"], ".vtcode/plans/task.md");
    assert_eq!(custom_events[1].0, "vtcode.plan_approval_resolved");
    assert_eq!(custom_events[1].1["decision"], "execute");
    assert_eq!(custom_events[1].1["automatic"], false);
}

#[test]
fn test_exec_command_invocation_preserves_public_name_and_arguments() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    let arguments = json!({
        "cmd": "git status --short",
        "workdir": "/workspace",
        "yield_time_ms": 1000,
        "max_output_tokens": 2000,
        "tty": false
    });

    let item = ThreadItem {
        context: None,
        id: "tool_1".to_string(),
        details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: "exec_command".to_string(),
            arguments: Some(arguments.clone()),
            tool_call_id: Some("tool_call_0".to_string()),
            status: ToolCallStatus::Completed,
            outcome: None,
        })),
    };

    builder.process_event(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item }), &mut emitter);

    match &builder.response().output[0] {
        OutputItem::FunctionCall(call) => {
            assert_eq!(call.name, "exec_command");
            assert_eq!(call.arguments, arguments);
            assert_eq!(call.id.as_ref(), "tool_1");
            assert_eq!(call.status, ItemStatus::Completed);
            assert_eq!(call.call_id.as_deref(), Some("tool_call_0"));
        }
        other => panic!("expected function call, got {other:?}"),
    }
}

#[test]
fn test_write_stdin_invocation_preserves_public_name_and_arguments() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    let arguments = json!({
        "session_id": "session-1",
        "chars": "",
        "yield_time_ms": 250,
        "max_output_tokens": 512
    });
    let item = ThreadItem {
        context: None,
        id: "tool_2".to_string(),
        details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: "write_stdin".to_string(),
            arguments: Some(arguments.clone()),
            tool_call_id: Some("tool_call_1".to_string()),
            status: ToolCallStatus::InProgress,
            outcome: None,
        })),
    };

    builder.process_event(&ThreadEvent::ItemStarted(ItemStartedEvent { item }), &mut emitter);

    match &builder.response().output[0] {
        OutputItem::FunctionCall(call) => {
            assert_eq!(call.name, "write_stdin");
            assert_eq!(call.arguments, arguments);
            assert!(call.arguments.get("cmd").is_none());
            assert_eq!(call.id.as_ref(), "tool_2");
            assert_eq!(call.status, ItemStatus::InProgress);
            assert_eq!(call.call_id.as_deref(), Some("tool_call_1"));
        }
        other => panic!("expected function call, got {other:?}"),
    }
}

#[test]
fn test_legacy_shell_alias_is_preserved_without_argument_adaptation() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    let arguments = json!({
        "command": ["git", "status"]
    });
    let item = ThreadItem {
        context: None,
        id: "tool_legacy".to_string(),
        details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: "shell".to_string(),
            arguments: Some(arguments.clone()),
            tool_call_id: Some("tool_call_legacy".to_string()),
            status: ToolCallStatus::Completed,
            outcome: None,
        })),
    };

    builder.process_event(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item }), &mut emitter);

    match &builder.response().output[0] {
        OutputItem::FunctionCall(call) => {
            assert_eq!(call.name, "shell");
            assert_eq!(call.arguments, arguments);
            assert_ne!(call.name, "exec_command");
        }
        other => panic!("expected function call, got {other:?}"),
    }
}

#[test]
fn test_tool_output_updates_stream_as_function_call_output() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    builder.process_event(
        &ThreadEvent::ItemStarted(ItemStartedEvent {
            item: ThreadItem {
                context: None,
                id: "tool_1:output".to_string(),
                details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                    call_id: "tool_1".to_string(),
                    tool_call_id: Some("tool_call_0".to_string()),
                    spool_path: None,
                    output: String::new(),
                    exit_code: None,
                    status: ToolCallStatus::InProgress,
                })),
            },
        }),
        &mut emitter,
    );
    builder.process_event(
        &ThreadEvent::ItemUpdated(vtcode_exec_events::ItemUpdatedEvent {
            item: ThreadItem {
                context: None,
                id: "tool_1:output".to_string(),
                details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                    call_id: "tool_1".to_string(),
                    tool_call_id: Some("tool_call_0".to_string()),
                    spool_path: None,
                    output: "On branch".to_string(),
                    exit_code: None,
                    status: ToolCallStatus::InProgress,
                })),
            },
        }),
        &mut emitter,
    );
    builder.process_event(
        &ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: "tool_1:output".to_string(),
                details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                    call_id: "tool_1".to_string(),
                    tool_call_id: Some("tool_call_0".to_string()),
                    spool_path: None,
                    output: "On branch main".to_string(),
                    exit_code: Some(0),
                    status: ToolCallStatus::Completed,
                })),
            },
        }),
        &mut emitter,
    );

    match &builder.response().output[0] {
        OutputItem::FunctionCallOutput(output) => {
            assert_eq!(output.call_id.as_deref(), Some("tool_call_0"));
            assert_eq!(output.output, "On branch main");
        }
        other => panic!("expected function call output, got {other:?}"),
    }

    let events = emitter.into_events();
    assert!(events.iter().any(|event| matches!(
        event,
        ResponseStreamEvent::OutputItemAdded { item: OutputItem::FunctionCallOutput(_), .. }
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ResponseStreamEvent::OutputTextDelta { delta, .. } if delta == "On branch"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ResponseStreamEvent::OutputTextDone { text, .. } if text == "On branch main"
    )));
}

#[test]
fn test_tool_output_falls_back_to_harness_call_id_without_raw_tool_call_id() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    builder.process_event(
        &ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: "tool_1:output".to_string(),
                details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                    call_id: "tool_1".to_string(),
                    tool_call_id: None,
                    spool_path: None,
                    output: "done".to_string(),
                    exit_code: Some(0),
                    status: ToolCallStatus::Completed,
                })),
            },
        }),
        &mut emitter,
    );

    match &builder.response().output[0] {
        OutputItem::FunctionCallOutput(output) => {
            assert_eq!(output.call_id.as_deref(), Some("tool_1"));
            assert_eq!(output.output, "done");
        }
        other => panic!("expected function call output, got {other:?}"),
    }
}

#[test]
fn test_tool_output_uses_spool_reference_when_inline_output_is_empty() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    builder.process_event(
        &ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: "tool_1:output".to_string(),
                details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                    call_id: "tool_1".to_string(),
                    tool_call_id: Some("tool_call_0".to_string()),
                    spool_path: Some(".vtcode/context/tool_outputs/run-1.txt".to_string()),
                    output: String::new(),
                    exit_code: Some(0),
                    status: ToolCallStatus::Completed,
                })),
            },
        }),
        &mut emitter,
    );

    match &builder.response().output[0] {
        OutputItem::FunctionCallOutput(output) => {
            assert_eq!(output.output, "Output saved to .vtcode/context/tool_outputs/run-1.txt");
        }
        other => panic!("expected function call output, got {other:?}"),
    }
}

#[test]
fn test_reused_raw_tool_call_id_falls_back_to_harness_id_for_later_pair() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    for item in [
        ThreadItem {
            context: None,
            id: "tool_1".to_string(),
            details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "exec_command".to_string(),
                arguments: Some(json!({ "command": ["cargo", "check"] })),
                tool_call_id: Some("tool_call_0".to_string()),
                status: ToolCallStatus::Completed,
                outcome: None,
            })),
        },
        ThreadItem {
            context: None,
            id: "tool_2".to_string(),
            details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
                tool_name: "exec_command".to_string(),
                arguments: Some(json!({ "command": ["cargo", "test"] })),
                tool_call_id: Some("tool_call_0".to_string()),
                status: ToolCallStatus::Completed,
                outcome: None,
            })),
        },
        ThreadItem {
            context: None,
            id: "tool_2:output".to_string(),
            details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
                call_id: "tool_2".to_string(),
                tool_call_id: Some("tool_call_0".to_string()),
                spool_path: None,
                output: "ok".to_string(),
                exit_code: Some(0),
                status: ToolCallStatus::Completed,
            })),
        },
    ] {
        builder.process_event(&ThreadEvent::ItemCompleted(ItemCompletedEvent { item }), &mut emitter);
    }

    match &builder.response().output[0] {
        OutputItem::FunctionCall(call) => {
            assert_eq!(call.call_id.as_deref(), Some("tool_call_0"));
        }
        other => panic!("expected function call, got {other:?}"),
    }

    match &builder.response().output[1] {
        OutputItem::FunctionCall(call) => {
            assert_eq!(call.call_id.as_deref(), Some("tool_2"));
        }
        other => panic!("expected function call, got {other:?}"),
    }

    match &builder.response().output[2] {
        OutputItem::FunctionCallOutput(output) => {
            assert_eq!(output.call_id.as_deref(), Some("tool_2"));
        }
        other => panic!("expected function call output, got {other:?}"),
    }
}

#[test]
fn test_command_execution_maps_to_custom_extension() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    builder.process_event(
        &ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: "cmd_1".to_string(),
                details: ThreadItemDetails::CommandExecution(Box::new(CommandExecutionItem {
                    command: "git status".to_string(),
                    arguments: Some(json!({ "cwd": "/repo" })),
                    aggregated_output: "On branch main".to_string(),
                    exit_code: Some(0),
                    status: CommandExecutionStatus::Completed,
                })),
            },
        }),
        &mut emitter,
    );

    match &builder.response().output[0] {
        OutputItem::Custom(custom) => {
            assert_eq!(custom.custom_type, "vtcode:command_execution");
            assert_eq!(custom.data["command"], "git status");
            assert_eq!(custom.data["exit_code"], 0);
            assert_eq!(custom.data["status"], "completed");
        }
        other => panic!("expected custom output, got {other:?}"),
    }
}

#[test]
fn test_failed_response_ignores_late_completion() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    builder.process_event(
        &ThreadEvent::ThreadStarted(ThreadStartedEvent { thread_id: "thread_1".to_string() }),
        &mut emitter,
    );
    builder.process_event(
        &ThreadEvent::TurnFailed(vtcode_exec_events::TurnFailedEvent {
            completed_at: None,
            message: "boom".to_string(),
            usage: None,
        }),
        &mut emitter,
    );
    builder.process_event(
        &ThreadEvent::TurnCompleted(TurnCompletedEvent {
            completed_at: None,
            usage: Usage::default(),
            in_progress_exec_sessions: Vec::new(),
        }),
        &mut emitter,
    );

    assert_eq!(builder.response().status, ResponseStatus::Failed);
    let events = emitter.into_events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ResponseStreamEvent::ResponseFailed { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ResponseStreamEvent::ResponseCompleted { .. }))
    );
}

#[test]
fn test_response_builder_consumes_normalized_stream_events() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    for event in [
        NormalizedStreamEvent::TextDelta { delta: "Hello ".to_string() },
        NormalizedStreamEvent::ReasoningDelta {
            delta: "Thinking".to_string(),
            source: ReasoningSource::ProviderSummary,
        },
        NormalizedStreamEvent::ToolCallStart {
            call_id: "call_1".to_string(),
            name: Some("code_search".to_string()),
        },
        NormalizedStreamEvent::ToolCallDelta {
            call_id: "call_1".to_string(),
            delta: "{\"query\":\"phase\"}".to_string(),
        },
        NormalizedStreamEvent::Usage {
            usage: crate::provider::Usage {
                prompt_tokens: 10,
                completion_tokens: 4,
                total_tokens: 14,
                cached_prompt_tokens: None,
                cache_creation_tokens: None,
                cache_read_tokens: None,
                iterations: None,
            },
        },
        NormalizedStreamEvent::Done {
            response: Box::new(LLMResponse {
                content: Some("Hello world".to_string()),
                model: "gpt-5".to_string(),
                tool_calls: Some(vec![ToolCall::function(
                    "call_1".to_string(),
                    "code_search".to_string(),
                    "{\"query\":\"phase\"}".to_string(),
                )]),
                usage: None,
                finish_reason: FinishReason::ToolCalls,
                reasoning: Some("Thinking".to_string()),
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            }),
        },
    ] {
        builder.process_normalized_event(&event, &mut emitter);
    }

    assert_eq!(builder.response().status, ResponseStatus::Completed);
    assert_eq!(builder.response().usage.as_ref().map(|usage| usage.total_tokens), Some(14));
    assert_eq!(builder.response().output.len(), 3);

    let events = emitter.into_events();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ResponseStreamEvent::ResponseCreated { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        ResponseStreamEvent::OutputTextDelta { delta, .. } if delta == "Hello "
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ResponseStreamEvent::ReasoningDelta { delta, .. } if delta == "Thinking"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ResponseStreamEvent::FunctionCallArgumentsDelta { delta, .. } if delta == "{\"query\":\"phase\"}"
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ResponseStreamEvent::ResponseCompleted { .. }))
    );
}

#[test]
fn normalized_bridge_does_not_expose_completed_raw_reasoning() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    builder.process_normalized_event(
        &NormalizedStreamEvent::ReasoningDelta {
            delta: "private continuation".to_string(),
            source: ReasoningSource::Continuation,
        },
        &mut emitter,
    );
    builder.process_normalized_event(
        &NormalizedStreamEvent::Done {
            response: Box::new(LLMResponse {
                model: "gpt-5".to_string(),
                finish_reason: FinishReason::Stop,
                reasoning: Some("private continuation".to_string()),
                ..Default::default()
            }),
        },
        &mut emitter,
    );

    assert!(
        !builder
            .response()
            .output
            .iter()
            .any(|item| matches!(item, OutputItem::Reasoning(_)))
    );
    assert!(
        !emitter
            .into_events()
            .iter()
            .any(|event| matches!(event, ResponseStreamEvent::ReasoningDelta { .. }))
    );
}

#[test]
fn test_response_builder_marks_length_finish_as_incomplete() {
    let mut builder = ResponseBuilder::new("gpt-5");
    let mut emitter = VecStreamEmitter::new();

    builder.process_normalized_event(
        &NormalizedStreamEvent::Done {
            response: Box::new(LLMResponse {
                content: Some("truncated".to_string()),
                model: "gpt-5".to_string(),
                tool_calls: None,
                usage: None,
                finish_reason: FinishReason::Length,
                reasoning: None,
                reasoning_details: None,
                organization_id: None,
                request_id: None,
                tool_references: Vec::new(),
                compaction: None,
            }),
        },
        &mut emitter,
    );

    assert_eq!(builder.response().status, ResponseStatus::Incomplete);
    assert!(
        emitter
            .into_events()
            .iter()
            .any(|event| matches!(event, ResponseStreamEvent::ResponseIncomplete { .. }))
    );
}
