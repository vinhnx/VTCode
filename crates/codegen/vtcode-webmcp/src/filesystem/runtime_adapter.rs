//! The WebMCP runtime adapter for the filesystem workspace.
//!
//! Bridges the workspace's file operations to the runtime protocol the
//! WebMCP bridge speaks.

use super::*;

#[async_trait]
impl RuntimeAdapter for FilesystemWorkspace {
    async fn status(&self) -> Result<RuntimeStatus> {
        Ok(RuntimeStatus {
            explanations_available: false,
            workspace_root: self.root.display().to_string(),
            connected: true,
            turns_available: false,
            mutations_allowed: self.mutations_allowed,
            checks_allowed: self.checks_allowed,
            approval_authority: if self.mutations_allowed {
                "headless full-auto allowlist".into()
            } else {
                "headless policy (mutations disabled)".into()
            },
        })
    }

    async fn list_files(&self) -> Result<Vec<WorkspaceFile>> {
        let root = self.root.clone();
        let root_dir = self.root_dir.clone();
        let limits = self.limits;
        tokio::task::spawn_blocking(move || list_files_blocking(&root, &root_dir, limits))
            .await
            .map_err(|error| WebmcpError::Adapter(format!("file listing task failed: {error}")))?
    }

    async fn read_file(&self, path: &str) -> Result<FileSnapshot> {
        self.read_snapshot(path).await
    }

    async fn propose_changes(&self, changes: Vec<FileChange>) -> Result<PatchProposal> {
        let _mutation_guard = self.mutation_lock.lock().await;
        if changes.is_empty() || changes.len() > self.limits.max_changes {
            return Err(WebmcpError::LimitExceeded);
        }
        {
            let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.proposals.len() >= MAX_STORED_PROPOSALS {
                return Err(WebmcpError::LimitExceeded);
            }
        }
        let mut paths = HashSet::with_capacity(changes.len());
        let mut before = Vec::with_capacity(changes.len());
        let mut proposal_bytes = 0usize;
        for change in &changes {
            if change.content.len() > self.limits.max_change_bytes || !paths.insert(change.path.clone()) {
                return Err(WebmcpError::LimitExceeded);
            }
            let snapshot = self.read_snapshot(&change.path).await?;
            if snapshot.digest != change.base_digest {
                return Err(WebmcpError::Conflict {
                    path: change.path.clone(),
                    expected: change.base_digest.clone(),
                    actual: snapshot.digest,
                });
            }
            proposal_bytes = proposal_bytes
                .checked_add(snapshot.content.len())
                .and_then(|bytes| bytes.checked_add(change.content.len()))
                .ok_or(WebmcpError::LimitExceeded)?;
            if proposal_bytes > self.limits.max_total_bytes {
                return Err(WebmcpError::LimitExceeded);
            }
            before.push(snapshot);
        }

        let unified_diff = Self::proposal_diff(&before, &changes);
        if unified_diff.len() > self.limits.max_total_bytes {
            return Err(WebmcpError::LimitExceeded);
        }
        let stored_size_bytes = proposal_bytes
            .checked_add(unified_diff.len())
            .ok_or(WebmcpError::LimitExceeded)?;
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state
            .proposal_bytes
            .checked_add(stored_size_bytes)
            .is_none_or(|bytes| bytes > MAX_STORED_PROPOSAL_BYTES)
        {
            return Err(WebmcpError::LimitExceeded);
        }
        let proposal = PatchProposal {
            proposal_id: Uuid::new_v4().simple().to_string(),
            unified_diff,
            changes,
        };
        state.proposal_bytes += stored_size_bytes;
        drop(state.proposals.insert(
            proposal.proposal_id.clone(),
            StoredProposal {
                proposal: proposal.clone(),
                before,
                size_bytes: stored_size_bytes,
            },
        ));
        Ok(proposal)
    }

    async fn apply_proposal(&self, proposal_id: &str) -> Result<AppliedChange> {
        if !self.mutations_allowed {
            return Err(WebmcpError::ApprovalRequired);
        }
        let workspace = self.clone();
        let proposal_id = proposal_id.to_string();
        tokio::spawn(async move { workspace.apply_proposal_inner(&proposal_id).await })
            .await
            .map_err(|error| WebmcpError::Adapter(format!("WebMCP apply task failed: {error}")))?
    }

    async fn run_checks(&self, command: &str) -> Result<CheckResult> {
        let args = parse_safe_command(command, &self.allowed_commands)?;
        if !self.checks_allowed {
            return Err(WebmcpError::ApprovalRequired);
        }
        let _mutation_guard = self.mutation_lock.lock().await;
        let (program, arguments) = args
            .split_first()
            .ok_or_else(|| WebmcpError::InvalidRequest("check command cannot be empty".to_string()))?;
        let executable = resolve_check_executable(program, self.root.as_ref())?;
        // Capture host toolchain locations before replacing HOME with the
        // workspace sandbox HOME. This keeps checks reproducible without
        // allowing the child to inherit the caller's complete environment.
        let host_home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        let mut environment = SandboxEnvironment::new();
        let _ = environment.insert("PATH".to_string(), trusted_executable_path(&executable)?);
        let _ = environment.insert("HOME".to_string(), self.root.display().to_string());
        let _ = environment.insert("CARGO_NET_OFFLINE".to_string(), "true".to_string());
        let _ = environment.insert("CARGO_TERM_COLOR".to_string(), "never".to_string());
        let cargo_home = host_home
            .as_ref()
            .and_then(|home| trusted_toolchain_directory(&home.join(".cargo"), self.root.as_ref()));
        if let Some(cargo_home) = cargo_home {
            let _ = environment.insert("CARGO_HOME".to_string(), cargo_home.to_string_lossy().into_owned());
        }
        let rustup_home = host_home
            .as_ref()
            .and_then(|home| trusted_toolchain_directory(&home.join(".rustup"), self.root.as_ref()));
        if let Some(rustup_home) = rustup_home {
            let _ = environment.insert("RUSTUP_HOME".to_string(), rustup_home.to_string_lossy().into_owned());
        }
        let spec = CommandSpec::new(executable)
            .with_args(arguments.iter().cloned())
            .with_cwd(self.root.as_ref().to_path_buf())
            .with_env(environment)
            .with_expiration(ExecExpiration::Timeout(CHECK_TIMEOUT));
        let linux_launcher = LinuxSandboxLauncher::resolve();
        let check_policy = check_sandbox_policy(self.root.as_ref())
            .map_err(|error| WebmcpError::Adapter(format!("failed to build WebMCP check sandbox: {error}")))?;
        let exec_env = SandboxManager::new()
            .transform(spec, &check_policy, self.root.as_ref(), linux_launcher.as_ref())
            .map_err(|error| WebmcpError::Adapter(format!("failed to sandbox WebMCP check: {error}")))?;
        let mut child = Command::new(exec_env.program)
            .args(exec_env.args)
            .current_dir(exec_env.cwd)
            .env_clear()
            .envs(exec_env.env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| WebmcpError::Adapter("check process stdout was not captured".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| WebmcpError::Adapter("check process stderr was not captured".to_string()))?;
        let output = Box::pin(tokio::time::timeout(CHECK_TIMEOUT, async {
            let (status, stdout, stderr) =
                tokio::join!(child.wait(), read_process_output(stdout), read_process_output(stderr));
            Ok::<_, WebmcpError>((status?, stdout?, stderr?))
        }))
        .await;
        let (status, stdout, stderr) = match output {
            Ok(result) => result?,
            Err(_elapsed) => {
                drop(child.kill().await);
                drop(child.wait().await);
                return Err(WebmcpError::Timeout(CHECK_TIMEOUT));
            }
        };
        Ok(CheckResult {
            command: command.to_string(),
            exit_code: status.code(),
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        })
    }

    async fn revert_last_change(&self, change_id: &str) -> Result<AppliedChange> {
        if !self.mutations_allowed {
            return Err(WebmcpError::ApprovalRequired);
        }
        let workspace = self.clone();
        let change_id = change_id.to_string();
        tokio::spawn(async move { workspace.revert_last_change_inner(&change_id).await })
            .await
            .map_err(|error| WebmcpError::Adapter(format!("WebMCP revert task failed: {error}")))?
    }

    async fn request_turn(&self, prompt: &str, _proposal_id: Option<&str>) -> Result<TurnResult> {
        if prompt.trim().is_empty() {
            return Err(WebmcpError::InvalidRequest("agent turn prompt cannot be empty".to_string()));
        }
        if prompt.len() > 16 * 1024 {
            return Err(WebmcpError::LimitExceeded);
        }
        Err(WebmcpError::Unsupported(
            "agent turns require an active VT Code runtime; start `vtcode chat` and run `/webmcp pair <origin>` in that same session. The standalone `vtcode webmcp serve` command exposes workspace operations only".to_string(),
        ))
    }
}
