# vtcode-tools

Prototype extraction of VT Code's modular tool registry.

`vtcode-tools` is a thin wrapper that surfaces VT Code's tool registry, built-in
tools, caching, middleware, and workflow optimization to external consumers. Feature
flags let adopters opt into only the tool categories they need.

## Public entrypoints

- **Registry** — `ToolRegistry`, `ToolRegistration`, `ToolPermissionDecision`
- **Traits** — `Tool`, `ToolExecutor`
- **ACP tools** — `AcpTool`, `AcpDiscoveryTool`, `AcpHealthTool`
- **Cache** — `LruCache`, `CacheObserver`, `CacheStats`, `EvictionReason`
- **Middleware** — `LoggingMiddleware`, `MetricsMiddleware`, `MiddlewareChain`, `ToolRequest`, `ToolResponse`
- **Patterns** — `PatternDetector`, `DetectedPattern`, `ToolEvent`
- **Executor** — `CachedToolExecutor`, `ExecutorStats`
- **Optimizer** — `WorkflowOptimizer`, `Optimization`, `OptimizationType`
- **Commons re-exports** — `ErrorFormatter`, `ErrorReporter`, `PathResolver`, `TelemetrySink`, `WorkspacePaths`
- **Collaboration/utility schemas** — re-exported from `vtcode-collaboration-tool-specs` and `vtcode-utility-tool-specs`

## Usage

```rust
use vtcode_tools::{ToolRegistry, ToolRegistration, LruCache, MiddlewareChain};

// Build a registry and attach middleware
let registry = ToolRegistry::default();
let chain = MiddlewareChain::new();

// Use the LRU cache for repeated tool invocations
let cache: LruCache<String, String> = LruCache::new(128);
```

## Feature Flags

| Flag | Default | Description |
|------|---------|-------------|
| `bash` | yes | PTY-based command execution (`PtyManager`) |
| `search` | yes | Grep/file search (`GrepSearchManager`) |
| `net` | yes | Network-related tool support |
| `policies` | — | `ToolPolicyManager`, `RegistryBuilder`, `RegistryEvent` |
| `examples` | — | Helpers used by headless integration examples |

## API reference

See [docs.rs/vtcode-tools](https://docs.rs/vtcode-tools).

## Related docs

- [Tool extraction policy](../docs/modules/vtcode_tools_policy.md)
