use crate::error::{Result, WebmcpError};
use crate::protocol::FileChange;
use crate::runtime::{
    AppliedChange, CheckResult, FileSnapshot, PatchProposal, RuntimeAdapter, RuntimeStatus, TurnResult, WorkspaceFile,
};
use async_trait::async_trait;
use hashbrown::HashMap as SandboxEnvironment;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;
use vtcode_commons::exclusions::{SENSITIVE_FILES, is_sensitive_file};
use vtcode_diff::{DiffDocument, DiffOptions, format_unified_hunks};
use vtcode_safety::sandboxing::{
    CommandSpec, ExecExpiration, LinuxSandboxLauncher, SandboxManager, SandboxPolicy, SensitivePath,
    default_sensitive_paths,
};

const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_CHECK_OUTPUT_BYTES: usize = 512 * 1024;
const MAX_DIRECTORY_DEPTH: usize = 64;
const MAX_VISITED_DIRECTORIES: usize = 16_384;
const MAX_CHECK_SENSITIVE_PATHS: usize = 4096;
const MAX_STORED_PROPOSALS: usize = 64;
const MAX_STORED_PROPOSAL_BYTES: usize = 128 * 1024 * 1024;
const IGNORED_DIRECTORY_NAMES: &[&str] = &[
    ".cargo",
    ".cache",
    ".codegraph",
    ".git",
    ".mypy_cache",
    ".opencode",
    ".pytest_cache",
    ".ruff_cache",
    ".superpowers",
    ".vscode",
    ".worktrees",
    ".vtcode",
    "__pycache__",
    "dist",
    "node_modules",
    "target",
];
const SENSITIVE_DIRECTORY_NAMES: &[&str] = &[
    ".aws",
    ".azure",
    ".config",
    ".docker",
    ".gnupg",
    ".kube",
    ".pki",
    ".secrets",
    ".ssh",
    ".terraform.d",
];

mod secure_open;

use secure_open::{
    open_root_directory, open_workspace_file, read_snapshot_blocking, replace_snapshot_if_current_blocking,
};

/// Bounds applied by the headless filesystem adapter.
#[derive(Debug, Clone, Copy)]
pub struct FilesystemLimits {
    /// Maximum number of files returned by a listing.
    pub max_files: usize,
    /// Maximum size of one UTF-8 file.
    pub max_file_bytes: usize,
    /// Maximum total bytes read by one listing or one proposal.
    pub max_total_bytes: usize,
    /// Maximum files in one proposal.
    pub max_changes: usize,
    /// Maximum proposal content bytes per file.
    pub max_change_bytes: usize,
}

impl Default for FilesystemLimits {
    fn default() -> Self {
        Self {
            max_files: 8192,
            max_file_bytes: 2 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
            max_changes: 32,
            max_change_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
struct StoredProposal {
    proposal: PatchProposal,
    before: Vec<FileSnapshot>,
    size_bytes: usize,
}

#[derive(Debug, Clone)]
struct StoredChange {
    change_id: String,
    before: Vec<FileSnapshot>,
    after: Vec<FileSnapshot>,
}

#[derive(Debug, Default)]
struct FilesystemState {
    proposals: std::collections::HashMap<String, StoredProposal>,
    proposal_bytes: usize,
    last_change: Option<StoredChange>,
}

/// A safe headless adapter rooted at one canonical workspace directory.
#[derive(Clone)]
pub struct FilesystemWorkspace {
    root: Arc<PathBuf>,
    root_dir: Arc<std::fs::File>,
    allowed_roots: Arc<Vec<PathBuf>>,
    limits: FilesystemLimits,
    mutations_allowed: bool,
    checks_allowed: bool,
    allowed_commands: Arc<HashSet<String>>,
    state: Arc<Mutex<FilesystemState>>,
    mutation_lock: Arc<AsyncMutex<()>>,
}

impl std::fmt::Debug for FilesystemWorkspace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FilesystemWorkspace")
            .field("root", &self.root)
            .field("allowed_roots", &self.allowed_roots)
            .field("limits", &self.limits)
            .field("mutations_allowed", &self.mutations_allowed)
            .field("checks_allowed", &self.checks_allowed)
            .finish_non_exhaustive()
    }
}

impl FilesystemWorkspace {
    /// Construct an adapter. The current adapter exposes one canonical root;
    /// an empty allowlist means only the supplied root is visible.
    pub async fn new<I>(root: impl AsRef<Path>, allowed_roots: I, mutations_allowed: bool) -> Result<Self>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        let root = vtcode_commons::canonicalize_async(root.as_ref().to_path_buf()).await?;
        if !tokio::fs::metadata(&root).await?.is_dir() {
            return Err(WebmcpError::PathRejected("workspace root is not a directory".to_string()));
        }

        let allowed_roots = allowed_roots.into_iter().collect::<Vec<_>>();
        if allowed_roots.len() > 1 {
            return Err(WebmcpError::InvalidRequest(
                "headless WebMCP currently supports one workspace root".to_string(),
            ));
        }
        let roots = if allowed_roots.is_empty() {
            vec![root.clone()]
        } else {
            let mut roots = Vec::with_capacity(allowed_roots.len());
            for allowed_root in allowed_roots {
                let canonical = vtcode_commons::canonicalize_async(allowed_root).await?;
                if !tokio::fs::metadata(&canonical).await?.is_dir() {
                    return Err(WebmcpError::PathRejected("allowed root is not a directory".to_string()));
                }
                roots.push(canonical);
            }
            roots
        };
        if !roots.iter().any(|allowed| root.starts_with(allowed)) {
            return Err(WebmcpError::PathRejected("workspace root is not in the allowed roots".to_string()));
        }
        let root_dir = open_root_directory(&root)?;
        if !root_dir.metadata()?.is_dir() {
            return Err(WebmcpError::PathRejected("workspace root is not a directory".to_string()));
        }

        Ok(Self {
            root: Arc::new(root),
            root_dir: Arc::new(root_dir),
            allowed_roots: Arc::new(roots),
            limits: FilesystemLimits::default(),
            mutations_allowed,
            checks_allowed: mutations_allowed,
            allowed_commands: Arc::new(["cargo"].into_iter().map(str::to_string).collect()),
            state: Arc::new(Mutex::new(FilesystemState::default())),
            mutation_lock: Arc::new(AsyncMutex::new(())),
        })
    }

    /// Replace the default workspace limits.
    pub fn with_limits(mut self, limits: FilesystemLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Replace the allowlisted check executables.
    pub fn with_allowed_commands<I, S>(mut self, commands: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_commands = Arc::new(commands.into_iter().map(Into::into).collect());
        self
    }

    /// Configure whether the runtime may execute the allowlisted checks.
    pub fn with_checks_allowed(mut self, checks_allowed: bool) -> Self {
        self.checks_allowed = checks_allowed;
        self
    }

    /// Return a still-current proposal for an active runtime turn handoff.
    ///
    /// Rechecking the snapshots here prevents a browser proposal from being
    /// handed to the agent after an external edit occurred between proposal
    /// creation and turn submission.
    pub async fn proposal_for_turn(&self, proposal_id: &str) -> Result<PatchProposal> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let stored = {
            let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.proposals.get(proposal_id).cloned().ok_or(WebmcpError::ProposalNotFound)?
        };
        for expected in &stored.before {
            let current = self.read_snapshot(&expected.path).await?;
            if current.digest != expected.digest {
                return Err(WebmcpError::Conflict {
                    path: expected.path.clone(),
                    expected: expected.digest.clone(),
                    actual: current.digest,
                });
            }
        }
        Ok(stored.proposal)
    }

    /// Returns the canonical primary workspace path.
    pub fn root(&self) -> &Path {
        self.root.as_ref()
    }

    async fn read_snapshot(&self, path: &str) -> Result<FileSnapshot> {
        let root = self.root.clone();
        let root_dir = self.root_dir.clone();
        let path = path.to_string();
        let limits = self.limits;
        tokio::task::spawn_blocking(move || read_snapshot_blocking(&root, &root_dir, &path, limits))
            .await
            .map_err(|error| WebmcpError::Adapter(format!("file read task failed: {error}")))?
    }

    async fn replace_snapshot_if_current(&self, update: &FileSnapshot, expected: &FileSnapshot) -> Result<()> {
        let root = self.root.clone();
        let root_dir = self.root_dir.clone();
        let update = update.clone();
        let expected = expected.clone();
        tokio::task::spawn_blocking(move || replace_snapshot_if_current_blocking(&root, &root_dir, &update, &expected))
            .await
            .map_err(|error| WebmcpError::Adapter(format!("compare-and-replace task failed: {error}")))?
    }

    async fn apply_snapshots_if_current(&self, updates: &[FileSnapshot], expected: &[FileSnapshot]) -> Result<()> {
        if updates.len() != expected.len() {
            return Err(WebmcpError::Adapter("snapshot compare-and-swap lengths do not match".to_string()));
        }
        let mut applied = Vec::with_capacity(updates.len());
        for (index, (update, expected_snapshot)) in updates.iter().zip(expected).enumerate() {
            if let Err(error) = self.replace_snapshot_if_current(update, expected_snapshot).await {
                let rollback_error = self
                    .rollback_snapshots(&applied, expected.get(..index).unwrap_or(&[]))
                    .await
                    .err();
                return Err(rollback_error.unwrap_or(error));
            }
            applied.push(update.clone());
        }
        Ok(())
    }

    async fn apply_proposal_inner(&self, proposal_id: &str) -> Result<AppliedChange> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let stored = {
            let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.proposals.get(proposal_id).cloned().ok_or(WebmcpError::ProposalNotFound)?
        };

        let after = stored
            .proposal
            .changes
            .iter()
            .map(|change| FileSnapshot {
                path: change.path.clone(),
                content: change.content.clone(),
                digest: digest_text(&change.content),
            })
            .collect::<Vec<_>>();
        self.apply_snapshots_if_current(&after, &stored.before).await?;

        let change_id = Uuid::new_v4().simple().to_string();
        let result = AppliedChange {
            change_id: change_id.clone(),
            paths: after.iter().map(|snapshot| snapshot.path.clone()).collect(),
        };
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(removed) = state.proposals.remove(proposal_id) {
            state.proposal_bytes = state.proposal_bytes.saturating_sub(removed.size_bytes);
        }
        state.last_change = Some(StoredChange { change_id, before: stored.before, after });
        Ok(result)
    }

    async fn revert_last_change_inner(&self, change_id: &str) -> Result<AppliedChange> {
        let _mutation_guard = self.mutation_lock.lock().await;
        let stored = {
            let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            state.last_change.clone().ok_or(WebmcpError::ChangeNotFound)?
        };
        if stored.change_id != change_id {
            return Err(WebmcpError::ChangeNotFound);
        }
        self.apply_snapshots_if_current(&stored.before, &stored.after).await?;
        let reverted = AppliedChange {
            change_id: stored.change_id.clone(),
            paths: stored.before.iter().map(|snapshot| snapshot.path.clone()).collect(),
        };
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.last_change = None;
        Ok(reverted)
    }

    async fn rollback_snapshots(&self, applied: &[FileSnapshot], originals: &[FileSnapshot]) -> Result<()> {
        let mut failed = false;
        for (applied_snapshot, original) in applied.iter().zip(originals).rev() {
            if let Err(error) = self.replace_snapshot_if_current(original, applied_snapshot).await {
                tracing::error!(
                    path = %original.path,
                    error = %error,
                    "failed to roll back a partially applied WebMCP change"
                );
                failed = true;
            }
        }
        if failed { Err(WebmcpError::PartialApply) } else { Ok(()) }
    }

    fn proposal_diff(before: &[FileSnapshot], changes: &[FileChange]) -> String {
        let mut diff = String::new();
        for (snapshot, change) in before.iter().zip(changes) {
            let old_label = format!("a/{}", snapshot.path);
            let new_label = format!("b/{}", change.path);
            let options = DiffOptions {
                context_lines: 3,
                old_label: Some(old_label.as_str()),
                new_label: Some(new_label.as_str()),
                ..DiffOptions::default()
            };
            let document = DiffDocument::between(&snapshot.content, &change.content, options.clone());
            if document.hunks.is_empty() {
                continue;
            }
            diff.push_str(&format_unified_hunks(&document.hunks, &options));
        }
        diff
    }
}

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

fn validate_relative_path(path: &str) -> Result<PathBuf> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err(WebmcpError::PathRejected(path.to_string()));
    }
    let path = Path::new(path);
    if path.is_absolute() {
        return Err(WebmcpError::PathRejected(path.display().to_string()));
    }
    let mut validated = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => validated.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(WebmcpError::PathRejected(path.display().to_string()));
            }
        }
    }
    if validated.as_os_str().is_empty() {
        return Err(WebmcpError::PathRejected(path.display().to_string()));
    }
    Ok(validated)
}

fn reject_sensitive_relative_path(relative: &Path, original: &str) -> Result<()> {
    if is_sensitive_relative_path(relative) {
        return Err(WebmcpError::PathRejected(format!("sensitive workspace path is not exposed: {original}")));
    }
    Ok(())
}

/// Classify recorded paths before exposing change evidence. This lexical filter
/// does not authorize filesystem access; reads still require bound directory handles.
pub fn is_public_evidence_path(path: &str) -> bool {
    validate_relative_path(path).is_ok_and(|relative| {
        !is_sensitive_relative_path(&relative)
            && !relative.components().any(|component| {
                matches!(component, Component::Normal(name) if name.to_str().is_none_or(|name| IGNORED_DIRECTORY_NAMES.iter().any(|ignored| name.eq_ignore_ascii_case(ignored))))
            })
    })
}

fn is_sensitive_relative_path(relative: &Path) -> bool {
    relative.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let Some(name) = name.to_str() else {
            return true;
        };
        is_sensitive_file(name)
            || SENSITIVE_DIRECTORY_NAMES
                .iter()
                .any(|sensitive| name.eq_ignore_ascii_case(sensitive))
    })
}

#[cfg(unix)]
fn has_multiple_hard_links(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.nlink() > 1
}

#[cfg(not(unix))]
const fn has_multiple_hard_links(_metadata: &std::fs::Metadata) -> bool {
    false
}

fn reject_hard_links(metadata: &std::fs::Metadata, path: &str) -> Result<()> {
    if has_multiple_hard_links(metadata) {
        return Err(WebmcpError::PathRejected(format!("hard-linked file is not allowed: {path}")));
    }
    Ok(())
}

fn digest_text(content: &str) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(content.as_bytes());
    let mut encoded = String::with_capacity(7 + digest.len() * 2);
    encoded.push_str("sha256:");
    // `write!` appends into the pre-sized buffer: the previous per-byte
    // `format!` allocated a fresh `String` 32 times per digest.
    for byte in digest {
        let _ignored = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn list_files_blocking(root: &Path, root_dir: &std::fs::File, limits: FilesystemLimits) -> Result<Vec<WorkspaceFile>> {
    let mut files = Vec::new();
    let mut total_bytes = 0usize;
    let mut visited_directories = 0usize;
    visit_directory(root, root_dir, root, 0, limits, &mut total_bytes, &mut visited_directories, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn visit_directory(
    root: &Path,
    root_dir: &std::fs::File,
    directory: &Path,
    depth: usize,
    limits: FilesystemLimits,
    total_bytes: &mut usize,
    visited_directories: &mut usize,
    files: &mut Vec<WorkspaceFile>,
) -> Result<()> {
    if depth > MAX_DIRECTORY_DEPTH {
        return Err(WebmcpError::LimitExceeded);
    }
    *visited_directories = visited_directories.saturating_add(1);
    if *visited_directories > MAX_VISITED_DIRECTORIES {
        return Err(WebmcpError::LimitExceeded);
    }
    let entries = std::fs::read_dir(directory)?;
    for entry in entries {
        if files.len() >= limits.max_files {
            return Err(WebmcpError::LimitExceeded);
        }
        let entry = entry?;
        let path = entry.path();
        let relative_path = path
            .strip_prefix(root)
            .map_err(|_error| WebmcpError::PathRejected(path.display().to_string()))?;
        if is_sensitive_relative_path(relative_path) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| IGNORED_DIRECTORY_NAMES.contains(&name))
            {
                continue;
            }
            visit_directory(root, root_dir, &path, depth + 1, limits, total_bytes, visited_directories, files)?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }
        if has_multiple_hard_links(&metadata) {
            continue;
        }
        let max_file_bytes = u64::try_from(limits.max_file_bytes).unwrap_or(u64::MAX);
        if metadata.len() > max_file_bytes {
            continue;
        }
        let file = open_workspace_file(root, root_dir, relative_path, false)?;
        let opened_metadata = file.metadata()?;
        if !opened_metadata.is_file()
            || has_multiple_hard_links(&opened_metadata)
            || opened_metadata.len() > max_file_bytes
        {
            continue;
        }
        let mut reader = file.take(max_file_bytes.saturating_add(1));
        let mut bytes = Vec::with_capacity(limits.max_file_bytes.min(64 * 1024));
        let _bytes_read = reader.read_to_end(&mut bytes)?;
        if bytes.len() > limits.max_file_bytes {
            continue;
        }
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        *total_bytes = total_bytes.saturating_add(content.len());
        if *total_bytes > limits.max_total_bytes {
            return Err(WebmcpError::LimitExceeded);
        }
        let relative = relative_path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
        files.push(WorkspaceFile {
            path: relative,
            size_bytes: content.len() as u64,
            digest: digest_text(&content),
        });
    }
    Ok(())
}

fn trusted_toolchain_directory(path: &Path, workspace: &Path) -> Option<PathBuf> {
    let canonical = vtcode_commons::canonicalize(path).ok()?;
    (canonical.is_dir() && !canonical.starts_with(workspace)).then_some(canonical)
}

fn resolve_check_executable(program: &str, workspace: &Path) -> Result<PathBuf> {
    let executable_name = if cfg!(windows) {
        format!("{program}.exe")
    } else {
        program.to_string()
    };
    let mut candidates = Vec::new();

    #[cfg(unix)]
    {
        if program == "cargo" {
            if let Some(home) = std::env::var_os("HOME") {
                candidates.push(PathBuf::from(home).join(".cargo/bin").join(&executable_name));
            }
        }
        for directory in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin", "/bin"] {
            candidates.push(PathBuf::from(directory).join(&executable_name));
        }
    }

    #[cfg(windows)]
    {
        if program == "cargo"
            && let Some(profile) = std::env::var_os("USERPROFILE")
        {
            candidates.push(PathBuf::from(profile).join(".cargo/bin").join(&executable_name));
        }
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            candidates.push(PathBuf::from(system_root).join("System32").join(&executable_name));
        }
    }

    for candidate in candidates {
        let Ok(canonical) = vtcode_commons::canonicalize(&candidate) else {
            continue;
        };
        if canonical.starts_with(workspace) {
            continue;
        }
        let Ok(metadata) = std::fs::metadata(&canonical) else {
            continue;
        };
        if metadata.is_file() && is_executable_file(&metadata) {
            return Ok(canonical);
        }
    }

    Err(WebmcpError::InvalidRequest(format!(
        "allowlisted check executable is not installed in a trusted location: {program}"
    )))
}

fn is_executable_file(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

fn trusted_executable_path(executable: &Path) -> Result<String> {
    let mut directories = Vec::new();
    if let Some(parent) = executable.parent() {
        directories.push(parent.to_path_buf());
    }
    #[cfg(unix)]
    directories.extend([
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ]);
    #[cfg(windows)]
    if let Some(system_root) = std::env::var_os("SystemRoot") {
        directories.push(PathBuf::from(system_root).join("System32"));
    }
    directories.dedup();
    std::env::join_paths(directories)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| WebmcpError::Adapter(format!("failed to build trusted check PATH: {error}")))
}

fn sensitive_path_for_policy(path: &Path) -> Result<SensitivePath> {
    let path_string = path.to_str().ok_or_else(|| {
        WebmcpError::Adapter(format!("cannot sandbox a non-UTF-8 sensitive path: {}", path.display()))
    })?;
    if path_string
        .chars()
        .any(|character| character == '"' || character == '\\' || character.is_control())
    {
        return Err(WebmcpError::Adapter(format!(
            "cannot sandbox a sensitive path containing an unsafe character: {path_string}"
        )));
    }
    Ok(SensitivePath::new(path_string))
}

fn collect_workspace_sensitive_paths(root: &Path) -> Result<Vec<SensitivePath>> {
    let mut sensitive_paths = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut visited_directories = 0usize;

    while let Some((directory, depth)) = pending.pop() {
        if depth > MAX_DIRECTORY_DEPTH {
            return Err(WebmcpError::LimitExceeded);
        }
        visited_directories = visited_directories.saturating_add(1);
        if visited_directories > MAX_VISITED_DIRECTORIES {
            return Err(WebmcpError::LimitExceeded);
        }

        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative_path = path
                .strip_prefix(root)
                .map_err(|_error| WebmcpError::PathRejected(path.display().to_string()))?;
            if is_sensitive_relative_path(relative_path) {
                if sensitive_paths.len() >= MAX_CHECK_SENSITIVE_PATHS {
                    return Err(WebmcpError::LimitExceeded);
                }
                sensitive_paths.push(sensitive_path_for_policy(&path)?);
                continue;
            }

            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push((path, depth + 1));
            }
        }
    }

    Ok(sensitive_paths)
}

fn check_sandbox_policy(root: &Path) -> Result<SandboxPolicy> {
    let mut sensitive_paths = default_sensitive_paths();
    for name in SENSITIVE_FILES.iter().copied().chain(SENSITIVE_DIRECTORY_NAMES.iter().copied()) {
        sensitive_paths.push(sensitive_path_for_policy(&root.join(name))?);
    }
    sensitive_paths.extend(collect_workspace_sensitive_paths(root)?);
    Ok(SandboxPolicy::workspace_write_with_sensitive_paths(vec![root.to_path_buf()], sensitive_paths))
}

fn parse_safe_command(command: &str, allowed_commands: &HashSet<String>) -> Result<Vec<String>> {
    if command.len() > 512
        || command
            .chars()
            .any(|character| matches!(character, ';' | '|' | '&' | '$' | '`' | '>' | '<' | '\n' | '\r'))
    {
        return Err(WebmcpError::InvalidRequest("check command contains shell syntax".to_string()));
    }
    let args = shell_words::split(command)
        .map_err(|error| WebmcpError::InvalidRequest(format!("invalid check command: {error}")))?;
    let Some(program) = args.first() else {
        return Err(WebmcpError::InvalidRequest("check command cannot be empty".to_string()));
    };
    if Path::new(program).components().count() != 1 || !allowed_commands.contains(program) {
        return Err(WebmcpError::InvalidRequest("check executable is not allowlisted".to_string()));
    }
    if args.iter().any(|argument| argument.contains('\0')) {
        return Err(WebmcpError::InvalidRequest("check command contains an invalid argument".to_string()));
    }
    match program.as_str() {
        "cargo"
            if args.get(1).is_some_and(|subcommand| subcommand == "check")
                && args.iter().skip(2).all(|argument| {
                    matches!(
                        argument.as_str(),
                        "--locked" | "--offline" | "--workspace" | "--all-targets" | "--all-features"
                    )
                }) =>
        {
            Ok(args)
        }
        "printf" => Ok(args),
        _ => Err(WebmcpError::InvalidRequest(
            "only the bounded cargo check and printf commands are supported".to_string(),
        )),
    }
}

async fn read_process_output<R>(mut reader: R) -> Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::with_capacity(MAX_CHECK_OUTPUT_BYTES.min(8192));
    let mut buffer = [0u8; 8192];
    let mut exceeded = false;
    loop {
        let bytes_read = reader.read(&mut buffer).await?;
        if bytes_read == 0 {
            break;
        }
        let remaining = MAX_CHECK_OUTPUT_BYTES.saturating_sub(captured.len());
        exceeded |= bytes_read > remaining;
        if remaining > 0 {
            let chunk = buffer
                .get(..bytes_read.min(remaining))
                .ok_or_else(|| WebmcpError::Adapter("check output read exceeded its buffer".to_string()))?;
            captured.extend_from_slice(chunk);
        }
    }
    if exceeded {
        Err(WebmcpError::LimitExceeded)
    } else {
        Ok(captured)
    }
}

#[cfg(test)]
mod tests;
