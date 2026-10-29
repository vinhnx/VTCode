use super::*;

#[derive(Clone)]
pub(super) struct DesktopWebmcpRuntime {
    pub(super) workspace: FilesystemWorkspace,
    pub(super) state: std::sync::Arc<Mutex<ServerState>>,
    pub(super) notifications: mpsc::Sender<protocol::Notification>,
}

#[async_trait]
impl RuntimeAdapter for DesktopWebmcpRuntime {
    async fn status(&self) -> vtcode_webmcp::Result<RuntimeStatus> {
        let mut status = self.workspace.status().await?;
        let state = self.state.lock().await;
        status.turns_available = !state.session_transitioning
            && state.pending_interactions.is_empty()
            && state.active_turn_id.is_none()
            && state.session_id.as_deref().is_some_and(|session_id| {
                state
                    .runtime
                    .as_ref()
                    .is_some_and(|runtime| runtime.session_id == session_id && !runtime.input.is_closed())
            });
        status.approval_authority = "active VT Code Desktop session".into();
        Ok(status)
    }

    async fn list_files(&self) -> vtcode_webmcp::Result<Vec<WorkspaceFile>> {
        self.workspace.list_files().await
    }

    async fn read_file(&self, path: &str) -> vtcode_webmcp::Result<FileSnapshot> {
        self.workspace.read_file(path).await
    }

    async fn propose_changes(&self, changes: Vec<FileChange>) -> vtcode_webmcp::Result<PatchProposal> {
        self.workspace.propose_changes(changes).await
    }

    async fn apply_proposal(&self, _proposal_id: &str) -> vtcode_webmcp::Result<AppliedChange> {
        Err(vtcode_webmcp::WebmcpError::ApprovalRequired)
    }

    async fn run_checks(&self, _command: &str) -> vtcode_webmcp::Result<CheckResult> {
        Err(vtcode_webmcp::WebmcpError::ApprovalRequired)
    }

    async fn revert_last_change(&self, _change_id: &str) -> vtcode_webmcp::Result<AppliedChange> {
        Err(vtcode_webmcp::WebmcpError::ApprovalRequired)
    }

    async fn request_turn(&self, prompt: &str, proposal_id: Option<&str>) -> vtcode_webmcp::Result<TurnResult> {
        let prompt =
            crate::agent::runloop::unified::webmcp::build_turn_prompt(&self.workspace, prompt, proposal_id).await?;
        let session_id = {
            let state = self.state.lock().await;
            state.session_id.clone().ok_or_else(|| {
                vtcode_webmcp::WebmcpError::Unsupported("select a VT Code Desktop session first".into())
            })?
        };
        let accepted = enqueue_turn(&self.state, &self.notifications, &session_id, prompt, Vec::new())
            .await
            .map_err(|error| vtcode_webmcp::WebmcpError::Adapter(error.to_string()))?;
        Ok(TurnResult { turn_id: accepted.turn_id, accepted: true })
    }
}

pub(super) async fn start_session(
    context: &ServiceContext,
    session_id: String,
    resume: Option<ResumeSession>,
) -> Result<()> {
    let vtcode_ui::tui::app::HostSession { session, input, commands } = new_host_session();
    let (command_sender, command_receiver) = mpsc::channel(protocol::DEFAULT_QUEUE_CAPACITY);
    let command_forwarder = std::thread::Builder::new()
        .name("vtcode-desktop-host-commands".to_string())
        .spawn(move || forward_host_commands(commands, command_sender))
        .context("failed to start desktop host command bridge")?;
    crate::main_helpers::set_runtime_archive_session_id(Some(session_id.clone()));
    let (event_sender, event_receiver) = protocol::live_event_channel(protocol::DEFAULT_QUEUE_CAPACITY);
    let config = active_agent_config(context).await;
    let mut vt_config = effective_speed_config(context, &config).await;
    vt_config.agent.theme.clone_from(&config.theme);
    let options = context.state.lock().await.session_options.clone();
    let skip_confirmations = session_options::skip_confirmations(&options, context.startup.skip_confirmations);
    if options.permission_mode == Some(protocol::PermissionMode::FullAccess) {
        vt_config.sandbox.enable_full_access();
    }
    if let Some(mode) = options.mode.as_ref() {
        vt_config.default_primary_agent.clone_from(mode);
    }
    let primary_agent_explicitly_configured =
        options.mode.is_some() || context.startup.primary_agent_explicitly_configured;
    let planning_entry_source = session_options::planning_source(&options, context.startup.planning_entry_source);
    let runtime_task = tokio::spawn(async move {
        run_single_agent_loop_unified_with_host(
            &config,
            Some(vt_config),
            skip_confirmations,
            false,
            primary_agent_explicitly_configured,
            planning_entry_source,
            resume,
            None,
            session,
            event_sender,
        )
        .await
    });
    let abort = runtime_task.abort_handle();
    let (exit_sender, exit) = oneshot::channel();
    tokio::spawn(async move {
        let result = match runtime_task.await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(sanitize_public_error(&error.to_string())),
            Err(error) => Err(sanitize_public_error(&error.to_string())),
        };
        let _ = exit_sender.send(result);
    });
    install_session_runtime(
        context,
        RuntimeSession { session_id, input, exit, abort, command_forwarder },
        command_receiver,
        event_receiver,
    )
    .await;
    Ok(())
}

pub(super) async fn install_session_runtime(
    context: &ServiceContext,
    runtime: RuntimeSession,
    commands: mpsc::Receiver<InlineCommand>,
    events: protocol::LiveEventReceiver,
) {
    let session_id = runtime.session_id.clone();
    {
        let mut state = context.state.lock().await;
        state.session_id = Some(session_id.clone());
        state.runtime = Some(runtime);
    }
    spawn_session_pump(
        session_id,
        commands,
        events,
        std::sync::Arc::clone(&context.state),
        context.notifications.clone(),
        std::sync::Arc::clone(&context.turn_finished),
    );
}

fn forward_host_commands(commands: HostCommandReceiver, sender: mpsc::Sender<InlineCommand>) {
    while let Ok(command) = commands.recv() {
        if sender.blocking_send(command).is_err() {
            break;
        }
    }
}
