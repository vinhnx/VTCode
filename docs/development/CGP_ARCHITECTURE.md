# Context-Generic Programming (CGP) in VT Code

## Overview

VT Code applies the **Context-Generic Programming** pattern from the
[RustLab 2025 talk](https://contextgeneric.dev/blog/rustlab-2025-coherence/) to
its tool runtime system. The pattern solves a concrete architectural problem:
the same tool implementation needs to behave differently (approval policy,
sandbox enforcement, logging, caching, retry) depending on the execution
context (interactive session, CI pipeline, benchmarks) — without adapter
explosion.

## Core Concepts

### 1. Provider Traits (explicit `Ctx` parameter)

Traditional Rust traits bind behavior to `Self`. CGP moves `Self` to an explicit
generic `Ctx` parameter, allowing multiple overlapping implementations:

```rust
// Traditional — one impl per type
impl ApprovalCheck for MyTool { ... }

// CGP provider — multiple impls for different contexts
impl<Ctx: Send + Sync> ApprovalProvider<Ctx> for AutoApproval { ... }
impl<Ctx: Send + Sync> ApprovalProvider<Ctx> for DenyAllApproval { ... }
```

### 2. Component Names (marker types)

Zero-sized enums act as keys in a type-level lookup table:

```rust
pub enum ApprovalComponent {}
pub enum SandboxComponent {}
pub enum ExecuteComponent {}
pub enum LoggingComponent {}
pub enum CacheComponent {}
pub enum RetryComponent {}
```

### 3. `HasComponent<Name>` (type-level lookup)

Maps component names to provider types for a given context:

```rust
pub trait HasComponent<Name> {
    type Provider;
}
```

### 4. `delegate_components!` (wiring macro)

Generates `HasComponent` impls per context:

```rust
delegate_components!(InteractiveCtx {
    ApprovalComponent => PromptApproval,
    SandboxComponent  => WorkspaceSandbox,
    ExecuteComponent  => PassthroughExecutor,
    LoggingComponent  => TracingLogging,
    CacheComponent    => NoCache,
    RetryComponent    => NoRetry,
});
```

### 5. Consumer Traits (blanket impls)

The RustLab pattern does not stop at provider traits. VT Code restores an
ergonomic API through blanket consumer traits implemented for any context with
the required component wiring:

```rust
#[async_trait]
pub trait CanApproveTool {
    async fn approve_tool(&self, tool_name: &str, description: &str) -> Result<()>;
}

#[async_trait]
pub trait CanExecuteTool {
    async fn execute_tool_json(&self, tool_name: &str, args: Value) -> Result<Value>;
    async fn execute_tool_dual(
        &self,
        tool_name: &str,
        args: Value,
    ) -> Result<SplitToolResult>;
}

pub trait CanProvideToolMetadata {
    fn tool_name(&self) -> &'static str;
    fn tool_description(&self) -> &'static str;
    fn parameter_schema(&self) -> Option<Value>;
    fn default_permission(&self) -> ToolPolicy;
    fn tool_kind(&self) -> &'static str;
}
```

This keeps `ToolFacade`, `HandlerFacade`, and `ComposableRuntime` dependent on
context capabilities instead of raw component lookups. `ToolFacade` also uses
`CanProvideToolMetadata` so direct CGP registrations preserve schema and policy
metadata instead of falling back to `Tool` defaults.

## Architecture

```
┌─────────────────────────────────────────────────┐
│               Existing Tools                     │
│  GrepTool, FileOpsTool, CommandTool, ...         │
│  (Arc<dyn Tool>)                                 │
└──────────────────┬──────────────────────────────┘
                   │
         wrap_tool_interactive()
         wrap_tool_ci()
                   │
┌──────────────────▼──────────────────────────────┐
│            ToolBridgeCtx<Runtime>                 │
│  ┌─────────────┐  ┌───────────────────────────┐ │
│  │ inner: Tool  │  │ runtime: InteractiveCtx   │ │
│  └─────────────┘  │          CiCtx            │ │
│                    │          BenchCtx          │ │
│                    └───────────────────────────┘ │
│  HasComponent delegates to Runtime               │
│  HasInnerTool delegates to inner                 │
│  HasWorkspaceRoot delegates to runtime           │
└──────────────────┬──────────────────────────────┘
                   │
     ┌─────────────┴─────────────┐
     │                           │
┌────▼────┐              ┌───────▼───────┐
│ToolFacade│             │ HandlerFacade  │
│impl Tool │             │impl ToolHandler│
└────┬────┘              └───────┬───────┘
     │                           │
     └─────────┬─────────────────┘
               │
     ToolRegistration::from_cgp_tool()
               │
     ┌─────────▼─────────┐
     │   ToolRegistry     │
     └───────────────────┘
```

## Runtime Contexts

| Context          | Approval         | Sandbox                  | Logging          | Cache     | Retry     |
|------------------|------------------|--------------------------|------------------|-----------|-----------|
| `InteractiveCtx` | `PromptApproval` | `WorkspaceSandbox`       | `TracingLogging` | `NoCache` | `NoRetry` |
| `CiCtx`          | `AutoApproval`   | `StrictWorkspaceSandbox` | `NoLogging`      | `NoCache` | `NoRetry` |
| `BenchCtx`       | `AutoApproval`   | `NoSandbox`              | `NoLogging`      | `NoCache` | `NoRetry` |

## Provider Traits

| Trait                    | Purpose                              |
|--------------------------|--------------------------------------|
| `ApprovalProvider<Ctx>`  | Permission checks before execution   |
| `SandboxProvider<Ctx>`   | Sandbox policy resolution            |
| `ExecuteProvider<Ctx>`   | Core tool execution (single + dual)  |
| `MetadataProvider<Ctx>`  | Name, description, schema, and policy hints |
| `LoggingProvider<Ctx>`   | Execution lifecycle logging          |
| `CacheProvider<Ctx>`     | JSON and dual-result caching         |
| `RetryProvider<Ctx>`     | Retry/backoff policy                 |
| `OutputMapProvider<Ctx>` | Output format conversion             |

## Consumer Traits

| Trait               | Purpose                                      |
|---------------------|----------------------------------------------|
| `CanApproveTool`         | Blanket approval API over `ApprovalProvider`      |
| `CanResolveSandbox`      | Blanket sandbox API over `SandboxProvider`        |
| `CanExecuteTool`         | Full runtime pipeline over execute/log/cache/retry |
| `CanProvideToolMetadata` | Blanket metadata API over `MetadataProvider`      |

## Named Providers

| Provider                  | Implements         | Behavior                    |
|---------------------------|--------------------|-----------------------------|
| `AutoApproval`            | `ApprovalProvider` | Always approves             |
| `DenyAllApproval`         | `ApprovalProvider` | Always denies               |
| `PromptApproval`          | `ApprovalProvider` | Delegates to ToolPolicyGateway |
| `NoSandbox`               | `SandboxProvider`  | No sandbox enforcement      |
| `WorkspaceSandbox`        | `SandboxProvider`  | Workspace-scoped sandbox    |
| `StrictWorkspaceSandbox`  | `SandboxProvider`  | Strict workspace boundaries |
| `PassthroughExecutor`     | `ExecuteProvider`  | Delegates to inner `Tool`   |
| `PassthroughMetadata`     | `MetadataProvider` | Delegates schema/policy/kind to inner `Tool` |
| `TypedToolExecutor<T>`    | `ExecuteProvider`  | Static dispatch to concrete `T: Tool` |
| `TypedToolMetadata<T>`    | `MetadataProvider` | Static metadata projection from concrete `T: Tool` |
| `RegistryFnTool`          | `Tool` wrapper     | Projects `ToolExecutorFn` registrations into native CGP facades |
| `NoLogging`               | `LoggingProvider`  | No-op logging               |
| `TracingLogging`          | `LoggingProvider`  | `tracing` start/success/failure |
| `NoCache`                 | `CacheProvider`    | No cached results           |
| `CachedResults`           | `CacheProvider`    | `UnifiedCache`-backed results |
| `NoRetry`                 | `RetryProvider`    | Fail-fast execution         |
| `ExponentialBackoffRetry` | `RetryProvider`    | Static exponential backoff  |

## Usage

### Wrapping an existing tool

```rust
use vtcode_core::components::wrap_tool_interactive;

let tool: Arc<dyn Tool> = Arc::new(MyTool::new());
let facade = wrap_tool_interactive(tool, workspace_root);

// Register with the tool registry
let reg = ToolRegistration::from_cgp_tool("my_tool", CapabilityLevel::Basic, facade);
registry.register_tool(reg).await?;
```

### Defining a custom executor

```rust
struct MyExecutor;

#[async_trait]
impl<Ctx: Send + Sync> ExecuteProvider<Ctx> for MyExecutor {
    async fn execute(_ctx: &Ctx, args: Value) -> Result<Value> {
        // Custom execution logic
        Ok(json!({"result": "done"}))
    }
}

struct MyCtx;
delegate_components!(MyCtx {
    ApprovalComponent => AutoApproval,
    ExecuteComponent  => MyExecutor,
    MetadataComponent => MyMetadata,
    LoggingComponent  => NoLogging,
    CacheComponent    => NoCache,
    RetryComponent    => NoRetry,
});

let facade = ToolFacade::new(MyCtx);
```

### Wrapping a concrete tool natively

```rust
use vtcode_core::components::wrap_native_tool_interactive;

let facade = wrap_native_tool_interactive(MyTool::new(), workspace_root);
let reg = ToolRegistration::from_cgp_tool("my_tool", CapabilityLevel::Basic, facade);
registry.register_tool(reg).await?;
```

This avoids the `Arc<dyn Tool>` bridge entirely. The tool still implements the
public `Tool` trait, but CGP execution and metadata flow through
`TypedToolExecutor<T>` / `TypedToolMetadata<T>` with static dispatch.

## Application Bootstrap Integration (Phase 5)

The CGP pipeline is automatically enabled during session initialization in
`src/agent/runloop/unified/session_setup/init.rs`. After all builtin tools
are registered and config is applied, `enable_cgp_pipeline()` prefers any
registration-provided native CGP factory and otherwise wraps `TraitObject`
tools through the CGP approval → sandbox → logging/cache/retry pipeline.

Wrapping is idempotent: registrations created via `from_cgp_tool()` /
`register_cgp_tool()` are marked as already CGP-wrapped, and
`enable_cgp_pipeline()` skips them instead of nesting another facade layer.

The active CGP mode now persists in `ToolRegistry`. Any later
`register_tool()` call, including post-startup skill activation, is wrapped
through the same mode automatically. This closes the earlier gap where
dynamic registrations added after startup bypassed the CGP runtime entirely.

Builtin `Tool`-backed registrations such as `request_user_input`,
`enter_plan_mode`, `exit_plan_mode`, `task_tracker`, and
`plan_task_tracker` now attach a native CGP factory. Before CGP activation
they behave exactly like normal tool registrations; once the pipeline is
enabled they switch to a typed CGP facade instead of `PassthroughExecutor`.
The native factory now receives the final `ToolRegistration`, so the wrapped
tool preserves registration metadata such as canonical name, description,
schemas, prompt path, and default permission even when the inner `Tool`
implementation uses placeholder identifiers.

The same metadata preservation now applies to the fallback `TraitObject`
path. When a registration only carries an `Arc<dyn Tool>`, CGP wraps that
tool in a registration-backed metadata shim before entering the bridge
runtime. This keeps dynamic registrations such as MCP proxy tools aligned
with the public registration name and schemas after CGP activation.

Dynamic skill registrations follow the same model:

- session skill control tools (`list_skills`, `load_skill`, `load_skill_resource`)
  now attach native CGP factories when they are registered
- activated traditional skill tools attach a native CGP factory through
  `build_traditional_skill_tool_registration()`
- MCP proxy tool registrations now attach a native CGP factory through
  `build_mcp_registration()`
- if a later registration does not expose a native factory, the registry still
  applies the active CGP mode and falls back to the passthrough bridge

`RegistryFn` registrations now also participate in CGP. The registry bridges a
function-pointer registration into a concrete `RegistryFnTool`, then wraps that
typed tool through the same interactive/CI CGP runtime. That covers the unified
tool family (`unified_search`, `unified_exec`, `unified_file`) plus hidden
registry-function helpers such as `read_file`, `write_file`, and `apply_patch`.

## Native Tool Migration (Phase 7)

Phase 7 starts by replacing the most direct `Arc<dyn Tool>` bridges with
typed CGP contexts:

- `TypedToolCtx<Runtime, T>` stores a concrete tool instance plus runtime policy
- `TypedToolExecutor<T>` calls `T::execute` / `T::execute_dual` without trait-object indirection
- `TypedToolMetadata<T>` projects the same tool's schemas, policy, and hints
- `ToolRegistration` can now carry a native CGP factory so `enable_cgp_pipeline()`
  can pick the native facade when runtime mode becomes known
- native CGP factories are wrapped with registration-backed metadata so dynamic
  registrations keep their public tool identity even when the concrete adapter
  type has a generic internal name
- the trait-object fallback path also applies a registration-backed metadata
  shim before entering `ToolBridgeCtx`, so metadata fidelity no longer depends
  on the inner `Tool` implementation exposing the public registration name
- `native_cgp_tool_factory()` now centralizes the repeated closure wiring for
  concrete tool registrations so builtins, skills, MCP proxies, and session
  setup tools use the same factory shape

This keeps the migration incremental: existing pre-CGP registry behavior stays
unchanged, while runtime activation can progressively stop depending on
`PassthroughExecutor` for selected tools.

## Static Runtime Providers (Phase 6)

Phase 6 replaces the Phase 5 hook-only middleware slot with three explicit CGP
provider traits:

- `LoggingProvider<Ctx>` for tracing and lifecycle events
- `CacheProvider<Ctx>` for `UnifiedCache`-backed JSON and dual-output caching
- `RetryProvider<Ctx>` for static backoff policies around execution
- `MetadataProvider<Ctx>` for schemas, permissions, and tool hints

The production runtime wiring stays conservative for now:

- `InteractiveCtx` enables tracing only
- `CiCtx` and `BenchCtx` keep logging/cache/retry disabled
- cache and retry behavior are proved in dedicated CGP tests before broader rollout

### Runtime Mode Selection

| Condition     | CGP Mode      | Effect                                                       |
|---------------|---------------|--------------------------------------------------------------|
| `full_auto`   | `Ci`          | AutoApproval + StrictSandbox + no logging/cache/retry        |
| Normal TUI    | `Interactive` | PromptApproval + WorkspaceSandbox + tracing logging only     |

### Key APIs

```rust
use vtcode_core::tools::CgpRuntimeMode;

// Wrap all existing tools through CGP pipeline
tool_registry.enable_cgp_pipeline(CgpRuntimeMode::Interactive).await;

// Register a single new tool with CGP wrapping
tool_registry.register_cgp_tool(
    my_tool,
    CapabilityLevel::Basic,
    CgpRuntimeMode::Interactive,
).await?;
```

## File Locations

- `vtcode-core/src/components.rs` — CGP substrate, provider traits, facades, runtime contexts
- `vtcode-core/src/tools/registry/cgp_facade.rs` — Registry integration (enable/register)
- `src/agent/runloop/unified/session_setup/init.rs` — Bootstrap wiring

## Reference

- [CGP RustLab 2025 talk](https://contextgeneric.dev/blog/rustlab-2025-coherence/)
- [CGP crate](https://crates.io/crates/cgp) (not used as dependency — patterns applied manually)
