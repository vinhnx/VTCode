use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result, anyhow};
use hashbrown::HashMap;
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use vtcode_bash_runner::{PipeSpawnOptions, ProcessHandle, spawn_pipe_process_with_options};

use crate::tools::types::VTCodeExecSession;
use crate::utils::path::{canonicalize_workspace, ensure_path_within_workspace};

struct PipeSessionRecord {
    metadata: VTCodeExecSession,
    handle: Arc<ProcessHandle>,
    output: Arc<Mutex<String>>,
    pending_offset: AtomicUsize,
    output_task: Mutex<Option<JoinHandle<()>>>,
}

impl PipeSessionRecord {
    fn new(
        metadata: VTCodeExecSession,
        handle: Arc<ProcessHandle>,
        output: Arc<Mutex<String>>,
        output_task: JoinHandle<()>,
    ) -> Self {
        Self {
            metadata,
            handle,
            output,
            pending_offset: AtomicUsize::new(0),
            output_task: Mutex::new(Some(output_task)),
        }
    }
}

#[derive(Clone)]
pub struct PipeSessionManager {
    workspace_root: PathBuf,
    sessions: Arc<RwLock<HashMap<String, Arc<PipeSessionRecord>>>>,
}

impl PipeSessionManager {
    #[must_use]
    pub fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root: canonicalize_workspace(&workspace_root),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_session(
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
        let output_task = tokio::spawn(async move {
            loop {
                match output_rx.recv().await {
                    Ok(chunk) => {
                        let text = String::from_utf8_lossy(&chunk);
                        let mut guard = output_clone.lock().await;
                        guard.push_str(&text);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let handle = Arc::new(spawned.session);
        let record = Arc::new(PipeSessionRecord::new(
            metadata.clone(),
            handle,
            output,
            output_task,
        ));

        let mut sessions = self.sessions.write().await;
        sessions.insert(session_id, record);

        Ok(metadata)
    }

    pub async fn has_session(&self, session_id: &str) -> bool {
        let sessions = self.sessions.read().await;
        sessions.contains_key(session_id)
    }

    pub async fn list_sessions(&self) -> Vec<VTCodeExecSession> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .map(|record| record.metadata.clone())
            .collect()
    }

    pub async fn snapshot_session(&self, session_id: &str) -> Result<VTCodeExecSession> {
        let record = self.session_record(session_id).await?;
        Ok(record.metadata.clone())
    }

    pub async fn read_session_output(
        &self,
        session_id: &str,
        drain: bool,
    ) -> Result<Option<String>> {
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

    pub async fn send_input_to_session(
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

    pub async fn is_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        let record = self.session_record(session_id).await?;
        if record.handle.has_exited() {
            Ok(record.handle.exit_code())
        } else {
            Ok(None)
        }
    }

    pub async fn close_session(&self, session_id: &str) -> Result<VTCodeExecSession> {
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

        Ok(record.metadata.clone())
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
