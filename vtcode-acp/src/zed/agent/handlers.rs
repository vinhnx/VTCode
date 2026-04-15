use super::super::constants::*;
use super::super::helpers::{
    SESSION_CONFIG_MODE_ID, SESSION_CONFIG_MODEL_ID, SESSION_CONFIG_PROVIDER_ID,
    SESSION_CONFIG_THOUGHT_LEVEL_ID, acp_session_modes, agent_implementation_info, session_mode_id,
    text_chunk,
};
use super::super::types::{PlanProgress, ToolRuntime};
use super::ZedAgent;
use agent_client_protocol as acp;
use anyhow::Result;
use futures::StreamExt;
use serde_json::json;
use tracing::warn;
use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::core::interfaces::SessionMode;
use vtcode_core::llm::factory::ProviderConfig;
use vtcode_core::llm::factory::create_provider_with_config;
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

        let auth_methods = vec![
            // Agent Auth (OAuth)
            acp::AuthMethod::Agent(
                acp::AuthMethodAgent::new("oauth-openai", "OpenAI OAuth")
                    .description("Authenticate with OpenAI via OAuth 2.0 with PKCE"),
            ),
            acp::AuthMethod::Agent(
                acp::AuthMethodAgent::new("oauth-openrouter", "OpenRouter OAuth")
                    .description("Authenticate with OpenRouter via OAuth 2.0 with PKCE"),
            ),
            // Terminal Auth (Interactive Login)
            acp::AuthMethod::Terminal(
                acp::AuthMethodTerminal::new("terminal-login", "Terminal Login")
                    .description(
                        "Interactive terminal-based authentication via vtcode login command",
                    )
                    .args(vec!["login".to_string()]),
            ),
            // Env Var Auth (API Keys)
            acp::AuthMethod::EnvVar(acp::AuthMethodEnvVar::new(
                "env-api-keys",
                "API Key",
                vec![
                    acp::AuthEnvVar::new("OPENAI_API_KEY").label("OpenAI"),
                    acp::AuthEnvVar::new("ANTHROPIC_API_KEY").label("Anthropic"),
                    acp::AuthEnvVar::new("GEMINI_API_KEY").label("Google Gemini"),
                    acp::AuthEnvVar::new("OPENROUTER_API_KEY").label("OpenRouter"),
                    acp::AuthEnvVar::new("DEEPSEEK_API_KEY").label("DeepSeek"),
                    acp::AuthEnvVar::new("ZAI_API_KEY").label("Z.AI"),
                    acp::AuthEnvVar::new("MOONSHOT_API_KEY").label("Moonshot"),
                    acp::AuthEnvVar::new("MINIMAX_API_KEY").label("MiniMax"),
                    acp::AuthEnvVar::new("GROQ_API_KEY").label("Groq"),
                    acp::AuthEnvVar::new("XAI_API_KEY").label("xAI"),
                    acp::AuthEnvVar::new("COHERE_API_KEY").label("Cohere"),
                    acp::AuthEnvVar::new("HF_TOKEN").label("Hugging Face"),
                    acp::AuthEnvVar::new("MISTRAL_API_KEY").label("Mistral"),
                    acp::AuthEnvVar::new("GOOGLE_API_KEY")
                        .label("Google (alt)")
                        .optional(true),
                    acp::AuthEnvVar::new("OLLAMA_API_KEY")
                        .label("Ollama")
                        .optional(true),
                    acp::AuthEnvVar::new("LMSTUDIO_API_KEY")
                        .label("LM Studio")
                        .optional(true),
                ],
            )),
            // Env Var Auth (Base URLs)
            acp::AuthMethod::EnvVar(acp::AuthMethodEnvVar::new(
                "env-base-urls",
                "API Base URL",
                vec![
                    acp::AuthEnvVar::new("OPENAI_BASE_URL")
                        .label("OpenAI")
                        .optional(true),
                    acp::AuthEnvVar::new("ANTHROPIC_BASE_URL")
                        .label("Anthropic")
                        .optional(true),
                    acp::AuthEnvVar::new("GEMINI_BASE_URL")
                        .label("Gemini")
                        .optional(true),
                    acp::AuthEnvVar::new("OPENROUTER_BASE_URL")
                        .label("OpenRouter")
                        .optional(true),
                    acp::AuthEnvVar::new("DEEPSEEK_BASE_URL")
                        .label("DeepSeek")
                        .optional(true),
                    acp::AuthEnvVar::new("ZAI_BASE_URL")
                        .label("Z.AI")
                        .optional(true),
                    acp::AuthEnvVar::new("MOONSHOT_BASE_URL")
                        .label("Moonshot")
                        .optional(true),
                    acp::AuthEnvVar::new("MINIMAX_BASE_URL")
                        .label("MiniMax")
                        .optional(true),
                    acp::AuthEnvVar::new("XAI_BASE_URL")
                        .label("xAI")
                        .optional(true),
                    acp::AuthEnvVar::new("HUGGINGFACE_BASE_URL")
                        .label("Hugging Face")
                        .optional(true),
                    acp::AuthEnvVar::new("OLLAMA_BASE_URL")
                        .label("Ollama")
                        .optional(true),
                    acp::AuthEnvVar::new("LMSTUDIO_BASE_URL")
                        .label("LM Studio")
                        .optional(true),
                ],
            )),
        ];

        Ok(acp::InitializeResponse::new(acp::ProtocolVersion::V1)
            .agent_capabilities(capabilities)
            .agent_info(agent_implementation_info(self.title.clone()))
            .auth_methods(auth_methods))
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
        let session = self
            .session_handle(&session_id)
            .ok_or_else(acp::Error::internal_error)?;
        let available_modes = acp_session_modes();
        let config_options = self.current_session_config_options(&session);

        self.send_available_commands_update(&session_id).await?;

        let modes = acp::SessionModeState::new(session_mode_id(SessionMode::Code), available_modes);

        Ok(acp::NewSessionResponse::new(session_id)
            .modes(modes)
            .config_options(config_options))
    }

    async fn load_session(
        &self,
        args: acp::LoadSessionRequest,
    ) -> Result<acp::LoadSessionResponse, acp::Error> {
        let session = if let Some(session) = self.session_handle(&args.session_id) {
            session
        } else {
            let identifier = args.session_id.0.as_ref();
            self.attach_thread_from_archive(&args.session_id, identifier)
                .await
                .map_err(|err| acp::Error::internal_error().data(err.to_string()))?
        };

        self.send_available_commands_update(&args.session_id)
            .await?;

        let modes = acp::SessionModeState::new(
            session_mode_id(session.data.borrow().current_mode),
            acp_session_modes(),
        );
        let config_options = self.current_session_config_options(&session);

        Ok(acp::LoadSessionResponse::new()
            .modes(modes)
            .config_options(config_options))
    }

    async fn prompt(&self, args: acp::PromptRequest) -> Result<acp::PromptResponse, acp::Error> {
        let Some(session) = self.session_handle(&args.session_id) else {
            return Err(acp::Error::invalid_params().data(json!({ "reason": "unknown_session" })));
        };

        session.cancel_flag.set(false);

        let user_message = self.resolve_prompt(&args.session_id, &args.prompt).await?;

        self.push_message(&session, Message::user(user_message.clone()));

        let (session_provider_name, session_model, session_reasoning_effort) = {
            let data = session.data.borrow();
            (
                data.provider.clone(),
                data.model.clone(),
                data.reasoning_effort,
            )
        };
        let session_api_key = self.resolve_api_key_for_provider(&session_provider_name);
        let provider = create_provider_with_config(
            &session_provider_name,
            ProviderConfig {
                api_key: Some(session_api_key),
                openai_chatgpt_auth: if session_provider_name.eq_ignore_ascii_case("openai") {
                    self.config.openai_chatgpt_auth.clone()
                } else {
                    None
                },
                copilot_auth: None,
                base_url: None,
                model: Some(session_model.clone()),
                prompt_cache: Some(self.config.prompt_cache.clone()),
                timeouts: None,
                openai: None,
                anthropic: None,
                model_behavior: self.config.model_behavior.clone(),
                workspace_root: Some(self.config.workspace.clone()),
            },
        )
        .map_err(acp::Error::into_internal_error)?;

        let supports_streaming = provider.supports_streaming();
        let reasoning_effort = if provider.supports_reasoning_effort(&session_model) {
            Some(session_reasoning_effort)
        } else {
            None
        };

        let mut stop_reason = acp::StopReason::EndTurn;
        let mut assistant_message = String::with_capacity(4096);
        let client_supports_read_text_file = self.client_supports_read_text_file();
        let provider_supports_tools = provider.supports_tools(&session_model);
        let mut session_mode = session.data.borrow().current_mode;
        let availability = self.tool_availability(
            provider_supports_tools,
            client_supports_read_text_file,
            &session_provider_name,
            &session_model,
        );
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

        let mut has_local_tools = self.local_tools_available(session_mode);
        let mut tools_allowed =
            provider_supports_tools && (!enabled_tools.is_empty() || has_local_tools);
        let mut tool_definitions = self
            .tool_definitions(provider_supports_tools, &enabled_tools, session_mode)
            .map(std::sync::Arc::new);
        let mut messages = self.resolved_messages(&session);
        let allow_streaming = supports_streaming && !tools_allowed;

        tracing::debug!(
            session_mode = session_mode.as_str(),
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
                model: session_model.clone(),
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
                    model: session_model.clone(),
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
                    session_mode = session.data.borrow().current_mode;
                    has_local_tools = self.local_tools_available(session_mode);
                    tools_allowed =
                        provider_supports_tools && (!enabled_tools.is_empty() || has_local_tools);
                    tool_definitions = self
                        .tool_definitions(provider_supports_tools, &enabled_tools, session_mode)
                        .map(std::sync::Arc::new);
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

        let Some(mode) = SessionMode::parse(args.mode_id.0.as_ref()) else {
            return Err(acp::Error::invalid_params()
                .data(json!({ "reason": "unknown_mode", "mode_id": args.mode_id.0 })));
        };

        let _ = self
            .apply_session_mode(&args.session_id, &session, mode)
            .await?;

        Ok(acp::SetSessionModeResponse::new())
    }

    async fn set_session_config_option(
        &self,
        args: acp::SetSessionConfigOptionRequest,
    ) -> Result<acp::SetSessionConfigOptionResponse, acp::Error> {
        let Some(session) = self.session_handle(&args.session_id) else {
            return Err(acp::Error::invalid_params().data(json!({ "reason": "unknown_session" })));
        };

        match args.config_id.0.as_ref() {
            SESSION_CONFIG_MODE_ID => {
                let Some(mode) = SessionMode::parse(args.value.0.as_ref()) else {
                    return Err(acp::Error::invalid_params().data(json!({
                        "reason": "unknown_mode",
                        "mode_id": args.value.0,
                    })));
                };

                let _ = self
                    .apply_session_mode(&args.session_id, &session, mode)
                    .await?;
            }
            SESSION_CONFIG_THOUGHT_LEVEL_ID => {
                let (session_provider, session_model) = {
                    let data = session.data.borrow();
                    (data.provider.clone(), data.model.clone())
                };
                if !self.model_supports_thought_level(&session_provider, &session_model) {
                    return Err(acp::Error::invalid_params().data(json!({
                        "reason": "unsupported_config_option",
                        "config_id": args.config_id.0,
                    })));
                }
                let Some(reasoning_effort) = ReasoningEffortLevel::parse(args.value.0.as_ref())
                else {
                    return Err(acp::Error::invalid_params().data(json!({
                        "reason": "unknown_thought_level",
                        "value": args.value.0,
                    })));
                };

                if self.update_session_reasoning_effort(&session, reasoning_effort) {
                    let config_options = self.current_session_config_options(&session);
                    self.send_update(
                        &args.session_id,
                        acp::SessionUpdate::ConfigOptionUpdate(acp::ConfigOptionUpdate::new(
                            config_options,
                        )),
                    )
                    .await?;
                }
            }
            SESSION_CONFIG_PROVIDER_ID => {
                let provider = args.value.0.as_ref().trim().to_lowercase();
                let current_provider = session.data.borrow().provider.clone();
                if provider.is_empty() || !self.supports_provider(&provider, &current_provider) {
                    return Err(acp::Error::invalid_params().data(json!({
                        "reason": "unknown_provider",
                        "value": args.value.0,
                    })));
                }

                let current_model = session.data.borrow().model.clone();
                let resolved_model = if self.provider_supports_model(&provider, &current_model) {
                    current_model
                } else {
                    self.provider_default_model(&provider).ok_or_else(|| {
                        acp::Error::invalid_params().data(json!({
                            "reason": "provider_has_no_default_model",
                            "provider": provider,
                        }))
                    })?
                };

                if self.update_session_provider_and_model(
                    &session,
                    provider.to_string(),
                    resolved_model,
                ) {
                    let config_options = self.current_session_config_options(&session);
                    self.send_update(
                        &args.session_id,
                        acp::SessionUpdate::ConfigOptionUpdate(acp::ConfigOptionUpdate::new(
                            config_options,
                        )),
                    )
                    .await?;
                }
            }
            SESSION_CONFIG_MODEL_ID => {
                let model = args.value.0.as_ref().trim();
                if model.is_empty() {
                    return Err(acp::Error::invalid_params().data(json!({
                        "reason": "unknown_model",
                        "value": args.value.0,
                    })));
                }
                let provider = session.data.borrow().provider.clone();
                if !self.provider_supports_model(&provider, model) {
                    return Err(acp::Error::invalid_params().data(json!({
                        "reason": "model_not_supported_for_provider",
                        "provider": provider,
                        "model": model,
                    })));
                }

                if self.update_session_provider_and_model(&session, provider, model.to_string()) {
                    let config_options = self.current_session_config_options(&session);
                    self.send_update(
                        &args.session_id,
                        acp::SessionUpdate::ConfigOptionUpdate(acp::ConfigOptionUpdate::new(
                            config_options,
                        )),
                    )
                    .await?;
                }
            }
            _ => {
                return Err(acp::Error::invalid_params().data(json!({
                    "reason": "unknown_config_option",
                    "config_id": args.config_id.0,
                })));
            }
        }

        Ok(acp::SetSessionConfigOptionResponse::new(
            self.current_session_config_options(&session),
        ))
    }

    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        if let Some(session) = self.session_handle(&args.session_id) {
            session.cancel_flag.set(true);
        }
        Ok(())
    }
}

impl ZedAgent {
    fn resolve_api_key_for_provider(&self, provider: &str) -> String {
        if provider.eq_ignore_ascii_case(&self.config.provider) && !self.config.api_key.is_empty() {
            return self.config.api_key.clone();
        }

        get_api_key(provider, &ApiKeySources::default()).unwrap_or_default()
    }
}
