#![allow(clippy::result_large_err)]

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use async_stream::stream;
use async_trait::async_trait;
use tokio::sync::Mutex;
use vtcode_config::auth::CopilotAuthConfig;
use vtcode_config::constants::models::copilot as copilot_models;

use crate::copilot::{
    COPILOT_MODEL_ID, COPILOT_PROVIDER_KEY, CopilotAcpClient, PromptUpdate, probe_auth_status,
};
use crate::llm::provider::{
    LLMError, LLMProvider, LLMRequest, LLMResponse, LLMStream, LLMStreamEvent, Message, MessageRole,
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
    client: Arc<CopilotAcpClient>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedCopilotModel {
    request_model: String,
    raw_model: Option<&'static str>,
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
    ) -> Result<Arc<CopilotAcpClient>, LLMError> {
        if let Some(client) = self.cached_client(model).await {
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
            CopilotAcpClient::connect(&self.auth_config, &self.workspace_root, model.raw_model)
                .await
                .map_err(map_copilot_error)?,
        );

        let mut client = self.client.lock().await;
        if let Some(existing) = client.as_ref()
            && existing.raw_model.as_deref() == model.raw_model
        {
            return Ok(existing.client.clone());
        }
        *client = Some(CachedCopilotClient {
            raw_model: model.raw_model.map(ToString::to_string),
            client: created.clone(),
        });
        Ok(created)
    }

    async fn cached_client(&self, model: &ResolvedCopilotModel) -> Option<Arc<CopilotAcpClient>> {
        let client = self.client.lock().await;
        client
            .as_ref()
            .filter(|cached| cached.raw_model.as_deref() == model.raw_model)
            .map(|cached| cached.client.clone())
    }

    fn resolve_model(&self, request: &LLMRequest) -> Result<ResolvedCopilotModel, LLMError> {
        let requested = if request.model.trim().is_empty() {
            self.model.trim()
        } else {
            request.model.trim()
        };

        let raw_model = map_copilot_model_id(requested).ok_or_else(|| {
            invalid_request(&format!(
                "Unsupported GitHub Copilot model: {requested}. Choose one of {}.",
                copilot_models::SUPPORTED_MODELS.join(", ")
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
            validate_message_for_copilot(message)?;
            let label = match message.role {
                MessageRole::System => "System",
                MessageRole::User => "User",
                MessageRole::Assistant => "Assistant",
                MessageRole::Tool => unreachable!(),
            };
            append_block(&mut transcript, label, message.content.as_text().as_ref());
        }

        Ok(transcript)
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

    fn supports_tools(&self, _model: &str) -> bool {
        false
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
        let client = self.client(&model).await?;
        let prompt_session = client
            .start_prompt(transcript)
            .await
            .map_err(map_copilot_error)?;

        let stream = stream! {
            let mut updates = prompt_session.updates;
            let completion = prompt_session.completion;
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
                                reasoning.push_str(&delta);
                                yield Ok(LLMStreamEvent::Reasoning { delta });
                            }
                            None => {}
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

    fn supported_models(&self) -> Vec<String> {
        copilot_models::SUPPORTED_MODELS
            .iter()
            .map(|model| (*model).to_string())
            .collect()
    }

    fn validate_request(&self, request: &LLMRequest) -> Result<(), LLMError> {
        validate_request_common(
            request,
            "GitHub Copilot",
            COPILOT_PROVIDER_KEY,
            Some(&self.supported_models()),
        )?;

        if request
            .tools
            .as_ref()
            .is_some_and(|tools| !tools.is_empty())
        {
            return Err(invalid_request(
                "GitHub Copilot in VT Code is text-only in v1 and does not support tools.",
            ));
        }

        if request.output_format.is_some() {
            return Err(invalid_request(
                "GitHub Copilot in VT Code is text-only in v1 and does not support structured output.",
            ));
        }

        if request.messages.iter().any(|message| {
            message.role == MessageRole::Tool
                || message.has_tool_calls()
                || message.content.has_images()
        }) {
            return Err(invalid_request(
                "GitHub Copilot in VT Code is text-only in v1 and does not support tool or vision history.",
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

fn validate_message_for_copilot(message: &Message) -> Result<(), LLMError> {
    if message.role == MessageRole::Tool || message.has_tool_calls() {
        return Err(invalid_request(
            "GitHub Copilot in VT Code is text-only in v1 and does not support tool history.",
        ));
    }
    if message.content.has_images() {
        return Err(invalid_request(
            "GitHub Copilot in VT Code is text-only in v1 and does not support image inputs.",
        ));
    }
    Ok(())
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

fn map_copilot_model_id(model: &str) -> Option<Option<&'static str>> {
    match model {
        copilot_models::AUTO => Some(None),
        copilot_models::GPT_5_2_CODEX => Some(Some("gpt-5.2-codex")),
        copilot_models::GPT_5_1_CODEX_MAX => Some(Some("gpt-5.1-codex-max")),
        copilot_models::GPT_5_4 => Some(Some("gpt-5.4")),
        copilot_models::GPT_5_4_MINI => Some(Some("gpt-5.4-mini")),
        copilot_models::CLAUDE_SONNET_4_6 => Some(Some("claude-sonnet-4.6")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::CopilotProvider;
    use super::map_copilot_model_id;
    use crate::llm::provider::{LLMProvider, LLMRequest, Message};
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
        assert_eq!(map_copilot_model_id(copilot_models::AUTO), Some(None));
        assert_eq!(
            map_copilot_model_id(copilot_models::GPT_5_4),
            Some(Some("gpt-5.4"))
        );
    }

    #[test]
    fn validate_request_rejects_tool_history() {
        let provider = provider();
        let request = LLMRequest {
            messages: vec![Message::tool_response(
                "call-1".to_string(),
                "tool output".to_string(),
            )],
            ..Default::default()
        };

        let err = provider
            .validate_request(&request)
            .expect_err("tool history should be rejected");

        assert!(
            err.to_string()
                .contains("does not support tool or vision history")
        );
    }

    #[test]
    fn supported_models_return_curated_copilot_set() {
        let provider = provider();

        assert_eq!(
            provider.supported_models(),
            copilot_models::SUPPORTED_MODELS
                .iter()
                .map(|model| (*model).to_string())
                .collect::<Vec<_>>()
        );
    }
}
