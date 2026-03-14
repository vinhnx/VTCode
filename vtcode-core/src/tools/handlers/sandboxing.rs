//! Shared approvals and sandboxing traits used by tool runtimes (from Codex)
//!
//! Consolidates the approval flow primitives (`ApprovalDecision`, `ApprovalStore`,
//! `ApprovalCtx`, `Approvable`) together with the sandbox orchestration traits
//! and helpers (`Sandboxable`, `ToolRuntime`, `SandboxAttempt`, etc.).

use hashbrown::HashMap;
use std::fmt::Debug;
use std::future::Future;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::exec_policy::default_exec_approval_requirement as canonical_default_exec_approval_requirement;
pub use crate::exec_policy::{
    AskForApproval, ExecApprovalRequirement, ExecPolicyAmendment, RejectConfig,
};
use crate::sandboxing::{
    CommandSpec as CanonicalCommandSpec, ExecEnv as CanonicalExecEnv,
    ExecExpiration as CanonicalExecExpiration, ResourceLimits,
    SandboxManager as CanonicalSandboxManager, SandboxPolicy as CanonicalSandboxPolicy,
    SandboxTransformError as CanonicalSandboxTransformError, SandboxType as CanonicalSandboxType,
    SeccompProfile,
};

use super::tool_handler::{ToolSession, TurnContext};

// ============================================================================
// Review Decision Types (from Codex protocol)
// ============================================================================

/// User's decision on an approval request (from Codex)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReviewDecision {
    /// Approval granted for this single invocation
    Approved,
    /// Approval denied
    Denied,
    /// Abort the entire operation
    Abort,
    /// Approval granted for the entire session
    ApprovedForSession,
    /// Approval granted with exec policy amendment
    ApprovedExecpolicyAmendment {
        proposed_execpolicy_amendment: ExecPolicyAmendment,
    },
}

// ============================================================================
// Exec Approval Requirement (from Codex)
// ============================================================================

// ============================================================================
// Approval Store (from Codex)
// ============================================================================

/// Store for cached approval decisions (from Codex)
#[derive(Clone, Default)]
pub struct ApprovalStore {
    approvals: Arc<RwLock<HashMap<String, ReviewDecision>>>,
}

impl ApprovalStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get a cached approval decision
    pub async fn get(&self, key: &str) -> Option<ReviewDecision> {
        self.approvals.read().await.get(key).cloned()
    }

    /// Store an approval decision
    pub async fn set(&self, key: String, decision: ReviewDecision) {
        self.approvals.write().await.insert(key, decision);
    }

    /// Check if an approval exists
    pub async fn contains(&self, key: &str) -> bool {
        self.approvals.read().await.contains_key(key)
    }
}

/// Helper function to cache approval decisions (from Codex)
pub async fn with_cached_approval<K, F, Fut>(
    store: &ApprovalStore,
    key: K,
    fetch: F,
) -> ReviewDecision
where
    K: Serialize + Clone,
    F: FnOnce() -> Fut,
    Fut: Future<Output = ReviewDecision>,
{
    let key_str = serde_json::to_string(&key).unwrap_or_default();

    // Check if we already have a cached decision
    if let Some(decision) = store.get(&key_str).await
        && matches!(decision, ReviewDecision::ApprovedForSession)
    {
        return ReviewDecision::Approved;
    }

    // Fetch new decision
    let decision = fetch().await;

    // Cache the decision
    store.set(key_str, decision.clone()).await;

    decision
}

// ============================================================================
// Approval Context (from Codex)
// ============================================================================

/// Context for approval decisions (from Codex)
pub struct ApprovalCtx<'a> {
    pub session: &'a dyn ToolSession,
    pub turn: &'a TurnContext,
    pub call_id: &'a str,
    pub retry_reason: Option<String>,
}

// ============================================================================
// Sandbox Preferences (from Codex)
// ============================================================================

/// Sandbox override for first attempt (from Codex)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxOverride {
    /// Use default sandbox selection
    NoOverride,
    /// Bypass sandbox on first attempt
    BypassSandboxFirstAttempt,
}

/// Sandbox preference for a tool (from Codex)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SandboxablePreference {
    /// Let the orchestrator decide
    #[default]
    Auto,
    /// Require sandbox execution
    Require,
    /// Forbid sandbox
    Forbid,
}

// ============================================================================
// Sandboxable Trait (from Codex)
// ============================================================================

/// Trait for tools that can be sandboxed (from Codex)
pub trait Sandboxable {
    /// Get the sandbox preference for this tool
    fn sandbox_preference(&self) -> SandboxablePreference {
        SandboxablePreference::Auto
    }

    /// Whether to escalate to unsandboxed execution on failure
    fn escalate_on_failure(&self) -> bool {
        true
    }
}

// ============================================================================
// Approvable Trait (from Codex)
// ============================================================================

/// Type alias for boxed future (from Codex)
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for tools that require approval (from Codex)
pub trait Approvable<Req>: Send + Sync {
    /// The key type used for caching approvals
    type ApprovalKey: Hash + Eq + Clone + Debug + Serialize + Send + Sync;

    /// Generate an approval key for the request
    fn approval_key(&self, req: &Req) -> Self::ApprovalKey;

    /// Some tools may request to skip the sandbox on the first attempt
    fn sandbox_mode_for_first_attempt(&self, _req: &Req) -> SandboxOverride {
        SandboxOverride::NoOverride
    }

    /// Check if approval should be bypassed
    fn should_bypass_approval(&self, policy: AskForApproval, already_approved: bool) -> bool {
        if already_approved {
            return true;
        }
        matches!(policy, AskForApproval::Never)
    }

    /// Return custom exec approval requirement, or None for default
    fn exec_approval_requirement(&self, _req: &Req) -> Option<ExecApprovalRequirement> {
        None
    }

    /// Decide if we can request approval for no-sandbox execution
    fn wants_no_sandbox_approval(&self, policy: AskForApproval) -> bool {
        match policy {
            AskForApproval::OnFailure => true,
            AskForApproval::UnlessTrusted => true,
            AskForApproval::Never => false,
            AskForApproval::OnRequest => false,
            AskForApproval::Reject(_) => !policy.rejects_sandbox_prompt(),
        }
    }

    /// Start the approval process asynchronously (from Codex)
    fn start_approval_async<'a>(
        &'a mut self,
        req: &'a Req,
        ctx: ApprovalCtx<'a>,
    ) -> BoxFuture<'a, ReviewDecision>;
}

// ============================================================================
// Sandbox Policy (from Codex)
// ============================================================================

/// Sandbox policy configuration (from Codex protocol)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SandboxPolicy {
    pub mode: SandboxMode,
    pub network_access: NetworkAccess,
}

/// Sandbox mode (from Codex)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SandboxMode {
    /// Read-only filesystem access
    #[default]
    ReadOnly,
    /// Write access within workspace
    WorkspaceWrite,
    /// Full access (dangerous)
    DangerFullAccess,
    /// External sandbox (e.g., Docker)
    ExternalSandbox,
}

/// Network access policy
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NetworkAccess {
    /// No network access
    #[default]
    Restricted,
    /// Limited network access
    Limited,
    /// Full network access
    Full,
}

const LEGACY_EXTERNAL_SANDBOX_DESCRIPTION: &str = "legacy handler external sandbox";

impl SandboxPolicy {
    #[must_use]
    pub fn requires_approval_prompt(&self) -> bool {
        !matches!(
            self.mode,
            SandboxMode::DangerFullAccess | SandboxMode::ExternalSandbox
        )
    }

    #[must_use]
    pub fn uses_runtime_sandbox(&self) -> bool {
        matches!(
            self.mode,
            SandboxMode::ReadOnly | SandboxMode::WorkspaceWrite
        )
    }

    #[must_use]
    pub fn to_canonical_policy(&self, sandbox_cwd: &Path) -> CanonicalSandboxPolicy {
        match (&self.mode, &self.network_access) {
            (SandboxMode::ReadOnly, NetworkAccess::Restricted | NetworkAccess::Limited) => {
                CanonicalSandboxPolicy::read_only()
            }
            (SandboxMode::ReadOnly, NetworkAccess::Full) => {
                CanonicalSandboxPolicy::read_only_with_full_network()
            }
            (SandboxMode::WorkspaceWrite, NetworkAccess::Restricted | NetworkAccess::Limited) => {
                CanonicalSandboxPolicy::workspace_write(vec![sandbox_cwd.to_path_buf()])
            }
            (SandboxMode::WorkspaceWrite, NetworkAccess::Full) => {
                CanonicalSandboxPolicy::workspace_write_full(
                    vec![sandbox_cwd.to_path_buf()],
                    Vec::new(),
                    None,
                    ResourceLimits::default(),
                    SeccompProfile::strict().with_network(),
                )
            }
            (SandboxMode::DangerFullAccess, _) => CanonicalSandboxPolicy::full_access(),
            (SandboxMode::ExternalSandbox, _) => CanonicalSandboxPolicy::ExternalSandbox {
                description: LEGACY_EXTERNAL_SANDBOX_DESCRIPTION.to_string(),
            },
        }
    }

    #[must_use]
    pub fn to_canonical_policy_for_turn(&self, turn: &TurnContext) -> CanonicalSandboxPolicy {
        self.to_canonical_policy(&turn.cwd)
    }
}

#[must_use]
pub fn canonical_sandbox_policy(turn: &TurnContext) -> CanonicalSandboxPolicy {
    turn.sandbox_policy.get().to_canonical_policy_for_turn(turn)
}

/// Compute default exec approval requirement (from Codex)
pub fn default_exec_approval_requirement(
    policy: AskForApproval,
    sandbox_policy: &SandboxPolicy,
) -> ExecApprovalRequirement {
    canonical_default_exec_approval_requirement(
        policy,
        sandbox_requires_approval_prompt(sandbox_policy),
    )
}

fn sandbox_requires_approval_prompt(sandbox_policy: &SandboxPolicy) -> bool {
    sandbox_policy.requires_approval_prompt()
}

// ============================================================================
// Tool Context (from Codex)
// ============================================================================

/// Tool execution context for runtimes (from Codex)
pub struct ToolCtx {
    pub session: Arc<dyn ToolSession>,
    pub turn: Arc<TurnContext>,
    pub call_id: String,
    pub tool_name: String,
}

// ============================================================================
// Tool Error (from Codex)
// ============================================================================

/// Error from tool runtime execution (from Codex)
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool rejected: {0}")]
    Rejected(String),

    #[error("Internal error: {0}")]
    Codex(#[from] anyhow::Error),

    #[error("Sandbox denied: {0}")]
    SandboxDenied(String),

    #[error("Timeout after {0}ms")]
    Timeout(u64),
}

// ============================================================================
// Sandbox Attempt (from Codex)
// ============================================================================

/// Sandbox type for execution (from Codex)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxType {
    /// No sandbox
    None,
    /// macOS seatbelt
    Seatbelt,
    /// Linux sandbox
    LinuxSandbox,
}

impl SandboxType {
    fn from_canonical(value: CanonicalSandboxType) -> Self {
        match value {
            CanonicalSandboxType::None => Self::None,
            CanonicalSandboxType::MacosSeatbelt => Self::Seatbelt,
            CanonicalSandboxType::LinuxLandlock => Self::LinuxSandbox,
            CanonicalSandboxType::WindowsRestrictedToken => Self::None,
        }
    }
}

/// Sandbox attempt context (from Codex)
pub struct SandboxAttempt<'a> {
    pub sandbox: SandboxType,
    pub policy: &'a SandboxPolicy,
    pub sandbox_cwd: &'a Path,
    pub codex_linux_sandbox_exe: Option<&'a PathBuf>,
}

impl<'a> SandboxAttempt<'a> {
    /// Create execution environment for a command spec
    pub fn env_for(&self, spec: CommandSpec) -> Result<ExecEnv, SandboxTransformError> {
        let canonical_policy = if self.sandbox == SandboxType::None {
            CanonicalSandboxPolicy::full_access()
        } else {
            self.policy.to_canonical_policy(self.sandbox_cwd)
        };
        let canonical_spec = CanonicalCommandSpec::new(spec.program)
            .with_args(spec.args)
            .with_cwd(spec.cwd)
            .with_env(spec.env)
            .with_expiration(CanonicalExecExpiration::from(spec.timeout_ms));
        let canonical_env = CanonicalSandboxManager::new()
            .transform(
                canonical_spec,
                &canonical_policy,
                self.sandbox_cwd,
                self.codex_linux_sandbox_exe.map(PathBuf::as_path),
            )
            .map_err(SandboxTransformError::from)?;

        Ok(ExecEnv::from_canonical(canonical_env))
    }
}

/// Command specification for execution
#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub timeout_ms: Option<u64>,
}

/// Execution environment after sandbox transformation
#[derive(Clone, Debug)]
pub struct ExecEnv {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: HashMap<String, String>,
    pub timeout_ms: Option<u64>,
    pub sandbox: SandboxType,
}

impl ExecEnv {
    fn from_canonical(env: CanonicalExecEnv) -> Self {
        Self {
            program: env.program.to_string_lossy().into_owned(),
            args: env.args,
            cwd: env.cwd,
            env: env.env,
            timeout_ms: env.expiration.timeout_ms(),
            sandbox: SandboxType::from_canonical(env.sandbox_type),
        }
    }
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

impl From<CanonicalSandboxTransformError> for SandboxTransformError {
    fn from(value: CanonicalSandboxTransformError) -> Self {
        match value {
            CanonicalSandboxTransformError::MissingSandboxExecutable => {
                Self::MissingSandboxExecutable
            }
            CanonicalSandboxTransformError::UnavailableSandboxType(_) => Self::SandboxUnavailable,
            CanonicalSandboxTransformError::CreationFailed(message)
            | CanonicalSandboxTransformError::InvalidPolicy(message) => Self::ConfigError(message),
        }
    }
}

// ============================================================================
// Tool Runtime Trait (from Codex)
// ============================================================================

/// Trait for tool runtimes (from Codex)
///
/// A runtime handles the actual execution of a tool request within
/// a sandbox attempt context. Combines Approvable and Sandboxable traits.
#[async_trait]
pub trait ToolRuntime<Req, Out>: Approvable<Req> + Sandboxable
where
    Req: Send + Sync,
    Out: Send + Sync,
{
    /// Execute the tool request
    async fn run(
        &mut self,
        req: &Req,
        attempt: &SandboxAttempt<'_>,
        ctx: &ToolCtx,
    ) -> Result<Out, ToolError>;
}

// ============================================================================
// Sandbox Manager (from Codex)
// ============================================================================

/// Sandbox manager for creating sandbox attempts (from Codex)
#[derive(Default)]
pub struct SandboxManager;

impl SandboxManager {
    pub fn new() -> Self {
        Self
    }

    /// Select the initial sandbox type based on policy and preference
    pub fn select_initial(
        &self,
        policy: &SandboxPolicy,
        preference: SandboxablePreference,
    ) -> SandboxType {
        match preference {
            SandboxablePreference::Forbid => SandboxType::None,
            SandboxablePreference::Require => self.platform_sandbox(),
            SandboxablePreference::Auto => {
                if policy.uses_runtime_sandbox() {
                    self.platform_sandbox()
                } else {
                    SandboxType::None
                }
            }
        }
    }

    pub fn select_initial_for_canonical(
        &self,
        policy: &CanonicalSandboxPolicy,
        preference: SandboxablePreference,
    ) -> SandboxType {
        match preference {
            SandboxablePreference::Forbid => SandboxType::None,
            SandboxablePreference::Require => self.platform_sandbox(),
            SandboxablePreference::Auto => {
                if matches!(
                    policy,
                    CanonicalSandboxPolicy::DangerFullAccess
                        | CanonicalSandboxPolicy::ExternalSandbox { .. }
                ) {
                    SandboxType::None
                } else {
                    self.platform_sandbox()
                }
            }
        }
    }

    /// Get the platform-specific sandbox type
    #[cfg(target_os = "macos")]
    fn platform_sandbox(&self) -> SandboxType {
        SandboxType::Seatbelt
    }

    #[cfg(target_os = "linux")]
    fn platform_sandbox(&self) -> SandboxType {
        SandboxType::LinuxSandbox
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    fn platform_sandbox(&self) -> SandboxType {
        SandboxType::None
    }
}

// ============================================================================
// Execute Environment (from Codex)
// ============================================================================

/// Output from command execution (from Codex)
#[derive(Clone, Debug, Default)]
pub struct ExecToolCallOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl ExecToolCallOutput {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn combined_output(&self) -> String {
        if self.stderr.is_empty() {
            self.stdout.clone()
        } else if self.stdout.is_empty() {
            self.stderr.clone()
        } else {
            format!("{}\n{}", self.stdout, self.stderr)
        }
    }
}

/// Execute command with environment (from Codex)
pub async fn execute_env(
    env: ExecEnv,
    _policy: &SandboxPolicy,
) -> Result<ExecToolCallOutput, anyhow::Error> {
    use tokio::process::Command;

    let mut cmd = Command::new(&env.program);
    cmd.args(&env.args)
        .current_dir(&env.cwd)
        .envs(&env.env)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let timeout = env
        .timeout_ms
        .map(std::time::Duration::from_millis)
        .unwrap_or(std::time::Duration::from_secs(300));

    let output = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| anyhow::anyhow!("Command timed out"))?
        .map_err(|e| anyhow::anyhow!("Command failed: {}", e))?;

    Ok(ExecToolCallOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code().unwrap_or(-1),
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_external_sandbox_skips_exec_approval_on_request() {
        let result = default_exec_approval_requirement(
            AskForApproval::OnRequest,
            &SandboxPolicy {
                mode: SandboxMode::ExternalSandbox,
                network_access: NetworkAccess::Restricted,
            },
        );

        assert_eq!(
            result,
            ExecApprovalRequirement::Skip {
                bypass_sandbox: false,
                proposed_execpolicy_amendment: None,
            }
        );
    }

    #[test]
    fn test_restricted_sandbox_requires_exec_approval_on_request() {
        let result = default_exec_approval_requirement(
            AskForApproval::OnRequest,
            &SandboxPolicy {
                mode: SandboxMode::ReadOnly,
                network_access: NetworkAccess::Restricted,
            },
        );

        assert_eq!(
            result,
            ExecApprovalRequirement::NeedsApproval {
                reason: None,
                proposed_execpolicy_amendment: None,
            }
        );
    }

    #[test]
    fn reject_policy_still_flows_through_handler_wrapper() {
        let policy = AskForApproval::Reject(RejectConfig {
            sandbox_approval: true,
            rules: false,
            request_permissions: false,
            mcp_elicitations: false,
        });

        let requirement = default_exec_approval_requirement(
            policy,
            &SandboxPolicy {
                mode: SandboxMode::ReadOnly,
                network_access: NetworkAccess::Restricted,
            },
        );

        assert_eq!(
            requirement,
            ExecApprovalRequirement::Forbidden {
                reason: "approval policy rejected sandbox approval prompt".to_string(),
            }
        );
    }

    #[test]
    fn read_only_restricted_maps_to_canonical_read_only() {
        let policy = SandboxPolicy {
            mode: SandboxMode::ReadOnly,
            network_access: NetworkAccess::Restricted,
        };

        assert_eq!(
            policy.to_canonical_policy(Path::new("/workspace")),
            CanonicalSandboxPolicy::read_only()
        );
    }

    #[test]
    fn read_only_full_maps_to_canonical_full_network() {
        let policy = SandboxPolicy {
            mode: SandboxMode::ReadOnly,
            network_access: NetworkAccess::Full,
        };

        assert_eq!(
            policy.to_canonical_policy(Path::new("/workspace")),
            CanonicalSandboxPolicy::read_only_with_full_network()
        );
    }

    #[test]
    fn workspace_write_restricted_maps_to_canonical_workspace_write() {
        let root = PathBuf::from("/workspace");
        let policy = SandboxPolicy {
            mode: SandboxMode::WorkspaceWrite,
            network_access: NetworkAccess::Restricted,
        };

        assert_eq!(
            policy.to_canonical_policy(&root),
            CanonicalSandboxPolicy::workspace_write(vec![root])
        );
    }

    #[test]
    fn workspace_write_full_maps_to_canonical_workspace_write_with_network() {
        let root = PathBuf::from("/workspace");
        let policy = SandboxPolicy {
            mode: SandboxMode::WorkspaceWrite,
            network_access: NetworkAccess::Full,
        };

        assert_eq!(
            policy.to_canonical_policy(&root),
            CanonicalSandboxPolicy::workspace_write_full(
                vec![root],
                Vec::new(),
                None,
                ResourceLimits::default(),
                SeccompProfile::strict().with_network(),
            )
        );
    }

    #[test]
    fn danger_full_access_maps_to_canonical_full_access() {
        let policy = SandboxPolicy {
            mode: SandboxMode::DangerFullAccess,
            network_access: NetworkAccess::Restricted,
        };

        assert_eq!(
            policy.to_canonical_policy(Path::new("/workspace")),
            CanonicalSandboxPolicy::full_access()
        );
    }

    #[test]
    fn external_sandbox_maps_to_canonical_external_sandbox() {
        let policy = SandboxPolicy {
            mode: SandboxMode::ExternalSandbox,
            network_access: NetworkAccess::Full,
        };

        assert_eq!(
            policy.to_canonical_policy(Path::new("/workspace")),
            CanonicalSandboxPolicy::ExternalSandbox {
                description: LEGACY_EXTERNAL_SANDBOX_DESCRIPTION.to_string(),
            }
        );
    }

    #[test]
    fn limited_network_maps_to_restricted_network() {
        let policy = SandboxPolicy {
            mode: SandboxMode::WorkspaceWrite,
            network_access: NetworkAccess::Limited,
        };

        assert_eq!(
            policy.to_canonical_policy(Path::new("/workspace")),
            CanonicalSandboxPolicy::workspace_write(vec![PathBuf::from("/workspace")])
        );
    }

    #[tokio::test]
    async fn test_approval_store() {
        let store = ApprovalStore::new();

        assert!(!store.contains("test").await);

        store
            .set("test".to_string(), ReviewDecision::Approved)
            .await;
        assert!(store.contains("test").await);
        assert_eq!(store.get("test").await, Some(ReviewDecision::Approved));
    }

    #[test]
    fn test_sandbox_manager_platform_selection() {
        let manager = SandboxManager::new();

        // Auto preference respects policy
        let sandbox = manager.select_initial(
            &SandboxPolicy {
                mode: SandboxMode::DangerFullAccess,
                ..Default::default()
            },
            SandboxablePreference::Auto,
        );
        assert_eq!(sandbox, SandboxType::None);

        // Forbid always returns None
        let sandbox =
            manager.select_initial(&SandboxPolicy::default(), SandboxablePreference::Forbid);
        assert_eq!(sandbox, SandboxType::None);
    }

    #[test]
    fn test_exec_tool_call_output() {
        let output = ExecToolCallOutput {
            stdout: "hello".to_string(),
            stderr: "".to_string(),
            exit_code: 0,
        };
        assert!(output.success());
        assert_eq!(output.combined_output(), "hello");

        let output = ExecToolCallOutput {
            stdout: "out".to_string(),
            stderr: "err".to_string(),
            exit_code: 1,
        };
        assert!(!output.success());
        assert_eq!(output.combined_output(), "out\nerr");
    }
}
