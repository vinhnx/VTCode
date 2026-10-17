//! Tests for the skill executor: fixtures and unit tests.
//! All items in this file are compiled only under `cfg(test)`.

use super::*;

use crate::config::types::CapabilityLevel;
use crate::llm::provider::{LLMError, LLMNormalizedStream, LLMResponse, NormalizedStreamEvent};
use crate::skills::types::{SkillFileSystemPermissions, SkillManifest, SkillNetworkPolicy, SkillPermissionProfile};
use crate::tools::registry::{ToolNetworkAccess, ToolRegistration};
use futures::stream;
use serde_json::json;
use std::sync::Mutex;
use tempfile::tempdir;

struct FakeForkExecutor;

struct EchoFirstUserProvider;
struct UnknownToolThenFinalizeProvider {
    calls: Mutex<usize>,
}
struct OutOfScopeToolThenFinalizeProvider {
    tool_name: &'static str,
    calls: Mutex<usize>,
}
struct RepeatToolThenFinalizeProvider {
    tool_name: &'static str,
    calls: Mutex<usize>,
}
struct MaxIterationsThenFinalizeProvider {
    tool_names: Vec<String>,
    calls: Mutex<usize>,
}
struct StreamingOnlySkillProvider {
    stream_calls: Mutex<usize>,
}
struct EmptyFinalSkillProvider;
struct ToolOnlyThenFinalizeProvider {
    tool_name: &'static str,
    calls: Mutex<usize>,
}
struct StopWithToolCallsThenFinalizeProvider {
    tool_name: &'static str,
    calls: Mutex<usize>,
}
struct CountingSkillTool {
    calls: Arc<Mutex<usize>>,
}

#[async_trait]
impl LLMProvider for EchoFirstUserProvider {
    fn name(&self) -> &str {
        "echo-first-user"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let first_message = request
            .messages
            .first()
            .map(|message| message.content.as_text().to_string())
            .unwrap_or_default();

        Ok(LLMResponse {
            content: Some(first_message),
            model: request.model,
            finish_reason: FinishReason::Stop,
            ..Default::default()
        })
    }
}

#[async_trait]
impl LLMProvider for UnknownToolThenFinalizeProvider {
    fn name(&self) -> &str {
        "unknown-tool-then-finalize"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut calls = self.calls.lock().expect("provider calls mutex");
        *calls += 1;

        match *calls {
            1 => Ok(LLMResponse {
                content: Some(String::new()),
                model: request.model,
                tool_calls: Some(vec![ToolCall::function(
                    "call_unknown_tool".to_string(),
                    "unified_diff".to_string(),
                    "{}".to_string(),
                )]),
                finish_reason: FinishReason::ToolCalls,
                ..Default::default()
            }),
            2 => {
                assert!(request.tools.is_none());
                let prompt = request
                    .messages
                    .last()
                    .map(|message| message.content.as_text().to_string())
                    .unwrap_or_default();
                assert!(prompt.contains("unified_diff"));
                assert!(prompt.contains(SKILL_TOOL_FREE_SYNTHESIS_PROMPT));

                Ok(LLMResponse {
                    content: Some("finalized after unknown tool".to_string()),
                    model: request.model,
                    finish_reason: FinishReason::Stop,
                    ..Default::default()
                })
            }
            _ => panic!("unexpected provider call count: {}", *calls),
        }
    }
}

#[async_trait]
impl LLMProvider for OutOfScopeToolThenFinalizeProvider {
    fn name(&self) -> &str {
        "out-of-scope-tool-then-finalize"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut calls = self.calls.lock().expect("provider calls mutex");
        *calls += 1;

        match *calls {
            1 => Ok(LLMResponse {
                content: Some(String::new()),
                model: request.model,
                tool_calls: Some(vec![ToolCall::function(
                    "call_out_of_scope_tool".to_string(),
                    self.tool_name.to_string(),
                    "{}".to_string(),
                )]),
                finish_reason: FinishReason::ToolCalls,
                ..Default::default()
            }),
            2 => {
                assert!(request.tools.is_none());
                let prompt = request
                    .messages
                    .last()
                    .map(|message| message.content.as_text().to_string())
                    .unwrap_or_default();
                assert!(prompt.contains("not available for this skill"));
                assert!(prompt.contains(SKILL_TOOL_FREE_SYNTHESIS_PROMPT));

                Ok(LLMResponse {
                    content: Some("finalized after out-of-scope tool".to_string()),
                    model: request.model,
                    finish_reason: FinishReason::Stop,
                    ..Default::default()
                })
            }
            _ => panic!("unexpected provider call count: {}", *calls),
        }
    }
}

#[async_trait]
impl LLMProvider for RepeatToolThenFinalizeProvider {
    fn name(&self) -> &str {
        "repeat-tool-then-finalize"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut calls = self.calls.lock().expect("provider calls mutex");
        *calls += 1;

        match *calls {
            1 | 2 => Ok(LLMResponse {
                content: Some(String::new()),
                model: request.model,
                tool_calls: Some(vec![ToolCall::function(
                    format!("repeat_tool_call_{}", *calls),
                    self.tool_name.to_string(),
                    "{\"input\":\"same\"}".to_string(),
                )]),
                finish_reason: FinishReason::ToolCalls,
                ..Default::default()
            }),
            3 => {
                assert!(request.tools.is_none());
                let prompt = request
                    .messages
                    .last()
                    .map(|message| message.content.as_text().to_string())
                    .unwrap_or_default();
                assert!(prompt.contains(crate::core::loop_detector::HARD_STOP_PREFIX));
                assert!(prompt.contains(SKILL_TOOL_FREE_SYNTHESIS_PROMPT));

                Ok(LLMResponse {
                    content: Some("finalized after loop detection".to_string()),
                    model: request.model,
                    finish_reason: FinishReason::Stop,
                    ..Default::default()
                })
            }
            _ => panic!("unexpected provider call count: {}", *calls),
        }
    }
}

#[async_trait]
impl LLMProvider for MaxIterationsThenFinalizeProvider {
    fn name(&self) -> &str {
        "max-iterations-then-finalize"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut calls = self.calls.lock().expect("provider calls mutex");
        *calls += 1;

        if *calls <= MAX_SKILL_LLM_ITERATIONS {
            let tool_name = self.tool_names[*calls - 1].clone();
            return Ok(LLMResponse {
                content: Some(String::new()),
                model: request.model,
                tool_calls: Some(vec![ToolCall::function(
                    format!("max_iterations_tool_call_{}", *calls),
                    tool_name,
                    format!("{{\"step\":{}}}", *calls),
                )]),
                finish_reason: FinishReason::ToolCalls,
                ..Default::default()
            });
        }

        assert_eq!(*calls, MAX_SKILL_LLM_ITERATIONS + 1);
        assert!(request.tools.is_none());
        let prompt = request
            .messages
            .last()
            .map(|message| message.content.as_text().to_string())
            .unwrap_or_default();
        assert!(prompt.contains("maximum tool-call iterations"));
        assert!(prompt.contains(&MAX_SKILL_LLM_ITERATIONS.to_string()));
        assert!(prompt.contains(SKILL_TOOL_FREE_SYNTHESIS_PROMPT));

        Ok(LLMResponse {
            content: Some("finalized after max iterations".to_string()),
            model: request.model,
            finish_reason: FinishReason::Stop,
            ..Default::default()
        })
    }
}

#[async_trait]
impl LLMProvider for StreamingOnlySkillProvider {
    fn name(&self) -> &str {
        "streaming-only-skill"
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, _model: &str) -> bool {
        false
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.2-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, _request: LLMRequest) -> Result<LLMResponse, LLMError> {
        panic!("generate should not be called for streaming-only skill provider")
    }

    async fn stream_normalized(&self, request: LLMRequest) -> Result<LLMNormalizedStream, LLMError> {
        let mut stream_calls = self.stream_calls.lock().expect("stream calls mutex");
        *stream_calls += 1;

        Ok(Box::pin(stream::iter(vec![
            Ok(NormalizedStreamEvent::TextDelta { delta: "streamed ".to_string() }),
            Ok(NormalizedStreamEvent::TextDelta { delta: "skill result".to_string() }),
            Ok(NormalizedStreamEvent::Done {
                response: Box::new(LLMResponse {
                    content: None,
                    model: request.model,
                    finish_reason: FinishReason::Stop,
                    ..Default::default()
                }),
            }),
        ])))
    }
}

#[async_trait]
impl LLMProvider for EmptyFinalSkillProvider {
    fn name(&self) -> &str {
        "empty-final-skill"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        Ok(LLMResponse {
            content: None,
            model: request.model,
            finish_reason: FinishReason::Stop,
            ..Default::default()
        })
    }
}

#[async_trait]
impl LLMProvider for ToolOnlyThenFinalizeProvider {
    fn name(&self) -> &str {
        "tool-only-then-finalize"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut calls = self.calls.lock().expect("provider calls mutex");
        *calls += 1;

        match *calls {
            1 => Ok(LLMResponse {
                content: None,
                model: request.model,
                tool_calls: Some(vec![ToolCall::function(
                    "tool_only_call".to_string(),
                    self.tool_name.to_string(),
                    "{}".to_string(),
                )]),
                finish_reason: FinishReason::ToolCalls,
                ..Default::default()
            }),
            2 => Ok(LLMResponse {
                content: Some("finalized after tool-only response".to_string()),
                model: request.model,
                finish_reason: FinishReason::Stop,
                ..Default::default()
            }),
            _ => panic!("unexpected provider call count: {}", *calls),
        }
    }
}

#[async_trait]
impl LLMProvider for StopWithToolCallsThenFinalizeProvider {
    fn name(&self) -> &str {
        "stop-with-tool-calls-then-finalize"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut calls = self.calls.lock().expect("provider calls mutex");
        *calls += 1;

        match *calls {
            1 => Ok(LLMResponse {
                content: Some(String::new()),
                model: request.model,
                tool_calls: Some(vec![ToolCall::function(
                    "stop_tool_call".to_string(),
                    self.tool_name.to_string(),
                    "{}".to_string(),
                )]),
                finish_reason: FinishReason::Stop,
                ..Default::default()
            }),
            2 => Ok(LLMResponse {
                content: Some("finalized after stop tool call".to_string()),
                model: request.model,
                finish_reason: FinishReason::Stop,
                ..Default::default()
            }),
            _ => panic!("unexpected provider call count: {}", *calls),
        }
    }
}

#[async_trait]
impl ForkSkillExecutor for FakeForkExecutor {
    async fn execute(&self, skill: &Skill, user_input: Value) -> Result<Value> {
        Ok(serde_json::json!({
            "execution_context": "fork",
            "status": "success",
            "summary": format!("forked {}", skill.name()),
            "artifact_paths": [],
            "delegate_session_id": "child-session",
            "echo": user_input,
        }))
    }
}

#[async_trait]
impl Tool for CountingSkillTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        let mut calls = self.calls.lock().expect("tool calls mutex");
        *calls += 1;
        Ok(json!({
            "success": true,
            "echo": args,
        }))
    }

    fn name(&self) -> &str {
        "counting_skill_tool"
    }

    fn description(&self) -> &str {
        "Counts skill tool invocations"
    }
}

#[tokio::test]
async fn test_skill_tool_adapter_exposes_underlying_skill_name() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };

    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Instructions".to_string()).expect("failed to create skill");

    let adapter = SkillToolAdapter::new(skill);
    assert_eq!(adapter.skill().name(), "test-skill");
}

#[tokio::test]
async fn test_skill_tool_adapter_execute() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };

    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");

    let adapter = SkillToolAdapter::new(skill);
    let args = serde_json::json!({"test": "value"});
    let result = adapter.execute(args).await;

    assert!(result.is_ok());
    let res = result.unwrap();
    assert_eq!(res["skill_name"], "test-skill");
    assert_eq!(res["status"], "executing");
}

#[tokio::test]
async fn test_fork_skill_adapter_uses_fork_executor() {
    let manifest = SkillManifest {
        name: "fork-skill".to_string(),
        description: "Forked skill".to_string(),
        context: Some("fork".to_string()),
        vtcode_native: Some(true),
        ..Default::default()
    };

    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");

    let adapter = SkillToolAdapter::with_fork_executor(skill, Arc::new(FakeForkExecutor));
    let args = serde_json::json!({"task": "value"});
    let result = adapter.execute(args.clone()).await.expect("fork execution");

    assert_eq!(result["execution_context"], "fork");
    assert_eq!(result["delegate_session_id"], "child-session");
    assert_eq!(result["echo"], args);
}

#[tokio::test]
async fn blank_skill_input_uses_default_prompt_for_sub_llm() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;

    let result = execute_skill_with_sub_llm(
        &skill,
        String::new(),
        &EchoFirstUserProvider,
        &mut registry,
        Vec::new(),
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("blank input should be normalized");

    assert_eq!(result, EMPTY_SKILL_INPUT_PROMPT);
}

#[tokio::test]
async fn non_empty_skill_input_is_preserved_for_sub_llm() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;

    let result = execute_skill_with_sub_llm(
        &skill,
        "security".to_string(),
        &EchoFirstUserProvider,
        &mut registry,
        Vec::new(),
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("non-empty input should be preserved");

    assert_eq!(result, "security");
}

#[tokio::test]
async fn skill_executor_uses_normalized_stream_when_non_streaming_is_unsupported() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let provider = StreamingOnlySkillProvider { stream_calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        Vec::new(),
        "gpt-5.2-codex".to_string(),
    )
    .await
    .expect("streaming-only skill execution should succeed");

    assert_eq!(result, "streamed skill result");
    assert_eq!(*provider.stream_calls.lock().expect("stream calls mutex"), 1);
}

#[tokio::test]
async fn skill_executor_errors_when_final_response_has_no_visible_content() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;

    let error = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &EmptyFinalSkillProvider,
        &mut registry,
        Vec::new(),
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect_err("empty final response should be visible as an error");

    assert!(error.to_string().contains("completed without a visible final response"));
}

#[tokio::test]
async fn skill_executor_allows_tool_only_response_before_final_content() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_name = "tool_only_skill_test_tool";
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = ToolOnlyThenFinalizeProvider { tool_name, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            tool_name.to_string(),
            "Tool-only test tool".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("tool-only response should continue to final content");

    assert_eq!(result, "finalized after tool-only response");
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 1);
}

#[tokio::test]
async fn skill_executor_continues_after_stop_response_with_tool_calls() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_name = "stop_finish_reason_skill_test_tool";
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = StopWithToolCallsThenFinalizeProvider { tool_name, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            tool_name.to_string(),
            "Stop finish reason test tool".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("stop response with tool calls should continue to final content");

    assert_eq!(result, "finalized after stop tool call");
    assert_eq!(*provider.calls.lock().expect("provider calls mutex"), 2);
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 1);
}

#[tokio::test]
async fn skill_executor_forces_final_synthesis_after_unknown_tool() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry.allow_all_tools().await.expect("allow tools");
    let provider = UnknownToolThenFinalizeProvider { calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("unknown tool should trigger final synthesis");

    assert_eq!(result, "finalized after unknown tool");
}

#[tokio::test]
async fn skill_executor_blocks_registered_tool_outside_filtered_scope() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_name = "skill_out_of_scope_test_tool";
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = OutOfScopeToolThenFinalizeProvider { tool_name, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("out-of-scope tool should trigger final synthesis");

    assert_eq!(result, "finalized after out-of-scope tool");
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 0);
}

#[tokio::test]
async fn skill_executor_skips_repeated_tool_call_and_finalizes() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_name = "skill_loop_test_tool";
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = RepeatToolThenFinalizeProvider { tool_name, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            tool_name.to_string(),
            "Loop test tool".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("looping tool calls should force a final synthesis");

    assert_eq!(result, "finalized after loop detection");
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 1);
}

#[tokio::test]
async fn skill_executor_forces_final_synthesis_after_max_iterations() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_calls = Arc::new(Mutex::new(0usize));
    let mut available_tools = Vec::with_capacity(MAX_SKILL_LLM_ITERATIONS);
    let mut tool_names = Vec::with_capacity(MAX_SKILL_LLM_ITERATIONS);

    for index in 0..MAX_SKILL_LLM_ITERATIONS {
        let tool_name = format!("skill_iteration_test_tool_{index}");
        registry
            .register_tool(
                ToolRegistration::from_tool_instance(
                    tool_name.as_str(),
                    CapabilityLevel::CodeSearch,
                    CountingSkillTool { calls: Arc::clone(&tool_calls) },
                )
                .with_network_access(ToolNetworkAccess::Local),
            )
            .await
            .unwrap_or_else(|error| panic!("register tool {tool_name}: {error}"));
        available_tools.push(ToolDefinition::function(
            tool_name.clone(),
            format!("Iteration tool {index}"),
            json!({"type": "object"}),
        ));
        tool_names.push(tool_name);
    }

    registry.allow_all_tools().await.expect("allow tools");
    let provider = MaxIterationsThenFinalizeProvider { tool_names, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "analyze".to_string(),
        &provider,
        &mut registry,
        available_tools,
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("max-iteration recovery should force a final synthesis");

    assert_eq!(result, "finalized after max iterations");
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), MAX_SKILL_LLM_ITERATIONS);
}

#[test]
fn test_filter_tools_no_network_policy() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: None,
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let tools = vec![
        ToolDefinition::function("read_file".to_string(), "Read".to_string(), serde_json::json!({})),
        ToolDefinition::web_search(serde_json::json!({})),
        ToolDefinition::function("web_search".to_string(), "Search".to_string(), serde_json::json!({})),
    ];
    let filtered = filter_tools_for_skill(&skill, tools);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].function.as_ref().unwrap().name, "read_file");
}

#[test]
fn test_filter_tools_with_network_policy_updates_native_web_search() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: Some(
            SkillNetworkPolicy {
                allowed_domains: vec!["api.example.com".to_string()],
                denied_domains: vec!["blocked.example.com".to_string()],
            }
            .into(),
        ),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let tools = vec![ToolDefinition::web_search(serde_json::json!({
        "user_location": "US"
    }))];
    let filtered = filter_tools_for_skill(&skill, tools);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].tool_type, "web_search");
    assert_eq!(
        filtered[0].web_search.as_ref(),
        Some(&serde_json::json!({
            "user_location": "US",
            "allowed_domains": ["api.example.com"],
            "blocked_domains": ["blocked.example.com"]
        }))
    );
}

#[test]
fn test_filter_tools_no_network_policy_removes_gemini_native_network_tools() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: None,
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let tools = vec![
        ToolDefinition::google_maps(serde_json::json!({})),
        ToolDefinition::url_context(serde_json::json!({})),
        ToolDefinition::function("read_file".to_string(), "Read".to_string(), serde_json::json!({})),
    ];

    let filtered = filter_tools_for_skill(&skill, tools);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].function_name(), "read_file");
}

#[test]
fn test_filter_tools_with_network_policy_drops_gemini_native_network_tools() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: Some(
            SkillNetworkPolicy {
                allowed_domains: vec!["example.com".to_string()],
                denied_domains: vec![],
            }
            .into(),
        ),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let filtered = filter_tools_for_skill(
        &skill,
        vec![
            ToolDefinition::google_maps(serde_json::json!({})),
            ToolDefinition::url_context(serde_json::json!({})),
        ],
    );

    assert!(filtered.is_empty());
}

#[test]
fn test_filter_tools_drops_function_style_network_tools_when_policy_is_present() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: Some(
            SkillNetworkPolicy {
                allowed_domains: vec!["api.example.com".to_string()],
                denied_domains: vec![],
            }
            .into(),
        ),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let tools = vec![
        ToolDefinition::function("read_web_page".to_string(), "Read web page".to_string(), serde_json::json!({})),
        ToolDefinition::function("read_file".to_string(), "Read".to_string(), serde_json::json!({})),
    ];
    let filtered = filter_tools_for_skill(&skill, tools);

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].function_name(), "read_file");
}

#[test]
fn test_filter_tools_fails_closed_for_unrepresentable_web_search_policy() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test".to_string(),
        network_policy: Some(
            SkillNetworkPolicy {
                allowed_domains: vec!["docs.rs".to_string()],
                denied_domains: vec!["example.com".to_string()],
            }
            .into(),
        ),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "instructions".to_string()).expect("failed to create skill");

    let mut anthropic_web_search = ToolDefinition::web_search(serde_json::json!({}));
    anthropic_web_search.tool_type = "web_search_20250305".to_string();

    let filtered = filter_tools_for_skill(&skill, vec![anthropic_web_search]);

    assert!(filtered.is_empty());
}

#[test]
fn test_skill_execution_context() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };

    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "Instructions".to_string()).expect("failed to create skill");

    let tools = vec!["file_ops".to_string(), "shell".to_string()];
    let input = serde_json::json!({"test": "input"});

    let ctx = SkillExecutionContext::new(&skill, input, tools);
    assert_eq!(ctx.skill_name, "test-skill");
    assert_eq!(ctx.available_tools.len(), 2);
}

fn test_skill_with_permissions(permission_profile: Option<SkillPermissionProfile>) -> Skill {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        permissions: permission_profile.map(Into::into),
        vtcode_native: Some(true),
        ..Default::default()
    };

    Skill::new(manifest, PathBuf::from("/tmp/test-skill"), "Instructions".to_string()).expect("failed to create skill")
}

#[test]
fn skill_command_permissions_inject_additional_permissions() {
    let skill = test_skill_with_permissions(Some(SkillPermissionProfile {
        file_system: Some(
            SkillFileSystemPermissions {
                read: vec![PathBuf::from("references")],
                write: vec![PathBuf::from("outputs")],
            }
            .into(),
        ),
    }));

    let merged = merge_skill_command_permissions(&skill, "shell", serde_json::json!({"command": "pwd"}));

    assert_eq!(merged["sandbox_permissions"], serde_json::json!("with_additional_permissions"));
    assert_eq!(merged["additional_permissions"]["fs_read"], serde_json::json!(["/tmp/test-skill/references"]));
    assert_eq!(merged["additional_permissions"]["fs_write"], serde_json::json!(["/tmp/test-skill/outputs"]));
}

#[test]
fn skill_command_permissions_merge_existing_permissions() {
    let skill = test_skill_with_permissions(Some(SkillPermissionProfile {
        file_system: Some(
            SkillFileSystemPermissions {
                read: vec![PathBuf::from("references")],
                write: vec![PathBuf::from("outputs")],
            }
            .into(),
        ),
    }));

    let merged = merge_skill_command_permissions(
        &skill,
        "shell",
        serde_json::json!({
            "command": "pwd",
            "sandbox_permissions": "with_additional_permissions",
            "additional_permissions": {
                "fs_read": ["/tmp/existing-read"],
                "fs_write": ["/tmp/existing-write"]
            }
        }),
    );

    assert_eq!(
        merged["additional_permissions"]["fs_read"],
        serde_json::json!(["/tmp/existing-read", "/tmp/test-skill/references"])
    );
    assert_eq!(
        merged["additional_permissions"]["fs_write"],
        serde_json::json!(["/tmp/existing-write", "/tmp/test-skill/outputs"])
    );
}

#[test]
fn skill_command_permissions_ignore_require_escalated() {
    let skill = test_skill_with_permissions(Some(SkillPermissionProfile {
        file_system: Some(
            SkillFileSystemPermissions {
                read: Vec::new(),
                write: vec![PathBuf::from("outputs")],
            }
            .into(),
        ),
    }));
    let original = serde_json::json!({
        "command": "pwd",
        "sandbox_permissions": "require_escalated",
        "justification": "Do you want to run this command without sandbox restrictions?"
    });

    let merged = merge_skill_command_permissions(&skill, "shell", original.clone());

    assert_eq!(merged, original);
}

#[test]
fn skill_command_permissions_ignore_empty_skill_permissions() {
    let skill = test_skill_with_permissions(None);
    let original = serde_json::json!({"command": "pwd"});

    let merged = merge_skill_command_permissions(&skill, "shell", original.clone());

    assert_eq!(merged, original);
}

#[test]
fn textual_skill_tool_call_parses_reported_bash_markup() {
    let text = "Gathering diff and status information.<tool_call>bash<arg_key>command</arg_key><arg_value>git status --short && echo \"---STAGED---\" && git diff --cached --stat</arg_value><arg_key>description</arg_key><arg_value>Check git status and diff summary</arg_value></tool_call>";
    let (name, args) = parse_textual_skill_tool_call(text).expect("bash markup should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["action"], serde_json::json!("run"));
    assert!(
        args["command"].as_str().unwrap_or_default().contains("git status --short"),
        "unexpected args: {args}"
    );
}

#[test]
fn textual_skill_tool_call_parses_json_payload() {
    let text = r#"<tool_call>exec_command{"command": "git diff", "action": "run"}</tool_call>"#;
    let (name, args) = parse_textual_skill_tool_call(text).expect("json payload should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!("git diff"));
}

#[test]
fn textual_skill_tool_call_rejects_plain_prose() {
    assert!(parse_textual_skill_tool_call("Review the full diff for correctness").is_none());
    assert!(parse_textual_skill_tool_call("I can't access the repository").is_none());
}

#[test]
fn textual_skill_tool_name_canonicalizes_shell_aliases() {
    for alias in ["bash", "shell", "exec", "run", "command"] {
        assert_eq!(
            canonicalize_skill_textual_tool_name(alias).as_deref(),
            Some("exec_command"),
            "alias {alias} should map to exec_command"
        );
    }
    assert_eq!(canonicalize_skill_textual_tool_name("read_file").as_deref(), Some("read_file"));
}

#[test]
fn textual_skill_tool_call_preserves_angle_brackets_in_values() {
    let text =
        "<tool_call>bash<arg_key>command</arg_key><arg_value>git diff <verified-target>...HEAD</arg_value></tool_call>";
    let (name, args) = parse_textual_skill_tool_call(text).expect("placeholder markup should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!("git diff <verified-target>...HEAD"));
}

#[test]
fn textual_skill_tool_call_ignores_trailing_text_after_json() {
    let text = r#"<tool_call>exec_command{"command": "git diff"} trailing }</tool_call>"#;
    let (name, args) = parse_textual_skill_tool_call(text).expect("json payload should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!("git diff"));
}

#[test]
fn textual_skill_tool_call_ignores_fenced_documentation_example() {
    // Skill docs / test fixtures quoting markup must not execute.
    let text = "The skill documentation quotes this example:\n\n```sh\n<tool_call>bash<arg_key>command</arg_key><arg_value>rm -rf /tmp/demo</arg_value></tool_call>\n```\n\nDo not run it; summarize instead.";
    assert!(parse_textual_skill_tool_call(text).is_none(), "fenced tagged markup must not become a tool call");
}

#[test]
fn textual_skill_tool_call_ignores_mid_prose_tag_mention() {
    let text = "containing `<tool_call>` example markup trigger unintended tool execution? Possibly. Now check remaining commits.";
    assert!(parse_textual_skill_tool_call(text).is_none(), "prose mention must not bind");
}

#[test]
fn textual_skill_tool_call_skips_dirty_name_and_parses_clean_call() {
    let text = "Docs mention `<tool_call>` as a tag. Then:\n<tool_call>exec_command<arg_key>command</arg_key><arg_value>echo hi</arg_value></tool_call>";
    let (name, args) = parse_textual_skill_tool_call(text).expect("clean call after prose mention should parse");
    assert_eq!(name, tool_constants::EXEC_COMMAND);
    assert_eq!(args["command"], serde_json::json!("echo hi"));
}

#[test]
fn is_clean_skill_tool_name_rejects_prose() {
    use vtcode_commons::text_fence::is_clean_tool_name as is_clean_skill_tool_name;
    assert!(is_clean_skill_tool_name("exec_command"));
    assert!(is_clean_skill_tool_name("bash"));
    assert!(!is_clean_skill_tool_name(""));
    assert!(!is_clean_skill_tool_name("` in content"));
    assert!(!is_clean_skill_tool_name("has space"));
    assert!(!is_clean_skill_tool_name(&"x".repeat(65)));
}

struct TextualToolThenFinalizeProvider {
    calls: Mutex<usize>,
}

#[async_trait]
impl LLMProvider for TextualToolThenFinalizeProvider {
    fn name(&self) -> &str {
        "textual-tool-then-finalize"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut calls = self.calls.lock().expect("provider calls mutex");
        *calls += 1;

        match *calls {
            1 => Ok(LLMResponse {
                content: Some(
                    "Gathering diff.<tool_call>bash<arg_key>command</arg_key><arg_value>git status --short</arg_value><arg_key>action</arg_key><arg_value>run</arg_value></tool_call>"
                        .to_string(),
                ),
                model: request.model,
                tool_calls: None,
                finish_reason: FinishReason::Stop,
                ..Default::default()
            }),
            2 => Ok(LLMResponse {
                content: Some("finalized after textual tool call".to_string()),
                model: request.model,
                finish_reason: FinishReason::Stop,
                ..Default::default()
            }),
            _ => panic!("unexpected provider call count: {}", *calls),
        }
    }
}

#[tokio::test]
async fn skill_executor_runs_textual_tool_markup_without_native_calls() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    // Register under the canonical name the textual fallback produces so the
    // wiring (parse -> scope check -> registry dispatch) is exercised.
    let tool_name = tool_constants::EXEC_COMMAND;
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = TextualToolThenFinalizeProvider { calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            tool_name.to_string(),
            "Textual fallback test tool".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("textual tool markup should execute and finalize");

    assert_eq!(result, "finalized after textual tool call");
    assert_eq!(*provider.calls.lock().expect("provider calls mutex"), 2);
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 1);
}

struct TextualUnknownToolThenFinalizeProvider {
    calls: Mutex<usize>,
}

#[async_trait]
impl LLMProvider for TextualUnknownToolThenFinalizeProvider {
    fn name(&self) -> &str {
        "textual-unknown-tool-then-finalize"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut calls = self.calls.lock().expect("provider calls mutex");
        *calls += 1;

        match *calls {
            1 => Ok(LLMResponse {
                content: Some(
                    "Gathering diff.<tool_call>unified_diff<arg_key>path</arg_key><arg_value>src/main.rs</arg_value></tool_call>"
                        .to_string(),
                ),
                model: request.model,
                tool_calls: None,
                finish_reason: FinishReason::Stop,
                ..Default::default()
            }),
            2 => {
                assert!(request.tools.is_none());
                let prompt = request
                    .messages
                    .last()
                    .map(|message| message.content.as_text().to_string())
                    .unwrap_or_default();
                assert!(prompt.contains("unified_diff"));
                assert!(prompt.contains(SKILL_TOOL_FREE_SYNTHESIS_PROMPT));

                Ok(LLMResponse {
                    content: Some("finalized after textual unknown tool".to_string()),
                    model: request.model,
                    finish_reason: FinishReason::Stop,
                    ..Default::default()
                })
            }
            _ => panic!("unexpected provider call count: {}", *calls),
        }
    }
}

#[tokio::test]
async fn skill_executor_forces_final_synthesis_after_textual_unknown_tool() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry.allow_all_tools().await.expect("allow tools");
    let provider = TextualUnknownToolThenFinalizeProvider { calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("textual unknown tool should trigger final synthesis");

    assert_eq!(result, "finalized after textual unknown tool");
}

struct TextualOutOfScopeToolThenFinalizeProvider {
    calls: Mutex<usize>,
}

#[async_trait]
impl LLMProvider for TextualOutOfScopeToolThenFinalizeProvider {
    fn name(&self) -> &str {
        "textual-out-of-scope-tool-then-finalize"
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-5.1-codex".to_string()]
    }

    fn validate_request(&self, _request: &LLMRequest) -> Result<(), LLMError> {
        Ok(())
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let mut calls = self.calls.lock().expect("provider calls mutex");
        *calls += 1;

        match *calls {
            1 => Ok(LLMResponse {
                content: Some(
                    "Trying to run a command.<tool_call>bash<arg_key>command</arg_key><arg_value>rm -rf /</arg_value><arg_key>action</arg_key><arg_value>run</arg_value></tool_call>"
                        .to_string(),
                ),
                model: request.model,
                tool_calls: None,
                finish_reason: FinishReason::Stop,
                ..Default::default()
            }),
            2 => {
                assert!(request.tools.is_none());
                let prompt = request
                    .messages
                    .last()
                    .map(|message| message.content.as_text().to_string())
                    .unwrap_or_default();
                assert!(
                    prompt.contains("exec_command") || prompt.contains("bash"),
                    "forced synthesis must name the denied tool, got: {prompt}"
                );
                assert!(prompt.contains(SKILL_TOOL_FREE_SYNTHESIS_PROMPT));

                Ok(LLMResponse {
                    content: Some("finalized after out-of-scope textual tool".to_string()),
                    model: request.model,
                    finish_reason: FinishReason::Stop,
                    ..Default::default()
                })
            }
            _ => panic!("unexpected provider call count: {}", *calls),
        }
    }
}

/// Textual `tool_call` markup must not bypass skill tool scope: a call for a tool
/// absent from the skill's tool definitions is denied before registry dispatch
/// (even when the registry has the tool registered and allowed) and forces
/// tool-free synthesis.
#[tokio::test]
async fn skill_executor_denies_textual_tool_markup_outside_skill_scope() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    // Registry has exec_command available; skill scope must still deny it.
    let tool_name = tool_constants::EXEC_COMMAND;
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = TextualOutOfScopeToolThenFinalizeProvider { calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        // Skill only sees read_file; textual bash/exec_command must be denied.
        vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("out-of-scope textual tool should force final synthesis");

    assert_eq!(result, "finalized after out-of-scope textual tool");
    assert_eq!(
        *tool_calls.lock().expect("tool calls mutex"),
        0,
        "out-of-scope textual tool must never reach the registry"
    );
    assert_eq!(*provider.calls.lock().expect("provider calls mutex"), 2);
}
