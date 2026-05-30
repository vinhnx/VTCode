# vtcode-tools

[Root AGENTS.md](../AGENTS.md) | Thin facade over `vtcode-core::tools` with observability (middleware, cache, patterns).

## Modules

`registry` re-exports | `middleware` async chain | `cache` LRU | `patterns` sequence analysis | `executor` CachedToolExecutor | `optimizer` suggestions | `acp_tool` ACP tools

## Rules

- Feature flags: `bash`, `search`, `policies`, `examples`.
- Tool schemas belong in `vtcode-utility-tool-specs` / `vtcode-collaboration-tool-specs`, not here.
- `PatternDetector`: `MAX_EVENTS=500`, `ANALYZE_INTERVAL=10` — benchmark before changing.
- `CachedToolExecutor` stats use cache-line-padded `AtomicU64` — don't break lock-free path.

## Gotchas

- `planner` feature is commented out — don't re-enable without discussion.
- Tool name constants go in `vtcode_core`, re-exported here.
