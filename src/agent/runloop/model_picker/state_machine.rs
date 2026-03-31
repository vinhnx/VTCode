use vtcode_core::config::api_keys::{ApiKeySources, get_api_key};

use super::selection::supports_gpt5_none_reasoning;
use super::*;
use crate::agent::runloop::unified::external_url_guard::{
    ExternalUrlGuardContext, ExternalUrlOpenOutcome, request_external_url_open,
};

impl ModelPickerState {
    pub(super) fn handle_reasoning(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
    ) -> Result<ModelPickerProgress> {
        if self.selection.is_none() {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        }

        let normalized = input.to_ascii_lowercase();
        if matches!(normalized.as_str(), "off" | "disable") {
            return self.apply_reasoning_off_choice(renderer);
        }

        let level = match normalized.as_str() {
            "none" => Some(ReasoningEffortLevel::None),
            "easy" | "low" => Some(ReasoningEffortLevel::Low),
            "medium" => Some(ReasoningEffortLevel::Medium),
            "hard" | "high" => Some(ReasoningEffortLevel::High),
            "skip" => Some(self.current_reasoning),
            _ => None,
        };

        let Some(selected) = level else {
            renderer.line(
                MessageStyle::Error,
                "Unknown reasoning option. Use none, low, medium, high, skip, or off.",
            )?;
            if let Some(progress) = self.prompt_reasoning_step(renderer)? {
                return Ok(progress);
            }
            return Ok(ModelPickerProgress::InProgress);
        };

        self.apply_reasoning_choice(renderer, selected)
    }

    fn prompt_reasoning_step(
        &mut self,
        renderer: &mut AnsiRenderer,
    ) -> Result<Option<ModelPickerProgress>> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        };
        if self.inline_enabled {
            render_reasoning_inline(renderer, selection, self.current_reasoning)?;
            return Ok(None);
        }

        match select_reasoning_with_ratatui(selection, self.current_reasoning) {
            Ok(Some(ReasoningChoice::Level(level))) => {
                self.apply_reasoning_choice(renderer, level).map(Some)
            }
            Ok(Some(ReasoningChoice::Disable)) => {
                self.apply_reasoning_off_choice(renderer).map(Some)
            }
            Ok(None) => {
                prompt_reasoning_plain(renderer, selection, self.current_reasoning)?;
                Ok(None)
            }
            Err(err) => {
                if err.is::<SelectionInterrupted>() {
                    return Err(err);
                }
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Interactive reasoning selector unavailable ({}). Falling back to manual input.",
                        err
                    ),
                )?;
                prompt_reasoning_plain(renderer, selection, self.current_reasoning)?;
                Ok(None)
            }
        }
    }

    fn prompt_api_key_step(&mut self, renderer: &mut AnsiRenderer) -> Result<()> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("API key requested before selecting a model"));
        };
        if self.inline_enabled {
            show_secure_api_modal(renderer, selection, self.workspace.as_deref());
        }
        prompt_api_key_plain(renderer, selection, self.workspace.as_deref())
    }

    fn prompt_service_tier_step(
        &mut self,
        renderer: &mut AnsiRenderer,
    ) -> Result<Option<ModelPickerProgress>> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("Service tier requested before selecting a model"));
        };
        if self.inline_enabled {
            render_service_tier_inline(renderer, selection, self.current_service_tier)?;
            return Ok(None);
        }

        match select_service_tier_with_ratatui(selection, self.current_service_tier) {
            Ok(Some(ServiceTierChoice::ProjectDefault)) => {
                self.apply_service_tier_choice(renderer, None).map(Some)
            }
            Ok(Some(ServiceTierChoice::Flex)) => self
                .apply_service_tier_choice(renderer, Some(OpenAIServiceTier::Flex))
                .map(Some),
            Ok(Some(ServiceTierChoice::Priority)) => self
                .apply_service_tier_choice(renderer, Some(OpenAIServiceTier::Priority))
                .map(Some),
            Ok(None) => {
                prompt_service_tier_plain(renderer, selection, self.current_service_tier)?;
                Ok(None)
            }
            Err(err) => {
                if err.is::<SelectionInterrupted>() {
                    return Err(err);
                }
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Interactive service tier selector unavailable ({}). Falling back to manual input.",
                        err
                    ),
                )?;
                prompt_service_tier_plain(renderer, selection, self.current_service_tier)?;
                Ok(None)
            }
        }
    }

    fn continue_after_reasoning(
        &mut self,
        renderer: &mut AnsiRenderer,
    ) -> Result<ModelPickerProgress> {
        if self
            .selection
            .as_ref()
            .map(|detail| detail.service_tier_supported)
            .unwrap_or(false)
        {
            self.step = PickerStep::AwaitServiceTier;
            if let Some(progress) = self.prompt_service_tier_step(renderer)? {
                return Ok(progress);
            }
            return Ok(ModelPickerProgress::InProgress);
        }

        self.finish_after_service_tier(renderer)
    }

    fn finish_after_service_tier(
        &mut self,
        renderer: &mut AnsiRenderer,
    ) -> Result<ModelPickerProgress> {
        if self
            .selection
            .as_ref()
            .map(|detail| detail.requires_api_key)
            .unwrap_or(false)
        {
            self.step = PickerStep::AwaitApiKey;
            self.prompt_api_key_step(renderer)?;
            return Ok(ModelPickerProgress::InProgress);
        }

        let result = self.build_result();
        Ok(ModelPickerProgress::Completed(result?))
    }

    pub(super) fn apply_reasoning_choice(
        &mut self,
        renderer: &mut AnsiRenderer,
        level: ReasoningEffortLevel,
    ) -> Result<ModelPickerProgress> {
        let Some(_selection) = self.selection.as_ref() else {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        };
        self.selected_reasoning = Some(level);
        self.continue_after_reasoning(renderer)
    }

    pub(super) fn apply_reasoning_off_choice(
        &mut self,
        renderer: &mut AnsiRenderer,
    ) -> Result<ModelPickerProgress> {
        let Some(current_selection) = self.selection.clone() else {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        };

        // For GPT-5.2 and GPT-5.3 Codex models, disable reasoning by setting effort to "none" on the same model
        // rather than switching to a different model
        if supports_gpt5_none_reasoning(&current_selection.model_id) {
            self.selected_reasoning = Some(ReasoningEffortLevel::None);
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Reasoning disabled for {} by setting effort to 'none'.",
                    current_selection.model_display
                ),
            )?;

            return self.continue_after_reasoning(renderer);
        }

        let Some(target_model) = current_selection.reasoning_off_model else {
            renderer.line(
                MessageStyle::Error,
                "This model does not have a non-reasoning variant.",
            )?;
            if self.inline_enabled {
                render_reasoning_inline(renderer, &current_selection, self.current_reasoning)?;
            } else {
                prompt_reasoning_plain(renderer, &current_selection, self.current_reasoning)?;
            }
            return Ok(ModelPickerProgress::InProgress);
        };

        let Some(option) = self
            .options
            .iter()
            .find(|candidate| candidate.id.eq_ignore_ascii_case(target_model.as_str()))
        else {
            renderer.line(
                MessageStyle::Error,
                &format!(
                    "Unable to locate the non-reasoning variant {}.",
                    target_model.as_str()
                ),
            )?;
            if self.inline_enabled {
                render_reasoning_inline(renderer, &current_selection, self.current_reasoning)?;
            } else {
                prompt_reasoning_plain(renderer, &current_selection, self.current_reasoning)?;
            }
            return Ok(ModelPickerProgress::InProgress);
        };

        self.selected_reasoning = None;
        let mut new_selection = selection_from_option(option);
        if new_selection.provider_label != current_selection.provider_label {
            new_selection.provider_label = current_selection.provider_label.clone();
        }
        let alt_display = new_selection.model_display.clone();
        let alt_id = new_selection.model_id.clone();

        let progress = self.process_model_selection(renderer, new_selection)?;
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Reasoning disabled by switching to {} ({}).",
                alt_display, alt_id
            ),
        )?;
        Ok(progress)
    }

    pub(super) fn build_result(&self) -> Result<ModelSelectionResult> {
        let selection = self
            .selection
            .as_ref()
            .ok_or_else(|| anyhow!("Model selection missing"))?;
        let chosen_reasoning = self.selected_reasoning.unwrap_or(self.current_reasoning);
        let reasoning_changed = chosen_reasoning != self.current_reasoning;
        let chosen_service_tier = self
            .selected_service_tier
            .unwrap_or(self.current_service_tier);
        let service_tier_changed = chosen_service_tier != self.current_service_tier;

        Ok(ModelSelectionResult {
            provider: selection.provider_key.clone(),
            provider_label: selection.provider_label.clone(),
            provider_enum: selection.provider_enum,
            model: selection.model_id.clone(),
            model_display: selection.model_display.clone(),
            known_model: selection.known_model,
            reasoning_supported: selection.reasoning_supported,
            reasoning: chosen_reasoning,
            reasoning_changed,
            service_tier_supported: selection.service_tier_supported,
            service_tier: chosen_service_tier,
            service_tier_changed,
            api_key: self.pending_api_key.clone(),
            env_key: selection.env_key.clone(),
            requires_api_key: selection.requires_api_key,
            uses_chatgpt_auth: selection.uses_chatgpt_auth,
        })
    }

    pub(super) fn process_model_selection(
        &mut self,
        renderer: &mut AnsiRenderer,
        selection: SelectionDetail,
    ) -> Result<ModelPickerProgress> {
        let message = format!(
            "Selected {} ({}) from {}.",
            selection.model_display, selection.model_id, selection.provider_label
        );
        renderer.line(MessageStyle::Info, &message)?;

        if matches!(selection.provider_enum, Some(Provider::HuggingFace)) {
            renderer.line(
                MessageStyle::Info,
                "Hugging Face uses HF_TOKEN (stored in workspace .env) and honors HUGGINGFACE_BASE_URL (default: https://router.huggingface.co/v1).",
            )?;
            if selection.requires_api_key {
                renderer.line(
                    MessageStyle::Info,
                    "No HF_TOKEN detected; you'll be prompted to paste it and we'll save it to your workspace .env.",
                )?;
            }
        }

        self.pending_api_key = None;
        self.selected_service_tier = None;
        let mut selection = selection;
        if selection.requires_api_key {
            match self.find_existing_api_key(&selection.provider_key, &selection.env_key) {
                Ok(Some(ExistingKey::OAuthToken)) => {
                    selection.requires_api_key = false;
                    if matches!(selection.provider_enum, Some(Provider::OpenAI)) {
                        selection.uses_chatgpt_auth = true;
                    }
                    renderer.line(MessageStyle::Info, &oauth_auth_message(&selection))?;
                }
                Ok(Some(ExistingKey::Environment)) => {
                    selection.requires_api_key = false;
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Using existing environment variable {} for {}.",
                            selection.env_key, selection.provider_label
                        ),
                    )?;
                }
                Ok(Some(ExistingKey::WorkspaceDotenv)) => {
                    selection.requires_api_key = false;
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Loaded {} from workspace .env for {}.",
                            selection.env_key, selection.provider_label
                        ),
                    )?;
                }
                Ok(Some(ExistingKey::StoredCredential)) => {
                    selection.requires_api_key = false;
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Using stored API key for {}.", selection.provider_label),
                    )?;
                }
                Ok(None) => {}
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "Failed to inspect stored credentials for {}: {}",
                            selection.provider_label, err
                        ),
                    )?;
                }
            }
        }

        self.selection = Some(selection);
        if self
            .selection
            .as_ref()
            .map(|detail| detail.reasoning_supported)
            .unwrap_or(false)
        {
            self.step = PickerStep::AwaitReasoning;
            if let Some(progress) = self.prompt_reasoning_step(renderer)? {
                return Ok(progress);
            }
            return Ok(ModelPickerProgress::InProgress);
        }

        self.continue_after_reasoning(renderer)
    }

    pub(super) fn handle_service_tier(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
    ) -> Result<ModelPickerProgress> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("Service tier requested before selecting a model"));
        };

        match input.to_ascii_lowercase().as_str() {
            "flex" => self.apply_service_tier_choice(renderer, Some(OpenAIServiceTier::Flex)),
            "priority" => {
                self.apply_service_tier_choice(renderer, Some(OpenAIServiceTier::Priority))
            }
            "default" | "project" | "inherit" => self.apply_service_tier_choice(renderer, None),
            "skip" => self.apply_service_tier_choice(renderer, self.current_service_tier),
            _ => {
                renderer.line(
                    MessageStyle::Error,
                    "Unknown service tier option. Use flex, priority, default, or skip.",
                )?;
                prompt_service_tier_plain(renderer, selection, self.current_service_tier)?;
                Ok(ModelPickerProgress::InProgress)
            }
        }
    }

    pub(super) fn apply_service_tier_choice(
        &mut self,
        renderer: &mut AnsiRenderer,
        service_tier: Option<OpenAIServiceTier>,
    ) -> Result<ModelPickerProgress> {
        if self.selection.is_none() {
            return Err(anyhow!("Service tier requested before selecting a model"));
        }

        self.selected_service_tier = Some(service_tier);
        self.finish_after_service_tier(renderer)
    }

    pub(super) async fn handle_api_key(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
        url_guard: ExternalUrlGuardContext<'_>,
    ) -> Result<ModelPickerProgress> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("API key requested before selecting a model"));
        };
        if input.eq_ignore_ascii_case("login")
            && matches!(selection.provider_enum, Some(Provider::OpenAI))
        {
            let prepared = crate::cli::auth::prepare_openai_login(self.vt_cfg.as_ref())?;
            match request_external_url_open(url_guard, &prepared.auth_url).await? {
                ExternalUrlOpenOutcome::Opened => {
                    renderer.line(
                        MessageStyle::Info,
                        "Opening browser for OpenAI ChatGPT authentication...",
                    )?;
                    renderer.hyperlink_line(MessageStyle::Response, &prepared.auth_url)?;
                }
                ExternalUrlOpenOutcome::OpenFailed(err) => {
                    renderer.line(
                        MessageStyle::Info,
                        "Opening browser for OpenAI ChatGPT authentication...",
                    )?;
                    renderer.hyperlink_line(MessageStyle::Response, &prepared.auth_url)?;
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to open browser automatically: {}", err),
                    )?;
                    renderer.line(
                        MessageStyle::Info,
                        "Please open the URL manually in your browser.",
                    )?;
                }
                ExternalUrlOpenOutcome::Cancelled => {
                    renderer.line(MessageStyle::Info, "Cancelled opening authentication link.")?;
                    return Ok(ModelPickerProgress::InProgress);
                }
                ExternalUrlOpenOutcome::Exit => {
                    return Ok(ModelPickerProgress::Exit);
                }
                ExternalUrlOpenOutcome::Unsupported => {
                    renderer.line(
                        MessageStyle::Error,
                        "Blocked unsupported authentication link target.",
                    )?;
                    return Ok(ModelPickerProgress::InProgress);
                }
            }
            crate::cli::auth::complete_openai_login(prepared).await?;
            if self.inline_enabled {
                renderer.close_modal();
            }
            renderer.line(MessageStyle::Info, "Using ChatGPT subscription for OpenAI.")?;
            self.pending_api_key = None;
            if let Some(current) = self.selection.as_mut() {
                current.requires_api_key = false;
                current.uses_chatgpt_auth = true;
            }
            let result = self.build_result();
            return Ok(ModelPickerProgress::Completed(result?));
        }

        if input.eq_ignore_ascii_case("skip") {
            match self.find_existing_api_key(&selection.provider_key, &selection.env_key) {
                Ok(Some(ExistingKey::OAuthToken)) => {
                    if self.inline_enabled {
                        renderer.close_modal();
                    }
                    renderer.line(MessageStyle::Info, &oauth_auth_message(selection))?;
                    self.pending_api_key = None;
                    if let Some(current) = self.selection.as_mut() {
                        current.requires_api_key = false;
                        if matches!(current.provider_enum, Some(Provider::OpenAI)) {
                            current.uses_chatgpt_auth = true;
                        }
                    }
                    let result = self.build_result();
                    return Ok(ModelPickerProgress::Completed(result?));
                }
                Ok(Some(ExistingKey::Environment)) => {
                    if self.inline_enabled {
                        renderer.close_modal();
                    }
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Using existing environment variable {} for {}.",
                            selection.env_key, selection.provider_label
                        ),
                    )?;
                    self.pending_api_key = None;
                    if let Some(current) = self.selection.as_mut() {
                        current.requires_api_key = false;
                    }
                    let result = self.build_result();
                    return Ok(ModelPickerProgress::Completed(result?));
                }
                Ok(Some(ExistingKey::WorkspaceDotenv)) => {
                    if self.inline_enabled {
                        renderer.close_modal();
                    }
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Loaded {} from workspace .env for {}.",
                            selection.env_key, selection.provider_label
                        ),
                    )?;
                    self.pending_api_key = None;
                    if let Some(current) = self.selection.as_mut() {
                        current.requires_api_key = false;
                    }
                    let result = self.build_result();
                    return Ok(ModelPickerProgress::Completed(result?));
                }
                Ok(Some(ExistingKey::StoredCredential)) => {
                    if self.inline_enabled {
                        renderer.close_modal();
                    }
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Using stored API key for {}.", selection.provider_label),
                    )?;
                    self.pending_api_key = None;
                    if let Some(current) = self.selection.as_mut() {
                        current.requires_api_key = false;
                    }
                    let result = self.build_result();
                    return Ok(ModelPickerProgress::Completed(result?));
                }
                Ok(None) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "No stored API key found under {}. Provide a key or update your workspace .env.",
                            selection.env_key
                        ),
                    )?;
                    prompt_api_key_plain(renderer, selection, self.workspace.as_deref())?;
                    return Ok(ModelPickerProgress::InProgress);
                }
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!(
                            "Failed to inspect stored credentials for {}: {}",
                            selection.provider_label, err
                        ),
                    )?;
                    prompt_api_key_plain(renderer, selection, self.workspace.as_deref())?;
                    return Ok(ModelPickerProgress::InProgress);
                }
            }
        }

        self.pending_api_key = Some(input.to_string());
        if self.inline_enabled {
            renderer.close_modal();
        }
        let result = self.build_result();
        Ok(ModelPickerProgress::Completed(result?))
    }

    fn find_existing_api_key(&self, provider: &str, env_key: &str) -> Result<Option<ExistingKey>> {
        // For OpenRouter, check OAuth token first
        if env_key == "OPENROUTER_API_KEY"
            && let Ok(Some(_token)) = vtcode_config::auth::load_oauth_token()
        {
            return Ok(Some(ExistingKey::OAuthToken));
        }

        if env_key == "OPENAI_API_KEY"
            && let Ok(Some(_session)) = vtcode_config::auth::load_openai_chatgpt_session()
        {
            return Ok(Some(ExistingKey::OAuthToken));
        }

        if let Ok(value) = std::env::var(env_key)
            && !value.trim().is_empty()
        {
            return Ok(Some(ExistingKey::Environment));
        }

        if let Some(workspace) = self.workspace.as_deref()
            && let Some(value) = read_workspace_env(workspace, env_key)?
            && !value.trim().is_empty()
        {
            return Ok(Some(ExistingKey::WorkspaceDotenv));
        }

        if get_api_key(provider, &ApiKeySources::default()).is_ok() {
            return Ok(Some(ExistingKey::StoredCredential));
        }

        Ok(None)
    }
}

fn oauth_auth_message(selection: &SelectionDetail) -> String {
    if matches!(selection.provider_enum, Some(Provider::OpenAI)) {
        "Using ChatGPT subscription for OpenAI.".to_string()
    } else if matches!(selection.provider_enum, Some(Provider::Copilot)) {
        "Using managed authentication via GitHub Copilot CLI.".to_string()
    } else {
        format!(
            "Using OAuth authentication for {}.",
            selection.provider_label
        )
    }
}
