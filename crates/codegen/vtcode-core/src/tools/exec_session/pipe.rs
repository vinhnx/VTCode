//! Pipe sessions: records, spool files, and the PipeSessionManager lifecycle.

use super::*;

pub(crate) struct PipeSessionRecord {
    pub(crate) metadata: VTCodeExecSession,
    pub(crate) handle: Arc<ProcessHandle>,
    output: Arc<PipeOutputBuffer>,
    output_task: Mutex<Option<JoinHandle<()>>>,
    exit_task: Mutex<Option<JoinHandle<()>>>,
    activity_tx: watch::Sender<u64>,
    spool: PipeSpoolState,
}

pub(crate) struct PipeSpoolState {
    path: String,
    ready: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    integrity: Arc<ParkingMutex<Option<SpoolIntegrity>>>,
}

pub(crate) fn create_live_spool_file(workspace_root: &Path, session_id: &str) -> (PathBuf, Option<std::fs::File>) {
    let output_directory = Path::new(".vtcode/context/tool_outputs");
    for attempt in 0..16_u32 {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("_{attempt}")
        };
        let relative_path = output_directory.join(format!("write_stdin_{session_id}{suffix}.txt"));
        match vtcode_commons::fs::bound_file::create_file_beneath(workspace_root, &relative_path) {
            Ok(file) => return (relative_path, Some(file)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return (relative_path, None),
        }
    }
    (output_directory.join(format!("write_stdin_{session_id}_unavailable.txt")), None)
}

impl PipeSessionRecord {
    pub(crate) fn new(
        metadata: VTCodeExecSession,
        handle: Arc<ProcessHandle>,
        output: Arc<PipeOutputBuffer>,
        output_task: JoinHandle<()>,
        exit_task: JoinHandle<()>,
        activity_tx: watch::Sender<u64>,
        spool: PipeSpoolState,
    ) -> Self {
        Self {
            metadata,
            handle,
            output,
            output_task: Mutex::new(Some(output_task)),
            exit_task: Mutex::new(Some(exit_task)),
            activity_tx,
            spool,
        }
    }
}

#[derive(Clone)]
pub(crate) struct PipeSessionManager {
    workspace_root: PathBuf,
    sessions: Arc<RwLock<HashMap<ExecSessionId, Arc<PipeSessionRecord>>>>,
}

impl PipeSessionManager {
    pub(crate) fn new(workspace_root: PathBuf) -> Self {
        Self {
            workspace_root: canonicalize_workspace(&workspace_root),
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) async fn create_session(
        &self,
        session_id: ExecSessionId,
        command: Vec<String>,
        working_dir: PathBuf,
        env: HashMap<String, String>,
        background: bool,
        stdin_mode: PipeStdinMode,
    ) -> Result<VTCodeExecSession> {
        if command.is_empty() {
            return Err(anyhow!("exec session command cannot be empty"));
        }
        // Canonicalization does sync fs I/O; keep it off the runtime worker
        // since this runs per spawned exec session.
        let working_dir = tokio::task::spawn_blocking({
            let working_dir = working_dir.clone();
            move || vtcode_commons::canonicalize(&working_dir)
        })
        .await
        .context("join exec-session working-directory canonicalization")?
        .with_context(|| format!("canonicalize exec-session working directory {}", working_dir.display()))?;
        self.ensure_within_workspace(&working_dir)?;

        // Hold the write lock across check → spawn → insert so two concurrent
        // creates with the same session_id cannot both pass the existence check
        // and spawn; the second insert would silently overwrite the first and
        // leak its spawned process and background tasks.
        let mut sessions = self.sessions.write().await;
        if sessions.contains_key(session_id.as_str()) {
            return Err(anyhow!("exec session '{}' already exists", session_id.as_str()));
        }

        let mut command_parts = command;
        let program = command_parts.remove(0);
        let args = command_parts;

        let opts = PipeSpawnOptions::new(program.clone(), working_dir.clone())
            .args(args.clone())
            .env(env)
            .stdin_mode(stdin_mode)
            .lossless_output(true);
        let spawned = spawn_pipe_process_with_options(opts)
            .await
            .with_context(|| format!("failed to spawn pipe session '{session_id}'"))?;

        let metadata = VTCodeExecSession {
            id: session_id.clone(),
            backend: "pipe".to_string(),
            command: program,
            args,
            working_dir: Some(self.format_working_dir(&working_dir)),
            background,
            rows: None,
            cols: None,
            child_pid: Some(spawned.process_id),
            started_at: Some(Utc::now()),
            lifecycle_state: Some(crate::tools::types::VTCodeSessionLifecycleState::Running),
            exit_code: None,
        };

        let handle = Arc::new(spawned.session);
        let output = Arc::new(PipeOutputBuffer::default());
        let output_clone = Arc::clone(&output);
        let mut output_rx = spawned.reliable_output_rx;
        let output_handle = Arc::clone(&handle);
        let (activity_tx, _) = watch::channel(0u64);
        let output_activity_tx = activity_tx.clone();
        let workspace_root = tokio::task::spawn_blocking({
            let workspace_root = self.workspace_root.clone();
            move || vtcode_commons::canonicalize(&workspace_root)
        })
        .await
        .context("join exec-session workspace canonicalization")?
        .with_context(|| format!("canonicalize exec-session workspace {}", self.workspace_root.display()))?;
        let spool_session_id = session_id.as_str().to_owned();
        let (spool_relative_path, spool_file) =
            tokio::task::spawn_blocking(move || create_live_spool_file(&workspace_root, &spool_session_id))
                .await
                .unwrap_or_else(|_| {
                    (
                        PathBuf::from(format!(".vtcode/context/tool_outputs/write_stdin_{session_id}_unavailable.txt")),
                        None,
                    )
                });
        let spool_path = spool_relative_path.to_string_lossy().replace('\\', "/");
        let spool_ready = Arc::new(AtomicBool::new(false));
        let spool_ready_for_task = Arc::clone(&spool_ready);
        let spool_failed = Arc::new(AtomicBool::new(false));
        let spool_failed_for_task = Arc::clone(&spool_failed);
        let spool_finished = Arc::new(AtomicBool::new(false));
        let spool_finished_for_task = Arc::clone(&spool_finished);
        let spool_integrity = Arc::new(ParkingMutex::new(None));
        let spool_integrity_for_task = Arc::clone(&spool_integrity);
        let output_task = tokio::spawn(async move {
            let mut spool_file = spool_file.map(tokio::fs::File::from_std);
            if spool_file.is_none() {
                spool_failed_for_task.store(true, Ordering::Release);
            } else {
                spool_ready_for_task.store(true, Ordering::Release);
            }
            let mut spool_redactor = vtcode_commons::sanitizer::StreamingSecretRedactor::default();
            let mut spool_hasher = Sha256::new();
            let mut spool_byte_count = 0_u64;
            let mut spool_lines = SpoolLineCounter::default();
            loop {
                match tokio::time::timeout(tokio::time::Duration::from_millis(15), output_rx.recv()).await {
                    Ok(Some(chunk)) => {
                        // Keep the decoded text as a `Cow<str>` so that the
                        // common case (valid UTF-8) borrows `chunk` with zero
                        // allocation. `.into_owned()` would force a String copy
                        // on every chunk even when the bytes are already valid.
                        let text = String::from_utf8_lossy(&chunk);
                        if let Some(file) = spool_file.as_mut() {
                            let sanitized = spool_redactor.push(&text);
                            if !sanitized.is_empty() {
                                if file.write_all(sanitized.as_bytes()).await.is_err() {
                                    spool_failed_for_task.store(true, Ordering::Release);
                                    spool_file = None;
                                } else {
                                    spool_hasher.update(sanitized.as_bytes());
                                    spool_lines.append(sanitized.as_bytes());
                                    spool_byte_count = spool_byte_count.saturating_add(sanitized.len() as u64);
                                }
                            }
                        }
                        output_clone.append(&text, chunk.len()).await;
                        output_activity_tx.send_modify(|version| *version += 1);
                    }
                    Ok(None) => break,
                    Err(_) if output_handle.has_exited() && output_handle.is_output_drained() => {
                        break;
                    }
                    Err(_) => continue,
                }
            }
            if let Some(file) = spool_file.as_mut() {
                let sanitized = spool_redactor.finish();
                let write_failed = !sanitized.is_empty() && file.write_all(sanitized.as_bytes()).await.is_err();
                if !write_failed && !sanitized.is_empty() {
                    spool_hasher.update(sanitized.as_bytes());
                    spool_lines.append(sanitized.as_bytes());
                    spool_byte_count = spool_byte_count.saturating_add(sanitized.len() as u64);
                }
                // Spool files are live-read while the session runs, so every
                // chunk must reach disk immediately; buffering would hide
                // output from concurrent readers until `flush`.
                if write_failed || file.sync_all().await.is_err() {
                    spool_failed_for_task.store(true, Ordering::Release);
                } else {
                    *spool_integrity_for_task.lock() = Some(SpoolIntegrity {
                        byte_count: spool_byte_count,
                        line_count: spool_lines.line_count(),
                        sha256: encode_digest_hex(spool_hasher.finalize()),
                    });
                }
            }
            spool_finished_for_task.store(true, Ordering::Release);
        });
        let exit_rx = spawned.exit_rx;
        let exit_activity_tx = activity_tx.clone();
        let exit_task = tokio::spawn(async move {
            let _ = exit_rx.await;
            exit_activity_tx.send_modify(|version| *version += 1);
        });
        let record = Arc::new(PipeSessionRecord::new(
            metadata.clone(),
            handle,
            output,
            output_task,
            exit_task,
            activity_tx,
            PipeSpoolState {
                path: spool_path,
                ready: spool_ready,
                failed: spool_failed,
                finished: spool_finished,
                integrity: spool_integrity,
            },
        ));

        sessions.insert(session_id, record);

        Ok(metadata)
    }

    pub(crate) async fn read_session_output(&self, session_id: &str, drain: bool) -> Result<Option<String>> {
        let record = self.session_record(session_id).await?;
        if drain {
            Ok(record.output.drain_pending().await)
        } else {
            Ok(record.output.peek_pending().await)
        }
    }

    pub(crate) async fn output_stats(&self, session_id: &str) -> Result<PipeOutputStats> {
        let record = self.session_record(session_id).await?;
        let (total_bytes, truncated) = record.output.stats().await;
        let spool_available =
            record.spool.ready.load(Ordering::Acquire) && !record.spool.failed.load(Ordering::Acquire);
        Ok(PipeOutputStats {
            total_bytes,
            truncated,
            spool_path: record.spool.path.clone(),
            spool_available,
            spool_complete: spool_available && record.spool.finished.load(Ordering::Acquire),
            spool_integrity: record.spool.integrity.lock().clone(),
        })
    }

    pub(crate) async fn send_input_to_session(
        &self,
        session_id: &str,
        data: &[u8],
        append_newline: bool,
    ) -> Result<usize> {
        let record = self.session_record(session_id).await?;
        let mut input = Vec::with_capacity(data.len() + usize::from(append_newline));
        input.extend_from_slice(data);
        if append_newline {
            input.push(b'\n');
        }
        record
            .handle
            .write(input)
            .await
            .map_err(|e| anyhow!("exec session '{session_id}' is no longer writable: {e}"))?;

        Ok(data.len() + usize::from(append_newline))
    }

    pub(crate) async fn is_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        let record = self.session_record(session_id).await?;
        if record.handle.has_exited() {
            Ok(record.handle.exit_code())
        } else {
            Ok(None)
        }
    }

    pub(crate) async fn terminate_session(&self, session_id: &str) -> Result<()> {
        let record = self.session_record(session_id).await?;
        if let Some(pid) = record.metadata.child_pid {
            let termination = graceful_kill_process_group_default_async(pid).await;
            #[cfg(windows)]
            if matches!(termination, GracefulTerminationResult::AlreadyExited | GracefulTerminationResult::Error) {
                // The direct child may exit before descendants that inherited
                // the pipes, so finish by killing the cached process tree.
                record.handle.terminate_process();
            }
            #[cfg(not(windows))]
            {
                let _ = termination;
                // The direct child may exit before descendants that inherited
                // the pipes, so finish by killing the cached process group as
                // well.
                record.handle.terminate_process();
            }
        } else {
            record.handle.terminate_process();
        }
        Ok(())
    }

    pub(crate) async fn force_terminate_session(&self, session_id: &str) -> Result<()> {
        let record = self.session_record(session_id).await?;
        record.handle.terminate_process();
        Ok(())
    }

    pub(crate) async fn close_session(&self, session_id: &str) -> Result<VTCodeExecSession> {
        let record = {
            let mut sessions = self.sessions.write().await;
            sessions
                .remove(session_id)
                .ok_or_else(|| missing_exec_session_error(session_id))?
        };

        // Kill the whole process group even when the direct child has already
        // exited; descendants can keep the inherited pipe descriptors alive.
        record.handle.terminate_process();
        if let Some(mut task) = record.output_task.lock().await.take() {
            if tokio::time::timeout(tokio::time::Duration::from_secs(1), &mut task)
                .await
                .is_err()
            {
                record.handle.terminate();
                task.abort();
                let _ = task.await;
            }
        }
        if let Some(mut task) = record.exit_task.lock().await.take() {
            if tokio::time::timeout(tokio::time::Duration::from_secs(1), &mut task)
                .await
                .is_err()
            {
                record.handle.terminate();
                task.abort();
                let _ = task.await;
            }
        }

        Ok(record.metadata.clone())
    }

    pub(crate) async fn activity_receiver(&self, session_id: &str) -> Result<watch::Receiver<u64>> {
        let record = self.session_record(session_id).await?;
        Ok(record.activity_tx.subscribe())
    }

    pub(crate) async fn is_output_drained(&self, session_id: &str) -> Result<bool> {
        let record = self.session_record(session_id).await?;
        let output_task = record.output_task.lock().await;
        let output_task_finished = match output_task.as_ref() {
            Some(task) => task.is_finished(),
            None => true,
        };
        Ok(record.handle.is_output_drained() && output_task_finished)
    }

    pub(crate) async fn terminate_all_sessions(&self) -> Result<()> {
        let ids = {
            let sessions = self.sessions.read().await;
            sessions.keys().cloned().collect::<Vec<_>>()
        };

        if ids.is_empty() {
            return Ok(());
        }

        let results = futures::future::join_all(ids.into_iter().map(|session_id| {
            let this = self.clone();
            async move { this.close_session(&session_id).await }
        }))
        .await;

        for result in results {
            result?;
        }

        Ok(())
    }

    pub(crate) async fn session_record(&self, session_id: &str) -> Result<Arc<PipeSessionRecord>> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| missing_exec_session_error(session_id))
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
