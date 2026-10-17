use super::*;
use crate::config::VTCodeConfig;
use crate::llm::provider::{LLMError, LLMProvider, LLMRequest, LLMResponse, MessageRole, ToolCall};

struct CapacityProvider(usize);

#[test]
fn clearing_retains_small_arguments_at_the_byte_boundary() {
    for (bytes, cleared) in [(1024, false), (1025, true)] {
        let mut messages = batch_history();
        let arguments = serde_json::json!({"cmd": "x".repeat(bytes - 10)}).to_string();
        assert_eq!(arguments.len(), bytes);
        let call = &mut messages[1].tool_calls.as_mut().unwrap()[0];
        call.function.as_mut().unwrap().arguments = arguments.clone();
        call.text = Some("y".repeat(bytes));
        let projected = clear_old_tool_results(&messages, 1, 1, 1, true);
        assert!(projected[3].content.as_text().contains("\"cleared\":\"tool_result\""));
        let call = &projected[1].tool_calls.as_ref().unwrap()[0];
        assert_eq!(
            call.function.as_ref().unwrap().arguments,
            if cleared {
                CLEARED_TOOL_INPUT_PLACEHOLDER
            } else {
                &arguments
            }
        );
        assert_eq!(
            call.text.as_deref().unwrap(),
            if cleared {
                CLEARED_TOOL_INPUT_PLACEHOLDER.to_owned()
            } else {
                "y".repeat(bytes)
            }
        );
        assert!(call.thought_signature.is_none());
    }
}

#[test]
fn recent_evidence_window_stays_bounded_when_durable_history_grows() {
    for count in [12, 120] {
        let mut messages = vec![Message::user("earlier context ".repeat(4000))];
        for index in 0..count {
            let mut assistant = Message::assistant(String::new());
            assistant.tool_calls = Some(vec![ToolCall::function(
                format!("call_{index}"),
                "exec_command".into(),
                serde_json::json!({"cmd": format!("sed -n '{index}p' README.md")}).to_string(),
            )]);
            messages.push(assistant);
            messages.push(Message::tool_response(format!("call_{index}"), format!("source_{index} ").repeat(300)));
        }
        let projected = clear_old_tool_results_with_recent_batches(&messages, 8000, 1, 1, 1, true);
        let retained: Vec<_> = projected
            .iter()
            .filter(|message| {
                message.role == MessageRole::Tool && !message.content.as_text().contains("\"cleared\":\"tool_result\"")
            })
            .collect();
        assert!(retained.len() > 2, "retain more than two short tool batches");
        assert!(retained.len() < count, "old evidence is still reclaimed");
        assert!(retained.iter().map(|message| message.estimate_tokens()).sum::<usize>() <= 4000);
        assert_eq!(retained.last().unwrap().tool_call_id, Some(format!("call_{}", count - 1)));
        assert!(validate_history_invariants(&projected).is_valid());
        assert_eq!(clear_old_tool_results_with_recent_batches(&projected, 8000, 1, 1, 1, true), projected);
    }
}

#[async_trait::async_trait]
impl LLMProvider for CapacityProvider {
    fn name(&self) -> &str {
        "clearing-test"
    }

    fn effective_context_size(&self, _model: &str) -> usize {
        self.0
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse::new("unknown-test-model", "unused"))
    }

    fn supported_models(&self) -> Vec<String> {
        Vec::new()
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }
}

#[test]
fn clearing_trigger_intersects_limits_and_subtracts_overhead_and_output() {
    let mut config = VTCodeConfig::default();
    config.context.max_context_tokens = 32_000;
    let resolve = |config: &VTCodeConfig, capacity, overhead, output| {
        resolve_tool_result_clearing_trigger(
            config,
            &CapacityProvider(capacity),
            "unknown-test-model",
            overhead,
            output,
        )
    };
    assert_eq!(resolve(&config, 64_000, 2_000, 4_000), 15_600);
    assert_eq!(resolve(&config, 16_000, 2_000, 4_000), 6_000);
    assert_eq!(resolve(&config, 0, 2_000, 4_000), 15_600);
    config.context.max_context_tokens = 0;
    assert_eq!(resolve(&config, 64_000, 2_000, 4_000), 34_800);
    assert_eq!(resolve(&config, 0, usize::MAX, usize::MAX), 40_000);
    assert_eq!(resolve(&config, 1, 2_000, 4_000), 1);
    config.agent.harness.tool_result_clearing.trigger_tokens = 37;
    assert_eq!(resolve(&config, 64_000, 0, 0), 37);
    config.agent.harness.tool_result_clearing.enabled = false;
    assert_eq!(resolve(&config, 64_000, 0, 0), 0);
    config.agent.harness.tool_result_clearing.enabled = true;
    config.agent.harness.tool_result_clearing.trigger_tokens = 0;
    assert_eq!(resolve(&config, 64_000, 0, 0), 0);
    config.agent.harness.tool_result_clearing.trigger_tokens = u64::MAX;
    assert_eq!(resolve(&config, usize::MAX, 0, 0), (usize::MAX as u64 / 5) * 3 + (usize::MAX as u64 % 5) * 3 / 5);
}

fn batch_history() -> Vec<Message> {
    let mut messages = vec![Message::user("Inspect evidence".into())];
    for (batch, count) in [1, 4, 3].into_iter().enumerate() {
        let calls: Vec<_> = (0..count)
            .map(|call| {
                let mut tool_call = ToolCall::function(
                    format!("batch_{batch}_{call}"),
                    "read_file".into(),
                    serde_json::json!({"path": format!("evidence_{batch}_{call}"), "payload": "x".repeat(1024)})
                        .to_string(),
                );
                tool_call.thought_signature = Some(format!("signature_{batch}_{call}"));
                tool_call
            })
            .collect();
        let mut assistant = Message::assistant(format!("Inspect batch {batch}"));
        assistant.tool_calls = Some(calls);
        messages.push(assistant);
        // Reordered completions and system directives must not split batches.
        for call in (0..count).rev() {
            messages.push(Message::system(format!("directive {batch}_{call}")));
            messages.push(Message::tool_response(
                format!("batch_{batch}_{call}"),
                format!("evidence_{batch}_{call} ").repeat(400),
            ));
        }
    }
    messages
}

#[test]
fn clearing_retains_complete_asymmetric_batches_by_call_id() {
    let messages = batch_history();
    for (keep, cleared_ids) in [
        (1, vec!["batch_0_0", "batch_1_3", "batch_1_2", "batch_1_1", "batch_1_0"]),
        (3, vec!["batch_0_0", "batch_1_3", "batch_1_2", "batch_1_1", "batch_1_0"]),
        (4, vec!["batch_0_0"]),
        (7, vec!["batch_0_0"]),
        (8, vec![]),
    ] {
        let projected = clear_old_tool_results(&messages, 1, keep, 1, true);
        let actual: Vec<_> = projected
            .iter()
            .filter(|message| message.content.as_text().contains("\"cleared\":\"tool_result\""))
            .map(|message| message.tool_call_id.as_deref().unwrap())
            .collect();
        assert_eq!(actual, cleared_ids, "keep={keep}");
        assert!(validate_history_invariants(&projected).is_valid());
        for (original, retained) in messages.iter().zip(&projected) {
            assert_eq!(original.role, retained.role);
            assert_eq!(original.tool_call_id, retained.tool_call_id);
            if original.role == MessageRole::Tool && !cleared_ids.contains(&original.tool_call_id.as_deref().unwrap()) {
                assert_eq!(original, retained);
            }
            if let Some(calls) = &retained.tool_calls {
                for (original_call, call) in original.tool_calls.as_ref().unwrap().iter().zip(calls) {
                    if cleared_ids.contains(&call.id.as_str()) {
                        assert_eq!(call.function.as_ref().unwrap().arguments, CLEARED_TOOL_INPUT_PLACEHOLDER);
                    } else {
                        assert_eq!(original_call, call);
                    }
                }
            }
        }
        assert_eq!(clear_old_tool_results(&projected, 1, keep, 1, true), projected);
        assert!(
            projected.iter().map(Message::estimate_tokens).sum::<usize>()
                <= messages.iter().map(Message::estimate_tokens).sum::<usize>()
        );
    }
    assert_eq!(messages, batch_history(), "durable history remains intact");
}

#[test]
fn clearing_preserves_small_results_and_their_inputs_at_trigger_boundary() {
    let mut messages = batch_history();
    let result_index = messages.iter().position(|message| message.role == MessageRole::Tool).unwrap();
    messages[result_index].content = crate::llm::provider::MessageContent::Text("ok".into());
    let tokens = messages.iter().map(Message::estimate_tokens).sum::<usize>() as u64;
    assert_eq!(clear_old_tool_results(&messages, tokens + 1, 1, 1, true), messages);
    assert_eq!(clear_old_tool_results(&messages, 0, 1, 1, true), messages);
    let projected = clear_old_tool_results(&messages, tokens, 1, 1, true);
    assert_eq!(projected[result_index], messages[result_index]);
    assert_eq!(projected[1], messages[1], "small retained output keeps arguments and signature");
    assert!(
        projected
            .iter()
            .any(|message| message.content.as_text().contains("\"cleared\":\"tool_result\""))
    );
}

#[test]
fn clearing_unmatched_results_uses_count_retention() {
    let messages: Vec<_> = (0..3)
        .map(|index| Message::tool_response(format!("orphan_{index}"), "unmatched evidence ".repeat(400)))
        .collect();
    let projected = clear_old_tool_results(&messages, 1, 1, 1, true);
    assert!(projected[0].content.as_text().contains("\"cleared\":\"tool_result\""));
    assert!(projected[1].content.as_text().contains("\"cleared\":\"tool_result\""));
    assert_eq!(projected[2], messages[2]);
}

#[test]
fn clearing_reused_call_id_preserves_newest_origin_inputs() {
    let mut messages = batch_history();
    let first_id = "batch_0_0";
    let last_assistant = messages.iter_mut().rev().find(|message| message.tool_calls.is_some()).unwrap();
    last_assistant.tool_calls.as_mut().unwrap()[0].id = first_id.into();
    let last_result = messages
        .iter_mut()
        .rev()
        .find(|message| message.role == MessageRole::Tool)
        .unwrap();
    last_result.tool_call_id = Some(first_id.into());
    let projected = clear_old_tool_results(&messages, 1, 1, 1, true);
    let retained_assistant = projected.iter().rev().find(|message| message.tool_calls.is_some()).unwrap();
    let original_assistant = messages.iter().rev().find(|message| message.tool_calls.is_some()).unwrap();
    assert_eq!(retained_assistant, original_assistant);
    assert_eq!(
        projected[1].tool_calls.as_ref().unwrap()[0]
            .function
            .as_ref()
            .unwrap()
            .arguments,
        CLEARED_TOOL_INPUT_PLACEHOLDER
    );
    assert!(validate_history_invariants(&projected).is_valid());
}

#[test]
fn recent_batch_retention_is_additive_and_preserves_evidence() {
    let messages = batch_history();
    for (batches, keep, cleared_ids) in [
        (0, 2, vec!["batch_0_0", "batch_1_3", "batch_1_2", "batch_1_1", "batch_1_0"]),
        (1, 2, vec!["batch_0_0", "batch_1_3", "batch_1_2", "batch_1_1", "batch_1_0"]),
        (2, 2, vec!["batch_0_0"]),
        (9, 2, vec![]),
        (1, 4, vec!["batch_0_0"]),
    ] {
        let projected = clear_old_tool_results_with_recent_batches(&messages, 1, keep, batches, 1, true);
        let actual: Vec<_> = projected
            .iter()
            .filter(|message| message.content.as_text().contains("\"cleared\":\"tool_result\""))
            .map(|message| message.tool_call_id.as_deref().unwrap())
            .collect();
        assert_eq!(actual, cleared_ids, "batches={batches}, keep={keep}");
        for (original, shaped) in messages.iter().zip(&projected) {
            if original.role == MessageRole::Tool && !cleared_ids.contains(&original.tool_call_id.as_deref().unwrap()) {
                assert_eq!(original, shaped);
            }
            if let Some(calls) = &original.tool_calls {
                for (original_call, shaped_call) in calls.iter().zip(shaped.tool_calls.as_ref().unwrap()) {
                    if !cleared_ids.contains(&original_call.id.as_str()) {
                        assert_eq!(original_call, shaped_call, "retained inputs and signatures");
                    }
                }
            }
        }
        assert!(validate_history_invariants(&projected).is_valid());
        assert_eq!(clear_old_tool_results_with_recent_batches(&projected, 1, keep, batches, 1, true), projected);
        if batches == 0 {
            assert_eq!(projected, clear_old_tool_results(&messages, 1, keep, 1, true));
        }
    }
    assert_eq!(messages, batch_history(), "durable history stays unchanged");
}

#[test]
fn recent_batches_follow_origins_not_completion_order_or_orphans() {
    let mut messages = batch_history();
    // Complete the oldest batch last. Count retention protects it in addition
    // to the two newest origins; it must not displace the middle origin.
    let old_result_index = messages.iter().position(|message| message.role == MessageRole::Tool).unwrap();
    let old_result = messages.remove(old_result_index);
    messages.push(old_result);
    let projected = clear_old_tool_results_with_recent_batches(&messages, 1, 1, 2, 1, true);
    assert_eq!(projected, messages);

    messages.push(Message::tool_response("orphan_old".into(), "orphan evidence ".repeat(400)));
    messages.push(Message::tool_response("orphan_new".into(), "new evidence ".repeat(400)));
    let projected = clear_old_tool_results_with_recent_batches(&messages, 1, 1, 2, 1, true);
    assert!(
        projected[projected.len() - 2]
            .content
            .as_text()
            .contains("\"cleared\":\"tool_result\"")
    );
    assert_eq!(projected.last(), messages.last());
    for message in projected.iter().filter(|message| message.role == MessageRole::Tool) {
        let id = message.tool_call_id.as_deref().unwrap();
        if id.starts_with("batch_1_") || id.starts_with("batch_2_") {
            assert!(!message.content.as_text().contains("\"cleared\":\"tool_result\""));
        }
    }
}

#[test]
fn recent_batches_preserve_reused_ids_and_small_old_outputs() {
    let mut messages = batch_history();
    messages[1].tool_calls.as_mut().unwrap()[0].id = "batch_2_0".into();
    let old = messages.iter_mut().find(|message| message.role == MessageRole::Tool).unwrap();
    old.tool_call_id = Some("batch_2_0".into());
    let projected = clear_old_tool_results_with_recent_batches(&messages, 1, 2, 2, 1, true);
    assert_eq!(
        projected[1].tool_calls.as_ref().unwrap()[0]
            .function
            .as_ref()
            .unwrap()
            .arguments,
        CLEARED_TOOL_INPUT_PLACEHOLDER
    );
    assert_eq!(projected.last(), messages.last());
    assert!(validate_history_invariants(&projected).is_valid());
    messages
        .iter_mut()
        .find(|message| message.role == MessageRole::Tool)
        .unwrap()
        .content = crate::llm::provider::MessageContent::Text("ok".into());
    assert_eq!(clear_old_tool_results_with_recent_batches(&messages, 1, 2, 2, 1, true), messages);
}
