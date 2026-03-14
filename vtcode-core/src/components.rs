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
use std::hash::Hasher;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Error, Result};
use async_trait::async_trait;
use serde_json::Value;

use crate::cache::{CacheKey, UnifiedCache};
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::tool_handler::{
    ToolCallError, ToolHandler, ToolInvocation, ToolKind, ToolOutput, ToolPayload,
};
use crate::tools::result::ToolResult as SplitToolResult;
use crate::tools::traits::Tool;

// ============================================================================
// Core wiring trait
// ============================================================================

/// Type-level lookup: maps a component **Name** to a concrete **Provider**
/// type for a given implementor (the "context").
///
/// This is the single foundational trait of the CGP substrate. All
/// composition flows through it.
pub trait HasComponent<Name> {
    /// The concrete provider type wired to `Name` for this context.
    type Provider;
}

/// Wire multiple component names to provider types for a context.
///
/// Generates one `HasComponent<Name>` implementation per entry.
///
/// ```rust,ignore
/// delegate_components!(MyCtx {
///     ApprovalComponent => PromptApproval,
///     SandboxComponent  => WorkspaceSandbox,
/// });
/// ```
#[macro_export]
macro_rules! delegate_components {
    ($ctx:ty { $($name:ty => $provider:ty),* $(,)? }) => {
        $(
            impl $crate::components::HasComponent<$name> for $ctx {
                type Provider = $provider;
            }
        )*
    };
}

// ============================================================================
// Component name markers
// ============================================================================

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
#[async_trait]
pub trait SandboxProvider<Ctx: Send + Sync>: Send + Sync {
    /// Resolve the sandbox policy for the given context.
    fn sandbox_enabled(ctx: &Ctx) -> bool;

    /// Get the workspace root enforced by the sandbox.
    fn workspace_root(ctx: &Ctx) -> Option<&PathBuf>;
}

/// Provider trait for tool metadata.
pub trait MetadataProvider<Ctx>: Send + Sync {
    fn tool_name(_ctx: &Ctx) -> &'static str {
        "unknown"
    }

    fn tool_description(_ctx: &Ctx) -> &'static str {
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

    fn on_success(
        _ctx: &Ctx,
        _tool_name: &str,
        _duration: Duration,
        _attempt: u32,
        _from_cache: bool,
    ) {
    }

    fn on_retry(
        _ctx: &Ctx,
        _tool_name: &str,
        _next_attempt: u32,
        _backoff: Duration,
        _error: &Error,
    ) {
    }

    fn on_failure(
        _ctx: &Ctx,
        _tool_name: &str,
        _duration: Duration,
        _attempt: u32,
        _error: &Error,
    ) {
    }
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
        tracing::debug!(tool = %tool_name, "CGP tool execution started");
    }

    fn on_cache_hit(_ctx: &Ctx, tool_name: &str, _args: &Value) {
        tracing::debug!(tool = %tool_name, "CGP tool result served from cache");
    }

    fn on_success(_ctx: &Ctx, tool_name: &str, duration: Duration, attempt: u32, from_cache: bool) {
        tracing::debug!(
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

/// Retry policy shared by CGP retry providers.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(1),
        }
    }
}

/// Trait for contexts that expose retry configuration.
pub trait HasRetryPolicy: Send + Sync {
    fn retry_policy(&self) -> RetryPolicy;
}

/// Exponential-backoff retry provider.
///
/// This preserves the legacy async middleware behavior of retrying failed
/// executions according to the context's retry policy.
pub struct ExponentialBackoffRetry;

impl<Ctx: HasRetryPolicy> RetryProvider<Ctx> for ExponentialBackoffRetry {
    fn max_attempts(ctx: &Ctx, _tool_name: &str, _args: &Value) -> u32 {
        ctx.retry_policy().max_attempts.max(1)
    }

    fn should_retry(_ctx: &Ctx, _tool_name: &str, _attempt: u32, _error: &Error) -> bool {
        true
    }

    fn backoff_duration(ctx: &Ctx, _tool_name: &str, attempt: u32) -> Duration {
        let policy = ctx.retry_policy();
        let exponent = attempt.saturating_sub(1).min(31);
        let factor = 2_u64.saturating_pow(exponent);
        let millis = policy.initial_backoff.as_millis() as u64;
        Duration::from_millis(millis.saturating_mul(factor)).min(policy.max_backoff)
    }
}

/// Cache key for tool execution results.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ToolExecutionCacheKey(String);

impl CacheKey for ToolExecutionCacheKey {
    fn to_cache_key(&self) -> String {
        self.0.clone()
    }
}

/// Trait for contexts that expose JSON and dual-result caches.
pub trait HasExecutionCaches: Send + Sync {
    fn json_cache(&self) -> &UnifiedCache<ToolExecutionCacheKey, Value>;
    fn dual_cache(&self) -> &UnifiedCache<ToolExecutionCacheKey, SplitToolResult>;
}

fn build_execution_cache_key(tool_name: &str, args: &Value) -> ToolExecutionCacheKey {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    hasher.write(tool_name.as_bytes());
    hasher.write(
        serde_json::to_string(args)
            .unwrap_or_else(|_| args.to_string())
            .as_bytes(),
    );
    ToolExecutionCacheKey(format!("{tool_name}::{}", hasher.finish()))
}

/// Cache provider backed by `UnifiedCache`.
pub struct CachedResults;

impl<Ctx: HasExecutionCaches> CacheProvider<Ctx> for CachedResults {
    fn get_json(ctx: &Ctx, tool_name: &str, args: &Value) -> Option<Value> {
        ctx.json_cache()
            .get_owned(&build_execution_cache_key(tool_name, args))
    }

    fn put_json(ctx: &Ctx, tool_name: &str, args: &Value, result: &Value) {
        let key = build_execution_cache_key(tool_name, args);
        let size = serde_json::to_string(result)
            .map(|json| json.len() as u64)
            .unwrap_or_default();
        ctx.json_cache().insert(key, result.clone(), size);
    }

    fn get_dual(ctx: &Ctx, tool_name: &str, args: &Value) -> Option<SplitToolResult> {
        ctx.dual_cache()
            .get_owned(&build_execution_cache_key(tool_name, args))
    }

    fn put_dual(ctx: &Ctx, tool_name: &str, args: &Value, result: &SplitToolResult) {
        let key = build_execution_cache_key(tool_name, args);
        let size = (result.llm_content.len() + result.ui_content.len()) as u64;
        ctx.dual_cache().insert(key, result.clone(), size);
    }
}

/// Metadata provider that delegates to an inner `Tool`.
pub struct PassthroughMetadata;

impl<Ctx: HasInnerTool> MetadataProvider<Ctx> for PassthroughMetadata {
    fn tool_name(ctx: &Ctx) -> &'static str {
        ctx.inner_tool().name()
    }

    fn tool_description(ctx: &Ctx) -> &'static str {
        ctx.inner_tool().description()
    }

    fn parameter_schema(ctx: &Ctx) -> Option<Value> {
        ctx.inner_tool().parameter_schema()
    }

    fn config_schema(ctx: &Ctx) -> Option<Value> {
        ctx.inner_tool().config_schema()
    }

    fn state_schema(ctx: &Ctx) -> Option<Value> {
        ctx.inner_tool().state_schema()
    }

    fn prompt_path(ctx: &Ctx) -> Option<Cow<'static, str>> {
        ctx.inner_tool().prompt_path()
    }

    fn default_permission(ctx: &Ctx) -> ToolPolicy {
        ctx.inner_tool().default_permission()
    }

    fn allow_patterns(ctx: &Ctx) -> Option<&'static [&'static str]> {
        ctx.inner_tool().allow_patterns()
    }

    fn deny_patterns(ctx: &Ctx) -> Option<&'static [&'static str]> {
        ctx.inner_tool().deny_patterns()
    }

    fn is_mutating(ctx: &Ctx) -> bool {
        ctx.inner_tool().is_mutating()
    }

    fn is_parallel_safe(ctx: &Ctx) -> bool {
        ctx.inner_tool().is_parallel_safe()
    }

    fn tool_kind(ctx: &Ctx) -> &'static str {
        ctx.inner_tool().kind()
    }

    fn resource_hints(ctx: &Ctx, args: &Value) -> Vec<String> {
        ctx.inner_tool().resource_hints(args)
    }

    fn execution_cost(ctx: &Ctx) -> u8 {
        ctx.inner_tool().execution_cost()
    }
}

// ============================================================================
// Consumer traits (blanket impls over provider wiring)
// ============================================================================

fn value_to_text(value: &Value) -> String {
    if value.is_string() {
        value.as_str().unwrap_or("").to_string()
    } else {
        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
    }
}

#[async_trait]
pub trait CanApproveTool: Send + Sync {
    async fn approve_tool(&self, tool_name: &str, description: &str) -> Result<()>;
}

#[async_trait]
impl<Ctx> CanApproveTool for Ctx
where
    Ctx: HasComponent<ApprovalComponent> + Send + Sync,
    <Ctx as HasComponent<ApprovalComponent>>::Provider: ApprovalProvider<Ctx>,
{
    async fn approve_tool(&self, tool_name: &str, description: &str) -> Result<()> {
        <<Ctx as HasComponent<ApprovalComponent>>::Provider as ApprovalProvider<Ctx>>::check_approval(
            self,
            tool_name,
            description,
        )
        .await
    }
}

pub trait CanResolveSandbox: Send + Sync {
    fn sandbox_enabled(&self) -> bool;

    fn workspace_root(&self) -> Option<&PathBuf>;
}

impl<Ctx> CanResolveSandbox for Ctx
where
    Ctx: HasComponent<SandboxComponent> + Send + Sync,
    <Ctx as HasComponent<SandboxComponent>>::Provider: SandboxProvider<Ctx>,
{
    fn sandbox_enabled(&self) -> bool {
        <<Ctx as HasComponent<SandboxComponent>>::Provider as SandboxProvider<Ctx>>::sandbox_enabled(
            self,
        )
    }

    fn workspace_root(&self) -> Option<&PathBuf> {
        <<Ctx as HasComponent<SandboxComponent>>::Provider as SandboxProvider<Ctx>>::workspace_root(
            self,
        )
    }
}

pub trait CanProvideToolMetadata: Send + Sync {
    fn tool_name(&self) -> &'static str;

    fn tool_description(&self) -> &'static str;

    fn parameter_schema(&self) -> Option<Value>;

    fn config_schema(&self) -> Option<Value>;

    fn state_schema(&self) -> Option<Value>;

    fn prompt_path(&self) -> Option<Cow<'static, str>>;

    fn default_permission(&self) -> ToolPolicy;

    fn allow_patterns(&self) -> Option<&'static [&'static str]>;

    fn deny_patterns(&self) -> Option<&'static [&'static str]>;

    fn is_mutating(&self) -> bool;

    fn is_parallel_safe(&self) -> bool;

    fn tool_kind(&self) -> &'static str;

    fn resource_hints(&self, args: &Value) -> Vec<String>;

    fn execution_cost(&self) -> u8;
}

impl<Ctx> CanProvideToolMetadata for Ctx
where
    Ctx: HasComponent<MetadataComponent> + Send + Sync,
    <Ctx as HasComponent<MetadataComponent>>::Provider: MetadataProvider<Ctx>,
{
    fn tool_name(&self) -> &'static str {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::tool_name(
            self,
        )
    }

    fn tool_description(&self) -> &'static str {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::tool_description(
            self,
        )
    }

    fn parameter_schema(&self) -> Option<Value> {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::parameter_schema(
            self,
        )
    }

    fn config_schema(&self) -> Option<Value> {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::config_schema(
            self,
        )
    }

    fn state_schema(&self) -> Option<Value> {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::state_schema(
            self,
        )
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::prompt_path(
            self,
        )
    }

    fn default_permission(&self) -> ToolPolicy {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::default_permission(
            self,
        )
    }

    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::allow_patterns(
            self,
        )
    }

    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::deny_patterns(
            self,
        )
    }

    fn is_mutating(&self) -> bool {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::is_mutating(
            self,
        )
    }

    fn is_parallel_safe(&self) -> bool {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::is_parallel_safe(
            self,
        )
    }

    fn tool_kind(&self) -> &'static str {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::tool_kind(
            self,
        )
    }

    fn resource_hints(&self, args: &Value) -> Vec<String> {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::resource_hints(
            self,
            args,
        )
    }

    fn execution_cost(&self) -> u8 {
        <<Ctx as HasComponent<MetadataComponent>>::Provider as MetadataProvider<Ctx>>::execution_cost(
            self,
        )
    }
}

#[async_trait]
pub trait CanExecuteTool: Send + Sync {
    async fn execute_tool_json(&self, tool_name: &str, args: Value) -> Result<Value>;

    async fn execute_tool_dual(&self, tool_name: &str, args: Value) -> Result<SplitToolResult>;
}

#[async_trait]
impl<Ctx> CanExecuteTool for Ctx
where
    Ctx: HasComponent<ExecuteComponent>
        + HasComponent<LoggingComponent>
        + HasComponent<CacheComponent>
        + HasComponent<RetryComponent>
        + Send
        + Sync,
    <Ctx as HasComponent<ExecuteComponent>>::Provider: ExecuteProvider<Ctx>,
    <Ctx as HasComponent<LoggingComponent>>::Provider: LoggingProvider<Ctx>,
    <Ctx as HasComponent<CacheComponent>>::Provider: CacheProvider<Ctx>,
    <Ctx as HasComponent<RetryComponent>>::Provider: RetryProvider<Ctx>,
{
    async fn execute_tool_json(&self, tool_name: &str, args: Value) -> Result<Value> {
        <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_start(
            self, tool_name, &args,
        );

        if let Some(result) = <<Ctx as HasComponent<CacheComponent>>::Provider as CacheProvider<
            Ctx,
        >>::get_json(self, tool_name, &args)
        {
            <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_cache_hit(
                self, tool_name, &args,
            );
            <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_success(
                self,
                tool_name,
                Duration::ZERO,
                1,
                true,
            );
            return Ok(result);
        }

        let started = Instant::now();
        let max_attempts = <<Ctx as HasComponent<RetryComponent>>::Provider as RetryProvider<
            Ctx,
        >>::max_attempts(self, tool_name, &args)
        .max(1);

        for attempt_index in 0..max_attempts {
            let attempt = attempt_index + 1;
            match <<Ctx as HasComponent<ExecuteComponent>>::Provider as ExecuteProvider<
                Ctx,
            >>::execute(self, args.clone())
            .await
            {
                Ok(result) => {
                    <<Ctx as HasComponent<CacheComponent>>::Provider as CacheProvider<Ctx>>::put_json(
                        self, tool_name, &args, &result,
                    );
                    <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_success(
                        self,
                        tool_name,
                        started.elapsed(),
                        attempt,
                        false,
                    );
                    return Ok(result);
                }
                Err(error) => {
                    let should_retry = attempt < max_attempts
                        && <<Ctx as HasComponent<RetryComponent>>::Provider as RetryProvider<
                            Ctx,
                        >>::should_retry(self, tool_name, attempt, &error);
                    if should_retry {
                        let backoff =
                            <<Ctx as HasComponent<RetryComponent>>::Provider as RetryProvider<Ctx>>::backoff_duration(
                                self, tool_name, attempt,
                            );
                        <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_retry(
                            self,
                            tool_name,
                            attempt + 1,
                            backoff,
                            &error,
                        );
                        if !backoff.is_zero() {
                            tokio::time::sleep(backoff).await;
                        }
                        continue;
                    }

                    <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_failure(
                        self,
                        tool_name,
                        started.elapsed(),
                        attempt,
                        &error,
                    );
                    return Err(error);
                }
            }
        }

        unreachable!("retry loop always returns or continues")
    }

    async fn execute_tool_dual(&self, tool_name: &str, args: Value) -> Result<SplitToolResult> {
        <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_start(
            self, tool_name, &args,
        );

        if let Some(result) = <<Ctx as HasComponent<CacheComponent>>::Provider as CacheProvider<
            Ctx,
        >>::get_dual(self, tool_name, &args)
        {
            <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_cache_hit(
                self, tool_name, &args,
            );
            <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_success(
                self,
                tool_name,
                Duration::ZERO,
                1,
                true,
            );
            return Ok(result);
        }

        let started = Instant::now();
        let max_attempts = <<Ctx as HasComponent<RetryComponent>>::Provider as RetryProvider<
            Ctx,
        >>::max_attempts(self, tool_name, &args)
        .max(1);

        for attempt_index in 0..max_attempts {
            let attempt = attempt_index + 1;
            match <<Ctx as HasComponent<ExecuteComponent>>::Provider as ExecuteProvider<
                Ctx,
            >>::execute_dual(self, args.clone())
            .await
            {
                Ok(result) => {
                    <<Ctx as HasComponent<CacheComponent>>::Provider as CacheProvider<Ctx>>::put_dual(
                        self, tool_name, &args, &result,
                    );
                    <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_success(
                        self,
                        tool_name,
                        started.elapsed(),
                        attempt,
                        false,
                    );
                    return Ok(result);
                }
                Err(error) => {
                    let should_retry = attempt < max_attempts
                        && <<Ctx as HasComponent<RetryComponent>>::Provider as RetryProvider<Ctx>>::should_retry(
                            self, tool_name, attempt, &error,
                        );
                    if should_retry {
                        let backoff =
                            <<Ctx as HasComponent<RetryComponent>>::Provider as RetryProvider<Ctx>>::backoff_duration(
                                self, tool_name, attempt,
                            );
                        <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_retry(
                            self,
                            tool_name,
                            attempt + 1,
                            backoff,
                            &error,
                        );
                        if !backoff.is_zero() {
                            tokio::time::sleep(backoff).await;
                        }
                        continue;
                    }

                    <<Ctx as HasComponent<LoggingComponent>>::Provider as LoggingProvider<Ctx>>::on_failure(
                        self,
                        tool_name,
                        started.elapsed(),
                        attempt,
                        &error,
                    );
                    return Err(error);
                }
            }
        }

        unreachable!("retry loop always returns or continues")
    }
}

// ============================================================================
// CGP-backed Tool facade
// ============================================================================

/// A facade that projects a CGP-wired context as a `Tool` trait object.
///
/// This replaces `HandlerToToolAdapter` — instead of wrapping a `ToolHandler`
/// in a `Tool` newtype, the context itself carries all the component wiring.
/// The facade simply delegates to the wired providers.
pub struct ToolFacade<Ctx> {
    ctx: Ctx,
}

impl<Ctx> ToolFacade<Ctx> {
    pub fn new(ctx: Ctx) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl<Ctx> Tool for ToolFacade<Ctx>
where
    Ctx: CanApproveTool + CanExecuteTool + CanProvideToolMetadata + Send + Sync + 'static,
{
    async fn execute(&self, args: Value) -> Result<Value> {
        self.ctx.approve_tool(self.name(), "execute").await?;

        self.ctx.execute_tool_json(self.name(), args).await
    }

    async fn execute_dual(&self, args: Value) -> Result<SplitToolResult> {
        self.ctx.approve_tool(self.name(), "execute_dual").await?;

        self.ctx.execute_tool_dual(self.name(), args).await
    }

    fn name(&self) -> &'static str {
        CanProvideToolMetadata::tool_name(&self.ctx)
    }

    fn description(&self) -> &'static str {
        CanProvideToolMetadata::tool_description(&self.ctx)
    }

    fn parameter_schema(&self) -> Option<Value> {
        CanProvideToolMetadata::parameter_schema(&self.ctx)
    }

    fn config_schema(&self) -> Option<Value> {
        CanProvideToolMetadata::config_schema(&self.ctx)
    }

    fn state_schema(&self) -> Option<Value> {
        CanProvideToolMetadata::state_schema(&self.ctx)
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        CanProvideToolMetadata::prompt_path(&self.ctx)
    }

    fn default_permission(&self) -> ToolPolicy {
        CanProvideToolMetadata::default_permission(&self.ctx)
    }

    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        CanProvideToolMetadata::allow_patterns(&self.ctx)
    }

    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        CanProvideToolMetadata::deny_patterns(&self.ctx)
    }

    fn is_mutating(&self) -> bool {
        CanProvideToolMetadata::is_mutating(&self.ctx)
    }

    fn is_parallel_safe(&self) -> bool {
        CanProvideToolMetadata::is_parallel_safe(&self.ctx)
    }

    fn kind(&self) -> &'static str {
        CanProvideToolMetadata::tool_kind(&self.ctx)
    }

    fn resource_hints(&self, args: &Value) -> Vec<String> {
        CanProvideToolMetadata::resource_hints(&self.ctx, args)
    }

    fn execution_cost(&self) -> u8 {
        CanProvideToolMetadata::execution_cost(&self.ctx)
    }
}

// ============================================================================
// CGP-backed ToolHandler facade
// ============================================================================

/// A facade that projects a CGP-wired context as a `ToolHandler` trait object.
///
/// This replaces `ToolToHandlerAdapter` — instead of wrapping a `Tool` in a
/// `ToolHandler` newtype, the same context used by `ToolFacade` can also be
/// projected as a `ToolHandler` with zero additional adaptation code.
pub struct HandlerFacade<Ctx> {
    ctx: Ctx,
}

impl<Ctx> HandlerFacade<Ctx> {
    pub fn new(ctx: Ctx) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl<Ctx> ToolHandler for HandlerFacade<Ctx>
where
    Ctx: CanApproveTool + CanExecuteTool + Send + Sync + 'static,
{
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
        // Extract arguments from payload
        let args: Value = match &invocation.payload {
            ToolPayload::Function { arguments } => serde_json::from_str(arguments)
                .map_err(|e| ToolCallError::respond(format!("Invalid arguments: {e}")))?,
            _ => return Err(ToolCallError::respond("Unsupported payload type")),
        };

        self.ctx
            .approve_tool(&invocation.tool_name, "handle")
            .await
            .map_err(|e| ToolCallError::respond(e.to_string()))?;

        match self
            .ctx
            .execute_tool_json(&invocation.tool_name, args)
            .await
        {
            Ok(result) => {
                let text = value_to_text(&result);
                Ok(ToolOutput::simple(text))
            }
            Err(e) => Err(ToolCallError::Internal(e)),
        }
    }
}

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
    pub async fn run_with_sandbox<Ctx>(
        ctx: &Ctx,
        tool_name: &str,
        description: &str,
    ) -> Result<bool>
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
/// Reuses `WorkspaceSandbox` since the `HasWorkspaceRoot` trait
/// makes it generic over any context with a workspace root.
pub struct StrictWorkspaceSandbox;

#[async_trait]
impl<Ctx: HasWorkspaceRoot> SandboxProvider<Ctx> for StrictWorkspaceSandbox {
    fn sandbox_enabled(_ctx: &Ctx) -> bool {
        true
    }

    fn workspace_root(ctx: &Ctx) -> Option<&PathBuf> {
        Some(HasWorkspaceRoot::workspace_root(ctx))
    }
}

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

/// Trait for contexts that carry an inner `Tool` reference for passthrough.
pub trait HasInnerTool: Send + Sync {
    fn inner_tool(&self) -> &Arc<dyn Tool>;
}

#[async_trait]
impl<Ctx: HasInnerTool> ExecuteProvider<Ctx> for PassthroughExecutor {
    async fn execute(ctx: &Ctx, args: Value) -> Result<Value> {
        ctx.inner_tool().execute(args).await
    }

    async fn execute_dual(ctx: &Ctx, args: Value) -> Result<SplitToolResult> {
        ctx.inner_tool().execute_dual(args).await
    }
}

// ============================================================================
// Registry integration bridge
// ============================================================================

/// Create a `ToolFacade` that wraps an existing `Arc<dyn Tool>` with
/// CGP-wired approval, sandbox, and middleware for interactive sessions.
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
pub fn wrap_tool_ci(
    tool: Arc<dyn Tool>,
    workspace_root: PathBuf,
) -> ToolFacade<ToolBridgeCtx<CiCtx>> {
    let ctx = ToolBridgeCtx {
        inner: tool,
        runtime: CiCtx::new(workspace_root),
    };
    ToolFacade::new(ctx)
}

/// Bridge context: combines a runtime context with an inner `Tool` reference.
///
/// This allows `PassthroughExecutor` to delegate to existing tool impls
/// while the rest of the CGP pipeline (approval, sandbox, middleware)
/// is provided by the runtime context's component wiring.
pub struct ToolBridgeCtx<Runtime> {
    inner: Arc<dyn Tool>,
    /// The runtime context whose component wiring is inherited via
    /// the blanket `HasComponent` impl below. Accessed at the type
    /// level, not at runtime — the compiler doesn't see field reads.
    #[allow(dead_code)]
    runtime: Runtime,
}

impl<Runtime: HasWorkspaceRoot> HasWorkspaceRoot for ToolBridgeCtx<Runtime> {
    fn workspace_root(&self) -> &PathBuf {
        self.runtime.workspace_root()
    }
}

impl<Runtime: Send + Sync> HasInnerTool for ToolBridgeCtx<Runtime> {
    fn inner_tool(&self) -> &Arc<dyn Tool> {
        &self.inner
    }
}

// Component wiring for bridge contexts delegates to the runtime's wiring.
impl<Name, Runtime> HasComponent<Name> for ToolBridgeCtx<Runtime>
where
    Runtime: HasComponent<Name>,
{
    type Provider = <Runtime as HasComponent<Name>>::Provider;
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
    Ctx: HasToolInstance<T> + Send + Sync,
    T: Tool + Send + Sync,
{
    async fn execute(ctx: &Ctx, args: Value) -> Result<Value> {
        ctx.tool_instance().execute(args).await
    }

    async fn execute_dual(ctx: &Ctx, args: Value) -> Result<SplitToolResult> {
        ctx.tool_instance().execute_dual(args).await
    }
}

/// Metadata provider that dispatches directly to a typed tool instance.
pub struct TypedToolMetadata<T>(PhantomData<T>);

impl<Ctx, T> MetadataProvider<Ctx> for TypedToolMetadata<T>
where
    Ctx: HasToolInstance<T>,
    T: Tool + Send + Sync,
{
    fn tool_name(ctx: &Ctx) -> &'static str {
        ctx.tool_instance().name()
    }

    fn tool_description(ctx: &Ctx) -> &'static str {
        ctx.tool_instance().description()
    }

    fn parameter_schema(ctx: &Ctx) -> Option<Value> {
        ctx.tool_instance().parameter_schema()
    }

    fn config_schema(ctx: &Ctx) -> Option<Value> {
        ctx.tool_instance().config_schema()
    }

    fn state_schema(ctx: &Ctx) -> Option<Value> {
        ctx.tool_instance().state_schema()
    }

    fn prompt_path(ctx: &Ctx) -> Option<Cow<'static, str>> {
        ctx.tool_instance().prompt_path()
    }

    fn default_permission(ctx: &Ctx) -> ToolPolicy {
        ctx.tool_instance().default_permission()
    }

    fn allow_patterns(ctx: &Ctx) -> Option<&'static [&'static str]> {
        ctx.tool_instance().allow_patterns()
    }

    fn deny_patterns(ctx: &Ctx) -> Option<&'static [&'static str]> {
        ctx.tool_instance().deny_patterns()
    }

    fn is_mutating(ctx: &Ctx) -> bool {
        ctx.tool_instance().is_mutating()
    }

    fn is_parallel_safe(ctx: &Ctx) -> bool {
        ctx.tool_instance().is_parallel_safe()
    }

    fn tool_kind(ctx: &Ctx) -> &'static str {
        ctx.tool_instance().kind()
    }

    fn resource_hints(ctx: &Ctx, args: &Value) -> Vec<String> {
        ctx.tool_instance().resource_hints(args)
    }

    fn execution_cost(ctx: &Ctx) -> u8 {
        ctx.tool_instance().execution_cost()
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

impl<Runtime: HasWorkspaceRoot + Send + Sync, T: Send + Sync> HasWorkspaceRoot
    for TypedToolCtx<Runtime, T>
{
    fn workspace_root(&self) -> &PathBuf {
        self.runtime.workspace_root()
    }
}

impl<Runtime: Send + Sync, T: Send + Sync> HasToolInstance<T> for TypedToolCtx<Runtime, T> {
    fn tool_instance(&self) -> &T {
        &self.tool
    }
}

impl<Runtime, T> HasComponent<ApprovalComponent> for TypedToolCtx<Runtime, T>
where
    Runtime: HasComponent<ApprovalComponent>,
{
    type Provider = <Runtime as HasComponent<ApprovalComponent>>::Provider;
}

impl<Runtime, T> HasComponent<SandboxComponent> for TypedToolCtx<Runtime, T>
where
    Runtime: HasComponent<SandboxComponent>,
{
    type Provider = <Runtime as HasComponent<SandboxComponent>>::Provider;
}

impl<Runtime, T> HasComponent<SessionComponent> for TypedToolCtx<Runtime, T>
where
    Runtime: HasComponent<SessionComponent>,
{
    type Provider = <Runtime as HasComponent<SessionComponent>>::Provider;
}

impl<Runtime, T> HasComponent<OutputMapComponent> for TypedToolCtx<Runtime, T>
where
    Runtime: HasComponent<OutputMapComponent>,
{
    type Provider = <Runtime as HasComponent<OutputMapComponent>>::Provider;
}

impl<Runtime, T> HasComponent<LoggingComponent> for TypedToolCtx<Runtime, T>
where
    Runtime: HasComponent<LoggingComponent>,
{
    type Provider = <Runtime as HasComponent<LoggingComponent>>::Provider;
}

impl<Runtime, T> HasComponent<CacheComponent> for TypedToolCtx<Runtime, T>
where
    Runtime: HasComponent<CacheComponent>,
{
    type Provider = <Runtime as HasComponent<CacheComponent>>::Provider;
}

impl<Runtime, T> HasComponent<RetryComponent> for TypedToolCtx<Runtime, T>
where
    Runtime: HasComponent<RetryComponent>,
{
    type Provider = <Runtime as HasComponent<RetryComponent>>::Provider;
}

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
pub fn wrap_native_tool_interactive<T>(
    tool: T,
    workspace_root: PathBuf,
) -> ToolFacade<TypedToolCtx<InteractiveCtx, T>>
where
    T: Tool + Send + Sync + 'static,
{
    ToolFacade::new(TypedToolCtx::new(tool, InteractiveCtx::new(workspace_root)))
}

/// Wrap a concrete tool instance in a CI CGP facade without
/// erasing it to `Arc<dyn Tool>` first.
pub fn wrap_native_tool_ci<T>(
    tool: T,
    workspace_root: PathBuf,
) -> ToolFacade<TypedToolCtx<CiCtx, T>>
where
    T: Tool + Send + Sync + 'static,
{
    ToolFacade::new(TypedToolCtx::new(tool, CiCtx::new(workspace_root)))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::EvictionPolicy;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // ================================================================
    // Test executor provider
    // ================================================================

    struct EchoExecutor;

    #[async_trait]
    impl<Ctx: Send + Sync> ExecuteProvider<Ctx> for EchoExecutor {
        async fn execute(_ctx: &Ctx, args: Value) -> Result<Value> {
            Ok(serde_json::json!({
                "tool_name": "echo",
                "echoed": args,
            }))
        }
    }

    // ================================================================
    // Test contexts with different component wiring
    // ================================================================

    struct TestAutoCtx;

    struct EchoMetadata;

    impl<Ctx> MetadataProvider<Ctx> for EchoMetadata {
        fn tool_name(_ctx: &Ctx) -> &'static str {
            "echo"
        }

        fn tool_description(_ctx: &Ctx) -> &'static str {
            "Echo tool"
        }
    }

    delegate_components!(TestAutoCtx {
        ApprovalComponent => AutoApproval,
        SandboxComponent  => NoSandbox,
        ExecuteComponent  => EchoExecutor,
        MetadataComponent => EchoMetadata,
        LoggingComponent  => NoLogging,
        CacheComponent    => NoCache,
        RetryComponent    => NoRetry,
    });

    struct TestDenyCtx;

    struct ExecMetadata;

    impl<Ctx> MetadataProvider<Ctx> for ExecMetadata {
        fn tool_name(_ctx: &Ctx) -> &'static str {
            "exec"
        }

        fn tool_description(_ctx: &Ctx) -> &'static str {
            "Exec tool"
        }
    }

    delegate_components!(TestDenyCtx {
        ApprovalComponent => DenyAllApproval,
        SandboxComponent  => NoSandbox,
        ExecuteComponent  => EchoExecutor,
        MetadataComponent => ExecMetadata,
        LoggingComponent  => NoLogging,
        CacheComponent    => NoCache,
        RetryComponent    => NoRetry,
    });

    struct TestTracingCtx;

    struct TracedToolMetadata;

    impl<Ctx> MetadataProvider<Ctx> for TracedToolMetadata {
        fn tool_name(_ctx: &Ctx) -> &'static str {
            "traced_tool"
        }

        fn tool_description(_ctx: &Ctx) -> &'static str {
            "A traced tool"
        }
    }

    delegate_components!(TestTracingCtx {
        ApprovalComponent => AutoApproval,
        SandboxComponent  => NoSandbox,
        ExecuteComponent  => EchoExecutor,
        MetadataComponent => TracedToolMetadata,
        LoggingComponent  => TracingLogging,
        CacheComponent    => NoCache,
        RetryComponent    => NoRetry,
    });

    struct NamedToolCtx;

    struct NamedToolMetadata;

    impl<Ctx> MetadataProvider<Ctx> for NamedToolMetadata {
        fn tool_name(_ctx: &Ctx) -> &'static str {
            "my_tool"
        }

        fn tool_description(_ctx: &Ctx) -> &'static str {
            "My description"
        }
    }

    delegate_components!(NamedToolCtx {
        ApprovalComponent => AutoApproval,
        SandboxComponent  => NoSandbox,
        ExecuteComponent  => EchoExecutor,
        MetadataComponent => NamedToolMetadata,
        LoggingComponent  => NoLogging,
        CacheComponent    => NoCache,
        RetryComponent    => NoRetry,
    });

    trait HasExecutionCount: Send + Sync {
        fn execution_count(&self) -> &AtomicUsize;
    }

    struct CountingExecutor;

    #[async_trait]
    impl<Ctx: HasExecutionCount + Send + Sync> ExecuteProvider<Ctx> for CountingExecutor {
        async fn execute(ctx: &Ctx, args: Value) -> Result<Value> {
            let count = ctx.execution_count().fetch_add(1, Ordering::SeqCst) + 1;
            Ok(serde_json::json!({
                "tool_name": "counting",
                "attempt": count,
                "args": args,
            }))
        }
    }

    struct TestCachingCtx {
        executions: Arc<AtomicUsize>,
        json_cache: UnifiedCache<ToolExecutionCacheKey, Value>,
        dual_cache: UnifiedCache<ToolExecutionCacheKey, SplitToolResult>,
    }

    impl TestCachingCtx {
        fn new(executions: Arc<AtomicUsize>) -> Self {
            Self {
                executions,
                json_cache: UnifiedCache::new(8, Duration::from_secs(60), EvictionPolicy::Lru),
                dual_cache: UnifiedCache::new(8, Duration::from_secs(60), EvictionPolicy::Lru),
            }
        }
    }

    impl HasExecutionCount for TestCachingCtx {
        fn execution_count(&self) -> &AtomicUsize {
            self.executions.as_ref()
        }
    }

    impl HasExecutionCaches for TestCachingCtx {
        fn json_cache(&self) -> &UnifiedCache<ToolExecutionCacheKey, Value> {
            &self.json_cache
        }

        fn dual_cache(&self) -> &UnifiedCache<ToolExecutionCacheKey, SplitToolResult> {
            &self.dual_cache
        }
    }

    struct CachedToolMetadata;

    impl<Ctx> MetadataProvider<Ctx> for CachedToolMetadata {
        fn tool_name(_ctx: &Ctx) -> &'static str {
            "cached_tool"
        }

        fn tool_description(_ctx: &Ctx) -> &'static str {
            "A cached tool"
        }
    }

    delegate_components!(TestCachingCtx {
        ApprovalComponent => AutoApproval,
        SandboxComponent  => NoSandbox,
        ExecuteComponent  => CountingExecutor,
        MetadataComponent => CachedToolMetadata,
        LoggingComponent  => NoLogging,
        CacheComponent    => CachedResults,
        RetryComponent    => NoRetry,
    });

    struct FlakyExecutor;

    #[async_trait]
    impl<Ctx: HasExecutionCount + Send + Sync> ExecuteProvider<Ctx> for FlakyExecutor {
        async fn execute(ctx: &Ctx, args: Value) -> Result<Value> {
            let attempt = ctx.execution_count().fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 1 {
                anyhow::bail!("transient failure")
            }

            Ok(serde_json::json!({
                "tool_name": "flaky",
                "attempt": attempt,
                "args": args,
            }))
        }
    }

    struct TestRetryCtx {
        executions: Arc<AtomicUsize>,
        retry_policy: RetryPolicy,
    }

    impl TestRetryCtx {
        fn new(executions: Arc<AtomicUsize>, retry_policy: RetryPolicy) -> Self {
            Self {
                executions,
                retry_policy,
            }
        }
    }

    impl HasExecutionCount for TestRetryCtx {
        fn execution_count(&self) -> &AtomicUsize {
            self.executions.as_ref()
        }
    }

    impl HasRetryPolicy for TestRetryCtx {
        fn retry_policy(&self) -> RetryPolicy {
            self.retry_policy
        }
    }

    struct FlakyToolMetadata;

    impl<Ctx> MetadataProvider<Ctx> for FlakyToolMetadata {
        fn tool_name(_ctx: &Ctx) -> &'static str {
            "flaky_tool"
        }

        fn tool_description(_ctx: &Ctx) -> &'static str {
            "A flaky tool"
        }
    }

    delegate_components!(TestRetryCtx {
        ApprovalComponent => AutoApproval,
        SandboxComponent  => NoSandbox,
        ExecuteComponent  => FlakyExecutor,
        MetadataComponent => FlakyToolMetadata,
        LoggingComponent  => NoLogging,
        CacheComponent    => NoCache,
        RetryComponent    => ExponentialBackoffRetry,
    });

    // ================================================================
    // Phase 1 tests: approval + sandbox
    // ================================================================

    #[tokio::test]
    async fn auto_ctx_approves() {
        let ctx = TestAutoCtx;
        let result = ComposableRuntime::run(&ctx, "grep", "search files").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn consumer_trait_executes_directly_on_context() {
        let ctx = TestAutoCtx;
        let result = ctx
            .execute_tool_json("echo", serde_json::json!({"via": "consumer"}))
            .await
            .expect("context capability should execute");

        assert_eq!(
            result
                .get("echoed")
                .and_then(|value| value.get("via"))
                .and_then(|value| value.as_str()),
            Some("consumer")
        );
    }

    #[tokio::test]
    async fn deny_ctx_rejects() {
        let ctx = TestDenyCtx;
        let result = ComposableRuntime::run(&ctx, "exec", "run command").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("operation denied"));
    }

    #[tokio::test]
    async fn sandbox_check_returns_policy() {
        let ctx = TestAutoCtx;
        let sandboxed = ComposableRuntime::run_with_sandbox(&ctx, "file_write", "write file")
            .await
            .expect("should succeed");
        assert!(!sandboxed); // NoSandbox wired
    }

    struct StrictSandbox;

    #[async_trait]
    impl<Ctx: Send + Sync> SandboxProvider<Ctx> for StrictSandbox {
        fn sandbox_enabled(_ctx: &Ctx) -> bool {
            true
        }

        fn workspace_root(_ctx: &Ctx) -> Option<&PathBuf> {
            None
        }
    }

    struct StrictCtx;

    delegate_components!(StrictCtx {
        ApprovalComponent => AutoApproval,
        SandboxComponent  => StrictSandbox,
    });

    #[tokio::test]
    async fn strict_ctx_enables_sandbox() {
        let ctx = StrictCtx;
        let sandboxed = ComposableRuntime::run_with_sandbox(&ctx, "exec", "run cmd")
            .await
            .expect("should succeed");
        assert!(sandboxed);
    }

    // ================================================================
    // Phase 2 tests: ToolFacade — same context projected as Tool
    // ================================================================

    #[tokio::test]
    async fn tool_facade_executes_via_cgp() {
        let facade = ToolFacade::new(TestAutoCtx);
        let result = facade
            .execute(serde_json::json!({"msg": "hello"}))
            .await
            .expect("should succeed");

        assert_eq!(
            result
                .get("echoed")
                .and_then(|v| v.get("msg"))
                .and_then(|v| v.as_str()),
            Some("hello")
        );
    }

    #[tokio::test]
    async fn tool_facade_denied_by_ctx() {
        let facade = ToolFacade::new(TestDenyCtx);
        let result = facade.execute(serde_json::json!({})).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("operation denied"));
    }

    #[tokio::test]
    async fn tool_facade_name_and_description() {
        let facade = ToolFacade::new(NamedToolCtx);
        assert_eq!(facade.name(), "my_tool");
        assert_eq!(facade.description(), "My description");
    }

    #[tokio::test]
    async fn tool_facade_dual_output() {
        let facade = ToolFacade::new(TestAutoCtx);
        let result = facade
            .execute_dual(serde_json::json!({"key": "value"}))
            .await
            .expect("should succeed");

        assert!(result.success);
    }

    // ================================================================
    // Phase 2 tests: HandlerFacade — same context projected as ToolHandler
    // ================================================================

    #[tokio::test]
    async fn handler_facade_executes_via_cgp() {
        let facade = HandlerFacade::new(TestAutoCtx);
        let session: Arc<dyn crate::tools::handlers::tool_handler::ToolSession> = Arc::new(
            crate::tools::handlers::adapter::DefaultToolSession::new(PathBuf::from("/tmp")),
        );
        let turn = Arc::new(crate::tools::handlers::tool_handler::TurnContext {
            cwd: PathBuf::from("/tmp"),
            turn_id: "test".to_string(),
            sub_id: None,
            shell_environment_policy:
                crate::tools::handlers::tool_handler::ShellEnvironmentPolicy::default(),
            approval_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
                crate::tools::handlers::tool_handler::ApprovalPolicy::default(),
            ),
            codex_linux_sandbox_exe: None,
            sandbox_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
                Default::default(),
            ),
        });
        let invocation = ToolInvocation {
            session,
            turn,
            tracker: None,
            call_id: "test-call".to_string(),
            tool_name: "echo".to_string(),
            payload: ToolPayload::Function {
                arguments: r#"{"msg":"handler"}"#.to_string(),
            },
        };

        let output = facade.handle(invocation).await.expect("should succeed");
        assert!(output.is_success());
        let content = output.content().expect("should have content");
        assert!(content.contains("handler"));
    }

    #[tokio::test]
    async fn handler_facade_denied_by_ctx() {
        let facade = HandlerFacade::new(TestDenyCtx);
        let session: Arc<dyn crate::tools::handlers::tool_handler::ToolSession> = Arc::new(
            crate::tools::handlers::adapter::DefaultToolSession::new(PathBuf::from("/tmp")),
        );
        let turn = Arc::new(crate::tools::handlers::tool_handler::TurnContext {
            cwd: PathBuf::from("/tmp"),
            turn_id: "test".to_string(),
            sub_id: None,
            shell_environment_policy:
                crate::tools::handlers::tool_handler::ShellEnvironmentPolicy::default(),
            approval_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
                crate::tools::handlers::tool_handler::ApprovalPolicy::default(),
            ),
            codex_linux_sandbox_exe: None,
            sandbox_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
                Default::default(),
            ),
        });
        let invocation = ToolInvocation {
            session,
            turn,
            tracker: None,
            call_id: "test-call".to_string(),
            tool_name: "exec".to_string(),
            payload: ToolPayload::Function {
                arguments: "{}".to_string(),
            },
        };

        let result = facade.handle(invocation).await;
        assert!(result.is_err());
    }

    // ================================================================
    // Phase 2 tests: same context, two facades — unification proof
    // ================================================================

    #[tokio::test]
    async fn same_context_both_facades() {
        // ToolFacade
        let tool = ToolFacade::new(TestAutoCtx);
        let tool_result = tool
            .execute(serde_json::json!({"via": "tool"}))
            .await
            .expect("tool facade should succeed");
        assert!(tool_result.get("echoed").is_some());

        // HandlerFacade — same wiring, different projection
        let handler = HandlerFacade::new(TestAutoCtx);
        let session: Arc<dyn crate::tools::handlers::tool_handler::ToolSession> = Arc::new(
            crate::tools::handlers::adapter::DefaultToolSession::new(PathBuf::from("/tmp")),
        );
        let turn = Arc::new(crate::tools::handlers::tool_handler::TurnContext {
            cwd: PathBuf::from("/tmp"),
            turn_id: "test".to_string(),
            sub_id: None,
            shell_environment_policy:
                crate::tools::handlers::tool_handler::ShellEnvironmentPolicy::default(),
            approval_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
                crate::tools::handlers::tool_handler::ApprovalPolicy::default(),
            ),
            codex_linux_sandbox_exe: None,
            sandbox_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
                Default::default(),
            ),
        });
        let invocation = ToolInvocation {
            session,
            turn,
            tracker: None,
            call_id: "test-call".to_string(),
            tool_name: "echo".to_string(),
            payload: ToolPayload::Function {
                arguments: r#"{"via":"handler"}"#.to_string(),
            },
        };

        let handler_output = handler
            .handle(invocation)
            .await
            .expect("handler facade should succeed");
        assert!(handler_output.is_success());
        let content = handler_output.content().expect("should have content");
        assert!(content.contains("handler"));
    }

    // ================================================================
    // Phase 6 tests: static logging, cache, and retry providers
    // ================================================================

    #[tokio::test]
    async fn tracing_logging_executes() {
        let facade = ToolFacade::new(TestTracingCtx);
        let result = facade
            .execute(serde_json::json!({"test": true}))
            .await
            .expect("should succeed with tracing logging");

        assert!(result.get("echoed").is_some());
    }

    #[tokio::test]
    async fn cached_results_short_circuit_second_execute() {
        let executions = Arc::new(AtomicUsize::new(0));
        let facade = ToolFacade::new(TestCachingCtx::new(executions.clone()));

        let first = facade
            .execute(serde_json::json!({"query": "same"}))
            .await
            .expect("first execution should succeed");
        let second = facade
            .execute(serde_json::json!({"query": "same"}))
            .await
            .expect("second execution should succeed");

        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn cached_results_short_circuit_dual_output() {
        let executions = Arc::new(AtomicUsize::new(0));
        let facade = ToolFacade::new(TestCachingCtx::new(executions.clone()));

        let first = facade
            .execute_dual(serde_json::json!({"query": "same"}))
            .await
            .expect("first dual execution should succeed");
        let second = facade
            .execute_dual(serde_json::json!({"query": "same"}))
            .await
            .expect("second dual execution should succeed");

        assert_eq!(executions.load(Ordering::SeqCst), 1);
        assert_eq!(first.ui_content, second.ui_content);
        assert_eq!(first.llm_content, second.llm_content);
    }

    #[tokio::test]
    async fn retry_provider_retries_failed_execute() {
        let executions = Arc::new(AtomicUsize::new(0));
        let retry_policy = RetryPolicy {
            max_attempts: 2,
            initial_backoff: Duration::ZERO,
            max_backoff: Duration::ZERO,
        };
        let facade = ToolFacade::new(TestRetryCtx::new(executions.clone(), retry_policy));

        let result = facade
            .execute(serde_json::json!({"retry": true}))
            .await
            .expect("retry should recover the transient failure");

        assert_eq!(executions.load(Ordering::SeqCst), 2);
        assert_eq!(
            result.get("attempt").and_then(|value| value.as_u64()),
            Some(2)
        );
    }

    // ================================================================
    // Phase 3 tests: concrete runtime contexts
    // ================================================================

    #[tokio::test]
    async fn interactive_ctx_enables_sandbox() {
        let ctx = InteractiveCtx::new(PathBuf::from("/workspace"));
        let sandboxed = ComposableRuntime::run_with_sandbox(&ctx, "exec", "run cmd")
            .await
            .expect("should succeed");
        assert!(sandboxed);
    }

    #[tokio::test]
    async fn ci_ctx_auto_approves_with_sandbox() {
        let ctx = CiCtx::new(PathBuf::from("/ci/workspace"));
        let sandboxed = ComposableRuntime::run_with_sandbox(&ctx, "exec", "run cmd")
            .await
            .expect("should succeed");
        assert!(sandboxed);
    }

    #[tokio::test]
    async fn bench_ctx_no_sandbox() {
        let sandboxed = ComposableRuntime::run_with_sandbox(&BenchCtx, "exec", "run cmd")
            .await
            .expect("should succeed");
        assert!(!sandboxed);
    }

    // ================================================================
    // Phase 3 tests: ToolBridgeCtx + PassthroughExecutor
    // ================================================================

    /// A minimal concrete Tool for testing the bridge pattern.
    struct SimpleTool;

    #[async_trait]
    impl Tool for SimpleTool {
        async fn execute(&self, args: Value) -> Result<Value> {
            Ok(serde_json::json!({
                "tool_name": "simple",
                "input": args,
                "result": "ok"
            }))
        }

        fn name(&self) -> &'static str {
            "simple"
        }

        fn description(&self) -> &'static str {
            "A simple test tool"
        }

        fn parameter_schema(&self) -> Option<Value> {
            Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                }
            }))
        }

        fn default_permission(&self) -> ToolPolicy {
            ToolPolicy::Allow
        }

        fn is_mutating(&self) -> bool {
            false
        }

        fn kind(&self) -> &'static str {
            "test"
        }
    }

    #[tokio::test]
    async fn bridge_interactive_passthrough() {
        let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
        let facade = wrap_tool_interactive(tool, PathBuf::from("/workspace"));

        assert_eq!(facade.name(), "simple");
        assert_eq!(facade.description(), "A simple test tool");

        let result = facade
            .execute(serde_json::json!({"query": "test"}))
            .await
            .expect("should succeed");

        assert_eq!(result.get("result").and_then(|v| v.as_str()), Some("ok"));
        assert_eq!(
            result
                .get("input")
                .and_then(|v| v.get("query"))
                .and_then(|v| v.as_str()),
            Some("test")
        );
    }

    #[tokio::test]
    async fn bridge_ci_passthrough() {
        let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
        let facade = wrap_tool_ci(tool, PathBuf::from("/ci"));

        let result = facade
            .execute(serde_json::json!({"key": "value"}))
            .await
            .expect("should succeed");

        assert_eq!(result.get("result").and_then(|v| v.as_str()), Some("ok"));
    }

    #[tokio::test]
    async fn bridge_passthrough_metadata_is_preserved() {
        let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
        let facade = wrap_tool_interactive(tool, PathBuf::from("/workspace"));

        assert!(facade.parameter_schema().is_some());
        assert_eq!(facade.default_permission(), ToolPolicy::Allow);
        assert!(!facade.is_mutating());
        assert_eq!(facade.kind(), "test");
    }

    #[tokio::test]
    async fn native_typed_tool_preserves_metadata_and_execution() {
        let facade = wrap_native_tool_interactive(SimpleTool, PathBuf::from("/workspace"));

        assert_eq!(facade.name(), "simple");
        assert_eq!(facade.description(), "A simple test tool");
        assert!(facade.parameter_schema().is_some());
        assert_eq!(facade.default_permission(), ToolPolicy::Allow);
        assert!(!facade.is_mutating());
        assert_eq!(facade.kind(), "test");

        let result = facade
            .execute(serde_json::json!({"query": "native"}))
            .await
            .expect("should succeed");
        assert_eq!(
            result
                .get("input")
                .and_then(|v| v.get("query"))
                .and_then(|v| v.as_str()),
            Some("native")
        );
    }

    #[tokio::test]
    async fn bridge_dual_output() {
        let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
        let facade = wrap_tool_interactive(tool, PathBuf::from("/workspace"));

        let result = facade
            .execute_dual(serde_json::json!({"x": 1}))
            .await
            .expect("should succeed");

        assert!(result.success);
        assert_eq!(result.tool_name, "simple");
    }

    #[tokio::test]
    async fn bridge_handler_facade() {
        let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
        let bridge_ctx = ToolBridgeCtx {
            inner: tool,
            runtime: InteractiveCtx::new(PathBuf::from("/workspace")),
        };
        let handler = HandlerFacade::new(bridge_ctx);

        let session: Arc<dyn crate::tools::handlers::tool_handler::ToolSession> = Arc::new(
            crate::tools::handlers::adapter::DefaultToolSession::new(PathBuf::from("/tmp")),
        );
        let turn = Arc::new(crate::tools::handlers::tool_handler::TurnContext {
            cwd: PathBuf::from("/tmp"),
            turn_id: "test".to_string(),
            sub_id: None,
            shell_environment_policy:
                crate::tools::handlers::tool_handler::ShellEnvironmentPolicy::default(),
            approval_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
                crate::tools::handlers::tool_handler::ApprovalPolicy::default(),
            ),
            codex_linux_sandbox_exe: None,
            sandbox_policy: crate::tools::handlers::tool_handler::Constrained::allow_any(
                Default::default(),
            ),
        });
        let invocation = ToolInvocation {
            session,
            turn,
            tracker: None,
            call_id: "bridge-test".to_string(),
            tool_name: "simple".to_string(),
            payload: ToolPayload::Function {
                arguments: r#"{"via":"bridge"}"#.to_string(),
            },
        };

        let output = handler.handle(invocation).await.expect("should succeed");
        assert!(output.is_success());
        let content = output.content().expect("should have content");
        assert!(content.contains("bridge"));
    }

    // ================================================================
    // Phase 3 tests: HasComponent delegation through ToolBridgeCtx
    // ================================================================

    #[tokio::test]
    async fn bridge_ctx_delegates_components() {
        let tool: Arc<dyn Tool> = Arc::new(SimpleTool);
        let bridge = ToolBridgeCtx {
            inner: tool,
            runtime: InteractiveCtx::new(PathBuf::from("/workspace")),
        };

        // Verify sandbox is enabled (delegated through InteractiveCtx → WorkspaceSandbox)
        let sandboxed = ComposableRuntime::run_with_sandbox(&bridge, "exec", "test")
            .await
            .expect("should succeed");
        assert!(sandboxed);
    }
}
