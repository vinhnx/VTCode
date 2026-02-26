use super::*;

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
            renderer.close_modal();
            show_secure_api_modal(renderer, selection, self.workspace.as_deref());
        }
        prompt_api_key_plain(renderer, selection, self.workspace.as_deref())
    }

    pub(super) fn apply_reasoning_choice(
        &mut self,
        renderer: &mut AnsiRenderer,
        level: ReasoningEffortLevel,
    ) -> Result<ModelPickerProgress> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("Reasoning requested before selecting a model"));
        };
        self.selected_reasoning = Some(level);
        if selection.requires_api_key {
            self.step = PickerStep::AwaitApiKey;
            self.prompt_api_key_step(renderer)?;
            return Ok(ModelPickerProgress::InProgress);
        }
        let result = self.build_result();
        Ok(ModelPickerProgress::Completed(result?))
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
        if current_selection.model_id.starts_with("gpt-5.2")
            || current_selection.model_id.starts_with("gpt-5.3")
        {
            self.selected_reasoning = Some(ReasoningEffortLevel::None);
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Reasoning disabled for {} by setting effort to 'none'.",
                    current_selection.model_display
                ),
            )?;

            if current_selection.requires_api_key {
                self.step = PickerStep::AwaitApiKey;
                self.prompt_api_key_step(renderer)?;
                return Ok(ModelPickerProgress::InProgress);
            }

            let result = self.build_result();
            return Ok(ModelPickerProgress::Completed(result?));
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

    fn build_result(&self) -> Result<ModelSelectionResult> {
        let selection = self
            .selection
            .as_ref()
            .ok_or_else(|| anyhow!("Model selection missing"))?;
        let chosen_reasoning = self.selected_reasoning.unwrap_or(self.current_reasoning);
        let reasoning_changed = chosen_reasoning != self.current_reasoning;

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
            api_key: self.pending_api_key.clone(),
            env_key: selection.env_key.clone(),
            requires_api_key: selection.requires_api_key,
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
        let mut selection = selection;
        if selection.requires_api_key {
            match self.find_existing_api_key(&selection.env_key) {
                Ok(Some(ExistingKey::OAuthToken)) => {
                    selection.requires_api_key = false;
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Using OAuth authentication for {}.",
                            selection.provider_label
                        ),
                    )?;
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
                Ok(Some(ExistingKey::WorkspaceDotenv(value))) => {
                    selection.requires_api_key = false;
                    // SAFETY: Keys are sanitized and values come from configuration sources.
                    unsafe {
                        std::env::set_var(&selection.env_key, &value);
                    }
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Loaded {} from workspace .env for {}.",
                            selection.env_key, selection.provider_label
                        ),
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

    pub(super) fn handle_api_key(
        &mut self,
        renderer: &mut AnsiRenderer,
        input: &str,
    ) -> Result<ModelPickerProgress> {
        let Some(selection) = self.selection.as_ref() else {
            return Err(anyhow!("API key requested before selecting a model"));
        };
        if input.eq_ignore_ascii_case("skip") {
            match self.find_existing_api_key(&selection.env_key) {
                Ok(Some(ExistingKey::OAuthToken)) => {
                    renderer.close_modal();
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "Using OAuth authentication for {}.",
                            selection.provider_label
                        ),
                    )?;
                    self.pending_api_key = None;
                    if let Some(current) = self.selection.as_mut() {
                        current.requires_api_key = false;
                    }
                    let result = self.build_result();
                    return Ok(ModelPickerProgress::Completed(result?));
                }
                Ok(Some(ExistingKey::Environment)) => {
                    renderer.close_modal();
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
                Ok(Some(ExistingKey::WorkspaceDotenv(value))) => {
                    renderer.close_modal();
                    // SAFETY: Keys are sanitized and values come from configuration sources.
                    unsafe {
                        std::env::set_var(&selection.env_key, &value);
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
        renderer.close_modal();
        let result = self.build_result();
        Ok(ModelPickerProgress::Completed(result?))
    }

    fn find_existing_api_key(&self, env_key: &str) -> Result<Option<ExistingKey>> {
        // For OpenRouter, check OAuth token first
        if env_key == "OPENROUTER_API_KEY"
            && let Ok(Some(_token)) = vtcode_config::auth::load_oauth_token()
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
            return Ok(Some(ExistingKey::WorkspaceDotenv(value)));
        }

        Ok(None)
    }
}
