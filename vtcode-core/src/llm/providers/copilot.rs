#![allow(clippy::result_large_err)]

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use async_stream::stream;
use async_trait::async_trait;
use tokio::sync::Mutex;
use vtcode_config::auth::CopilotAuthConfig;
use vtcode_config::constants::models::copilot as copilot_models;
use vtcode_config::models::supported_models_for_provider;

use crate::copilot::{
    COPILOT_MODEL_ID, COPILOT_PROVIDER_KEY, CopilotAcpClient, CopilotPromptSessionFuture,
    CopilotRuntimeRequest, CopilotToolCallFailure, CopilotToolCallResponse, PromptSession,
    PromptSessionCancelHandle, PromptUpdate, probe_auth_status,
};
use crate::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent, Message,
    MessageRole, ToolDefinition,
};
use crate::llm::providers::common::validate_request_common;

pub struct CopilotProvider {
    model: String,
    auth_config: CopilotAuthConfig,
    workspace_root: PathBuf,
    client: Mutex<Option<CachedCopilotClient>>,
}

struct CachedCopilotClient {
    raw_model: Option<String>,
    tool_signature: String,
    client: Arc<CopilotAcpClient>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedCopilotModel {
    request_model: String,
    raw_model: Option<String>,
}

impl CopilotProvider {
    pub fn from_config(
        model: Option<String>,
        auth_config: Option<CopilotAuthConfig>,
        workspace_root: Option<PathBuf>,
    ) -> Self {
        Self {
            model: model.unwrap_or_else(|| COPILOT_MODEL_ID.to_string()),
            auth_config: auth_config.unwrap_or_default(),
            workspace_root: workspace_root
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
            client: Mutex::new(None),
        }
    }

    async fn client(
        &self,
        model: &ResolvedCopilotModel,
        tools: &[ToolDefinition],
    ) -> Result<Arc<CopilotAcpClient>, LLMError> {
        let tool_signature = copilot_tool_signature(tools);
        if let Some(client) = self.cached_client(model, &tool_signature).await {
            return Ok(client);
        }

        let auth_status = probe_auth_status(&self.auth_config, Some(&self.workspace_root)).await;
        if !auth_status.is_authenticated() {
            return Err(LLMError::Authentication {
                message: auth_status.message.unwrap_or_else(|| {
                    "GitHub Copilot is not authenticated. Run `vtcode login copilot`.".to_string()
                }),
                metadata: None,
            });
        }

        let created = Arc::new(
            CopilotAcpClient::connect(
                &self.auth_config,
                &self.workspace_root,
                model.raw_model.as_deref(),
                tools,
            )
            .await
            .map_err(map_copilot_error)?,
        );

        let mut client = self.client.lock().await;
        if let Some(existing) = client.as_ref()
            && existing.raw_model.as_deref() == model.raw_model.as_deref()
            && existing.tool_signature == tool_signature
        {
            return Ok(existing.client.clone());
        }
        *client = Some(CachedCopilotClient {
            raw_model: model.raw_model.clone(),
            tool_signature,
            client: created.clone(),
        });
        Ok(created)
    }

    async fn cached_client(
        &self,
        model: &ResolvedCopilotModel,
        tool_signature: &str,
    ) -> Option<Arc<CopilotAcpClient>> {
        let client = self.client.lock().await;
        client
            .as_ref()
            .filter(|cached| {
                cached.raw_model.as_deref() == model.raw_model.as_deref()
                    && cached.tool_signature == tool_signature
            })
            .map(|cached| cached.client.clone())
    }

    fn resolve_model(&self, request: &LLMRequest) -> Result<ResolvedCopilotModel, LLMError> {
        let requested = if request.model.trim().is_empty() {
            self.model.trim()
        } else {
            request.model.trim()
        };

        let raw_model = normalize_copilot_model_id(requested).ok_or_else(|| {
            invalid_request(&format!(
                "Unsupported GitHub Copilot model: {requested}. Choose `copilot-auto` or a live GitHub Copilot model id from the picker."
            ))
        })?;

        Ok(ResolvedCopilotModel {
            request_model: requested.to_string(),
            raw_model,
        })
    }

    fn build_transcript(&self, request: &LLMRequest) -> Result<String, LLMError> {
        let mut transcript = String::new();

        if let Some(system_prompt) = request.system_prompt.as_ref() {
            append_block(&mut transcript, "System", system_prompt);
        }

        for message in &request.messages {
            let label = match message.role {
                MessageRole::System => "System",
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
                MessageRole::Tool => "Tool",
            };
            append_block(&mut transcript, label, &render_message_for_copilot(message));
        }

        Ok(transcript)
    }

    async fn stream_from_session(
        &self,
        model: ResolvedCopilotModel,
        prompt_session: PromptSession,
    ) -> Result<LLMStream, LLMError> {
        struct PromptCancellationGuard {
            cancel_handle: Option<PromptSessionCancelHandle>,
        }

        impl PromptCancellationGuard {
            fn new(cancel_handle: PromptSessionCancelHandle) -> Self {
                Self {
                    cancel_handle: Some(cancel_handle),
                }
            }

            fn disarm(&mut self) {
                self.cancel_handle = None;
            }
        }

        impl Drop for PromptCancellationGuard {
            fn drop(&mut self) {
                if let Some(cancel_handle) = self.cancel_handle.take() {
                    cancel_handle.cancel();
                }
            }
        }

        let (mut updates, mut runtime_requests, completion, cancel_handle) =
            prompt_session.into_parts();
        let stream = stream! {
            let mut cancellation_guard = PromptCancellationGuard::new(cancel_handle);
            let completion = completion;
            tokio::pin!(completion);

            let mut content = String::new();
            let mut reasoning = String::new();

            loop {
                tokio::select! {
                    update = updates.recv() => {
                        match update {
                            Some(PromptUpdate::Text(delta)) => {
                                content.push_str(&delta);
                                yield Ok(LLMStreamEvent::Token { delta });
                            }
                            Some(PromptUpdate::Thought(delta)) => {
                                let delta = if !reasoning.is_empty()
                                    && !reasoning.ends_with('\n')
                                    && !delta.starts_with('\n')
                                {
                                    format!("\n{delta}")
                                } else {
                                    delta
                                };
                                reasoning.push_str(&delta);
                                yield Ok(LLMStreamEvent::Reasoning { delta });
                            }
                            None => {}
                        }
                    }
                    runtime_request = runtime_requests.recv() => {
                        if let Some(runtime_request) = runtime_request {
                            let response = match runtime_request {
                                CopilotRuntimeRequest::Permission(request) => {
                                    request.respond(crate::copilot::CopilotPermissionDecision::DeniedNoApprovalRule)
                                }
                                CopilotRuntimeRequest::ToolCall(request) => {
                                    let tool_name = request.request.tool_name.clone();
                                    request.respond(CopilotToolCallResponse::Failure(CopilotToolCallFailure {
                                        text_result_for_llm: format!(
                                            "GitHub Copilot tool execution is not available in this session mode. Tool `{tool_name}` was not executed."
                                        ),
                                        error: format!(
                                            "tool '{tool_name}' cannot be executed outside the VT Code agent runloop session"
                                        ),
                                    }))
                                }
                                CopilotRuntimeRequest::ObservedToolCall(_) => {
                                    continue;
                                }
                                CopilotRuntimeRequest::CompatibilityNotice(_) => {
                                    continue;
                                }
                            };
                            if let Err(err) = response {
                                yield Err(map_copilot_error(err));
                                break;
                            }
                        }
                    }
                    result = &mut completion => {
                        let completion = match result.context("copilot acp prompt task join failed") {
                            Ok(completion) => completion,
                            Err(err) => {
                                yield Err(map_copilot_error(err));
                                break;
                            }
                        };
                        let completion = match completion {
                            Ok(completion) => completion,
                            Err(err) => {
                                yield Err(map_copilot_error(err));
                                break;
                            }
                        };
                        let finish_reason = map_stop_reason(&completion.stop_reason);
                        while let Ok(update) = updates.try_recv() {
                            match update {
                                PromptUpdate::Text(delta) => {
                                    content.push_str(&delta);
                                    yield Ok(LLMStreamEvent::Token { delta });
                                }
                                PromptUpdate::Thought(delta) => {
                                    let delta = if !reasoning.is_empty()
                                        && !reasoning.ends_with('\n')
                                        && !delta.starts_with('\n')
                                    {
                                        format!("\n{delta}")
                                    } else {
                                        delta
                                    };
                                    reasoning.push_str(&delta);
                                    yield Ok(LLMStreamEvent::Reasoning { delta });
                                }
                            }
                        }

                        let mut response =
                            LLMResponse::new(model.request_model.clone(), content.clone());
                        response.finish_reason = finish_reason;
                        if !reasoning.is_empty() {
                            response.reasoning = Some(reasoning.clone());
                        }
                        cancellation_guard.disarm();
                        yield Ok(LLMStreamEvent::Completed {
                            response: Box::new(response),
                        });
                        break;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn start_prompt_session_impl(
        &self,
        request: LLMRequest,
        tools: &[ToolDefinition],
    ) -> Result<PromptSession, LLMError> {
        self.validate_request(&request)?;
        let model = self.resolve_model(&request)?;
        let transcript = self.build_transcript(&request)?;
        let client = self.client(&model, tools).await?;
        client
            .start_prompt(transcript)
            .await
            .map_err(map_copilot_error)
    }
}

#[async_trait]
impl LLMProvider for CopilotProvider {
    fn name(&self) -> &str {
        COPILOT_PROVIDER_KEY
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_non_streaming(&self, _model: &str) -> bool {
        false
    }

    fn supports_reasoning(&self, _model: &str) -> bool {
        true
    }

    fn supports_tools(&self, _model: &str) -> bool {
        true
    }

    fn supports_structured_output(&self, _model: &str) -> bool {
        false
    }

    fn supports_vision(&self, _model: &str) -> bool {
        false
    }

    async fn generate(&self, request: LLMRequest) -> Result<LLMResponse, LLMError> {
        let model = self.resolve_model(&request)?;
        let mut stream = self.stream(request).await?;
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut completed = None;

        use futures::StreamExt;
        while let Some(event) = stream.next().await {
            match event? {
                LLMStreamEvent::Token { delta } => content.push_str(&delta),
                LLMStreamEvent::Reasoning { delta } => reasoning.push_str(&delta),
                LLMStreamEvent::ReasoningStage { .. } => {}
                LLMStreamEvent::Completed { response } => {
                    completed = Some(*response);
                    break;
                }
            }
        }

        Ok(completed.unwrap_or_else(|| {
            let mut response = LLMResponse::new(model.request_model.clone(), content);
            if !reasoning.is_empty() {
                response.reasoning = Some(reasoning);
            }
            response
        }))
    }

    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        self.validate_request(&request)?;
        let model = self.resolve_model(&request)?;
        let transcript = self.build_transcript(&request)?;
        let client = self.client(&model, &[]).await?;
        let prompt_session = client
            .start_prompt(transcript)
            .await
            .map_err(map_copilot_error)?;
        self.stream_from_session(model, prompt_session).await
    }

    fn start_copilot_prompt_session<'a>(
        &'a self,
        request: LLMRequest,
        tools: &'a [ToolDefinition],
    ) -> Option<CopilotPromptSessionFuture<'a>> {
        Some(Box::pin(async move {
            self.start_prompt_session_impl(request, tools).await
        }))
    }

    fn supported_models(&self) -> Vec<String> {
        supported_models_for_provider(COPILOT_PROVIDER_KEY)
            .map(|models| models.iter().map(|model| (*model).to_string()).collect())
            .unwrap_or_else(|| {
                copilot_models::SUPPORTED_MODELS
                    .iter()
                    .map(|model| (*model).to_string())
                    .collect()
            })
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validate_request_common(request, "GitHub Copilot", COPILOT_PROVIDER_KEY, None)?;

        if request
            .tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty())
        {
            return Err(invalid_request(
                "GitHub Copilot in VT Code v1 does not accept VT Code tool definitions.",
            ));
        }

        if request.output_format.is_some() {
            return Err(invalid_request(
                "GitHub Copilot in VT Code v1 does not support structured output.",
            ));
        }

        Ok(())
    }
}

fn append_block(buffer: &mut String, label: &str, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    if !buffer.is_empty() {
        buffer.push_str("\n\n");
    }
    buffer.push_str(label);
    buffer.push_str(":\n");
    buffer.push_str(text.trim());
}

fn render_message_for_copilot(message: &Message) -> String {
    let mut sections = Vec::new();
    let text = message.content.as_text();
    let trimmed = text.trim();
    if !trimmed.is_empty() {
        sections.push(trimmed.to_string());
    }

    if let Some(tool_calls) = message
        .tool_calls
        .as_ref()
        .filter(|calls| !calls.is_empty())
    {
        let mut tool_history = String::from("[VT Code tool call history]");
        for call in tool_calls {
            let (tool_name, args) = call
                .function
                .as_ref()
                .map(|function| (function.name.as_str(), function.arguments.trim()))
                .unwrap_or((call.call_type.as_str(), ""));
            if args.is_empty() {
                tool_history.push_str(&format!("\n- {tool_name} id={}", call.id));
            } else {
                tool_history.push_str(&format!("\n- {tool_name} id={} args={args}", call.id));
            }
        }
        sections.push(tool_history);
    }

    if message.role == MessageRole::Tool {
        let mut tool_result = String::from("[VT Code tool result]");
        if let Some(tool_call_id) = message.tool_call_id.as_deref() {
            tool_result.push_str(&format!("\n- tool_call_id: {tool_call_id}"));
        }
        if let Some(origin_tool) = message.origin_tool.as_deref() {
            tool_result.push_str(&format!("\n- tool: {origin_tool}"));
        }
        sections.insert(0, tool_result);
    }

    let (image_count, file_count) = count_non_text_parts(message);
    if image_count > 0 {
        sections.push(format!(
            "[VT Code omitted {image_count} image input{} because GitHub Copilot v1 only accepts text input.]",
            plural_suffix(image_count)
        ));
    }
    if file_count > 0 {
        sections.push(format!(
            "[VT Code omitted {file_count} file attachment{} because GitHub Copilot v1 only accepts text input.]",
            plural_suffix(file_count)
        ));
    }

    sections.join("\n\n")
}

fn count_non_text_parts(message: &Message) -> (usize, usize) {
    match &message.content {
        crate::llm::provider::MessageContent::Text(_) => (0, 0),
        crate::llm::provider::MessageContent::Parts(parts) => {
            let image_count = parts.iter().filter(|part| part.is_image()).count();
            let file_count = parts.iter().filter(|part| part.is_file()).count();
            (image_count, file_count)
        }
    }
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn invalid_request(message: &str) -> LLMError {
    LLMError::InvalidRequest {
        message: message.to_string(),
        metadata: None,
    }
}

fn map_copilot_error(error: anyhow::Error) -> LLMError {
    let message = error.to_string();
    if message.contains("rpc error -32001") || message.contains("Authentication required") {
        return LLMError::Authentication {
            message: "GitHub Copilot authentication is required. Run `vtcode login copilot`."
                .to_string(),
            metadata: None,
        };
    }

    LLMError::Provider {
        message,
        metadata: None,
    }
}

fn map_stop_reason(stop_reason: &str) -> crate::llm::provider::FinishReason {
    match stop_reason {
        "end_turn" => crate::llm::provider::FinishReason::Stop,
        "max_tokens" => crate::llm::provider::FinishReason::Length,
        "refusal" => crate::llm::provider::FinishReason::Refusal,
        "cancelled" => crate::llm::provider::FinishReason::Error("cancelled".to_string()),
        other => crate::llm::provider::FinishReason::Error(other.to_string()),
    }
}

fn copilot_tool_signature(tools: &[ToolDefinition]) -> String {
    let mut signature_parts = tools
        .iter()
        .filter_map(|tool| {
            let function = tool.function.as_ref()?;
            Some(format!(
                "{}:{}",
                function.name,
                serde_json::to_string(&function.parameters).ok()?
            ))
        })
        .collect::<Vec<_>>();
    signature_parts.sort_unstable();
    signature_parts.join("|")
}

fn normalize_copilot_model_id(model: &str) -> Option<Option<String>> {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return None;
    }

    match trimmed {
        copilot_models::AUTO => Some(None),
        copilot_models::GPT_5_2_CODEX => Some(Some("gpt-5.2-codex".to_string())),
        copilot_models::GPT_5_1_CODEX_MAX => Some(Some("gpt-5.1-codex-max".to_string())),
        copilot_models::GPT_5_4 => Some(Some("gpt-5.4".to_string())),
        copilot_models::GPT_5_4_MINI => Some(Some("gpt-5.4-mini".to_string())),
        copilot_models::CLAUDE_SONNET_4_6 => Some(Some("claude-sonnet-4.6".to_string())),
        _ if trimmed.contains(char::is_whitespace) => None,
        _ => Some(Some(trimmed.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::CopilotProvider;
    use super::normalize_copilot_model_id;
    use crate::llm::provider::{ContentPart, LLMProvider, LLMRequest, Message, ToolCall};
    use std::path::PathBuf;
    use std::sync::Arc;
    use vtcode_config::constants::models::copilot as copilot_models;

    fn provider() -> CopilotProvider {
        CopilotProvider::from_config(None, None, Some(PathBuf::from("/tmp")))
    }

    #[test]
    fn transcript_flattens_system_user_and_assistant_messages() {
        let provider = provider();
        let request = LLMRequest {
            system_prompt: Some(Arc::new("Follow repository conventions.".to_string())),
            messages: vec![
                Message::user("Inspect the diff.".to_string()),
                Message::assistant("The diff looks safe.".to_string()),
            ],
            ..Default::default()
        };

        let transcript = provider
            .build_transcript(&request)
            .expect("transcript should build");

        assert_eq!(
            transcript,
            "System:\nFollow repository conventions.\n\nUser:\nInspect the diff.\n\nAssistant:\nThe diff looks safe."
        );
    }

    #[test]
    fn curated_model_mapping_uses_auto_as_empty_override() {
        assert_eq!(normalize_copilot_model_id(copilot_models::AUTO), Some(None));
        assert_eq!(
            normalize_copilot_model_id(copilot_models::GPT_5_4),
            Some(Some("gpt-5.4".to_string()))
        );
    }

    #[test]
    fn normalize_copilot_model_id_accepts_raw_model_ids() {
        assert_eq!(
            normalize_copilot_model_id("gpt-5.3-codex"),
            Some(Some("gpt-5.3-codex".to_string()))
        );
        assert_eq!(normalize_copilot_model_id("gpt 5.3"), None);
    }

    #[test]
    fn validate_request_allows_tool_history_followups() {
        let provider = provider();
        let request = LLMRequest {
            messages: vec![Message::tool_response(
                "call-1".to_string(),
                "tool output".to_string(),
            )],
            ..Default::default()
        };

        provider
            .validate_request(&request)
            .expect("tool history should be flattened for Copilot");
    }

    #[test]
    fn transcript_flattens_tool_history_and_image_inputs() {
        let provider = provider();
        let request = LLMRequest {
            messages: vec![
                Message::assistant_with_tools(
                    "Running checks.".to_string(),
                    vec![ToolCall::function(
                        "call-1".to_string(),
                        "unified_exec".to_string(),
                        r#"{"cmd":"cargo check"}"#.to_string(),
                    )],
                ),
                Message::tool_response_with_origin(
                    "call-1".to_string(),
                    "cargo check completed successfully.".to_string(),
                    "unified_exec".to_string(),
                ),
                Message::user_with_parts(vec![
                    ContentPart::text("Tell me more.".to_string()),
                    ContentPart::image("AAAA".to_string(), "image/png".to_string()),
                ]),
            ],
            ..Default::default()
        };

        let transcript = provider
            .build_transcript(&request)
            .expect("transcript should flatten Copilot-incompatible history");

        assert!(transcript.contains("Assistant:\nRunning checks."));
        assert!(transcript.contains("[VT Code tool call history]"));
        assert!(transcript.contains("- unified_exec id=call-1 args={\"cmd\":\"cargo check\"}"));
        assert!(transcript.contains("Tool:\n[VT Code tool result]"));
        assert!(transcript.contains("- tool_call_id: call-1"));
        assert!(transcript.contains("- tool: unified_exec"));
        assert!(transcript.contains("cargo check completed successfully."));
        assert!(transcript.contains("User:\nTell me more."));
        assert!(transcript.contains("omitted 1 image input"));
    }

    #[test]
    fn supported_models_include_copilot_auto() {
        let provider = provider();

        assert!(
            provider
                .supported_models()
                .iter()
                .any(|model| model == copilot_models::AUTO)
        );
    }

    #[test]
    fn supports_reasoning_for_alias_and_live_raw_models() {
        let provider = provider();

        assert!(provider.supports_reasoning(copilot_models::AUTO));
        assert!(provider.supports_reasoning("gpt-5.3-codex"));
    }
}
