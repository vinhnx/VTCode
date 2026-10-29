//! Tests for the skill executor: fixtures and unit tests.
//! All items in this file are compiled only under `cfg(test)`.

pub(super) use super::*;

pub(super) use crate::config::types::CapabilityLevel;
pub(super) use crate::llm::provider::{LLMError, LLMNormalizedStream, LLMResponse, NormalizedStreamEvent};
pub(super) use crate::skills::types::{
    SkillFileSystemPermissions, SkillManifest, SkillNetworkPolicy, SkillPermissionProfile,
};
pub(super) use crate::tools::registry::{ToolNetworkAccess, ToolRegistration};
pub(super) use futures::stream;
pub(super) use serde_json::json;
pub(super) use std::sync::Mutex;
pub(super) use tempfile::tempdir;

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

mod executor_flows;
mod filter_tools;
mod permissions;
mod textual_calls;
mod textual_executor;
