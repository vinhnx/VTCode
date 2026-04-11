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

use crate::cache::{CacheKey, UnifiedCache};
use crate::tool_policy::ToolPolicy;
use crate::tools::handlers::tool_handler::{
    ToolCallError, ToolHandler, ToolInvocation, ToolKind, ToolOutput, ToolPayload,
};
use crate::tools::result::ToolResult as SplitToolResult;
use crate::tools::traits::Tool;

const MAX_RETRY_ATTEMPTS: u32 = 16;
const MAX_RETRY_BACKOFF: Duration = Duration::from_secs(30);

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

/// The elaborated provider/dictionary selected by `Ctx` for component `Name`.
pub type ComponentProvider<Ctx, Name> = <Ctx as HasComponent<Name>>::Provider;

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
    let encoded_args = serde_json::to_string(args).unwrap_or_else(|_| args.to_string());
    hasher.write(encoded_args.as_bytes());

    let mut cache_key = String::with_capacity(tool_name.len() + 22);
    cache_key.push_str(tool_name);
    cache_key.push_str("::");
    let _ = write!(&mut cache_key, "{}", hasher.finish());

    ToolExecutionCacheKey(cache_key)
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

impl<Ctx: HasToolRef> MetadataProvider<Ctx> for PassthroughMetadata {
    fn tool_name(ctx: &Ctx) -> &'static str {
        ctx.tool().name()
    }

    fn tool_description(ctx: &Ctx) -> &'static str {
        ctx.tool().description()
    }

    fn parameter_schema(ctx: &Ctx) -> Option<Value> {
        ctx.tool().parameter_schema()
    }

    fn config_schema(ctx: &Ctx) -> Option<Value> {
        ctx.tool().config_schema()
    }

    fn state_schema(ctx: &Ctx) -> Option<Value> {
        ctx.tool().state_schema()
    }

    fn prompt_path(ctx: &Ctx) -> Option<Cow<'static, str>> {
        ctx.tool().prompt_path()
    }

    fn default_permission(ctx: &Ctx) -> ToolPolicy {
        ctx.tool().default_permission()
    }

    fn allow_patterns(ctx: &Ctx) -> Option<&'static [&'static str]> {
        ctx.tool().allow_patterns()
    }

    fn deny_patterns(ctx: &Ctx) -> Option<&'static [&'static str]> {
        ctx.tool().deny_patterns()
    }

    fn is_mutating(ctx: &Ctx) -> bool {
        ctx.tool().is_mutating()
    }

    fn is_parallel_safe(ctx: &Ctx) -> bool {
        ctx.tool().is_parallel_safe()
    }

    fn tool_kind(ctx: &Ctx) -> &'static str {
        ctx.tool().kind()
    }

    fn resource_hints(ctx: &Ctx, args: &Value) -> Vec<String> {
        ctx.tool().resource_hints(args)
    }

    fn execution_cost(ctx: &Ctx) -> u8 {
        ctx.tool().execution_cost()
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
trait ExecutionMode<Ctx>
where
    Ctx: HasComponent<ExecuteComponent> + HasComponent<CacheComponent> + Send + Sync,
    ComponentProvider<Ctx, ExecuteComponent>: ExecuteProvider<Ctx>,
    ComponentProvider<Ctx, CacheComponent>: CacheProvider<Ctx>,
{
    type Output: Send;

    fn get_cached(ctx: &Ctx, tool_name: &str, args: &Value) -> Option<Self::Output>;

    fn put_cached(ctx: &Ctx, tool_name: &str, args: &Value, result: &Self::Output);

    async fn execute(ctx: &Ctx, args: Value) -> Result<Self::Output>;
}

struct JsonExecution;

#[async_trait]
impl<Ctx> ExecutionMode<Ctx> for JsonExecution
where
    Ctx: HasComponent<ExecuteComponent> + HasComponent<CacheComponent> + Send + Sync,
    ComponentProvider<Ctx, ExecuteComponent>: ExecuteProvider<Ctx>,
    ComponentProvider<Ctx, CacheComponent>: CacheProvider<Ctx>,
{
    type Output = Value;

    fn get_cached(ctx: &Ctx, tool_name: &str, args: &Value) -> Option<Self::Output> {
        <ComponentProvider<Ctx, CacheComponent> as CacheProvider<Ctx>>::get_json(
            ctx, tool_name, args,
        )
    }

    fn put_cached(ctx: &Ctx, tool_name: &str, args: &Value, result: &Self::Output) {
        <ComponentProvider<Ctx, CacheComponent> as CacheProvider<Ctx>>::put_json(
            ctx, tool_name, args, result,
        );
    }

    async fn execute(ctx: &Ctx, args: Value) -> Result<Self::Output> {
        <ComponentProvider<Ctx, ExecuteComponent> as ExecuteProvider<Ctx>>::execute(ctx, args).await
    }
}

struct DualExecution;

#[async_trait]
impl<Ctx> ExecutionMode<Ctx> for DualExecution
where
    Ctx: HasComponent<ExecuteComponent> + HasComponent<CacheComponent> + Send + Sync,
    ComponentProvider<Ctx, ExecuteComponent>: ExecuteProvider<Ctx>,
    ComponentProvider<Ctx, CacheComponent>: CacheProvider<Ctx>,
{
    type Output = SplitToolResult;

    fn get_cached(ctx: &Ctx, tool_name: &str, args: &Value) -> Option<Self::Output> {
        <ComponentProvider<Ctx, CacheComponent> as CacheProvider<Ctx>>::get_dual(
            ctx, tool_name, args,
        )
    }

    fn put_cached(ctx: &Ctx, tool_name: &str, args: &Value, result: &Self::Output) {
        <ComponentProvider<Ctx, CacheComponent> as CacheProvider<Ctx>>::put_dual(
            ctx, tool_name, args, result,
        );
    }

    async fn execute(ctx: &Ctx, args: Value) -> Result<Self::Output> {
        <ComponentProvider<Ctx, ExecuteComponent> as ExecuteProvider<Ctx>>::execute_dual(ctx, args)
            .await
    }
}

fn retry_backoff<Ctx>(ctx: &Ctx, tool_name: &str, attempt: u32) -> Duration
where
    Ctx: HasComponent<RetryComponent> + Send + Sync,
    ComponentProvider<Ctx, RetryComponent>: RetryProvider<Ctx>,
{
    <ComponentProvider<Ctx, RetryComponent> as RetryProvider<Ctx>>::backoff_duration(
        ctx, tool_name, attempt,
    )
    .min(MAX_RETRY_BACKOFF)
}

async fn execute_tool_with_mode<Ctx, Mode>(
    ctx: &Ctx,
    tool_name: &str,
    args: Value,
) -> Result<Mode::Output>
where
    Ctx: HasComponent<ExecuteComponent>
        + HasComponent<LoggingComponent>
        + HasComponent<CacheComponent>
        + HasComponent<RetryComponent>
        + Send
        + Sync,
    ComponentProvider<Ctx, ExecuteComponent>: ExecuteProvider<Ctx>,
    ComponentProvider<Ctx, LoggingComponent>: LoggingProvider<Ctx>,
    ComponentProvider<Ctx, CacheComponent>: CacheProvider<Ctx>,
    ComponentProvider<Ctx, RetryComponent>: RetryProvider<Ctx>,
    Mode: ExecutionMode<Ctx>,
{
    <ComponentProvider<Ctx, LoggingComponent> as LoggingProvider<Ctx>>::on_start(
        ctx, tool_name, &args,
    );

    if let Some(result) = Mode::get_cached(ctx, tool_name, &args) {
        <ComponentProvider<Ctx, LoggingComponent> as LoggingProvider<Ctx>>::on_cache_hit(
            ctx, tool_name, &args,
        );
        <ComponentProvider<Ctx, LoggingComponent> as LoggingProvider<Ctx>>::on_success(
            ctx,
            tool_name,
            Duration::ZERO,
            1,
            true,
        );
        return Ok(result);
    }

    let started = Instant::now();
    let max_attempts =
        <ComponentProvider<Ctx, RetryComponent> as RetryProvider<Ctx>>::max_attempts(
            ctx, tool_name, &args,
        )
        .clamp(1, MAX_RETRY_ATTEMPTS);

    let mut attempt = 1;
    loop {
        match Mode::execute(ctx, args.clone()).await {
            Ok(result) => {
                Mode::put_cached(ctx, tool_name, &args, &result);
                <ComponentProvider<Ctx, LoggingComponent> as LoggingProvider<Ctx>>::on_success(
                    ctx,
                    tool_name,
                    started.elapsed(),
                    attempt,
                    false,
                );
                return Ok(result);
            }
            Err(error) => {
                let should_retry = attempt < max_attempts
                    && <ComponentProvider<Ctx, RetryComponent> as RetryProvider<Ctx>>::should_retry(
                        ctx, tool_name, attempt, &error,
                    );

                if !should_retry {
                    <ComponentProvider<Ctx, LoggingComponent> as LoggingProvider<Ctx>>::on_failure(
                        ctx,
                        tool_name,
                        started.elapsed(),
                        attempt,
                        &error,
                    );
                    return Err(error);
                }

                let backoff = retry_backoff(ctx, tool_name, attempt);
                <ComponentProvider<Ctx, LoggingComponent> as LoggingProvider<Ctx>>::on_retry(
                    ctx,
                    tool_name,
                    attempt + 1,
                    backoff,
                    &error,
                );
                if !backoff.is_zero() {
                    tokio::time::sleep(backoff).await;
                }
                attempt += 1;
            }
        }
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
    ComponentProvider<Ctx, ApprovalComponent>: ApprovalProvider<Ctx>,
{
    async fn approve_tool(&self, tool_name: &str, description: &str) -> Result<()> {
        <ComponentProvider<Ctx, ApprovalComponent> as ApprovalProvider<Ctx>>::check_approval(
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
    ComponentProvider<Ctx, SandboxComponent>: SandboxProvider<Ctx>,
{
    fn sandbox_enabled(&self) -> bool {
        <ComponentProvider<Ctx, SandboxComponent> as SandboxProvider<Ctx>>::sandbox_enabled(self)
    }

    fn workspace_root(&self) -> Option<&PathBuf> {
        <ComponentProvider<Ctx, SandboxComponent> as SandboxProvider<Ctx>>::workspace_root(self)
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
    ComponentProvider<Ctx, MetadataComponent>: MetadataProvider<Ctx>,
{
    fn tool_name(&self) -> &'static str {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::tool_name(self)
    }

    fn tool_description(&self) -> &'static str {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::tool_description(self)
    }

    fn parameter_schema(&self) -> Option<Value> {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::parameter_schema(self)
    }

    fn config_schema(&self) -> Option<Value> {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::config_schema(self)
    }

    fn state_schema(&self) -> Option<Value> {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::state_schema(self)
    }

    fn prompt_path(&self) -> Option<Cow<'static, str>> {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::prompt_path(self)
    }

    fn default_permission(&self) -> ToolPolicy {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::default_permission(
            self,
        )
    }

    fn allow_patterns(&self) -> Option<&'static [&'static str]> {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::allow_patterns(self)
    }

    fn deny_patterns(&self) -> Option<&'static [&'static str]> {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::deny_patterns(self)
    }

    fn is_mutating(&self) -> bool {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::is_mutating(self)
    }

    fn is_parallel_safe(&self) -> bool {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::is_parallel_safe(self)
    }

    fn tool_kind(&self) -> &'static str {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::tool_kind(self)
    }

    fn resource_hints(&self, args: &Value) -> Vec<String> {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::resource_hints(
            self, args,
        )
    }

    fn execution_cost(&self) -> u8 {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::execution_cost(self)
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
    ComponentProvider<Ctx, ExecuteComponent>: ExecuteProvider<Ctx>,
    ComponentProvider<Ctx, LoggingComponent>: LoggingProvider<Ctx>,
    ComponentProvider<Ctx, CacheComponent>: CacheProvider<Ctx>,
    ComponentProvider<Ctx, RetryComponent>: RetryProvider<Ctx>,
{
    async fn execute_tool_json(&self, tool_name: &str, args: Value) -> Result<Value> {
        execute_tool_with_mode::<Ctx, JsonExecution>(self, tool_name, args).await
    }

    async fn execute_tool_dual(&self, tool_name: &str, args: Value) -> Result<SplitToolResult> {
        execute_tool_with_mode::<Ctx, DualExecution>(self, tool_name, args).await
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

/// Trait for contexts that carry an inner `Tool` reference for passthrough.
pub trait HasInnerTool: Send + Sync {
    fn inner_tool(&self) -> &Arc<dyn Tool>;
}

impl<Ctx> HasToolRef for Ctx
where
    Ctx: HasInnerTool + Send + Sync,
{
    fn tool(&self) -> &dyn Tool {
        self.inner_tool().as_ref()
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
    fn tool_name(ctx: &Ctx) -> &'static str {
        <PassthroughMetadata as MetadataProvider<Ctx>>::tool_name(ctx)
    }

    fn tool_description(ctx: &Ctx) -> &'static str {
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

#[cfg(test)]
mod tests;
