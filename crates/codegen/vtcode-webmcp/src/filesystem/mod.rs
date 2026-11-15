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

mod policy;
mod runtime_adapter;
mod secure_open;

pub use policy::is_public_evidence_path;
use policy::*;
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

#[cfg(test)]
mod tests;
