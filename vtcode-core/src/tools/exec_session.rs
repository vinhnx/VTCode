use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, anyhow};
use hashbrown::HashMap;
use portable_pty::PtySize;
use tokio::sync::{Mutex, RwLock, watch};
use tokio::task::JoinHandle;
use vtcode_bash_runner::{PipeSpawnOptions, ProcessHandle, spawn_pipe_process_with_options};

use crate::tools::registry::{PtySessionGuard, PtySessionManager};
use crate::tools::types::VTCodeExecSession;
use crate::utils::path::{canonicalize_workspace, ensure_path_within_workspace};
use crate::zsh_exec_bridge::ZshExecBridgeSession;

struct PipeSessionRecord {
    metadata: VTCodeExecSession,
    handle: Arc<ProcessHandle>,
    output: Arc<Mutex<String>>,
    pending_offset: AtomicUsize,
    output_task: Mutex<Option<JoinHandle<()>>>,
    exit_task: Mutex<Option<JoinHandle<()>>>,
    activity_tx: watch::Sender<u64>,
}

impl PipeSessionRecord {
    fn new(
        metadata: VTCodeExecSession,
        handle: Arc<ProcessHandle>,
        output: Arc<Mutex<String>>,
        output_task: JoinHandle<()>,
        exit_task: JoinHandle<()>,
        activity_tx: watch::Sender<u64>,
    ) -> Self {
        Self {
            metadata,
            handle,
            output,
            pending_offset: AtomicUsize::new(0),
            output_task: Mutex::new(Some(output_task)),
            exit_task: Mutex::new(Some(exit_task)),
            activity_tx,
        }
    }
}

#[derive(Clone)]
struct PipeSessionManager {
    workspace_root: PathBuf,
    sessions: Arc<RwLock<HashMap<String, Arc<PipeSessionRecord>>>>,
}

impl PipeSessionManager {
    fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root: canonicalize_workspace(&workspace_root),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn create_session(
        &self,
        session_id: String,
        command: Vec<String>,
        working_dir: PathBuf,
        env: HashMap<String, String>,
    ) -> Result<VTCodeExecSession> {
        if command.is_empty() {
            return Err(anyhow!("exec session command cannot be empty"));
        }
        let working_dir = canonicalize_workspace(&working_dir);
        self.ensure_within_workspace(&working_dir)?;

        {
            let sessions = self.sessions.read().await;
            if sessions.contains_key(&session_id) {
                return Err(anyhow!("exec session '{}' already exists", session_id));
            }
        }

        let mut command_parts = command;
        let program = command_parts.remove(0);
        let args = command_parts;

        let opts = PipeSpawnOptions::new(program.clone(), working_dir.clone())
            .args(args.clone())
            .env(env);
        let spawned = spawn_pipe_process_with_options(opts)
            .await
            .with_context(|| format!("failed to spawn pipe session '{}'", session_id))?;

        let metadata = VTCodeExecSession {
            id: session_id.clone(),
            backend: "pipe".to_string(),
            command: program,
            args,
            working_dir: Some(self.format_working_dir(&working_dir)),
            rows: None,
            cols: None,
        };

        let output = Arc::new(Mutex::new(String::new()));
        let output_clone = Arc::clone(&output);
        let mut output_rx = spawned.output_rx;
        let (activity_tx, _) = watch::channel(0u64);
        let output_activity_tx = activity_tx.clone();
        let output_task = tokio::spawn(async move {
            loop {
                match output_rx.recv().await {
                    Ok(chunk) => {
                        let text = String::from_utf8_lossy(&chunk);
                        let mut guard = output_clone.lock().await;
                        guard.push_str(&text);
                        let _ = output_activity_tx.send_modify(|version| *version += 1);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        let exit_rx = spawned.exit_rx;
        let exit_activity_tx = activity_tx.clone();
        let exit_task = tokio::spawn(async move {
            let _ = exit_rx.await;
            let _ = exit_activity_tx.send_modify(|version| *version += 1);
        });

        let handle = Arc::new(spawned.session);
        let record = Arc::new(PipeSessionRecord::new(
            metadata.clone(),
            handle,
            output,
            output_task,
            exit_task,
            activity_tx,
        ));

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id, record);

        Ok(metadata)
    }

    async fn read_session_output(&self, session_id: &str, drain: bool) -> Result<Option<String>> {
        let record = self.session_record(session_id).await?;
        let start = record.pending_offset.load(Ordering::SeqCst);
        let output = record.output.lock().await;
        if start >= output.len() {
            return Ok(None);
        }

        let pending = output.get(start..).map(ToOwned::to_owned).ok_or_else(|| {
            anyhow!(
                "pipe session '{}' produced invalid output boundary",
                session_id
            )
        })?;

        if drain {
            record.pending_offset.store(output.len(), Ordering::SeqCst);
        }

        if pending.is_empty() {
            Ok(None)
        } else {
            Ok(Some(pending))
        }
    }

    async fn send_input_to_session(
        &self,
        session_id: &str,
        data: &[u8],
        append_newline: bool,
    ) -> Result<usize> {
        let record = self.session_record(session_id).await?;
        record
            .handle
            .write(data.to_vec())
            .await
            .map_err(|_| anyhow!("exec session '{}' is no longer writable", session_id))?;

        if append_newline {
            record
                .handle
                .write(b"\n".to_vec())
                .await
                .map_err(|_| anyhow!("exec session '{}' is no longer writable", session_id))?;
        }

        Ok(data.len() + usize::from(append_newline))
    }

    async fn is_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        let record = self.session_record(session_id).await?;
        if record.handle.has_exited() {
            Ok(record.handle.exit_code())
        } else {
            Ok(None)
        }
    }

    async fn terminate_session(&self, session_id: &str) -> Result<()> {
        let record = self.session_record(session_id).await?;
        record.handle.terminate();
        Ok(())
    }

    async fn close_session(&self, session_id: &str) -> Result<VTCodeExecSession> {
        let record = {
            let mut sessions = self.sessions.write().await;
            sessions
                .remove(session_id)
                .ok_or_else(|| anyhow!("exec session '{}' not found", session_id))?
        };

        record.handle.terminate();
        if let Some(task) = record.output_task.lock().await.take() {
            task.abort();
        }
        if let Some(task) = record.exit_task.lock().await.take() {
            task.abort();
        }

        Ok(record.metadata.clone())
    }

    async fn activity_receiver(&self, session_id: &str) -> Result<watch::Receiver<u64>> {
        let record = self.session_record(session_id).await?;
        Ok(record.activity_tx.subscribe())
    }

    async fn terminate_all_sessions(&self) -> Result<()> {
        let ids = {
            let sessions = self.sessions.read().await;
            sessions.keys().cloned().collect::<Vec<_>>()
        };

        for session_id in ids {
            self.close_session(&session_id).await?;
        }

        Ok(())
    }

    async fn session_record(&self, session_id: &str) -> Result<Arc<PipeSessionRecord>> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow!("exec session '{}' not found", session_id))
    }

    fn ensure_within_workspace(&self, candidate: &Path) -> Result<()> {
        ensure_path_within_workspace(candidate, &self.workspace_root).map(|_| ())
    }

    fn format_working_dir(&self, path: &Path) -> String {
        match path.strip_prefix(&self.workspace_root) {
            Ok(relative) if relative.as_os_str().is_empty() => ".".into(),
            Ok(relative) => relative.to_string_lossy().replace("\\", "/"),
            Err(_) => path.to_string_lossy().into_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecSessionBackend {
    Pipe,
    Pty,
}

struct ExecSessionRecord {
    metadata: VTCodeExecSession,
    backend: ExecSessionBackend,
    _pty_guard: Option<PtySessionGuard>,
}

impl ExecSessionRecord {
    fn new(
        metadata: VTCodeExecSession,
        backend: ExecSessionBackend,
        pty_guard: Option<PtySessionGuard>,
    ) -> Self {
        Self {
            metadata,
            backend,
            _pty_guard: pty_guard,
        }
    }
}

#[derive(Clone)]
pub struct ExecSessionManager {
    pipe_sessions: PipeSessionManager,
    pty_sessions: PtySessionManager,
    sessions: Arc<RwLock<HashMap<String, Arc<ExecSessionRecord>>>>,
}

impl ExecSessionManager {
    #[must_use]
    pub fn new(workspace_root: PathBuf, pty_sessions: PtySessionManager) -> Self {
        Self {
            pipe_sessions: PipeSessionManager::new(workspace_root),
            pty_sessions,
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) async fn create_pipe_session(
        &self,
        session_id: String,
        command: Vec<String>,
        working_dir: PathBuf,
        env: HashMap<String, String>,
    ) -> Result<VTCodeExecSession> {
        self.ensure_session_absent(&session_id).await?;
        let metadata = self
            .pipe_sessions
            .create_session(session_id, command, working_dir, env)
            .await?;
        self.insert_session(metadata.clone(), ExecSessionBackend::Pipe, None)
            .await?;
        Ok(metadata)
    }

    pub(crate) async fn create_pty_session(
        &self,
        session_id: String,
        command: Vec<String>,
        working_dir: PathBuf,
        size: PtySize,
        extra_env: HashMap<String, String>,
        zsh_exec_bridge: Option<ZshExecBridgeSession>,
    ) -> Result<VTCodeExecSession> {
        self.ensure_session_absent(&session_id).await?;
        let pty_guard = self.pty_sessions.start_session()?;
        let metadata = self.pty_sessions.manager().create_session_with_bridge(
            session_id,
            command,
            working_dir,
            size,
            extra_env,
            zsh_exec_bridge,
        )?;
        let exec_metadata = VTCodeExecSession::from(metadata);
        self.insert_session(
            exec_metadata.clone(),
            ExecSessionBackend::Pty,
            Some(pty_guard),
        )
        .await?;
        Ok(exec_metadata)
    }

    pub(crate) async fn snapshot_session(&self, session_id: &str) -> Result<VTCodeExecSession> {
        let record = self.session_record(session_id).await?;
        match record.backend {
            ExecSessionBackend::Pipe => self
                .pipe_sessions
                .session_record(session_id)
                .await
                .map(|r| r.metadata.clone()),
            ExecSessionBackend::Pty => self
                .pty_sessions
                .manager()
                .snapshot_session(session_id)
                .map(VTCodeExecSession::from),
        }
    }

    pub(crate) async fn list_sessions(&self) -> Vec<VTCodeExecSession> {
        let sessions = self.sessions.read().await;
        let mut listed = sessions
            .values()
            .map(|record| record.metadata.clone())
            .collect::<Vec<_>>();
        listed.sort_by(|left, right| left.id.cmp(&right.id));
        listed
    }

    pub(crate) async fn read_session_output(
        &self,
        session_id: &str,
        drain: bool,
    ) -> Result<Option<String>> {
        let record = self.session_record(session_id).await?;
        match record.backend {
            ExecSessionBackend::Pipe => {
                self.pipe_sessions
                    .read_session_output(session_id, drain)
                    .await
            }
            ExecSessionBackend::Pty => self
                .pty_sessions
                .manager()
                .read_session_output(session_id, drain),
        }
    }

    pub(crate) async fn send_input_to_session(
        &self,
        session_id: &str,
        data: &[u8],
        append_newline: bool,
    ) -> Result<usize> {
        let record = self.session_record(session_id).await?;
        match record.backend {
            ExecSessionBackend::Pipe => {
                self.pipe_sessions
                    .send_input_to_session(session_id, data, append_newline)
                    .await
            }
            ExecSessionBackend::Pty => {
                self.pty_sessions
                    .manager()
                    .send_input_to_session(session_id, data, append_newline)
            }
        }
    }

    pub(crate) async fn is_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        let record = self.session_record(session_id).await?;
        match record.backend {
            ExecSessionBackend::Pipe => self.pipe_sessions.is_session_completed(session_id).await,
            ExecSessionBackend::Pty => self.pty_sessions.manager().is_session_completed(session_id),
        }
    }

    pub(crate) async fn activity_receiver(
        &self,
        session_id: &str,
    ) -> Result<Option<watch::Receiver<u64>>> {
        let record = self.session_record(session_id).await?;
        match record.backend {
            ExecSessionBackend::Pipe => self
                .pipe_sessions
                .activity_receiver(session_id)
                .await
                .map(Some),
            ExecSessionBackend::Pty => Ok(None),
        }
    }

    pub(crate) async fn terminate_session(&self, session_id: &str) -> Result<()> {
        let record = self.session_record(session_id).await?;
        match record.backend {
            ExecSessionBackend::Pipe => self.pipe_sessions.terminate_session(session_id).await,
            ExecSessionBackend::Pty => self.pty_sessions.manager().terminate_session(session_id),
        }
    }

    pub(crate) async fn close_session(&self, session_id: &str) -> Result<VTCodeExecSession> {
        let record = {
            let mut sessions = self.sessions.write().await;
            sessions
                .remove(session_id)
                .ok_or_else(|| anyhow!("exec session '{}' not found", session_id))?
        };

        let metadata = match record.backend {
            ExecSessionBackend::Pipe => self.pipe_sessions.close_session(session_id).await?,
            ExecSessionBackend::Pty => self
                .pty_sessions
                .manager()
                .close_session(session_id)
                .map(VTCodeExecSession::from)?,
        };

        Ok(metadata)
    }

    pub(crate) async fn prune_exited_session(
        &self,
        session_id: &str,
    ) -> Result<Option<VTCodeExecSession>> {
        if self.is_session_completed(session_id).await?.is_some() {
            return self.close_session(session_id).await.map(Some);
        }
        Ok(None)
    }

    pub(crate) async fn terminate_all_sessions_async(&self) -> Result<()> {
        let ids = {
            let sessions = self.sessions.read().await;
            sessions.keys().cloned().collect::<Vec<_>>()
        };

        let mut failures = Vec::new();
        for session_id in ids {
            if let Err(err) = self.close_session(&session_id).await {
                failures.push(format!("{session_id}: {err}"));
            }
        }

        if let Err(err) = self.pipe_sessions.terminate_all_sessions().await {
            failures.push(err.to_string());
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(anyhow!(
                "failed to terminate all exec sessions: {}",
                failures.join("; ")
            ))
        }
    }

    async fn insert_session(
        &self,
        metadata: VTCodeExecSession,
        backend: ExecSessionBackend,
        pty_guard: Option<PtySessionGuard>,
    ) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if sessions.contains_key(&metadata.id) {
            return Err(anyhow!("exec session '{}' already exists", metadata.id));
        }
        sessions.insert(
            metadata.id.clone(),
            Arc::new(ExecSessionRecord::new(metadata, backend, pty_guard)),
        );
        Ok(())
    }

    async fn ensure_session_absent(&self, session_id: &str) -> Result<()> {
        let sessions = self.sessions.read().await;
        if sessions.contains_key(session_id) {
            return Err(anyhow!("exec session '{}' already exists", session_id));
        }
        Ok(())
    }

    async fn session_record(&self, session_id: &str) -> Result<Arc<ExecSessionRecord>> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow!("exec session '{}' not found", session_id))
    }
}

#[cfg(test)]
mod tests {
    use hashbrown::HashMap;
    use portable_pty::PtySize;
    use tempfile::tempdir;
    use tokio::time::{Duration, timeout};

    use super::ExecSessionManager;
    use crate::config::PtyConfig;
    use crate::tools::registry::PtySessionManager;
    use crate::utils::path::canonicalize_workspace;

    #[cfg(unix)]
    #[tokio::test]
    async fn pty_session_limit_holds_until_exec_session_close() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let workspace_root = canonicalize_workspace(temp_dir.path());
        let pty_sessions = PtySessionManager::new(
            workspace_root.clone(),
            PtyConfig {
                max_sessions: 1,
                ..Default::default()
            },
        );
        let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };

        manager
            .create_pty_session(
                "run-1".to_string(),
                vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "sleep 1".to_string(),
                ],
                workspace_root.clone(),
                size,
                HashMap::new(),
                None,
            )
            .await?;

        let second = manager
            .create_pty_session(
                "run-2".to_string(),
                vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "sleep 1".to_string(),
                ],
                workspace_root.clone(),
                size,
                HashMap::new(),
                None,
            )
            .await;
        assert!(second.is_err());
        assert!(
            second
                .unwrap_err()
                .to_string()
                .contains("Maximum PTY sessions")
        );

        manager.close_session("run-1").await?;
        manager
            .create_pty_session(
                "run-3".to_string(),
                vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "sleep 1".to_string(),
                ],
                workspace_root,
                size,
                HashMap::new(),
                None,
            )
            .await?;
        manager.close_session("run-3").await?;

        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pipe_session_activity_receiver_notifies_on_output() -> anyhow::Result<()> {
        let temp_dir = tempdir()?;
        let workspace_root = canonicalize_workspace(temp_dir.path());
        let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
        let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

        manager
            .create_pipe_session(
                "run-1".to_string(),
                vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    "printf hello".to_string(),
                ],
                workspace_root,
                HashMap::new(),
            )
            .await?;

        let mut activity_rx = manager
            .activity_receiver("run-1")
            .await?
            .expect("pipe sessions should expose activity receiver");

        timeout(Duration::from_secs(2), activity_rx.changed()).await??;
        let output = manager
            .read_session_output("run-1", true)
            .await?
            .expect("session output");
        assert!(output.contains("hello"));

        manager.close_session("run-1").await?;
        Ok(())
    }
}
