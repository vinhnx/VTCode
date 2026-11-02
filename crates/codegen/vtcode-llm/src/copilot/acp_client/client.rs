use super::*;

impl CopilotAcpClient {
    pub(crate) async fn connect(
        config: &CopilotAuthConfig,
        workspace_root: &Path,
        raw_model: Option<&str>,
        custom_tools: &[ToolDefinition],
    ) -> Result<Self> {
        match Self::connect_once(
            config,
            workspace_root,
            raw_model,
            custom_tools,
            CopilotModelSelectionMode::CliArgument,
        )
        .await
        {
            Ok(client) => Ok(client),
            Err(primary_error) if raw_model.is_some() => Self::connect_once(
                config,
                workspace_root,
                raw_model,
                custom_tools,
                CopilotModelSelectionMode::EnvironmentVariable,
            )
            .await
            .with_context(|| format!("copilot acp startup with --model failed first: {primary_error}")),
            Err(error) => Err(error),
        }
    }

    async fn connect_once(
        config: &CopilotAuthConfig,
        workspace_root: &Path,
        raw_model: Option<&str>,
        custom_tools: &[ToolDefinition],
        model_selection_mode: CopilotModelSelectionMode,
    ) -> Result<Self> {
        let resolved = resolve_copilot_command(config)?;
        let mut child = spawn_copilot_acp_process(&resolved, config, workspace_root, raw_model, model_selection_mode)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("copilot acp child stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("copilot acp child stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("copilot acp child stderr unavailable"))?;

        let transport = StdioTransport::from_child(child, stdin, stdout, stderr, resolved.auth_timeout);

        let inner = Arc::new(CopilotAcpClientInner {
            transport,
            active_prompt: StdMutex::new(None),
            session_id: StdMutex::new(None),
            compatibility_state: StdMutex::new(CopilotAcpCompatibilityState::Unavailable),
        });

        // Register the Copilot-specific notification handler.
        // Use a Weak reference to avoid a retain cycle:
        //   Arc<Inner> → StdioTransport → handler → Weak<Inner>
        let inner_weak = Arc::downgrade(&inner);
        inner.transport.set_notification_handler(Arc::new(move |message| {
            if let Some(inner_strong) = inner_weak.upgrade() {
                handle_acp_message(&inner_strong, message)?;
            }
            Ok(())
        }));

        let client = Self { inner };
        timeout(resolved.startup_timeout, async {
            client.initialize().await?;
            let session_id = client
                .create_session(config, workspace_root.to_path_buf(), raw_model, custom_tools)
                .await?;
            *client
                .inner
                .session_id
                .lock()
                .map_err(|_e| anyhow!("copilot acp session mutex poisoned"))? = Some(session_id);
            *client
                .inner
                .compatibility_state
                .lock()
                .map_err(|_e| anyhow!("copilot acp compatibility mutex poisoned"))? =
                CopilotAcpCompatibilityState::FullTools;
            Ok::<(), anyhow::Error>(())
        })
        .await
        .context("copilot acp startup timeout")??;
        Ok(client)
    }

    fn session_id(&self) -> Result<String> {
        self.inner
            .session_id
            .lock()
            .map_err(|_e| anyhow!("copilot acp session mutex poisoned"))?
            .clone()
            .ok_or_else(|| anyhow!("copilot acp session not initialized"))
    }

    pub(crate) async fn start_prompt(&self, prompt_text: String) -> Result<PromptSession> {
        let (updates_tx, updates_rx) = tokio::sync::mpsc::unbounded_channel();
        let (runtime_tx, runtime_rx) = tokio::sync::mpsc::unbounded_channel();
        {
            let mut active_prompt = self
                .inner
                .active_prompt
                .lock()
                .map_err(|_e| anyhow!("copilot acp active prompt mutex poisoned"))?;
            if active_prompt.is_some() {
                return Err(anyhow!("copilot acp only supports one active prompt"));
            }
            *active_prompt = Some(ActivePrompt { updates: updates_tx, runtime_requests: runtime_tx });
        }

        if self.compatibility_state()? == CopilotAcpCompatibilityState::PromptOnly {
            enqueue_runtime_request(
            &self.inner,
            CopilotRuntimeRequest::CompatibilityNotice(CopilotCompatibilityNotice {
                state: CopilotAcpCompatibilityState::PromptOnly,
                message: "GitHub Copilot ACP is running in prompt-only degraded mode. VT Code will keep the session alive, but Copilot-native runtime hooks are partially incompatible.".to_string(),
            }),
        )?;
        }

        let client = self.clone();
        let session_id = self.session_id()?;
        let completion = tokio::spawn(async move {
            let result = client
                .call(
                    "session/prompt",
                    json!({
                        "sessionId": session_id,
                        "prompt": [
                            {
                                "type": "text",
                                "text": prompt_text,
                            }
                        ]
                    }),
                )
                .await
                .context("copilot acp session/prompt");

            client.clear_active_prompt();
            let result = result?;

            let stop_reason = result
                .get("stopReason")
                .and_then(Value::as_str)
                .unwrap_or("end_turn")
                .to_string();
            Ok(PromptCompletion { stop_reason })
        });
        let cancel_handle = PromptSessionCancelHandle {
            client: self.clone(),
            completion_abort: completion.abort_handle(),
        };

        Ok(PromptSession {
            updates: updates_rx,
            runtime_requests: runtime_rx,
            completion,
            cancel_handle,
        })
    }

    pub(super) fn cancel(&self) -> Result<()> {
        self.inner
            .transport
            .notify(
                "session/cancel",
                json!({
                    "sessionId": self.session_id()?,
                }),
            )
            .map_err(anyhow::Error::from)
    }

    async fn initialize(&self) -> Result<()> {
        let response = self
            .call(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {
                        "fs": {
                            "readTextFile": false,
                            "writeTextFile": false,
                        },
                        "terminal": true,
                    },
                    "clientInfo": {
                        "name": "vtcode",
                        "title": vtcode_config::constants::app::DISPLAY_NAME,
                        "version": env!("CARGO_PKG_VERSION"),
                    }
                }),
            )
            .await
            .context("copilot acp initialize")?;

        let protocol_version = response.get("protocolVersion").and_then(Value::as_i64).unwrap_or(1);
        if protocol_version != 1 {
            return Err(anyhow!("unsupported copilot acp protocol version {protocol_version}"));
        }

        Ok(())
    }

    async fn create_session(
        &self,
        config: &CopilotAuthConfig,
        workspace_root: PathBuf,
        raw_model: Option<&str>,
        custom_tools: &[ToolDefinition],
    ) -> Result<String> {
        match self
            .create_session_v2(config, workspace_root.clone(), raw_model, custom_tools)
            .await
        {
            Ok(session_id) => Ok(session_id),
            Err(v2_error) => self
                .create_session_v1(workspace_root)
                .await
                .with_context(|| format!("copilot acp session.create failed first: {v2_error}")),
        }
    }

    pub(super) async fn create_session_v2(
        &self,
        config: &CopilotAuthConfig,
        workspace_root: PathBuf,
        raw_model: Option<&str>,
        custom_tools: &[ToolDefinition],
    ) -> Result<String> {
        let mut params = serde_json::Map::from_iter([
            ("clientName".to_string(), Value::String(vtcode_config::constants::app::DISPLAY_NAME.to_string())),
            ("workingDirectory".to_string(), json!(workspace_root)),
            ("requestPermission".to_string(), Value::Bool(true)),
            ("streaming".to_string(), Value::Bool(true)),
            ("mcpServers".to_string(), Value::Array(Vec::new())),
        ]);

        if let Some(raw_model) = raw_model.filter(|value| !value.trim().is_empty()) {
            params.insert("model".to_string(), Value::String(raw_model.to_string()));
        }
        let custom_tools = custom_tools_payload(custom_tools);
        if !custom_tools.is_empty() {
            params.insert("tools".to_string(), Value::Array(custom_tools));
        }
        // Empty is an explicit deny-all catalog, not the provider default.
        params.insert("availableTools".to_string(), json!(config.available_tools));
        if !config.excluded_tools.is_empty() {
            params.insert("excludedTools".to_string(), json!(config.excluded_tools));
        }

        let response = self
            .call("session.create", Value::Object(params))
            .await
            .context("copilot acp session.create")?;

        response
            .get("sessionId")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| anyhow!("copilot acp session.create missing sessionId"))
    }

    async fn create_session_v1(&self, workspace_root: PathBuf) -> Result<String> {
        let response = self
            .call(
                "session/new",
                json!({
                    "cwd": workspace_root,
                    "mcpServers": [],
                }),
            )
            .await
            .context("copilot acp session/new")?;

        response
            .get("sessionId")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .ok_or_else(|| anyhow!("copilot acp session/new missing sessionId"))
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value> {
        self.inner.transport.call(method, params).await.map_err(anyhow::Error::from)
    }

    pub(super) fn clear_active_prompt(&self) {
        if let Ok(mut active_prompt) = self.inner.active_prompt.lock() {
            *active_prompt = None;
        }
    }

    fn compatibility_state(&self) -> Result<CopilotAcpCompatibilityState> {
        self.inner
            .compatibility_state
            .lock()
            .map(|state| *state)
            .map_err(|_e| anyhow!("copilot acp compatibility mutex poisoned"))
    }
}
