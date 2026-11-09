use super::*;
use serial_test::serial;
use std::sync::Arc;
use vtcode_core::llm::provider::{Message, MessageRole};
use vtcode_core::utils::session_archive::{SessionMessage, SessionSnapshot};
use vtcode_ui::tui::app::SubmittedInput;

fn config_request(id: &str, method: &str, params: Value) -> Request {
    Request {
        jsonrpc: "2.0".into(),
        id: RequestId::String(id.into()),
        method: method.into(),
        params,
    }
}

/// A live fake runtime installed under `session_id`, mirroring the harness in
/// `session_history.rs`. Config-change restarts shut this runtime down and
/// boot a real runloop under the same id.
struct LiveSession {
    context: Arc<ServiceContext>,
    _workspace: tempfile::TempDir,
    input_receiver: mpsc::UnboundedReceiver<InlineEvent>,
    exit_sender: Option<oneshot::Sender<std::result::Result<(), String>>>,
    runtime_task: tokio::task::JoinHandle<()>,
}

impl LiveSession {
    async fn install(session_id: &str) -> Self {
        let workspace = hermetic_workspace();
        let (context, _) = test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
        let (input, input_receiver) = mpsc::unbounded_channel();
        let (exit_sender, exit) = oneshot::channel();
        let runtime_task = tokio::spawn(std::future::pending::<()>());
        {
            let mut state = context.state.lock().await;
            state.initialized = true;
            state.session_id = Some(session_id.into());
            state.runtime = Some(RuntimeSession {
                session_id: session_id.into(),
                input,
                stop: stop_requests::new_stop_handle_slot(),
                exit,
                abort: runtime_task.abort_handle(),
                command_forwarder: std::thread::spawn(|| {}),
            });
        }
        Self {
            context: Arc::new(context),
            _workspace: workspace,
            input_receiver,
            exit_sender: Some(exit_sender),
            runtime_task,
        }
    }

    /// Drive a config request through the restart handshake: the old runtime
    /// must observe `Exit`, then its exit channel completes the shutdown.
    async fn restart_with(&mut self, request: Request) -> Value {
        let context = Arc::clone(&self.context);
        let task = tokio::spawn(async move { handle_request(&context, &request).await });
        assert!(
            matches!(self.input_receiver.recv().await, Some(InlineEvent::Exit)),
            "config change shuts the old runtime down"
        );
        self.exit_sender
            .take()
            .expect("exit sender")
            .send(Ok(()))
            .expect("finish shutdown");
        let (result, shutdown) = task.await.expect("handler task").expect("config request");
        assert!(!shutdown);
        result
    }

    async fn shutdown_real_runtime(&self) {
        timeout(Duration::from_secs(20), shutdown_current_session(&self.context, false))
            .await
            .expect("shutdown completes before the test timeout")
            .expect("shutdown succeeds");
    }
}

#[test]
#[serial]
fn config_options_preserves_live_thread_and_rebuilds_runtime() {
    block_on_with_desktop_stack(async {
        let sessions = IsolatedSessionStore::new();
        let mut live = LiveSession::install("live-session").await;
        let workspace = live.context.workspace.clone();
        seed_archive(
            sessions.path(),
            "live-session",
            &workspace,
            vec![
                SessionMessage::from(&Message::user("hello".into())),
                SessionMessage::from(&Message::assistant("answer".into())),
            ],
        )
        .await;
        {
            let mut state = live.context.state.lock().await;
            state
                .queue
                .push_back(QueuedInput::new(SubmittedInput::new("queued prompt", Vec::new()), None));
            state.queue_dropped_total = 2;
            state.restored_output_spools.insert("item-1".into(), "spool-1".into());
        }
        let result = live
            .restart_with(config_request("options-1", protocol::method::CONFIG_OPTIONS, json!({ "mode": "plan" })))
            .await;
        let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
        assert_eq!(snapshot.session_options.as_ref().expect("options").mode, "plan");
        {
            let state = live.context.state.lock().await;
            assert_eq!(state.session_id.as_deref(), Some("live-session"), "thread keeps its id");
            let runtime = state.runtime.as_ref().expect("rebuilt runtime");
            assert_eq!(runtime.session_id, "live-session");
            assert!(!runtime.input.is_closed(), "new runtime accepts turns");
            assert_eq!(state.session_options.mode.as_deref(), Some("plan"));
            assert_eq!(state.queue.len(), 1, "queued prompts ride across the restart");
            assert_eq!(state.queue.front().expect("queued").input.text, "queued prompt");
            assert_eq!(state.queue_dropped_total, 2);
            assert_eq!(state.restored_output_spools.get("item-1").map(String::as_str), Some("spool-1"));
            assert!(!state.session_transitioning, "transition gate releases");
        }
        // A follow-up theme change applies to the rebuilt session without another restart.
        let (result, _) = handle_request(
            &live.context,
            &config_request("theme-1", protocol::method::CONFIG_THEME, json!({ "theme": "ciapre-blue" })),
        )
        .await
        .expect("theme");
        let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("theme snapshot");
        assert_eq!(snapshot.shared_ui.expect("shared ui").theme, "ciapre-blue");
        assert!(live.input_receiver.try_recv().is_err(), "theme change sends no shutdown signal");
        let state = live.context.state.lock().await;
        assert_eq!(state.session_id.as_deref(), Some("live-session"));
        assert_eq!(state.theme_override.as_deref(), Some("ciapre-blue"));
        drop(state);
        live.shutdown_real_runtime().await;
        live.runtime_task.abort();
    });
}

#[test]
#[serial]
fn config_options_switch_preserves_full_thread_history_and_single_lineage() {
    block_on_with_desktop_stack(async {
        let sessions = IsolatedSessionStore::new();
        let mut live = LiveSession::install("live-session").await;
        let workspace = live.context.workspace.clone();
        // Asymmetric two-turn thread: alternating roles with distinct texts so
        // reversal, duplication, or truncation all fail the ordered comparison.
        let seeded = vec![
            SessionMessage::from(&Message::user("first-turn-question".into())),
            SessionMessage::from(&Message::assistant("first-turn-answer".into())),
            SessionMessage::from(&Message::user("second-turn-followup".into())),
            SessionMessage::from(&Message::assistant("second-turn-answer".into())),
        ];
        seed_archive(sessions.path(), "live-session", &workspace, seeded.clone()).await;
        let result = live
            .restart_with(config_request("options-1", protocol::method::CONFIG_OPTIONS, json!({ "mode": "plan" })))
            .await;
        let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
        assert_eq!(snapshot.session_options.as_ref().expect("options").mode, "plan");
        assert_eq!(live.context.state.lock().await.session_id.as_deref(), Some("live-session"), "thread keeps its id");
        // Overwrite the seed with a marker: the rebooted runloop holds the
        // resumed history in memory, so its exit-time persist must replace the
        // marker with the full thread. A dropped resume (or no persist at all)
        // leaves the marker behind and fails below.
        seed_archive(
            sessions.path(),
            "live-session",
            &workspace,
            vec![SessionMessage::from(&Message::user(
                "marker-should-be-overwritten".into(),
            ))],
        )
        .await;
        live.shutdown_real_runtime().await;
        live.runtime_task.abort();

        // Independent oracle: raw archive bytes, never `load_resume_session`.
        let archive_path = sessions.path().join("live-session.json");
        let deadline = Instant::now() + Duration::from_secs(10);
        let snapshot = loop {
            let bytes = std::fs::read(&archive_path).expect("re-persisted archive");
            let snapshot: SessionSnapshot = serde_json::from_slice(&bytes).expect("archive parses");
            let texts: Vec<String> = snapshot
                .messages
                .iter()
                .map(|message| message.content.as_text().into_owned())
                .collect();
            if texts.len() == seeded.len() && !texts.iter().any(|text| text.contains("marker-should-be-overwritten")) {
                break snapshot;
            }
            assert!(Instant::now() < deadline, "runloop never re-persisted the resumed thread: {texts:?}");
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let round_tripped: Vec<(MessageRole, String)> = snapshot
            .messages
            .iter()
            .map(|message| (message.role, message.content.as_text().into_owned()))
            .collect();
        let expected: Vec<(MessageRole, String)> = seeded
            .iter()
            .map(|message| (message.role, message.content.as_text().into_owned()))
            .collect();
        assert_eq!(round_tripped, expected, "full ordered history survives the switch");
        let mut archives: Vec<String> = std::fs::read_dir(sessions.path())
            .expect("sessions dir")
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        archives.sort();
        assert_eq!(archives, vec!["live-session.json"], "no forked archive lineage");
    });
}

#[test]
#[serial]
fn config_select_preserves_live_thread_and_applies_override() {
    block_on_with_desktop_stack(async {
        let _sessions = IsolatedSessionStore::new();
        let mut live = LiveSession::install("live-session").await;
        let local = catalog_models("", "")
            .into_iter()
            .find(|model| model.provider == "ollama" && !model.id.to_ascii_lowercase().contains("cloud"))
            .expect("local ollama catalog entry");
        let result = live
            .restart_with(config_request(
                "select-1",
                protocol::method::CONFIG_SELECT,
                json!({ "provider": local.provider.clone(), "model": local.id.clone() }),
            ))
            .await;
        let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
        assert_eq!(snapshot.provider, local.provider);
        assert_eq!(snapshot.model, local.id);
        {
            let state = live.context.state.lock().await;
            assert_eq!(state.session_id.as_deref(), Some("live-session"), "thread keeps its id");
            let runtime = state.runtime.as_ref().expect("rebuilt runtime");
            assert_eq!(runtime.session_id, "live-session");
            assert!(!runtime.input.is_closed());
            let efforts = session_options::supported_efforts(&local.provider, &local.id);
            match state.session_options.reasoning_effort.clone() {
                Some(effort) => assert!(
                    efforts.iter().any(|supported| supported == &effort),
                    "inherited effort maps onto a level the new route supports"
                ),
                None => assert!(
                    !efforts
                        .iter()
                        .any(|effort| { vtcode_commons::reasoning::ReasoningEffortLevel::parse(effort).is_some() }),
                    "effort clears only when the new route supports nothing mappable",
                ),
            }
        }
        live.shutdown_real_runtime().await;
        live.runtime_task.abort();
    });
}

#[tokio::test]
async fn config_options_without_session_stores_overrides_and_starts_nothing() {
    let workspace = hermetic_workspace();
    let (context, _) = test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
    context.state.lock().await.initialized = true;
    let (result, shutdown) = handle_request(
        &context,
        &config_request("options-1", protocol::method::CONFIG_OPTIONS, json!({ "mode": "plan" })),
    )
    .await
    .expect("options");
    assert!(!shutdown);
    let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
    assert_eq!(snapshot.session_options.as_ref().expect("options").mode, "plan");
    let state = context.state.lock().await;
    assert!(state.session_id.is_none());
    assert!(state.runtime.is_none(), "no runtime starts without a session");
    assert_eq!(state.session_options.mode.as_deref(), Some("plan"));
}

#[tokio::test]
async fn config_options_persists_reasoning_effort_to_cli_config() {
    let workspace = hermetic_workspace();
    let (context, _) = test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
    context.state.lock().await.initialized = true;
    let target = catalog_models("", "")
        .into_iter()
        .find(|model| {
            session_options::supported_efforts(&model.provider, &model.id)
                .iter()
                .any(|effort| vtcode_config::types::ReasoningEffortLevel::parse(effort).is_some())
        })
        .expect("catalog entry with CLI-representable reasoning efforts");
    let effort = session_options::supported_efforts(&target.provider, &target.id)
        .into_iter()
        .find(|effort| vtcode_config::types::ReasoningEffortLevel::parse(effort).is_some())
        .expect("effort");
    // Select the route first so effort validation runs against it (no
    // session selected, so neither request boots a runtime).
    handle_request(
        &context,
        &config_request(
            "select-1",
            protocol::method::CONFIG_SELECT,
            json!({ "provider": target.provider, "model": target.id }),
        ),
    )
    .await
    .expect("select");
    let (result, shutdown) = handle_request(
        &context,
        &config_request("options-1", protocol::method::CONFIG_OPTIONS, json!({ "reasoningEffort": effort.clone() })),
    )
    .await
    .expect("options");
    assert!(!shutdown);
    let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
    assert_eq!(snapshot.session_options.as_ref().expect("options").reasoning_effort, effort);
    let manager =
        vtcode_core::config::loader::ConfigManager::load_from_workspace(workspace.path()).expect("reload CLI config");
    assert_eq!(
        manager.config().agent.reasoning_effort,
        vtcode_config::types::ReasoningEffortLevel::parse(&effort).expect("persisted effort parses")
    );
}

#[tokio::test]
async fn config_theme_leaves_live_runtime_untouched() {
    let mut live = LiveSession::install("live-session").await;
    let (result, shutdown) = handle_request(
        &live.context,
        &config_request("theme-1", protocol::method::CONFIG_THEME, json!({ "theme": "ciapre-blue" })),
    )
    .await
    .expect("theme");
    assert!(!shutdown);
    assert!(live.input_receiver.try_recv().is_err(), "live runtime gets no shutdown signal");
    let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
    assert_eq!(snapshot.shared_ui.expect("shared ui").theme, "ciapre-blue");
    let state = live.context.state.lock().await;
    assert_eq!(state.session_id.as_deref(), Some("live-session"));
    assert!(state.runtime.is_some(), "fake runtime is never replaced");
    assert_eq!(state.theme_override.as_deref(), Some("ciapre-blue"));
    drop(state);
    live.runtime_task.abort();
}

#[test]
#[serial]
fn config_settings_write_preserves_live_thread() {
    block_on_with_desktop_stack(async {
        let _sessions = IsolatedSessionStore::new();
        let mut live = LiveSession::install("live-session").await;
        let (read_result, _) = handle_request(
            &live.context,
            &config_request("settings-read", protocol::method::CONFIG_SETTINGS_READ, json!({})),
        )
        .await
        .expect("settings read");
        let catalog: protocol::ConfigSettingsSnapshot = serde_json::from_value(read_result).expect("catalog");
        let setting = catalog
            .settings
            .iter()
            .find(|setting| setting.editable && !setting.value.is_null())
            .expect("editable setting")
            .clone();
        let version = catalog
            .target_versions
            .iter()
            .find(|target| target.target == protocol::ConfigTarget::Workspace)
            .expect("workspace layer")
            .version
            .clone();
        let result = live
            .restart_with(config_request(
                "settings-write",
                protocol::method::CONFIG_SETTINGS_WRITE,
                json!({
                    "target": "workspace",
                    "path": setting.path.clone(),
                    "value": setting.value.clone(),
                    "expectedLayerVersion": version,
                }),
            ))
            .await;
        let outcome: protocol::ConfigSettingsWriteResult = serde_json::from_value(result).expect("write result");
        assert_eq!(outcome.path, setting.path);
        let state = live.context.state.lock().await;
        assert_eq!(state.session_id.as_deref(), Some("live-session"), "thread keeps its id");
        assert_eq!(state.runtime.as_ref().expect("rebuilt runtime").session_id, "live-session");
        drop(state);
        live.shutdown_real_runtime().await;
        live.runtime_task.abort();
    });
}

#[tokio::test]
#[serial]
async fn config_change_with_unreadable_archive_leaves_live_session_untouched() {
    let sessions = IsolatedSessionStore::new();
    std::fs::write(sessions.path().join("live-session.json"), b"not valid json {{{").expect("corrupt fixture");
    let mut live = LiveSession::install("live-session").await;
    let error = handle_request(
        &live.context,
        &config_request("options-1", protocol::method::CONFIG_OPTIONS, json!({ "mode": "plan" })),
    )
    .await
    .expect_err("unreadable history refuses the change");
    assert!(error.to_string().contains("parse"), "fails at resume load: {error}");
    assert!(live.input_receiver.try_recv().is_err(), "live runtime is never signalled");
    let state = live.context.state.lock().await;
    assert_eq!(state.session_id.as_deref(), Some("live-session"));
    assert!(state.runtime.is_some(), "fake runtime is never replaced");
    assert!(state.session_options.mode.is_none(), "overrides are not applied");
    assert!(!state.session_transitioning, "transition gate releases");
    drop(state);
    live.runtime_task.abort();
}

#[test]
#[serial]
fn config_restart_without_archive_keeps_session_id_and_forks_no_lineage() {
    block_on_with_desktop_stack(async {
        let sessions = IsolatedSessionStore::new();
        let mut live = LiveSession::install("live-session").await;
        // No seeded archive: the thread was never persisted (no completed
        // turn yet). The rebooted runtime must adopt the desktop session id
        // via the reserved archive id instead of minting a fresh thread.
        let result = live
            .restart_with(config_request("options-1", protocol::method::CONFIG_OPTIONS, json!({ "mode": "plan" })))
            .await;
        let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
        assert_eq!(snapshot.session_options.as_ref().expect("options").mode, "plan");
        {
            let state = live.context.state.lock().await;
            assert_eq!(state.session_id.as_deref(), Some("live-session"), "thread keeps its id");
            let runtime = state.runtime.as_ref().expect("rebuilt runtime");
            assert_eq!(runtime.session_id, "live-session");
            assert!(!runtime.input.is_closed(), "new runtime accepts turns");
        }
        live.shutdown_real_runtime().await;
        live.runtime_task.abort();
        // The rebooted runloop adopts the desktop session id for its archive;
        // a minted fresh id would persist under a different filename. Poll
        // for the file (slow-disk tolerant), then require it to be the only
        // lineage.
        let archive_path = sessions.path().join("live-session.json");
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if archive_path.is_file() {
                break;
            }
            assert!(Instant::now() < deadline, "rebooted runloop never persisted under the desktop session id");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let mut archives: Vec<String> = std::fs::read_dir(sessions.path())
            .expect("sessions dir")
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        archives.sort();
        assert_eq!(archives, vec!["live-session.json"], "no forked archive lineage");
    });
}

#[tokio::test]
async fn config_select_persists_model_to_cli_config() {
    let workspace = hermetic_workspace();
    let (context, _) = test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
    context.state.lock().await.initialized = true;
    let local = catalog_models("", "")
        .into_iter()
        .find(|model| model.provider == "ollama" && !model.id.to_ascii_lowercase().contains("cloud"))
        .expect("local ollama catalog entry");
    let (result, shutdown) = handle_request(
        &context,
        &config_request(
            "select-1",
            protocol::method::CONFIG_SELECT,
            json!({ "provider": local.provider, "model": local.id }),
        ),
    )
    .await
    .expect("select");
    assert!(!shutdown);
    let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
    assert_eq!(snapshot.provider, local.provider);
    assert_eq!(snapshot.model, local.id);
    // CLI parity: the selection must survive a service restart, so it is
    // written to the workspace config — not kept as a memory-only override.
    let manager =
        vtcode_core::config::loader::ConfigManager::load_from_workspace(workspace.path()).expect("reload CLI config");
    assert_eq!(manager.config().agent.provider, local.provider);
    assert_eq!(manager.config().agent.default_model, local.id);
}

#[tokio::test]
async fn config_select_maps_inherited_effort_onto_new_route() {
    let workspace = hermetic_workspace();
    let (context, _) = test_service_context(test_startup_context(workspace.path()), workspace.path().to_path_buf());
    context.state.lock().await.initialized = true;
    // Asymmetric fixture: the boot default is excluded from the target
    // route, so a reset-to-default and a nearest-supported mapping produce
    // observably different snapshots.
    let boot_default = vtcode_commons::reasoning::ReasoningEffortLevel::default().as_str();
    let target = catalog_models("", "")
        .into_iter()
        .find(|model| {
            let efforts = session_options::supported_efforts(&model.provider, &model.id);
            !efforts.is_empty()
                && !efforts.iter().any(|effort| effort == boot_default)
                && efforts
                    .iter()
                    .any(|effort| vtcode_commons::reasoning::ReasoningEffortLevel::parse(effort).is_some())
        })
        .expect("catalog entry whose efforts exclude the boot default");
    let supported = session_options::supported_efforts(&target.provider, &target.id);
    let (result, shutdown) = handle_request(
        &context,
        &config_request(
            "select-1",
            protocol::method::CONFIG_SELECT,
            json!({ "provider": target.provider, "model": target.id }),
        ),
    )
    .await
    .expect("select");
    assert!(!shutdown);
    let snapshot: ConfigurationSnapshot = serde_json::from_value(result).expect("snapshot");
    let effort = snapshot.session_options.as_ref().expect("options").reasoning_effort.clone();
    assert_ne!(effort, boot_default, "inherited effort maps instead of resetting to the boot default");
    assert!(
        supported.iter().any(|supported| supported == &effort),
        "mapped effort is supported by the new route"
    );
    let state = context.state.lock().await;
    assert_eq!(state.session_options.reasoning_effort.as_deref(), Some(effort.as_str()));
    drop(state);
    let manager =
        vtcode_core::config::loader::ConfigManager::load_from_workspace(workspace.path()).expect("reload CLI config");
    assert_eq!(manager.config().agent.reasoning_effort.as_str(), effort.as_str());
}
