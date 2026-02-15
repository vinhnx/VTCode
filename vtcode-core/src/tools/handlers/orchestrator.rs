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

impl SandboxMode {
    fn from_turn_mode(mode: &super::sandboxing::SandboxMode) -> Self {
        match mode {
            super::sandboxing::SandboxMode::DangerFullAccess => Self::Disabled,
            super::sandboxing::SandboxMode::ExternalSandbox => Self::Strict,
            super::sandboxing::SandboxMode::ReadOnly
            | super::sandboxing::SandboxMode::WorkspaceWrite => Self::Auto,
        }
    }
}

impl SandboxPolicy {
    fn from_turn_policy(policy: &super::sandboxing::SandboxPolicy) -> Self {
        Self {
            mode: SandboxMode::from_turn_mode(&policy.mode),
            allow_network: !matches!(
                policy.network_access,
                super::sandboxing::NetworkAccess::Restricted
            ),
            allow_env_inherit: true,
            writable_paths: Vec::new(),
            readable_paths: Vec::new(),
        }
    }

    fn with_preference(self, preference: SandboxablePreference) -> Self {
        let mode = match preference {
            SandboxablePreference::Auto => self.mode,
            SandboxablePreference::Forbid => SandboxMode::Disabled,
            SandboxablePreference::Require => {
                if self.mode == SandboxMode::Disabled {
                    SandboxMode::Strict
                } else {
                    self.mode
                }
            }
        };

        Self { mode, ..self }
    }
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

// Re-export from sandboxing module (canonical definition)
pub use super::sandboxing::SandboxTransformError;

/// Tool execution context for runtimes
pub struct ToolCtx<'a> {
    pub session: &'a dyn ToolSession,
    pub turn: &'a TurnContext,
    pub call_id: String,
    pub tool_name: String,
}

// Re-export from sandboxing module (canonical definition)
pub use super::sandboxing::ToolError;

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
        turn: &TurnContext,
        _approval_policy: ApprovalPolicy,
    ) -> Result<Out, ToolError>
    where
        R: ToolRuntime<Req, Out>,
        Req: Send + Sync,
        Out: Send + Sync,
    {
        let policy = SandboxPolicy::from_turn_policy(&turn.sandbox_policy)
            .with_preference(runtime.sandbox_preference());

        // First attempt with sandbox (if applicable)
        let attempt = self.sandbox.create_attempt(&policy);

        match runtime.run(req, &attempt, ctx).await {
            Ok(out) => Ok(out),
            Err(ToolError::SandboxDenied(_)) if runtime.escalate_on_failure() => {
                // SandboxDenied = policy prevented execution; escalate to unsandboxed.
                // Other errors (Rejected, Codex, Timeout) are not sandbox-related
                // and would not benefit from retrying without a sandbox.
                tracing::debug!("Sandbox policy denied execution, escalating to unsandboxed");
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
    policy: &SandboxPolicy,
    _stdout_stream: Option<StdoutStream>,
) -> Result<ExecToolCallOutput> {
    let canonical_env = to_canonical_exec_env(env);
    let canonical_policy = to_canonical_sandbox_policy(policy);
    let canonical_output = super::sandboxing::execute_env(canonical_env, &canonical_policy).await?;
    Ok(canonical_output.into())
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

impl From<super::sandboxing::ExecToolCallOutput> for ExecToolCallOutput {
    fn from(output: super::sandboxing::ExecToolCallOutput) -> Self {
        Self {
            stdout: OutputText {
                text: output.stdout,
            },
            stderr: OutputText {
                text: output.stderr,
            },
            exit_code: output.exit_code,
        }
    }
}

impl ExecToolCallOutput {
    pub fn success_with_stdout(stdout: impl Into<String>) -> Self {
        Self {
            stdout: OutputText {
                text: stdout.into(),
            },
            stderr: OutputText::default(),
            exit_code: 0,
        }
    }

    pub fn failure_with_stderr(stderr: impl Into<String>) -> Self {
        Self {
            stdout: OutputText::default(),
            stderr: OutputText {
                text: stderr.into(),
            },
            exit_code: 1,
        }
    }

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

#[cfg(target_os = "macos")]
fn platform_sandbox_type() -> super::sandboxing::SandboxType {
    super::sandboxing::SandboxType::Seatbelt
}

#[cfg(target_os = "linux")]
fn platform_sandbox_type() -> super::sandboxing::SandboxType {
    super::sandboxing::SandboxType::LinuxSandbox
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_sandbox_type() -> super::sandboxing::SandboxType {
    super::sandboxing::SandboxType::None
}

fn to_canonical_exec_env(env: ExecEnv) -> super::sandboxing::ExecEnv {
    let sandbox = if env.sandbox.is_some() {
        platform_sandbox_type()
    } else {
        super::sandboxing::SandboxType::None
    };

    super::sandboxing::ExecEnv {
        program: env.program,
        args: env.args,
        cwd: env.cwd,
        env: env.env,
        timeout_ms: env.timeout.timeout_ms,
        sandbox,
    }
}

fn to_canonical_sandbox_policy(policy: &SandboxPolicy) -> super::sandboxing::SandboxPolicy {
    let mode = match policy.mode {
        SandboxMode::Disabled => super::sandboxing::SandboxMode::DangerFullAccess,
        SandboxMode::Auto => super::sandboxing::SandboxMode::WorkspaceWrite,
        SandboxMode::Strict => super::sandboxing::SandboxMode::ExternalSandbox,
    };

    let network_access = if policy.allow_network {
        super::sandboxing::NetworkAccess::Full
    } else {
        super::sandboxing::NetworkAccess::Restricted
    };

    super::sandboxing::SandboxPolicy {
        mode,
        network_access,
    }
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
            self.approved
                .read()
                .map(|approved| approved.contains(key))
                .unwrap_or(false)
        }

        /// Mark a key as approved
        pub fn approve(&self, key: K) {
            if let Ok(mut approved) = self.approved.write() {
                approved.insert(key);
            }
        }

        /// Clear all approvals
        pub fn clear(&self) {
            if let Ok(mut approved) = self.approved.write() {
                approved.clear();
            }
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
    use async_trait::async_trait;
    use std::path::PathBuf;

    use crate::tools::handlers::tool_handler::{ShellEnvironmentPolicy, ToolEvent};

    struct TestSession {
        cwd: PathBuf,
    }

    impl TestSession {
        fn new(cwd: PathBuf) -> Self {
            Self { cwd }
        }
    }

    #[async_trait]
    impl ToolSession for TestSession {
        fn cwd(&self) -> &PathBuf {
            &self.cwd
        }

        fn workspace_root(&self) -> &PathBuf {
            &self.cwd
        }

        async fn record_warning(&self, _message: String) {}

        fn user_shell(&self) -> &str {
            "/bin/zsh"
        }

        async fn send_event(&self, _event: ToolEvent) {}
    }

    struct TestRuntime {
        calls: usize,
        escalate: bool,
    }

    impl TestRuntime {
        fn new(escalate: bool) -> Self {
            Self { calls: 0, escalate }
        }
    }

    impl Sandboxable for TestRuntime {
        fn sandbox_preference(&self) -> SandboxablePreference {
            SandboxablePreference::Auto
        }

        fn escalate_on_failure(&self) -> bool {
            self.escalate
        }
    }

    #[async_trait]
    impl ToolRuntime<(), &'static str> for TestRuntime {
        async fn run(
            &mut self,
            _req: &(),
            attempt: &SandboxAttempt<'_>,
            _ctx: &ToolCtx<'_>,
        ) -> Result<&'static str, ToolError> {
            self.calls += 1;
            if attempt.is_escalated {
                Ok("ok")
            } else {
                Err(ToolError::SandboxDenied("denied".to_string()))
            }
        }
    }

    struct FirstAttemptProbeRuntime {
        first_is_escalated: Option<bool>,
        preference: SandboxablePreference,
    }

    impl FirstAttemptProbeRuntime {
        fn new(preference: SandboxablePreference) -> Self {
            Self {
                first_is_escalated: None,
                preference,
            }
        }
    }

    impl Sandboxable for FirstAttemptProbeRuntime {
        fn sandbox_preference(&self) -> SandboxablePreference {
            self.preference
        }
    }

    #[async_trait]
    impl ToolRuntime<(), &'static str> for FirstAttemptProbeRuntime {
        async fn run(
            &mut self,
            _req: &(),
            attempt: &SandboxAttempt<'_>,
            _ctx: &ToolCtx<'_>,
        ) -> Result<&'static str, ToolError> {
            if self.first_is_escalated.is_none() {
                self.first_is_escalated = Some(attempt.is_escalated);
            }
            Ok("ok")
        }
    }

    fn test_turn_context(
        cwd: PathBuf,
        sandbox_policy: super::super::sandboxing::SandboxPolicy,
    ) -> TurnContext {
        TurnContext {
            cwd,
            turn_id: "turn-1".to_string(),
            sub_id: None,
            shell_environment_policy: ShellEnvironmentPolicy::Inherit,
            approval_policy: ApprovalPolicy::Never,
            codex_linux_sandbox_exe: None,
            sandbox_policy,
        }
    }

    #[test]
    fn test_sandbox_preference_default() {
        struct DefaultRuntime;
        impl Sandboxable for DefaultRuntime {}

        let runtime = DefaultRuntime;
        assert_eq!(runtime.sandbox_preference(), SandboxablePreference::Auto);
        assert!(!runtime.escalate_on_failure());
    }

    #[tokio::test]
    async fn test_orchestrator_escalates_on_sandbox_denied() {
        let cwd = PathBuf::from(".");
        let session = TestSession::new(cwd.clone());
        let turn = test_turn_context(cwd, super::super::sandboxing::SandboxPolicy::default());
        let ctx = ToolCtx {
            session: &session,
            turn: &turn,
            call_id: "call-1".to_string(),
            tool_name: "test".to_string(),
        };

        let mut runtime = TestRuntime::new(true);
        let mut orchestrator = ToolOrchestrator::new();

        let out = orchestrator
            .run(
                &mut runtime,
                &(),
                &ctx,
                &turn,
                crate::tools::handlers::tool_handler::ApprovalPolicy::Never,
            )
            .await
            .expect("expected successful escalated run");

        assert_eq!(out, "ok");
        assert_eq!(runtime.calls, 2);
    }

    #[tokio::test]
    async fn test_orchestrator_does_not_escalate_when_disabled() {
        let cwd = PathBuf::from(".");
        let session = TestSession::new(cwd.clone());
        let turn = test_turn_context(cwd, super::super::sandboxing::SandboxPolicy::default());
        let ctx = ToolCtx {
            session: &session,
            turn: &turn,
            call_id: "call-2".to_string(),
            tool_name: "test".to_string(),
        };

        let mut runtime = TestRuntime::new(false);
        let mut orchestrator = ToolOrchestrator::new();

        let err = orchestrator
            .run(
                &mut runtime,
                &(),
                &ctx,
                &turn,
                crate::tools::handlers::tool_handler::ApprovalPolicy::Never,
            )
            .await
            .expect_err("expected sandbox denial without escalation");

        assert!(matches!(err, ToolError::SandboxDenied(_)));
        assert_eq!(runtime.calls, 1);
    }

    #[test]
    fn test_mode_mapping_from_turn_mode() {
        assert_eq!(
            SandboxMode::from_turn_mode(&super::super::sandboxing::SandboxMode::DangerFullAccess),
            SandboxMode::Disabled
        );
        assert_eq!(
            SandboxMode::from_turn_mode(&super::super::sandboxing::SandboxMode::ReadOnly),
            SandboxMode::Auto
        );
        assert_eq!(
            SandboxMode::from_turn_mode(&super::super::sandboxing::SandboxMode::WorkspaceWrite),
            SandboxMode::Auto
        );
        assert_eq!(
            SandboxMode::from_turn_mode(&super::super::sandboxing::SandboxMode::ExternalSandbox),
            SandboxMode::Strict
        );
    }

    #[test]
    fn test_legacy_policy_mapping_to_canonical_policy() {
        let auto_policy = SandboxPolicy {
            mode: SandboxMode::Auto,
            allow_network: false,
            allow_env_inherit: false,
            writable_paths: Vec::new(),
            readable_paths: Vec::new(),
        };
        let canonical_auto = to_canonical_sandbox_policy(&auto_policy);
        assert_eq!(
            canonical_auto.mode,
            super::super::sandboxing::SandboxMode::WorkspaceWrite
        );
        assert_eq!(
            canonical_auto.network_access,
            super::super::sandboxing::NetworkAccess::Restricted
        );

        let disabled_policy = SandboxPolicy {
            mode: SandboxMode::Disabled,
            allow_network: true,
            allow_env_inherit: true,
            writable_paths: Vec::new(),
            readable_paths: Vec::new(),
        };
        let canonical_disabled = to_canonical_sandbox_policy(&disabled_policy);
        assert_eq!(
            canonical_disabled.mode,
            super::super::sandboxing::SandboxMode::DangerFullAccess
        );
        assert_eq!(
            canonical_disabled.network_access,
            super::super::sandboxing::NetworkAccess::Full
        );
    }

    #[tokio::test]
    async fn test_orchestrator_uses_turn_sandbox_policy_for_initial_attempt() {
        let cwd = PathBuf::from(".");
        let session = TestSession::new(cwd.clone());
        let turn = test_turn_context(
            cwd,
            super::super::sandboxing::SandboxPolicy {
                mode: super::super::sandboxing::SandboxMode::DangerFullAccess,
                network_access: super::super::sandboxing::NetworkAccess::Full,
            },
        );
        let ctx = ToolCtx {
            session: &session,
            turn: &turn,
            call_id: "call-3".to_string(),
            tool_name: "test".to_string(),
        };

        let mut runtime = FirstAttemptProbeRuntime::new(SandboxablePreference::Auto);
        let mut orchestrator = ToolOrchestrator::new();

        let out = orchestrator
            .run(
                &mut runtime,
                &(),
                &ctx,
                &turn,
                crate::tools::handlers::tool_handler::ApprovalPolicy::Never,
            )
            .await
            .expect("expected successful run");

        assert_eq!(out, "ok");
        assert_eq!(runtime.first_is_escalated, Some(true));
    }

    #[tokio::test]
    async fn test_orchestrator_respects_forbid_preference() {
        let cwd = PathBuf::from(".");
        let session = TestSession::new(cwd.clone());
        let turn = test_turn_context(cwd, super::super::sandboxing::SandboxPolicy::default());
        let ctx = ToolCtx {
            session: &session,
            turn: &turn,
            call_id: "call-4".to_string(),
            tool_name: "test".to_string(),
        };

        let mut runtime = FirstAttemptProbeRuntime::new(SandboxablePreference::Forbid);
        let mut orchestrator = ToolOrchestrator::new();

        let out = orchestrator
            .run(
                &mut runtime,
                &(),
                &ctx,
                &turn,
                crate::tools::handlers::tool_handler::ApprovalPolicy::Never,
            )
            .await
            .expect("expected successful run");

        assert_eq!(out, "ok");
        assert_eq!(runtime.first_is_escalated, Some(true));
    }

    #[tokio::test]
    async fn test_orchestrator_respects_require_preference() {
        let cwd = PathBuf::from(".");
        let session = TestSession::new(cwd.clone());
        let turn = test_turn_context(
            cwd,
            super::super::sandboxing::SandboxPolicy {
                mode: super::super::sandboxing::SandboxMode::DangerFullAccess,
                network_access: super::super::sandboxing::NetworkAccess::Full,
            },
        );
        let ctx = ToolCtx {
            session: &session,
            turn: &turn,
            call_id: "call-5".to_string(),
            tool_name: "test".to_string(),
        };

        let mut runtime = FirstAttemptProbeRuntime::new(SandboxablePreference::Require);
        let mut orchestrator = ToolOrchestrator::new();

        let out = orchestrator
            .run(
                &mut runtime,
                &(),
                &ctx,
                &turn,
                crate::tools::handlers::tool_handler::ApprovalPolicy::Never,
            )
            .await
            .expect("expected successful run");

        assert_eq!(out, "ok");
        assert_eq!(runtime.first_is_escalated, Some(false));
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
