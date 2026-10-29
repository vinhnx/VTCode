use super::*;

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

    fn should_retry(_ctx: &Ctx, _tool_name: &str, _attempt: u32, error: &Error) -> bool {
        vtcode_commons::detect_misconfiguration_in_anyhow(error).is_none()
    }

    fn backoff_duration(ctx: &Ctx, _tool_name: &str, attempt: u32) -> Duration {
        let policy = ctx.retry_policy();
        // The CGP pipeline uses 1-indexed attempts; delay_for_attempt is 0-indexed.
        policy.delay_for_attempt(attempt.saturating_sub(1))
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

/// Recursively hash a `serde_json::Value` tree without allocating a temporary
/// String.  Type-discriminant bytes prevent cross-type collisions (e.g. the
/// string `"true"` vs the boolean `true`).
fn hash_json_value<H: Hasher>(hasher: &mut H, value: &Value) {
    match value {
        Value::Null => hasher.write_u8(0x00),
        Value::Bool(b) => {
            hasher.write_u8(0x01);
            hasher.write_u8(u8::from(*b));
        }
        Value::Number(n) => {
            hasher.write_u8(0x02);
            let s = n.to_string();
            hasher.write(s.as_bytes());
        }
        Value::String(s) => {
            hasher.write_u8(0x03);
            hasher.write(s.as_bytes());
        }
        Value::Array(arr) => {
            hasher.write_u8(0x04);
            hasher.write_usize(arr.len());
            for item in arr {
                hash_json_value(hasher, item);
            }
        }
        Value::Object(map) => {
            hasher.write_u8(0x05);
            hasher.write_usize(map.len());
            // Sort keys for deterministic hashing regardless of insertion order.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                hasher.write(k.as_bytes());
                #[allow(
                    clippy::expect_used,
                    reason = "Intentional compatibility, platform, or test-only suppression."
                )]
                let value = map.get(k).expect("key from map.keys() must exist in map");
                hash_json_value(hasher, value);
            }
        }
    }
}

fn build_execution_cache_key(tool_name: &str, args: &Value) -> ToolExecutionCacheKey {
    let mut hasher = crate::core::agent::hash_utils::StableHasher::new();
    hasher.write(tool_name.as_bytes());
    hash_json_value(&mut hasher, args);

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
        ctx.json_cache().get_owned(&build_execution_cache_key(tool_name, args))
    }

    fn put_json(ctx: &Ctx, tool_name: &str, args: &Value, result: &Value) {
        let key = build_execution_cache_key(tool_name, args);
        let size = estimate_json_size(result);
        ctx.json_cache().insert(key, result.clone(), size);
    }

    fn get_dual(ctx: &Ctx, tool_name: &str, args: &Value) -> Option<SplitToolResult> {
        ctx.dual_cache().get_owned(&build_execution_cache_key(tool_name, args))
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
    fn tool_name(ctx: &Ctx) -> &str {
        ctx.tool().name()
    }

    fn tool_description(ctx: &Ctx) -> &str {
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

pub(super) fn value_to_text(value: &Value) -> String {
    if value.is_string() {
        value.as_str().unwrap_or("").to_string()
    } else {
        json_to_string_pretty(value)
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
        <ComponentProvider<Ctx, CacheComponent> as CacheProvider<Ctx>>::get_json(ctx, tool_name, args)
    }

    fn put_cached(ctx: &Ctx, tool_name: &str, args: &Value, result: &Self::Output) {
        <ComponentProvider<Ctx, CacheComponent> as CacheProvider<Ctx>>::put_json(ctx, tool_name, args, result);
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
        <ComponentProvider<Ctx, CacheComponent> as CacheProvider<Ctx>>::get_dual(ctx, tool_name, args)
    }

    fn put_cached(ctx: &Ctx, tool_name: &str, args: &Value, result: &Self::Output) {
        <ComponentProvider<Ctx, CacheComponent> as CacheProvider<Ctx>>::put_dual(ctx, tool_name, args, result);
    }

    async fn execute(ctx: &Ctx, args: Value) -> Result<Self::Output> {
        <ComponentProvider<Ctx, ExecuteComponent> as ExecuteProvider<Ctx>>::execute_dual(ctx, args).await
    }
}

fn retry_backoff<Ctx>(ctx: &Ctx, tool_name: &str, attempt: u32) -> Duration
where
    Ctx: HasComponent<RetryComponent> + Send + Sync,
    ComponentProvider<Ctx, RetryComponent>: RetryProvider<Ctx>,
{
    <ComponentProvider<Ctx, RetryComponent> as RetryProvider<Ctx>>::backoff_duration(ctx, tool_name, attempt)
        .min(MAX_RETRY_BACKOFF)
}

async fn execute_tool_with_mode<Ctx, Mode>(ctx: &Ctx, tool_name: &str, args: Value) -> Result<Mode::Output>
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
    <ComponentProvider<Ctx, LoggingComponent> as LoggingProvider<Ctx>>::on_start(ctx, tool_name, &args);

    if let Some(result) = Mode::get_cached(ctx, tool_name, &args) {
        <ComponentProvider<Ctx, LoggingComponent> as LoggingProvider<Ctx>>::on_cache_hit(ctx, tool_name, &args);
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
        <ComponentProvider<Ctx, RetryComponent> as RetryProvider<Ctx>>::max_attempts(ctx, tool_name, &args)
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
    fn tool_name(&self) -> &str;

    fn tool_description(&self) -> &str;

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
    fn tool_name(&self) -> &str {
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::tool_name(self)
    }

    fn tool_description(&self) -> &str {
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
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::default_permission(self)
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
        <ComponentProvider<Ctx, MetadataComponent> as MetadataProvider<Ctx>>::resource_hints(self, args)
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

    fn name(&self) -> &str {
        CanProvideToolMetadata::tool_name(&self.ctx)
    }

    fn description(&self) -> &str {
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
