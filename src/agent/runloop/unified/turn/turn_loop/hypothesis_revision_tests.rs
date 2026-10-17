//! Controlled providers exercise the production interactive loop and real tools.

use std::fs;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use vtcode_core::config::constants::tools;
use vtcode_core::exec::events::{ThreadEvent, ThreadItemDetails, ToolCallStatus, VersionedThreadEvent};
use vtcode_core::llm::provider as uni;

use super::run_turn_loop;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_processing::test_support::TestTurnProcessingBacking;

struct Step {
    tool: &'static str,
    args: Value,
    feedback: &'static str,
}

fn step(tool: &'static str, args: Value, feedback: &'static str) -> Step {
    Step { tool, args, feedback }
}

struct RevisionProvider {
    steps: Vec<Step>,
    cursor: Arc<Mutex<usize>>,
    final_feedback: &'static str,
}

#[async_trait::async_trait]
impl uni::LLMProvider for RevisionProvider {
    fn name(&self) -> &str {
        "openai"
    }
    fn supports_streaming(&self) -> bool {
        false
    }
    fn supported_models(&self) -> Vec<String> {
        vec!["noop-model".to_owned()]
    }
    fn validate_request(&self, _: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }

    async fn generate(&self, request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        let auxiliary = request
            .system_prompt
            .as_deref()
            .is_some_and(|text| text.contains("failure-diagnosis checkpoint"));
        let (content, tool_calls) = if auxiliary {
            (Some(r#"{"observed":"the tool reported a failure","likely_cause":"consult the returned evidence","next_action":"Use the reported cause to correct this action."}"#.to_owned()), None)
        } else {
            let mut cursor = self.cursor.lock().unwrap();
            let feedback = self.steps.get(*cursor).map_or(self.final_feedback, |step| step.feedback);
            let visible = request
                .messages
                .iter()
                .map(|message| message.content.as_text().into_owned())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(visible.contains(feedback), "missing model-visible feedback {feedback:?}: {visible}");
            if let Some(step) = self.steps.get(*cursor) {
                let calls = vec![uni::ToolCall::function(
                    format!("revision-{cursor}"),
                    step.tool.to_owned(),
                    step.args.to_string(),
                )];
                *cursor += 1;
                (None, Some(calls))
            } else {
                (Some("Done; the requested outcome is complete.".to_owned()), None)
            }
        };
        Ok(uni::LLMResponse {
            content,
            tool_calls,
            model: request.model,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: Vec::new(),
            compaction: None,
        })
    }
}

async fn run_script(steps: Vec<Step>, final_feedback: &'static str) -> (TestTurnProcessingBacking, Vec<ThreadEvent>) {
    let cursor = Arc::new(Mutex::new(0));
    let expected_calls = steps.len();
    let mut backing = TestTurnProcessingBacking::new(16).await;
    let workspace = backing.workspace_path();
    fs::write(workspace.join("alpha.txt"), "alpha\n").unwrap();
    fs::write(workspace.join("beta.txt"), "target=7\n").unwrap();
    fs::write(workspace.join("code.rs"), "pub fn value() -> u8 { \"bad\" }\n").unwrap();
    let harness_path = backing.enable_harness_emitter();
    backing.set_provider(Box::new(RevisionProvider { steps, cursor: Arc::clone(&cursor), final_feedback }));
    {
        let context = backing.turn_loop_context();
        context.tool_registry.allow_all_tools().await.unwrap();
        let mut commands = context.tool_registry.commands_config();
        commands.allow_list.extend(["cat", "rg", "sed", "rustc"].map(str::to_owned));
        context.tool_registry.apply_commands_config(&commands);
    }
    let mut history = vec![uni::Message::user(
        "Produce the requested fixture outcome from the tool evidence.".to_owned(),
    )];
    let context = backing.turn_loop_context();
    let emitter = context.harness_emitter.cloned().unwrap();
    let outcome = run_turn_loop(&mut history, context).await.unwrap();
    emitter.finish().await.unwrap();
    assert!(matches!(outcome.result, TurnLoopResult::Completed { .. }), "{:?}", outcome.result);
    assert_eq!(*cursor.lock().unwrap(), expected_calls);
    let events = fs::read_to_string(harness_path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<VersionedThreadEvent>(line).unwrap().into_event())
        .collect::<Vec<_>>();
    // Independently inspect canonical records, not the diagnostic classifier.
    for (index, event) in events.iter().enumerate() {
        if let ThreadEvent::ItemCompleted(item) = event
            && let ThreadItemDetails::Reasoning(reasoning) = &item.item.details
            && reasoning.stage.as_deref() == Some("diagnosis")
        {
            let preceding_output = events[..index]
                .iter()
                .rev()
                .find_map(|event| match event {
                    ThreadEvent::ItemCompleted(item) => match &item.item.details {
                        ThreadItemDetails::ToolOutput(output) => Some(output),
                        _ => None,
                    },
                    _ => None,
                })
                .expect("diagnosis must follow a canonical output");
            assert!(
                preceding_output.exit_code.is_some_and(|code| code != 0)
                    || preceding_output.status == ToolCallStatus::Failed
            );
        }
    }
    (backing, events)
}

fn add_result(feedback: &'static str) -> Step {
    step(
        tools::APPLY_PATCH,
        json!({"input":"*** Begin Patch\n*** Add File: result.txt\n+target=7\n*** End Patch\n"}),
        feedback,
    )
}

#[tokio::test]
async fn hypothesis_revision_nonzero_exit_corrects_target_from_reported_cause() {
    let (backing, events) = run_script(
        vec![
            step(tools::EXEC_COMMAND, json!({"cmd":"cat missing.txt"}), ""),
            step(
                tools::EXEC_COMMAND,
                json!({"cmd":"cat beta.txt"}),
                // Require the selected model action and the trusted suffix,
                // so a deterministic fallback cannot satisfy this assertion.
                "Use the reported cause to correct this action. Inspect the bounded evidence, revise the hypothesis, then retry with corrected arguments.",
            ),
            add_result("target=7"),
        ],
        "",
    )
    .await;
    assert_eq!(fs::read_to_string(backing.workspace_path().join("result.txt")).unwrap(), "target=7\n");
    assert!(events.iter().any(|event| matches!(event, ThreadEvent::ItemCompleted(item)
        if matches!(&item.item.details, ThreadItemDetails::ToolOutput(output) if output.exit_code.is_some_and(|code| code != 0)))));
}

#[tokio::test]
async fn hypothesis_revision_empty_search_changes_scope() {
    let (backing, _) = run_script(
        vec![
            step(tools::EXEC_COMMAND, json!({"cmd":"rg target alpha.txt"}), ""),
            step(tools::EXEC_COMMAND, json!({"cmd":"rg target beta.txt"}), "new scope or question"),
            add_result("target=7"),
        ],
        "",
    )
    .await;
    assert_eq!(fs::read_to_string(backing.workspace_path().join("result.txt")).unwrap(), "target=7\n");
}

#[tokio::test]
async fn hypothesis_revision_stale_patch_reads_current_context_and_repairs() {
    let (backing, _) = run_script(
        vec![
            step(
                tools::APPLY_PATCH,
                json!({"input":"*** Begin Patch\n*** Update File: alpha.txt\n@@\n-stale\n+target=7\n*** End Patch\n"}),
                "",
            ),
            step(tools::READ_FILE, json!({"path":"alpha.txt","offset":1,"limit":20}), "revise the patch hypothesis"),
            step(
                tools::APPLY_PATCH,
                json!({"input":"*** Begin Patch\n*** Update File: alpha.txt\n@@\n-alpha\n+target=7\n*** End Patch\n"}),
                "alpha",
            ),
        ],
        "",
    )
    .await;
    assert_eq!(fs::read_to_string(backing.workspace_path().join("alpha.txt")).unwrap(), "target=7\n");
}

#[tokio::test]
async fn hypothesis_revision_failed_verifier_repairs_and_obtains_terminal_success() {
    let check = json!({"cmd":"rustc --crate-type lib code.rs -o checked.rlib"});
    let (backing, events) = run_script(vec![
        step(tools::EXEC_COMMAND, check.clone(), ""),
        step(tools::APPLY_PATCH, json!({"input":"*** Begin Patch\n*** Update File: code.rs\n@@\n-pub fn value() -> u8 { \"bad\" }\n+pub fn value() -> u8 { 7 }\n*** End Patch\n"}), "revise the fix hypothesis"),
        step(tools::EXEC_COMMAND, check, ""),
    ], "").await;
    assert_eq!(
        fs::read_to_string(backing.workspace_path().join("code.rs")).unwrap(),
        "pub fn value() -> u8 { 7 }\n"
    );
    assert!(backing.workspace_path().join("checked.rlib").metadata().unwrap().len() > 0);
    let statuses = events
        .iter()
        .filter_map(|event| match event {
            ThreadEvent::ItemCompleted(item) => match &item.item.details {
                ThreadItemDetails::ToolOutput(output) => output.exit_code,
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(statuses.iter().any(|code| *code != 0));
    assert_eq!(statuses.last(), Some(&0));
}

#[tokio::test]
async fn hypothesis_revision_repeated_evidence_reuses_retained_context() {
    let (backing, _) = run_script(
        vec![
            step(tools::EXEC_COMMAND, json!({"cmd":"sed -n '1,2p' beta.txt"}), ""),
            step(tools::EXEC_COMMAND, json!({"cmd":"sed -n '1,1p' beta.txt"}), "target=7"),
            add_result("revise the hypothesis and change scope or approach"),
        ],
        "",
    )
    .await;
    assert_eq!(fs::read_to_string(backing.workspace_path().join("result.txt")).unwrap(), "target=7\n");
}

#[tokio::test]
async fn hypothesis_revision_counterexamples_do_not_establish_requested_outcome() {
    for second in [
        step(tools::EXEC_COMMAND, json!({"cmd":"cat missing.txt"}), "revise the hypothesis"),
        step(tools::EXEC_COMMAND, json!({"cmd":"cat  missing.txt"}), "revise the hypothesis"),
        step(tools::READ_FILE, json!({"path":"beta.txt","offset":1,"limit":20}), "revise the hypothesis"),
    ] {
        let (backing, _) =
            run_script(vec![step(tools::EXEC_COMMAND, json!({"cmd":"cat missing.txt"}), ""), second], "").await;
        // An identical retry, cosmetic spacing, or a reread followed by a
        // scripted success claim cannot replace the independent file oracle.
        assert!(!backing.workspace_path().join("result.txt").exists());
    }
}
