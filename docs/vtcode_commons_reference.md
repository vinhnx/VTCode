# vtcode-commons Reference Implementations

The `vtcode-commons` crate exposes foundational traits that the extracted
crates (`vtcode-llm`, `vtcode-tools`) share for filesystem paths, telemetry, and
error handling. This guide provides ready-to-use implementations for downstream
projects that want to integrate quickly without designing their own adapters
from scratch.

## Static workspace paths

Use [`StaticWorkspacePaths`](../vtcode-commons/src/reference.rs) when you want to
wire concrete directories into the shared traits without additional indirection:

```rust
use std::path::PathBuf;
use vtcode_commons::StaticWorkspacePaths;

let workspace_root = PathBuf::from("/opt/vtcode-demo");
let config_dir = workspace_root.join("config");
let paths = StaticWorkspacePaths::new(workspace_root, config_dir)
    .with_cache_dir("/opt/vtcode-demo/cache")
    .with_telemetry_dir("/var/log/vtcode-demo");

assert_eq!(paths.config_dir(), PathBuf::from("/opt/vtcode-demo/config"));
```

These helpers satisfy the `WorkspacePaths` trait so crates like `vtcode-llm` can
resolve prompt caches or configuration files relative to your application’s
layout.

## Memory-backed telemetry

[`MemoryTelemetry`](../vtcode-commons/src/reference.rs) records cloneable events
for later inspection, making it ideal for tests and examples:

```rust
use vtcode_commons::{MemoryTelemetry, TelemetrySink};

let telemetry = MemoryTelemetry::new();
telemetry.record(&"event".to_string())?;
assert_eq!(telemetry.take(), vec!["event".to_string()]);
```

Downstream code can call `take()` to drain the buffered events and assert on the
contents without configuring a dedicated telemetry pipeline.

## Memory-backed error reporting

[`MemoryErrorReporter`](../vtcode-commons/src/reference.rs) captures formatted
error messages in memory. Pair it with the provided
[`DisplayErrorFormatter`](../vtcode-commons/src/errors.rs) to surface errors in a
human-readable format:

```rust
use anyhow::Error;
use vtcode_commons::{DisplayErrorFormatter, MemoryErrorReporter};

let formatter = DisplayErrorFormatter;
let reporter = MemoryErrorReporter::new();
let error = Error::msg("provider failed");

reporter.capture(&error)?;
let messages = reporter.take();
assert!(messages[0].contains(&formatter.format_error(&error)));
```

## When to build custom adapters

These reference implementations are intentionally lightweight and best suited
for prototypes, tests, or small integrations. Larger deployments should
implement the traits directly so they can:

- Route telemetry events into tracing, OpenTelemetry, or custom dashboards.
- Forward captured errors into production incident response tooling.
- Resolve workspace paths from configuration files or operating system
  conventions instead of static paths.

Use the reference types as scaffolding, then replace them with adapters tailored
to your infrastructure as adoption matures.
