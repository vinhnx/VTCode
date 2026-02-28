use super::super::constants::*;
use super::super::helpers::{acp_session_modes, agent_implementation_info, text_chunk};
use super::super::types::{PlanProgress, ToolRuntime};
use super::ZedAgent;
use agent_client_protocol as acp;
use anyhow::Result;
use futures::StreamExt;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::warn;
use vtcode_core::llm::factory::ProviderConfig;
use vtcode_core::llm::factory::{create_provider_for_model, create_provider_with_config};
use vtcode_core::llm::provider::{LLMRequest, LLMStreamEvent, Message};

#[async_trait::async_trait(?Send)]
impl acp::Agent for ZedAgent {
    async fn initialize(
        &self,
        args: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        self.client_capabilities
            .replace(Some(args.client_capabilities.clone()));

        if args.protocol_version != acp::ProtocolVersion::V1 {
            warn!(
                requested = %args.protocol_version,
                "{}",
                INITIALIZE_VERSION_MISMATCH_LOG
            );
        }
        let mut capabilities = acp::AgentCapabilities::default();
        capabilities.prompt_capabilities.embedded_context = true;
        capabilities.prompt_capabilities.image = true;
        capabilities.prompt_capabilities.audio = true;
        capabilities.mcp_capabilities.http = true;
        capabilities.mcp_capabilities.sse = false;
        capabilities.load_session = true;

        Ok(acp::InitializeResponse::new(acp::ProtocolVersion::V1)
            .agent_capabilities(capabilities)
            .agent_info(agent_implementation_info(self.title.clone())))
    }

    async fn authenticate(
        &self,
        _args: acp::AuthenticateRequest,
    ) -> Result<acp::AuthenticateResponse, acp::Error> {
        Ok(acp::AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        _args: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        let session_id = self.register_session();
        let available_modes = acp_session_modes();

        self.send_available_commands_update(&session_id).await?;

        let modes =
            acp::SessionModeState::new(acp::SessionModeId::from(MODE_ID_CODE), available_modes);

        Ok(acp::NewSessionResponse::new(session_id).modes(modes))
    }

    async fn load_session(
        &self,
        args: acp::LoadSessionRequest,
    ) -> Result<acp::LoadSessionResponse, acp::Error> {
        let Some(session) = self.session_handle(&args.session_id) else {
            return Err(acp::Error::invalid_params().data(json!({
                "reason": "unknown_session",
                "session_id": args.session_id.0
            })));
        };

        self.send_available_commands_update(&args.session_id)
            .await?;

        let modes = acp::SessionModeState::new(
            session.data.borrow().current_mode.clone(),
            acp_session_modes(),
        );

        Ok(acp::LoadSessionResponse::new().modes(modes))
    }

    async fn prompt(&self, args: acp::PromptRequest) -> Result<acp::PromptResponse, acp::Error> {
        let Some(session) = self.session_handle(&args.session_id) else {
            return Err(acp::Error::invalid_params().data(json!({ "reason": "unknown_session" })));
        };

        session.cancel_flag.set(false);

        let user_message = self.resolve_prompt(&args.session_id, &args.prompt).await?;

        for block in &args.prompt {
            if let acp::ContentBlock::Text(text) = block {
                self.send_update(
                    &args.session_id,
                    acp::SessionUpdate::UserMessageChunk(acp::ContentChunk::new(
                        acp::ContentBlock::Text(acp::TextContent::new(text.text.clone())),
                    )),
                )
                .await?;
            }
        }

        self.push_message(&session, Message::user(user_message.clone()));

        let provider = match create_provider_for_model(
            &self.config.model,
            self.config.api_key.clone(),
            Some(self.config.prompt_cache.clone()),
            self.config.model_behavior.clone(),
        ) {
            Ok(provider) => provider,
            Err(_) => create_provider_with_config(
                &self.config.provider,
                ProviderConfig {
                    api_key: Some(self.config.api_key.clone()),
                    base_url: None,
                    model: Some(self.config.model.clone()),
                    prompt_cache: Some(self.config.prompt_cache.clone()),
                    timeouts: None,
                    openai: None,
                    anthropic: None,
                    model_behavior: self.config.model_behavior.clone(),
                },
            )
            .map_err(acp::Error::into_internal_error)?,
        };

        let supports_streaming = provider.supports_streaming();
        let reasoning_effort = if provider.supports_reasoning_effort(&self.config.model) {
            Some(self.config.reasoning_effort)
        } else {
            None
        };

        let mut stop_reason = acp::StopReason::EndTurn;
        let mut assistant_message = String::with_capacity(4096);
        let client_supports_read_text_file = self.client_supports_read_text_file();
        let provider_supports_tools = provider.supports_tools(&self.config.model);
        let availability =
            self.tool_availability(provider_supports_tools, client_supports_read_text_file);
        let mut enabled_tools = Vec::with_capacity(5);
        let mut disabled_tools = Vec::with_capacity(5);
        for (tool, runtime) in availability {
            match runtime {
                ToolRuntime::Enabled => enabled_tools.push(tool),
                ToolRuntime::Disabled(reason) => disabled_tools.push((tool, reason)),
            }
        }
        disabled_tools.sort_by_key(|(tool, _)| tool.sort_key());
        if !disabled_tools.is_empty() && self.should_send_tool_notice(&session) {
            for (tool, reason) in &disabled_tools {
                self.log_tool_disable_reason(*tool, reason);
            }
            self.send_tool_disable_notices(&args.session_id, &disabled_tools)
                .await?;
            self.mark_tool_notice_sent(&session);
        }

        let has_local_tools = self.acp_tool_registry.has_local_tools();
        let tools_allowed =
            provider_supports_tools && (!enabled_tools.is_empty() || has_local_tools);
        let tool_definitions = self
            .tool_definitions(provider_supports_tools, &enabled_tools)
            .map(std::sync::Arc::new);
        let mut messages = self.resolved_messages(&session);
        let allow_streaming = supports_streaming && !tools_allowed;

        tracing::debug!(
            tools_allowed = tools_allowed,
            has_local_tools = has_local_tools,
            acp_tools_count = enabled_tools.len(),
            local_tools_count = tool_definitions.as_ref().map_or(0, |t| t.len()),
            "Tool configuration for ACP session"
        );

        let mut plan = PlanProgress::new(tools_allowed);
        if plan.has_entries() {
            self.send_plan_update(&args.session_id, &plan).await?;
            if plan.complete_analysis() {
                self.send_plan_update(&args.session_id, &plan).await?;
            }
        }

        if allow_streaming {
            let request = LLMRequest {
                messages: messages.clone(),
                model: self.config.model.clone(),
                stream: true,
                tools: tool_definitions,
                tool_choice: self.tool_choice(tools_allowed),
                reasoning_effort,
                ..Default::default()
            };

            let mut stream = provider
                .stream(request)
                .await
                .map_err(acp::Error::into_internal_error)?;

            if plan.start_response() {
                self.send_plan_update(&args.session_id, &plan).await?;
            }

            while let Some(event) = stream.next().await {
                let event = event.map_err(acp::Error::into_internal_error)?;

                if session.cancel_flag.get() {
                    stop_reason = acp::StopReason::Cancelled;
                    break;
                }

                match event {
                    LLMStreamEvent::Token { delta } => {
                        if !delta.is_empty() {
                            assistant_message.push_str(&delta);
                            let chunk = text_chunk(delta);
                            self.send_update(
                                &args.session_id,
                                acp::SessionUpdate::AgentMessageChunk(chunk),
                            )
                            .await?;
                        }
                    }
                    LLMStreamEvent::Reasoning { delta } => {
                        if !delta.is_empty() {
                            let chunk = text_chunk(delta);
                            self.send_update(
                                &args.session_id,
                                acp::SessionUpdate::AgentThoughtChunk(chunk),
                            )
                            .await?;
                        }
                    }
                    LLMStreamEvent::ReasoningStage { .. } => {
                        // ACP protocol doesn't currently support specific reasoning stages
                    }
                    LLMStreamEvent::Completed { response } => {
                        if assistant_message.is_empty()
                            && let Some(content) = response.content
                        {
                            if !content.is_empty() {
                                let chunk = text_chunk(content.clone());
                                self.send_update(
                                    &args.session_id,
                                    acp::SessionUpdate::AgentMessageChunk(chunk),
                                )
                                .await?;
                            }
                            assistant_message.push_str(&content);
                        }

                        if let Some(reasoning) =
                            response.reasoning.filter(|reasoning| !reasoning.is_empty())
                        {
                            let chunk = text_chunk(reasoning);
                            self.send_update(
                                &args.session_id,
                                acp::SessionUpdate::AgentThoughtChunk(chunk),
                            )
                            .await?;
                        }

                        stop_reason = Self::stop_reason_from_finish(response.finish_reason);
                        break;
                    }
                }
            }
        } else {
            loop {
                if session.cancel_flag.get() {
                    stop_reason = acp::StopReason::Cancelled;
                    break;
                }

                let request = LLMRequest {
                    messages: messages.clone(),
                    model: self.config.model.clone(),
                    tools: tool_definitions.clone(),
                    tool_choice: self.tool_choice(tools_allowed),
                    reasoning_effort,
                    ..Default::default()
                };

                let response = provider
                    .generate(request)
                    .await
                    .map_err(acp::Error::into_internal_error)?;

                if session.cancel_flag.get() {
                    stop_reason = acp::StopReason::Cancelled;
                    break;
                }

                if tools_allowed
                    && let Some(tool_calls) = response
                        .tool_calls
                        .clone()
                        .filter(|calls| !calls.is_empty())
                {
                    if plan.start_context() {
                        self.send_plan_update(&args.session_id, &plan).await?;
                    }
                    self.push_message(
                        &session,
                        Message::assistant_with_tools(
                            response.content.clone().unwrap_or_default(),
                            tool_calls.clone(),
                        ),
                    );
                    let tool_results = self
                        .execute_tool_calls(&session, &args.session_id, &tool_calls)
                        .await?;
                    if plan.complete_context() {
                        self.send_plan_update(&args.session_id, &plan).await?;
                    }
                    for result in tool_results {
                        self.push_message(
                            &session,
                            Message::tool_response(result.tool_call_id, result.llm_response),
                        );
                    }
                    if session.cancel_flag.get() {
                        stop_reason = acp::StopReason::Cancelled;
                        break;
                    }
                    messages = self.resolved_messages(&session);
                    continue;
                }

                if let Some(content) = &response.content {
                    if !content.is_empty() {
                        if plan.has_context_step()
                            && !plan.context_completed()
                            && plan.complete_context()
                        {
                            self.send_plan_update(&args.session_id, &plan).await?;
                        }
                        if plan.start_response() {
                            self.send_plan_update(&args.session_id, &plan).await?;
                        }
                        if session.cancel_flag.get() {
                            stop_reason = acp::StopReason::Cancelled;
                            break;
                        }
                        let chunk = text_chunk(content.clone());
                        self.send_update(
                            &args.session_id,
                            acp::SessionUpdate::AgentMessageChunk(chunk),
                        )
                        .await?;
                    }
                    assistant_message = content.clone();
                }

                if let Some(reasoning) =
                    response.reasoning.filter(|reasoning| !reasoning.is_empty())
                {
                    if session.cancel_flag.get() {
                        stop_reason = acp::StopReason::Cancelled;
                        break;
                    }
                    let chunk = text_chunk(reasoning);
                    self.send_update(
                        &args.session_id,
                        acp::SessionUpdate::AgentThoughtChunk(chunk),
                    )
                    .await?;
                }

                stop_reason = Self::stop_reason_from_finish(response.finish_reason);
                break;
            }
        }

        if stop_reason != acp::StopReason::Cancelled && !assistant_message.is_empty() {
            self.push_message(&session, Message::assistant(assistant_message));
        }

        if stop_reason != acp::StopReason::Cancelled {
            if plan.complete_context() {
                self.send_plan_update(&args.session_id, &plan).await?;
            }
            if plan.complete_response() {
                self.send_plan_update(&args.session_id, &plan).await?;
            }
        }

        Ok(acp::PromptResponse::new(stop_reason))
    }

    async fn set_session_mode(
        &self,
        args: acp::SetSessionModeRequest,
    ) -> Result<acp::SetSessionModeResponse, acp::Error> {
        let Some(session) = self.session_handle(&args.session_id) else {
            return Err(acp::Error::invalid_params().data(json!({ "reason": "unknown_session" })));
        };

        let valid_modes: HashSet<Arc<str>> = [
            Arc::from(MODE_ID_ASK),
            Arc::from(MODE_ID_ARCHITECT),
            Arc::from(MODE_ID_CODE),
        ]
        .into_iter()
        .collect();
        if !valid_modes.contains(&args.mode_id.0) {
            return Err(acp::Error::invalid_params()
                .data(json!({ "reason": "unknown_mode", "mode_id": args.mode_id.0 })));
        }

        if self.update_session_mode(&session, args.mode_id.clone()) {
            self.send_update(
                &args.session_id,
                acp::SessionUpdate::CurrentModeUpdate(acp::CurrentModeUpdate::new(
                    args.mode_id.clone(),
                )),
            )
            .await?;
        }

        Ok(acp::SetSessionModeResponse::new())
    }

    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        if let Some(session) = self.session_handle(&args.session_id) {
            session.cancel_flag.set(true);
        }
        Ok(())
    }
}
