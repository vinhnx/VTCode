//! Context-Generic Programming (CGP) substrate for VT Code
//!
//! Applies the provider-trait pattern from CGP to decouple tool runtime
//! composition from coherence restrictions. Instead of relying on blanket
//! impls or adapter newtypes, each capability (approval, sandboxing,
//! execution, metadata, etc.) is expressed as a **provider trait** with an
//! explicit `Context` parameter. A lightweight wiring step maps named
//! **components** to concrete **providers** per context type.
//!
//! # Key ideas (from the RustLab 2025 CGP talk)
//!
//! 1. **Provider traits** move `Self` to an explicit generic parameter,
//!    bypassing Rust's coherence/orphan restrictions.
//! 2. **Component names** are zero-sized marker types that act as keys in a
//!    type-level lookup table.
//! 3. **`delegate_components!`** wires component names to provider types for
//!    a given context, producing `HasComponent` implementations.
//! 4. Consumer code depends only on the component name, not the concrete
//!    provider, enabling the same tool/request to run under different
//!    policies by simply switching the context.
//!
//! ## Dictionary-passing interpretation
//!
//! This module intentionally mirrors dictionary-passing style for Rust traits.
//! `HasComponent<Name>::Provider` is the elaborated "dictionary" selected for a
//! capability, while the blanket consumer impls (`CanApproveTool`,
//! `CanExecuteTool`, etc.) are the point where the compiler proves that the
//! selected provider implements the required provider trait for the current
//! context. That keeps the capability wiring explicit and avoids depending on
//! deeper associated-type reasoning at call sites.
//!
//! # Example
//!
//! ```rust,ignore
//! use vtcode_core::components::*;
//!
//! // Define a context for interactive sessions
//! struct InteractiveCtx { /* ... */ }
//!
//! // Wire components to providers
//! delegate_components!(InteractiveCtx {
//!     ApprovalComponent  => PromptApproval,
//!     SandboxComponent   => WorkspaceSandbox,
//!     ExecuteComponent   => DefaultExecutor,
//! });
//!
//! // Define a CI context with different providers
//! struct CiCtx { /* ... */ }
//!
//! delegate_components!(CiCtx {
//!     ApprovalComponent  => AutoApproval,
//!     SandboxComponent   => StrictSandbox,
//!     ExecuteComponent   => DefaultExecutor,
//! });
//! ```

use std::borrow::Cow;
use std::fmt::Write as _;
use std::hash::Hasher;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Error, Result};
use async_trait::async_trait;
use serde_json::Value;
use vtcode_commons::serde_helpers::json_to_string_pretty;

use crate::cache::{CacheKey, UnifiedCache, estimate_json_size};
use crate::tool_policy::ToolPolicy;
use crate::tools::result::ToolResult as SplitToolResult;
use crate::tools::traits::Tool;

const MAX_RETRY_ATTEMPTS: u32 = 16;
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);

// ============================================================================
// Core wiring trait (re-exported from vtcode-commons::cgp)
// ============================================================================

pub use vtcode_commons::cgp::{ComponentProvider, HasComponent};
pub use vtcode_commons::delegate_components;

// ============================================================================
// Component name markers
// ============================================================================

mod execution;
use execution::value_to_text;
pub use execution::{
    CachedResults, CanApproveTool, CanExecuteTool, CanProvideToolMetadata, CanResolveSandbox, ExponentialBackoffRetry,
    HasExecutionCaches, HasRetryPolicy, PassthroughMetadata, ToolExecutionCacheKey, ToolFacade,
};

/// Component for approval/permission checks before tool execution.
pub enum ApprovalComponent {}

/// Component for sandbox policy selection and enforcement.
pub enum SandboxComponent {}

/// Component for the core tool execution logic.
pub enum ExecuteComponent {}

/// Component for tool metadata (name, description, schemas).
pub enum MetadataComponent {}

/// Component for session/turn context creation.
pub enum SessionComponent {}

/// Component for mapping between output formats (JSON ↔ dual-channel, etc.).
pub enum OutputMapComponent {}

/// Component for execution logging and telemetry.
pub enum LoggingComponent {}

/// Component for cached tool results.
pub enum CacheComponent {}

/// Component for retry policy around tool execution.
pub enum RetryComponent {}

// ============================================================================
// Provider traits (explicit Context parameter — the CGP "provider" pattern)
// ============================================================================

/// Provider trait for approval/permission checks.
///
/// Moves the traditional `Self` type to an explicit `Ctx` parameter so that
/// multiple overlapping implementations can coexist (e.g., prompt-based,
/// auto-approve, session-cached).
#[async_trait]
pub trait ApprovalProvider<Ctx: Send + Sync>: Send + Sync {
    /// Check whether the operation described by `description` is approved
    /// in the given context.
    async fn check_approval(ctx: &Ctx, tool_name: &str, description: &str) -> Result<()>;
}

/// Provider trait for sandbox policy resolution.
pub trait SandboxProvider<Ctx: Send + Sync>: Send + Sync {
    /// Resolve the sandbox policy for the given context.
    fn sandbox_enabled(ctx: &Ctx) -> bool;

    /// Get the workspace root enforced by the sandbox.
    fn workspace_root(ctx: &Ctx) -> Option<&PathBuf>;
}

/// Provider trait for tool metadata.
pub trait MetadataProvider<Ctx>: Send + Sync {
    fn tool_name(_ctx: &Ctx) -> &str {
        "unknown"
    }

    fn tool_description(_ctx: &Ctx) -> &str {
        ""
    }

    fn parameter_schema(_ctx: &Ctx) -> Option<Value> {
        None
    }

    fn config_schema(_ctx: &Ctx) -> Option<Value> {
        None
    }

    fn state_schema(_ctx: &Ctx) -> Option<Value> {
        None
    }

    fn prompt_path(_ctx: &Ctx) -> Option<Cow<'static, str>> {
        None
    }

    fn default_permission(_ctx: &Ctx) -> ToolPolicy {
        ToolPolicy::Prompt
    }

    fn allow_patterns(_ctx: &Ctx) -> Option<&'static [&'static str]> {
        None
    }

    fn deny_patterns(_ctx: &Ctx) -> Option<&'static [&'static str]> {
        None
    }

    fn is_mutating(_ctx: &Ctx) -> bool {
        true
    }

    fn is_parallel_safe(ctx: &Ctx) -> bool {
        !Self::is_mutating(ctx)
    }

    fn tool_kind(_ctx: &Ctx) -> &'static str {
        "unknown"
    }

    fn resource_hints(_ctx: &Ctx, _args: &Value) -> Vec<String> {
        Vec::new()
    }

    fn execution_cost(_ctx: &Ctx) -> u8 {
        5
    }
}

/// Provider trait for output format mapping.
pub trait OutputMapProvider<Ctx>: Send + Sync {
    type Input;
    type Output;

    fn map_output(ctx: &Ctx, input: Self::Input) -> Self::Output;
}

/// Provider trait for tool execution (the core "handle" operation).
///
/// Separates execution logic from metadata, approval, and output mapping
/// so the same executor can be projected through both `Tool` and `ToolHandler`
/// facades without bidirectional adapters.
#[async_trait]
pub trait ExecuteProvider<Ctx: Send + Sync>: Send + Sync {
    /// Execute a tool with JSON arguments and return JSON output.
    async fn execute(ctx: &Ctx, args: Value) -> Result<Value>;

    /// Execute with dual-channel output (LLM summary + UI content).
    ///
    /// Default delegates to `execute()` for backward compatibility.
    async fn execute_dual(ctx: &Ctx, args: Value) -> Result<SplitToolResult> {
        let result = Self::execute(ctx, args).await?;
        let name = if let Some(n) = result.get("tool_name").and_then(|v| v.as_str()) {
            n.to_string()
        } else {
            "unknown".to_string()
        };
        let content = value_to_text(&result);
        Ok(SplitToolResult::simple(&name, content))
    }
}

/// Provider trait for execution logging and telemetry.
pub trait LoggingProvider<Ctx>: Send + Sync {
    fn on_start(_ctx: &Ctx, _tool_name: &str, _args: &Value) {}

    fn on_cache_hit(_ctx: &Ctx, _tool_name: &str, _args: &Value) {}

    fn on_success(_ctx: &Ctx, _tool_name: &str, _duration: Duration, _attempt: u32, _from_cache: bool) {}

    fn on_retry(_ctx: &Ctx, _tool_name: &str, _next_attempt: u32, _backoff: Duration, _error: &Error) {}

    fn on_failure(_ctx: &Ctx, _tool_name: &str, _duration: Duration, _attempt: u32, _error: &Error) {}
}

/// Provider trait for cached tool results.
pub trait CacheProvider<Ctx>: Send + Sync {
    fn get_json(_ctx: &Ctx, _tool_name: &str, _args: &Value) -> Option<Value> {
        None
    }

    fn put_json(_ctx: &Ctx, _tool_name: &str, _args: &Value, _result: &Value) {}

    fn get_dual(_ctx: &Ctx, _tool_name: &str, _args: &Value) -> Option<SplitToolResult> {
        None
    }

    fn put_dual(_ctx: &Ctx, _tool_name: &str, _args: &Value, _result: &SplitToolResult) {}
}

/// Provider trait for retry behavior around tool execution.
pub trait RetryProvider<Ctx>: Send + Sync {
    fn max_attempts(_ctx: &Ctx, _tool_name: &str, _args: &Value) -> u32 {
        1
    }

    fn should_retry(_ctx: &Ctx, _tool_name: &str, _attempt: u32, _error: &Error) -> bool {
        false
    }

    fn backoff_duration(_ctx: &Ctx, _tool_name: &str, _attempt: u32) -> Duration {
        Duration::ZERO
    }
}

// ============================================================================
// Named provider implementations (CGP "named providers")
// ============================================================================

/// Always-approve provider for CI/test contexts.
pub struct AutoApproval;

#[async_trait]
impl<Ctx: Send + Sync> ApprovalProvider<Ctx> for AutoApproval {
    async fn check_approval(_ctx: &Ctx, _tool_name: &str, _description: &str) -> Result<()> {
        Ok(())
    }
}

/// Provider that denies all operations (useful for read-only/audit contexts).
pub struct DenyAllApproval;

#[async_trait]
impl<Ctx: Send + Sync> ApprovalProvider<Ctx> for DenyAllApproval {
    async fn check_approval(_ctx: &Ctx, tool_name: &str, _description: &str) -> Result<()> {
        anyhow::bail!("operation denied: {tool_name} is not permitted in this context")
    }
}

/// No-sandbox provider for trusted/test contexts.
pub struct NoSandbox;

#[async_trait]
impl<Ctx: Send + Sync> SandboxProvider<Ctx> for NoSandbox {
    fn sandbox_enabled(_ctx: &Ctx) -> bool {
        false
    }

    fn workspace_root(_ctx: &Ctx) -> Option<&PathBuf> {
        None
    }
}

/// Default metadata provider for contexts that do not customize tool metadata.
pub struct DefaultMetadata;

impl<Ctx> MetadataProvider<Ctx> for DefaultMetadata {}

/// No-op logging provider for contexts that don't need telemetry.
pub struct NoLogging;

impl<Ctx> LoggingProvider<Ctx> for NoLogging {}

/// No-op cache provider for contexts that should always execute directly.
pub struct NoCache;

impl<Ctx> CacheProvider<Ctx> for NoCache {}

/// No-op retry provider for contexts that should fail fast.
pub struct NoRetry;

impl<Ctx> RetryProvider<Ctx> for NoRetry {}

/// Logging provider that traces tool execution lifecycle.
pub struct TracingLogging;

impl<Ctx> LoggingProvider<Ctx> for TracingLogging {
    fn on_start(_ctx: &Ctx, tool_name: &str, _args: &Value) {
        tracing::trace!(tool = %tool_name, "CGP tool execution started");
    }

    fn on_cache_hit(_ctx: &Ctx, tool_name: &str, _args: &Value) {
        tracing::debug!(tool = %tool_name, "CGP tool result served from cache");
    }

    fn on_success(_ctx: &Ctx, tool_name: &str, duration: Duration, attempt: u32, from_cache: bool) {
        tracing::trace!(
            tool = %tool_name,
            duration_ms = duration.as_millis() as u64,
            attempt,
            from_cache,
            "CGP tool execution succeeded"
        );
    }

    fn on_retry(_ctx: &Ctx, tool_name: &str, next_attempt: u32, backoff: Duration, error: &Error) {
        tracing::debug!(
            tool = %tool_name,
            next_attempt,
            backoff_ms = backoff.as_millis() as u64,
            error = %error,
            "CGP tool execution retry scheduled"
        );
    }

    fn on_failure(_ctx: &Ctx, tool_name: &str, duration: Duration, attempt: u32, error: &Error) {
        tracing::warn!(
            tool = %tool_name,
            duration_ms = duration.as_millis() as u64,
            attempt,
            error = %error,
            "CGP tool execution failed"
        );
    }
}

// Re-export the canonical RetryPolicy from the retry module for backward
// compatibility with existing imports via `components::RetryPolicy`.
pub use crate::retry::RetryPolicy;

// ============================================================================
// Composite runtime via CGP delegation
// ============================================================================

/// A generic tool runtime that composes approval + sandbox + execution
/// through CGP component wiring.
///
/// Instead of hard-coding policy logic, `ComposableRuntime` delegates to
/// whatever providers the context has wired for each component.
pub struct ComposableRuntime;

impl ComposableRuntime {
    /// Execute a tool operation through the full approval → sandbox → execute
    /// pipeline, with all behavior determined by the context's component
    /// wiring.
    pub async fn run<Ctx>(ctx: &Ctx, tool_name: &str, description: &str) -> Result<()>
    where
        Ctx: CanApproveTool + Send + Sync,
    {
        ctx.approve_tool(tool_name, description).await?;
        Ok(())
    }

    /// Full pipeline: approval → sandbox check → delegate to caller for
    /// execution.
    pub async fn run_with_sandbox<Ctx>(ctx: &Ctx, tool_name: &str, description: &str) -> Result<bool>
    where
        Ctx: CanApproveTool + CanResolveSandbox + Send + Sync,
    {
        ctx.approve_tool(tool_name, description).await?;
        Ok(ctx.sandbox_enabled())
    }
}

// ============================================================================
// Concrete runtime contexts
// ============================================================================

/// Interactive runtime context — used during normal TUI sessions.
///
/// Wires prompt-based approval, workspace-scoped sandbox, and tracing-only
/// static middleware. Carries workspace root for path validation.
pub struct InteractiveCtx {
    pub workspace_root: PathBuf,
}

impl InteractiveCtx {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

/// Prompt-based approval provider for interactive sessions.
///
/// In the current implementation this auto-approves (the existing
/// `ToolPolicyGateway` handles actual prompting at a higher layer).
/// This provider exists so the CGP pipeline is structurally complete
/// and ready for deeper integration.
pub struct PromptApproval;

#[async_trait]
impl<Ctx: Send + Sync> ApprovalProvider<Ctx> for PromptApproval {
    async fn check_approval(_ctx: &Ctx, _tool_name: &str, _description: &str) -> Result<()> {
        // Approval is handled by ToolPolicyGateway at registry level;
        // this provider completes the CGP pipeline without double-gating.
        Ok(())
    }
}

/// Trait for contexts that expose a workspace root path.
pub trait HasWorkspaceRoot: Send + Sync {
    fn workspace_root(&self) -> &PathBuf;
}

impl HasWorkspaceRoot for InteractiveCtx {
    fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }
}

/// Workspace-scoped sandbox provider.
///
/// Reports sandbox as enabled and exposes the workspace root for path
/// validation. Generic over any context that implements `HasWorkspaceRoot`.
pub struct WorkspaceSandbox;

#[async_trait]
impl<Ctx: HasWorkspaceRoot> SandboxProvider<Ctx> for WorkspaceSandbox {
    fn sandbox_enabled(_ctx: &Ctx) -> bool {
        true
    }

    fn workspace_root(ctx: &Ctx) -> Option<&PathBuf> {
        Some(HasWorkspaceRoot::workspace_root(ctx))
    }
}

delegate_components!(InteractiveCtx {
    ApprovalComponent => PromptApproval,
    SandboxComponent  => WorkspaceSandbox,
    ExecuteComponent  => PassthroughExecutor,
    MetadataComponent => PassthroughMetadata,
    LoggingComponent  => TracingLogging,
    CacheComponent    => NoCache,
    RetryComponent    => NoRetry,
});

/// CI/automation runtime context — auto-approves, strict sandbox.
pub struct CiCtx {
    pub workspace_root: PathBuf,
}

impl CiCtx {
    pub fn new(workspace_root: PathBuf) -> Self {
        Self { workspace_root }
    }
}

impl HasWorkspaceRoot for CiCtx {
    fn workspace_root(&self) -> &PathBuf {
        &self.workspace_root
    }
}

/// Strict sandbox that enforces workspace boundaries in CI.
pub type StrictWorkspaceSandbox = WorkspaceSandbox;

delegate_components!(CiCtx {
    ApprovalComponent => AutoApproval,
    SandboxComponent  => StrictWorkspaceSandbox,
    ExecuteComponent  => PassthroughExecutor,
    MetadataComponent => PassthroughMetadata,
    LoggingComponent  => NoLogging,
    CacheComponent    => NoCache,
    RetryComponent    => NoRetry,
});

/// Benchmark runtime context — auto-approves, no sandbox, no static middleware.
pub struct BenchCtx;

delegate_components!(BenchCtx {
    ApprovalComponent => AutoApproval,
    SandboxComponent  => NoSandbox,
    ExecuteComponent  => PassthroughExecutor,
    MetadataComponent => PassthroughMetadata,
    LoggingComponent  => NoLogging,
    CacheComponent    => NoCache,
    RetryComponent    => NoRetry,
});

/// Passthrough executor — delegates to a stored `Arc<dyn Tool>`.
///
/// This bridges existing `Tool` implementations into the CGP pipeline,
/// so concrete tools (grep, file_ops, exec, etc.) can be composed with
/// CGP approval/sandbox/logging/cache/retry without rewriting them.
pub struct PassthroughExecutor;

/// Trait for contexts that can expose a tool reference for delegated metadata,
/// validation, and execution.
pub trait HasToolRef: Send + Sync {
    fn tool(&self) -> &dyn Tool;
}

/// Trait for contexts that can borrow an inner `Tool` for passthrough.
///
/// Keep shared ownership inside the bridge context; callers only need a borrowed
/// `dyn Tool` to delegate metadata, validation, and execution.
pub trait HasInnerTool: Send + Sync {
    fn inner_tool(&self) -> &dyn Tool;
}

impl<Ctx> HasToolRef for Ctx
where
    Ctx: HasInnerTool + Send + Sync,
{
    fn tool(&self) -> &dyn Tool {
        self.inner_tool()
    }
}

#[async_trait]
impl<Ctx: HasToolRef> ExecuteProvider<Ctx> for PassthroughExecutor {
    async fn execute(ctx: &Ctx, args: Value) -> Result<Value> {
        let tool = ctx.tool();
        tool.validate_args(&args)?;
        tool.execute(args).await
    }

    async fn execute_dual(ctx: &Ctx, args: Value) -> Result<SplitToolResult> {
        let tool = ctx.tool();
        tool.validate_args(&args)?;
        tool.execute_dual(args).await
    }
}

// ============================================================================
// Registry integration bridge
// ============================================================================

/// Create a `ToolFacade` that wraps an existing `Arc<dyn Tool>` with
/// CGP-wired approval, sandbox, and middleware for interactive sessions.
///
/// Prefer `wrap_native_tool_interactive()` when you still own the concrete
/// tool instance. Use this bridge when the tool is already shared elsewhere.
///
/// The returned facade implements `Tool` and can be registered directly
/// with `ToolRegistration::from_tool()`.
pub fn wrap_tool_interactive(
    tool: Arc<dyn Tool>,
    workspace_root: PathBuf,
) -> ToolFacade<ToolBridgeCtx<InteractiveCtx>> {
    let ctx = ToolBridgeCtx {
        inner: tool,
        runtime: InteractiveCtx::new(workspace_root),
    };
    ToolFacade::new(ctx)
}

/// Create a `ToolFacade` that wraps an existing `Arc<dyn Tool>` with
/// CGP-wired auto-approval and no middleware for CI contexts.
///
/// Prefer `wrap_native_tool_ci()` when you still own the concrete tool
/// instance. Use this bridge when the tool is already shared elsewhere.
pub fn wrap_tool_ci(tool: Arc<dyn Tool>, workspace_root: PathBuf) -> ToolFacade<ToolBridgeCtx<CiCtx>> {
    let ctx = ToolBridgeCtx { inner: tool, runtime: CiCtx::new(workspace_root) };
    ToolFacade::new(ctx)
}

/// Bridge context: combines a runtime context with shared tool ownership.
///
/// This keeps the `Arc<dyn Tool>` inside the bridge while
/// `PassthroughExecutor` only borrows `&dyn Tool`. The rest of the CGP
/// pipeline (approval, sandbox, middleware) is provided by the runtime
/// context's component wiring.
pub struct ToolBridgeCtx<Runtime> {
    inner: Arc<dyn Tool>,
    /// The runtime context whose component wiring is inherited via
    /// the blanket `HasComponent` impl below. Accessed at the type
    /// level, not at runtime — the compiler doesn't see field reads.
    runtime: Runtime,
}

impl<Runtime: HasWorkspaceRoot> HasWorkspaceRoot for ToolBridgeCtx<Runtime> {
    fn workspace_root(&self) -> &PathBuf {
        self.runtime.workspace_root()
    }
}

impl<Runtime: Send + Sync> HasInnerTool for ToolBridgeCtx<Runtime> {
    fn inner_tool(&self) -> &dyn Tool {
        self.inner.as_ref()
    }
}

// Component wiring for bridge contexts delegates to the runtime's wiring.
impl<Name, Runtime> HasComponent<Name> for ToolBridgeCtx<Runtime>
where
    Runtime: HasComponent<Name>,
{
    type Provider = ComponentProvider<Runtime, Name>;
}

/// Trait for contexts that carry a concrete tool instance.
pub trait HasToolInstance<T>: Send + Sync {
    fn tool_instance(&self) -> &T;
}

/// Execute provider that dispatches directly to a typed tool instance.
pub struct TypedToolExecutor<T>(PhantomData<T>);

#[async_trait]
impl<Ctx, T> ExecuteProvider<Ctx> for TypedToolExecutor<T>
where
    Ctx: HasToolRef + Send + Sync,
    T: Tool + Send + Sync,
{
    async fn execute(ctx: &Ctx, args: Value) -> Result<Value> {
        <PassthroughExecutor as ExecuteProvider<Ctx>>::execute(ctx, args).await
    }

    async fn execute_dual(ctx: &Ctx, args: Value) -> Result<SplitToolResult> {
        <PassthroughExecutor as ExecuteProvider<Ctx>>::execute_dual(ctx, args).await
    }
}

/// Metadata provider that dispatches directly to a typed tool instance.
pub struct TypedToolMetadata<T>(PhantomData<T>);

impl<Ctx, T> MetadataProvider<Ctx> for TypedToolMetadata<T>
where
    Ctx: HasToolRef,
    T: Tool + Send + Sync,
{
    fn tool_name(ctx: &Ctx) -> &str {
        <PassthroughMetadata as MetadataProvider<Ctx>>::tool_name(ctx)
    }

    fn tool_description(ctx: &Ctx) -> &str {
        <PassthroughMetadata as MetadataProvider<Ctx>>::tool_description(ctx)
    }

    fn parameter_schema(ctx: &Ctx) -> Option<Value> {
        <PassthroughMetadata as MetadataProvider<Ctx>>::parameter_schema(ctx)
    }

    fn config_schema(ctx: &Ctx) -> Option<Value> {
        <PassthroughMetadata as MetadataProvider<Ctx>>::config_schema(ctx)
    }

    fn state_schema(ctx: &Ctx) -> Option<Value> {
        <PassthroughMetadata as MetadataProvider<Ctx>>::state_schema(ctx)
    }

    fn prompt_path(ctx: &Ctx) -> Option<Cow<'static, str>> {
        <PassthroughMetadata as MetadataProvider<Ctx>>::prompt_path(ctx)
    }

    fn default_permission(ctx: &Ctx) -> ToolPolicy {
        <PassthroughMetadata as MetadataProvider<Ctx>>::default_permission(ctx)
    }

    fn allow_patterns(ctx: &Ctx) -> Option<&'static [&'static str]> {
        <PassthroughMetadata as MetadataProvider<Ctx>>::allow_patterns(ctx)
    }

    fn deny_patterns(ctx: &Ctx) -> Option<&'static [&'static str]> {
        <PassthroughMetadata as MetadataProvider<Ctx>>::deny_patterns(ctx)
    }

    fn is_mutating(ctx: &Ctx) -> bool {
        <PassthroughMetadata as MetadataProvider<Ctx>>::is_mutating(ctx)
    }

    fn is_parallel_safe(ctx: &Ctx) -> bool {
        <PassthroughMetadata as MetadataProvider<Ctx>>::is_parallel_safe(ctx)
    }

    fn tool_kind(ctx: &Ctx) -> &'static str {
        <PassthroughMetadata as MetadataProvider<Ctx>>::tool_kind(ctx)
    }

    fn resource_hints(ctx: &Ctx, args: &Value) -> Vec<String> {
        <PassthroughMetadata as MetadataProvider<Ctx>>::resource_hints(ctx, args)
    }

    fn execution_cost(ctx: &Ctx) -> u8 {
        <PassthroughMetadata as MetadataProvider<Ctx>>::execution_cost(ctx)
    }
}

/// Typed CGP context that carries a concrete tool instance plus runtime policy.
pub struct TypedToolCtx<Runtime, T> {
    tool: T,
    runtime: Runtime,
}

impl<Runtime, T> TypedToolCtx<Runtime, T> {
    pub fn new(tool: T, runtime: Runtime) -> Self {
        Self { tool, runtime }
    }
}

impl<Runtime: HasWorkspaceRoot + Send + Sync, T: Send + Sync> HasWorkspaceRoot for TypedToolCtx<Runtime, T> {
    fn workspace_root(&self) -> &PathBuf {
        self.runtime.workspace_root()
    }
}

impl<Runtime: Send + Sync, T: Send + Sync> HasToolInstance<T> for TypedToolCtx<Runtime, T> {
    fn tool_instance(&self) -> &T {
        &self.tool
    }
}

impl<Runtime: Send + Sync, T> HasToolRef for TypedToolCtx<Runtime, T>
where
    T: Tool + Send + Sync,
{
    fn tool(&self) -> &dyn Tool {
        &self.tool
    }
}

macro_rules! delegate_runtime_components_for_typed_ctx {
    ($($component:ty),+ $(,)?) => {
        $(
            impl<Runtime, T> HasComponent<$component> for TypedToolCtx<Runtime, T>
            where
                Runtime: HasComponent<$component>,
            {
                type Provider = ComponentProvider<Runtime, $component>;
            }
        )+
    };
}

delegate_runtime_components_for_typed_ctx!(
    ApprovalComponent,
    SandboxComponent,
    SessionComponent,
    OutputMapComponent,
    LoggingComponent,
    CacheComponent,
    RetryComponent,
);

impl<Runtime, T> HasComponent<ExecuteComponent> for TypedToolCtx<Runtime, T>
where
    Runtime: Send + Sync,
    T: Tool + Send + Sync,
{
    type Provider = TypedToolExecutor<T>;
}

impl<Runtime, T> HasComponent<MetadataComponent> for TypedToolCtx<Runtime, T>
where
    Runtime: Send + Sync,
    T: Tool + Send + Sync,
{
    type Provider = TypedToolMetadata<T>;
}

/// Wrap a concrete tool instance in an interactive CGP facade without
/// erasing it to `Arc<dyn Tool>` first.
///
/// Prefer this path when the caller still owns the tool and does not need
/// shared ownership.
pub fn wrap_native_tool_interactive<T>(tool: T, workspace_root: PathBuf) -> ToolFacade<TypedToolCtx<InteractiveCtx, T>>
where
    T: Tool + Send + Sync + 'static,
{
    ToolFacade::new(TypedToolCtx::new(tool, InteractiveCtx::new(workspace_root)))
}

/// Wrap a concrete tool instance in a CI CGP facade without
/// erasing it to `Arc<dyn Tool>` first.
///
/// Prefer this path when the caller still owns the tool and does not need
/// shared ownership.
pub fn wrap_native_tool_ci<T>(tool: T, workspace_root: PathBuf) -> ToolFacade<TypedToolCtx<CiCtx, T>>
where
    T: Tool + Send + Sync + 'static,
{
    ToolFacade::new(TypedToolCtx::new(tool, CiCtx::new(workspace_root)))
}

#[cfg(test)]
mod tests;
