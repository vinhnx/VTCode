# Golden Path Architecture

This document describes the unified tool execution architecture implemented to improve efficiency, scalability, and maintainability of the VT Code agent.

## Overview

The Golden Path architecture consolidates multiple overlapping tool execution frameworks into a single, well-defined execution path with consistent safety enforcement, error handling, and state management.

## Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                        Agent Runloop                              │
│  ┌─────────────────┐  ┌──────────────────┐  ┌────────────────┐  │
│  │ RunLoopContext  │→ │  tool_pipeline   │→ │  tool_routing  │  │
│  └─────────────────┘  └──────────────────┘  └────────────────┘  │
└───────────────────────────────┬─────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Golden Path Layer                             │
│  ┌─────────────────────────────────────────────────────────────┐ │
│  │                   golden_path.rs                             │ │
│  │  create_invocation() │ create_execution_context()            │ │
│  │  check_safety()      │ execute_via_golden_path()             │ │
│  └─────────────────────────────────────────────────────────────┘ │
│                                │                                  │
│  ┌──────────────┐  ┌──────────────────┐  ┌───────────────────┐  │
│  │ invocation   │  │ unified_executor │  │  safety_gateway   │  │
│  │    .rs       │  │       .rs        │  │       .rs         │  │
│  │              │  │                  │  │                   │  │
│  │ • Invocation │  │ • TrustLevel     │  │ • SafetyDecision  │  │
│  │   Id (UUID)  │  │ • ApprovalState  │  │ • Rate Limiting   │  │
│  │ • ToolInvo-  │  │ • PolicyConfig   │  │ • Plan Mode       │  │
│  │   cation     │  │ • Unified        │  │ • Command Policy  │  │
│  │ • Builder    │  │   ToolExecutor   │  │ • Risk Scoring    │  │
│  └──────────────┘  └──────────────────┘  └───────────────────┘  │
│                                │                                  │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │               parallel_tool_batch.rs                      │   │
│  │  • ParallelToolBatch for concurrent read-only execution   │   │
│  │  • is_parallel_safe() pattern detection                   │   │
│  │  • Semaphore-based concurrency limiting                   │   │
│  └──────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                                │
                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                        Tool Core                                  │
│  ┌──────────────┐  ┌────────────────┐  ┌─────────────────────┐  │
│  │ ToolRegistry │  │ CircuitBreaker │  │ unified_error.rs    │  │
│  │              │  │                │  │                     │  │
│  │ • execute_   │  │ • Per-tool     │  │ • UnifiedToolError  │  │
│  │   tool()     │  │   tracking     │  │ • UnifiedErrorKind  │  │
│  │ • available_ │  │ • Exponential  │  │ • ErrorSeverity     │  │
│  │   tools()    │  │   backoff      │  │ • is_retryable()    │  │
│  └──────────────┘  └────────────────┘  └─────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## Key Components

### 1. ToolInvocationId (`invocation.rs`)

UUID-based unique identifier for correlating tool executions across:
- Logs and telemetry
- Retry attempts  
- Nested subagent calls
- State tracking

```rust
let invocation = ToolInvocation::new("read_file", args, "session-123");
println!("Invocation: {}", invocation.id.short()); // "a1b2c3d4"
```

### 2. UnifiedToolExecutor (`unified_executor.rs`)

Single trait interface for all tool execution:

```rust
#[async_trait]
pub trait UnifiedToolExecutor: Send + Sync {
    async fn execute(
        &self,
        ctx: ToolExecutionContext,
        name: &str,
        args: Value,
    ) -> Result<Value, UnifiedToolError>;
    
    fn has_tool(&self, name: &str) -> bool;
    fn available_tools(&self) -> Vec<String>;
    async fn preflight(&self, ctx: &ToolExecutionContext, name: &str) -> bool;
}
```

Key types:
- `TrustLevel`: Untrusted → Standard → Elevated → Full
- `ApprovalState`: Pending → PreApproved/NeedsApproval/Approved/Denied
- `PolicyConfig`: Sandbox, allow/deny patterns, timeouts

### 3. SafetyGateway (`safety_gateway.rs`)

Unified safety enforcement:

```rust
let gateway = SafetyGateway::new(config);
let decision = gateway.check_safety(&ctx, "shell", &args).await;

match decision {
    SafetyDecision::Allow => { /* proceed */ }
    SafetyDecision::Deny(reason) => { /* block with reason */ }
    SafetyDecision::NeedsApproval(justification) => { /* prompt user */ }
}
```

Consolidates:
- Rate limiting (per-second, per-minute, per-turn, per-session)
- Plan mode restrictions
- Command policy (allow/deny lists)
- Risk scoring
- Destructive tool detection

### 4. ParallelToolBatch (`parallel_tool_batch.rs`)

Concurrent execution for read-only tools:

```rust
let mut batch = ParallelToolBatch::new(8); // max 8 concurrent

batch.add_call("read_file", args1, ctx1);
batch.add_call("grep_file", args2, ctx2);
batch.add_call("list_files", args3, ctx3);

let results = batch.execute_batch(&executor).await;
```

Auto-detects parallel-safe tools using prefixes: `read_`, `list_`, `get_`, `grep_`, `search_`, `find_`

### 5. UnifiedToolError (`unified_error.rs`)

Consistent error classification:

```rust
pub enum UnifiedErrorKind {
    Timeout,           // Retryable
    Network,           // Retryable
    RateLimit,         // Retryable
    ArgumentValidation, // LLM mistake (don't count in circuit breaker)
    PermissionDenied,  // Needs approval
    SandboxFailure,    // Permanent
    // ...
}
```

### 6. Declaration Caching (`declarations.rs`)

Avoids per-turn rebuilds:

```rust
// Old (rebuilds every call):
let decls = build_function_declarations_with_mode(mode);

// New (cached, O(1) after first call):
let decls = build_function_declarations_cached(mode);
```

## Integration

### Runloop Integration (`golden_path.rs`)

```rust
use crate::agent::runloop::unified::golden_path;

// Create invocation with unique ID
let invocation = golden_path::create_invocation(ctx, "read_file", &args);

// Check safety before execution
let decision = golden_path::check_safety(ctx, "read_file", &args).await;

// Execute via unified path
let result = golden_path::execute_via_golden_path(ctx, "read_file", &args).await;
```

## Benefits

1. **Consistency**: Single execution path eliminates policy drift between runloop and tool core
2. **Observability**: ToolInvocationId enables end-to-end tracing
3. **Safety**: Unified gateway ensures all checks happen in correct order
4. **Performance**: Declaration caching + parallel execution reduce latency
5. **Maintainability**: Clear separation of concerns with well-defined interfaces

## Migration Path

The architecture supports incremental adoption:

1. New code uses golden path APIs directly
2. Existing code continues working via adapters
3. Gradual migration of hot paths to unified executor

## Golden Path Functions

The `execute_golden_path` function is the single canonical entry point that consolidates all execution paths:

```rust
use vtcode_core::tools::{
    execute_golden_path, execute_golden_path_simple, execute_batch_golden_path,
    ExecutionBuilder, GoldenPathConfig, TrustLevel, ToolExecutionContext,
};

// Simple execution with default config
let result = execute_golden_path_simple(registry, "read_file", args, "session-123").await?;

// Full execution with custom context and config
let ctx = ToolExecutionContext::new("session-123");
let config = GoldenPathConfig::default();
let result = execute_golden_path(registry, "read_file", args, &ctx, &config).await?;

// With builder pattern for custom settings
let result = ExecutionBuilder::new(registry, "shell_command")
    .args(args)
    .elevated()
    .timeout(Duration::from_secs(60))
    .execute()
    .await?;

// Batch execution (parallel for read-only tools)
let results = execute_batch_golden_path(registry, vec![
    ("read_file", args1),
    ("grep_file", args2),
    ("write_file", args3),  // Runs sequentially
], &ctx, &config).await;
```

### GoldenPathResult

Execution result with full correlation metadata:

```rust
pub struct GoldenPathResult {
    pub invocation_id: ToolInvocationId,  // UUID for tracing
    pub value: Value,                      // Tool output
    pub duration: Duration,                // Execution time
    pub was_cached: bool,                  // Cache hit
    pub approval_state: ApprovalState,     // What approval was needed
    pub trust_level: TrustLevel,           // Trust used for execution
}
```

## Files

| File | Purpose |
|------|---------|
| `vtcode-core/src/tools/golden_path_orchestrator.rs` | **GoldenPathOrchestrator** - Consolidated entry point |
| `vtcode-core/src/tools/invocation.rs` | ToolInvocationId, ToolInvocation |
| `vtcode-core/src/tools/unified_executor.rs` | UnifiedToolExecutor trait, contexts |
| `vtcode-core/src/tools/safety_gateway.rs` | SafetyDecision, SafetyGateway |
| `vtcode-core/src/tools/parallel_tool_batch.rs` | ParallelToolBatch |
| `vtcode-core/src/tools/unified_error.rs` | UnifiedToolError, classification |
| `vtcode-core/src/tools/registry/declarations.rs` | build_function_declarations_cached |
| `src/agent/runloop/unified/golden_path.rs` | Runloop integration layer |
