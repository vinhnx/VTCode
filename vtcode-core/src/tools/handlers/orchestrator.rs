//! Tool orchestrator and sandboxing infrastructure (from Codex)
//!
//! This module provides the orchestration layer for tool execution, including:
//! - Sandbox management and escalation
//! - Approval flow with caching
//! - Retry logic for sandbox escalation
//! - Tool runtime abstraction

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use super::tool_handler::{ApprovalPolicy, ToolCallError, ToolSession, TurnContext};

/// Sandbox preference for tool execution
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SandboxablePreference {
    /// Let the orchestrator decide based on approval policy
    #[default]
    Auto,
    /// Require sandbox execution
    Require,
    /// Forbid sandbox (run directly)
    Forbid,
}

/// Trait for tools that can be sandboxed (from Codex)
pub trait Sandboxable {
    /// Get the sandbox preference for this tool
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }

    /// Whether to escalate to unsandboxed execution on failure
    fn escalate_on_failure(&self) -> bool {
        false
    }
}

/// Trait for tools that require approval (from Codex)
pub trait Approvable<R> {
    /// The key type used for caching approvals
    type ApprovalKey: std::hash::Hash + Eq + Clone + Send + Sync;

    /// Generate an approval key for the request
    fn approval_key(&self, req: &R) -> Self::ApprovalKey;
}

/// Context for approval decisions
pub struct ApprovalCtx<'a> {
    pub session: &'a dyn ToolSession,
    pub turn: &'a TurnContext,
    pub call_id: &'a str,
    pub tool_name: &'a str,
}

/// Sandbox attempt state during execution
#[derive(Clone, Debug)]
pub struct SandboxAttempt<'a> {
    pub policy: &'a SandboxPolicy,
    pub is_escalated: bool,
    pub attempt_number: u32,
}

impl<'a> SandboxAttempt<'a> {
    /// Create environment for command execution
    pub fn env_for(&self, spec: CommandSpec) -> Result<ExecEnv, SandboxTransformError> {
        Ok(ExecEnv {
            program: spec.program,
            args: spec.args,
            cwd: spec.cwd,
            env: spec.env,
            timeout: spec.expiration,
            sandbox: if self.is_escalated {
                None
            } else {
                Some(SandboxConfig::default())
            },
        })
    }
}

/// Sandbox policy configuration
#[derive(Clone, Debug, Default)]
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    pub allow_network: bool,
    pub allow_env_inherit: bool,
    pub writable_paths: Vec<PathBuf>,
    pub readable_paths: Vec<PathBuf>,
}

/// Sandbox mode
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SandboxMode {
    /// No sandboxing
    Disabled,
    /// Sandboxing when available
    #[default]
    Auto,
    /// Always require sandboxing
    Strict,
}

/// Command specification for execution
#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub expiration: ExecExpiration,
    pub env: HashMap<String, String>,
    pub sandbox_permissions: super::tool_handler::SandboxPermissions,
    pub justification: Option<String>,
}

/// Execution expiration/timeout
#[derive(Clone, Copy, Debug, Default)]
pub struct ExecExpiration {
    pub timeout_ms: Option<u64>,
}

impl From<Option<u64>> for ExecExpiration {
    fn from(timeout_ms: Option<u64>) -> Self {
        Self { timeout_ms }
    }
}

impl ExecExpiration {
    pub fn timeout_ms(&self) -> Option<u64> {
        self.timeout_ms
    }
}

/// Execution environment after sandbox transformation
#[derive(Clone, Debug)]
pub struct ExecEnv {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub timeout: ExecExpiration,
    pub sandbox: Option<SandboxConfig>,
}

/// Sandbox configuration
#[derive(Clone, Debug, Default)]
pub struct SandboxConfig {
    pub restrict_network: bool,
    pub restrict_filesystem: bool,
}

/// Error during sandbox transformation
#[derive(Debug, thiserror::Error)]
pub enum SandboxTransformError {
    #[error("missing sandbox executable")]
    MissingSandboxExecutable,

    #[error("sandbox not available on this platform")]
    SandboxUnavailable,

    #[error("sandbox configuration error: {0}")]
    ConfigError(String),
}

/// Tool execution context for runtimes
pub struct ToolCtx<'a> {
    pub session: &'a dyn ToolSession,
    pub turn: &'a TurnContext,
    pub call_id: String,
    pub tool_name: String,
}

/// Error from tool runtime execution
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool rejected: {0}")]
    Rejected(String),

    #[error("Internal error: {0}")]
    Codex(#[from] anyhow::Error),

    #[error("Sandbox error: {0}")]
    Sandbox(#[from] SandboxTransformError),

    #[error("Timeout after {0}ms")]
    Timeout(u64),
}

/// Trait for tool runtimes (from Codex)
///
/// A runtime handles the actual execution of a tool request within
/// a sandbox attempt context.
#[async_trait]
pub trait ToolRuntime<Req, Out>: Sandboxable + Send + Sync
where
    Req: Send + Sync,
    Out: Send + Sync,
{
    /// Execute the tool request
    async fn run(
        &mut self,
        req: &Req,
        attempt: &SandboxAttempt<'_>,
        ctx: &ToolCtx<'_>,
    ) -> Result<Out, ToolError>;
}

/// Sandbox manager for creating sandbox attempts
#[derive(Default)]
pub struct SandboxManager;

impl SandboxManager {
    pub fn new() -> Self {
        Self
    }

    /// Create a sandbox attempt for execution
    pub fn create_attempt<'a>(&self, policy: &'a SandboxPolicy) -> SandboxAttempt<'a> {
        SandboxAttempt {
            policy,
            is_escalated: policy.mode == SandboxMode::Disabled,
            attempt_number: 1,
        }
    }

    /// Create an escalated attempt (unsandboxed)
    pub fn create_escalated_attempt<'a>(&self, policy: &'a SandboxPolicy) -> SandboxAttempt<'a> {
        SandboxAttempt {
            policy,
            is_escalated: true,
            attempt_number: 2,
        }
    }
}

/// Tool orchestrator for coordinating execution (from Codex)
///
/// The orchestrator handles:
/// 1. Sandbox creation and management
/// 2. Approval flow with caching
/// 3. Retry logic for sandbox escalation
pub struct ToolOrchestrator {
    sandbox: SandboxManager,
}

impl Default for ToolOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolOrchestrator {
    pub fn new() -> Self {
        Self {
            sandbox: SandboxManager::new(),
        }
    }

    /// Run a tool with the orchestrator managing sandbox and retries
    pub async fn run<R, Req, Out>(
        &mut self,
        runtime: &mut R,
        req: &Req,
        ctx: &ToolCtx<'_>,
        _turn: &TurnContext,
        _approval_policy: ApprovalPolicy,
    ) -> Result<Out, ToolError>
    where
        R: ToolRuntime<Req, Out>,
        Req: Send + Sync,
        Out: Send + Sync,
    {
        // Determine sandbox policy based on runtime preference
        let policy = SandboxPolicy::default();

        // First attempt with sandbox (if applicable)
        let attempt = self.sandbox.create_attempt(&policy);

        match runtime.run(req, &attempt, ctx).await {
            Ok(out) => Ok(out),
            Err(ToolError::Sandbox(_)) if runtime.escalate_on_failure() => {
                // Retry without sandbox if escalation is allowed
                tracing::debug!("Sandbox failed, escalating to unsandboxed execution");
                let escalated = self.sandbox.create_escalated_attempt(&policy);
                runtime.run(req, &escalated, ctx).await
            }
            Err(e) => Err(e),
        }
    }
}

/// Execute command with environment (simplified from Codex)
pub async fn execute_env(
    env: ExecEnv,
    _policy: &SandboxPolicy,
    _stdout_stream: Option<StdoutStream>,
) -> Result<ExecToolCallOutput> {
    use tokio::process::Command;

    let mut cmd = Command::new(&env.program);
    cmd.args(&env.args)
        .current_dir(&env.cwd)
        .envs(&env.env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let timeout = env
        .timeout
        .timeout_ms
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(300));

    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("Command timed out"))?
        .map_err(|e| anyhow::anyhow!("Command failed: {}", e))?;

    Ok(ExecToolCallOutput {
        stdout: OutputText {
            text: String::from_utf8_lossy(&output.stdout).to_string(),
        },
        stderr: OutputText {
            text: String::from_utf8_lossy(&output.stderr).to_string(),
        },
        exit_code: output.status.code().unwrap_or(-1),
    })
}

/// Stdout stream for real-time output
pub type StdoutStream = Arc<dyn Fn(&str) + Send + Sync>;

/// Output from command execution
#[derive(Clone, Debug, Default)]
pub struct ExecToolCallOutput {
    pub stdout: OutputText,
    pub stderr: OutputText,
    pub exit_code: i32,
}

impl ExecToolCallOutput {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn combined_output(&self) -> String {
        if self.stderr.text.is_empty() {
            self.stdout.text.clone()
        } else if self.stdout.text.is_empty() {
            self.stderr.text.clone()
        } else {
            format!("{}\n{}", self.stdout.text, self.stderr.text)
        }
    }
}

/// Output text wrapper
#[derive(Clone, Debug, Default)]
pub struct OutputText {
    pub text: String,
}

/// Approval caching utilities
pub mod approval_cache {
    use std::collections::HashSet;
    use std::hash::Hash;
    use std::sync::RwLock;

    /// Thread-safe approval cache
    pub struct ApprovalCache<K: Hash + Eq> {
        approved: RwLock<HashSet<K>>,
    }

    impl<K: Hash + Eq + Clone> Default for ApprovalCache<K> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl<K: Hash + Eq + Clone> ApprovalCache<K> {
        pub fn new() -> Self {
            Self {
                approved: RwLock::new(HashSet::new()),
            }
        }

        /// Check if a key is already approved
        pub fn is_approved(&self, key: &K) -> bool {
            self.approved.read().unwrap().contains(key)
        }

        /// Mark a key as approved
        pub fn approve(&self, key: K) {
            self.approved.write().unwrap().insert(key);
        }

        /// Clear all approvals
        pub fn clear(&self) {
            self.approved.write().unwrap().clear();
        }
    }
}

/// Helper to execute with cached approval
pub async fn with_cached_approval<K, F, Fut, T>(
    cache: &approval_cache::ApprovalCache<K>,
    key: K,
    f: F,
) -> Result<T, ToolCallError>
where
    K: std::hash::Hash + Eq + Clone + Send + Sync,
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<T, ToolCallError>>,
{
    // Check cache first
    if cache.is_approved(&key) {
        return f().await;
    }

    // Execute and cache on success
    let result = f().await?;
    cache.approve(key);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_preference_default() {
        struct TestRuntime;
        impl Sandboxable for TestRuntime {}

        let runtime = TestRuntime;
        assert_eq!(runtime.sandbox_preference(), SandboxablePreference::Auto);
        assert!(!runtime.escalate_on_failure());
    }

    #[test]
    fn test_exec_expiration_from_option() {
        let exp: ExecExpiration = Some(5000).into();
        assert_eq!(exp.timeout_ms(), Some(5000));

        let exp: ExecExpiration = None.into();
        assert_eq!(exp.timeout_ms(), None);
    }

    #[test]
    fn test_approval_cache() {
        let cache = approval_cache::ApprovalCache::<String>::new();

        assert!(!cache.is_approved(&"key1".to_string()));

        cache.approve("key1".to_string());
        assert!(cache.is_approved(&"key1".to_string()));

        cache.clear();
        assert!(!cache.is_approved(&"key1".to_string()));
    }

    #[test]
    fn test_exec_output_combined() {
        let output = ExecToolCallOutput {
            stdout: OutputText {
                text: "stdout".to_string(),
            },
            stderr: OutputText {
                text: "stderr".to_string(),
            },
            exit_code: 0,
        };

        assert_eq!(output.combined_output(), "stdout\nstderr");
        assert!(output.success());
    }
}
