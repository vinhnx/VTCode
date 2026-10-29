use super::*;

impl ExecSessionManager {
    /// Test-only convenience: production paths go through
    /// [`Self::create_pipe_session_with_stdin`].
    #[cfg(test)]
    pub(crate) async fn create_pipe_session(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        env: HashMap<String, String>,
    ) -> Result<VTCodeExecSession> {
        self.create_pipe_session_with_sandbox_and_background(session_id, command, working_dir, env, false, false)
            .await
    }

    #[cfg(test)]
    pub(crate) async fn create_pipe_session_with_sandbox_and_background(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        env: HashMap<String, String>,
        sandbox_active: bool,
        background: bool,
    ) -> Result<VTCodeExecSession> {
        self.create_pipe_session_with_stdin(
            session_id,
            command,
            working_dir,
            env,
            sandbox_active,
            background,
            PipeStdinMode::Piped,
        )
        .await
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Launch options preserve the existing internal session API while public pipe runs select stdin explicitly."
    )]
    pub(crate) async fn create_pipe_session_with_stdin(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        env: HashMap<String, String>,
        sandbox_active: bool,
        background: bool,
        stdin_mode: PipeStdinMode,
    ) -> Result<VTCodeExecSession> {
        let launch_mode = if background {
            ExecSessionLaunchMode::UserBackground
        } else {
            ExecSessionLaunchMode::Foreground
        };
        self.create_pipe_session_with_launch_mode(
            session_id,
            command,
            working_dir,
            env,
            sandbox_active,
            launch_mode,
            stdin_mode,
        )
        .await
    }

    pub(crate) async fn create_pipe_session_for_managed_background(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        env: HashMap<String, String>,
    ) -> Result<VTCodeExecSession> {
        self.create_pipe_session_with_launch_mode(
            session_id,
            command,
            working_dir,
            env,
            false,
            ExecSessionLaunchMode::ManagedBackground,
            PipeStdinMode::Null,
        )
        .await
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Internal session launch carries sandbox, lifecycle, and stdin policy through one spawning boundary."
    )]
    async fn create_pipe_session_with_launch_mode(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        env: HashMap<String, String>,
        sandbox_active: bool,
        launch_mode: ExecSessionLaunchMode,
        stdin_mode: PipeStdinMode,
    ) -> Result<VTCodeExecSession> {
        let _create_guard = self.create_lock.lock().await;
        self.ensure_session_absent(&session_id).await?;
        let slot_reserved = if launch_mode.reserves_background_slot() {
            self.reserve_background_slot()?
        } else {
            false
        };
        let env = if sandbox_active {
            build_sanitized_env(&env, true, false, "exec-session", &[])
        } else {
            env
        };
        let metadata = match self
            .pipe_sessions
            .create_session(session_id.clone(), command, working_dir, env, launch_mode.is_background(), stdin_mode)
            .await
        {
            Ok(metadata) => metadata,
            Err(error) => {
                self.release_reserved_background_slot(slot_reserved);
                return Err(error);
            }
        };
        let record = match self
            .insert_session(metadata.clone(), ExecSessionBackend::Pipe, None, launch_mode, slot_reserved)
            .await
        {
            Ok(record) => record,
            Err(error) => {
                let _ = self.pipe_sessions.close_session(session_id.as_str()).await;
                self.release_reserved_background_slot(slot_reserved);
                return Err(error);
            }
        };
        if launch_mode.is_background() {
            self.start_background_watcher(record, session_id.to_string());
        } else if launch_mode.sets_foreground_session() {
            self.set_foreground_session(metadata.id.clone());
            self.start_foreground_watcher(record, session_id.to_string());
        }
        Ok(metadata)
    }

    /// Test-only convenience: production paths go through
    /// [`Self::create_pty_session_with_sandbox_and_background`].
    #[cfg(test)]
    pub(crate) async fn create_pty_session(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        size: PtySize,
        extra_env: HashMap<String, String>,
        zsh_exec_bridge: Option<ZshExecBridgeSession>,
    ) -> Result<VTCodeExecSession> {
        self.create_pty_session_with_sandbox_and_background(
            session_id,
            command,
            working_dir,
            size,
            extra_env,
            zsh_exec_bridge,
            HashMap::new(),
            false,
            false,
        )
        .await
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The constructor keeps sandbox, bridge, and background launch settings explicit at the session boundary."
    )]
    pub(crate) async fn create_pty_session_with_sandbox_and_background(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        size: PtySize,
        extra_env: HashMap<String, String>,
        zsh_exec_bridge: Option<ZshExecBridgeSession>,
        trusted_env: HashMap<String, String>,
        sandbox_active: bool,
        background: bool,
    ) -> Result<VTCodeExecSession> {
        let launch_mode = if background {
            ExecSessionLaunchMode::UserBackground
        } else {
            ExecSessionLaunchMode::Foreground
        };
        self.create_pty_session_with_launch_mode(
            session_id,
            command,
            working_dir,
            size,
            extra_env,
            zsh_exec_bridge,
            trusted_env,
            sandbox_active,
            launch_mode,
        )
        .await
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The managed background constructor keeps PTY launch settings explicit at the session boundary."
    )]
    pub(crate) async fn create_pty_session_for_managed_background(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        size: PtySize,
        extra_env: HashMap<String, String>,
        zsh_exec_bridge: Option<ZshExecBridgeSession>,
        trusted_env: HashMap<String, String>,
        sandbox_active: bool,
    ) -> Result<VTCodeExecSession> {
        self.create_pty_session_with_launch_mode(
            session_id,
            command,
            working_dir,
            size,
            extra_env,
            zsh_exec_bridge,
            trusted_env,
            sandbox_active,
            ExecSessionLaunchMode::ManagedBackground,
        )
        .await
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "The constructor keeps sandbox, bridge, and background launch settings explicit at the session boundary."
    )]
    async fn create_pty_session_with_launch_mode(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        size: PtySize,
        extra_env: HashMap<String, String>,
        zsh_exec_bridge: Option<ZshExecBridgeSession>,
        trusted_env: HashMap<String, String>,
        sandbox_active: bool,
        launch_mode: ExecSessionLaunchMode,
    ) -> Result<VTCodeExecSession> {
        let _create_guard = self.create_lock.lock().await;
        self.ensure_session_absent(&session_id).await?;
        let slot_reserved = if launch_mode.reserves_background_slot() {
            self.reserve_background_slot()?
        } else {
            false
        };
        let pty_guard = match self.pty_sessions.start_session() {
            Ok(guard) => guard,
            Err(error) => {
                self.release_reserved_background_slot(slot_reserved);
                return Err(error);
            }
        };
        let metadata = match self.pty_sessions.manager().create_session_with_bridge_sandboxed(
            session_id.clone().into(),
            command,
            working_dir,
            size,
            extra_env,
            zsh_exec_bridge,
            trusted_env,
            sandbox_active,
        ) {
            Ok(metadata) => metadata,
            Err(error) => {
                self.release_reserved_background_slot(slot_reserved);
                return Err(error);
            }
        };
        let mut exec_metadata = VTCodeExecSession::from(metadata);
        exec_metadata.background = launch_mode.is_background();
        let record = match self
            .insert_session(exec_metadata.clone(), ExecSessionBackend::Pty, Some(pty_guard), launch_mode, slot_reserved)
            .await
        {
            Ok(record) => record,
            Err(error) => {
                let _ = self.pty_sessions.manager().close_session(session_id.as_str());
                self.release_reserved_background_slot(slot_reserved);
                return Err(error);
            }
        };
        if launch_mode.is_background() {
            self.start_background_watcher(record, session_id.to_string());
        } else if launch_mode.sets_foreground_session() {
            self.set_foreground_session(exec_metadata.id.clone());
            self.start_foreground_watcher(record, session_id.to_string());
        }
        Ok(exec_metadata)
    }
}
