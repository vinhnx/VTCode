# Reference Implementations for `vtcode-commons`

This guide documents the ready-to-use adapters packaged inside the
`vtcode-commons` crate. Each implementation is designed to help
consumers of the extracted crates adopt the shared traits without
depending on VTCode's binary or storage defaults.

## Workspace Paths

`StaticWorkspacePaths` offers a straightforward implementation of the
[`WorkspacePaths`](../vtcode-commons/src/paths.rs) trait. Callers
provide concrete directories for the workspace root and configuration
data, with optional cache and telemetry paths.

```rust
use std::path::{Path, PathBuf};

use vtcode_commons::{StaticWorkspacePaths, WorkspacePaths};

let paths = StaticWorkspacePaths::new("/projects/demo", "/projects/demo/config")
    .with_cache_dir("/projects/demo/cache")
    .with_telemetry_dir("/projects/demo/telemetry");

assert_eq!(paths.workspace_root(), Path::new("/projects/demo"));
assert_eq!(paths.config_dir(), PathBuf::from("/projects/demo/config"));
assert_eq!(paths.cache_dir(), Some(PathBuf::from("/projects/demo/cache")));
assert_eq!(
    paths.telemetry_dir(),
    Some(PathBuf::from("/projects/demo/telemetry"))
);
```

Because the adapter stores concrete `PathBuf` instances, it is ideal for
embedding the extracted crates into existing applications or tests where
paths are already known.

## Telemetry

`MemoryTelemetry<Event>` implements [`TelemetrySink`](../vtcode-commons/src/telemetry.rs)
by collecting cloned event payloads in memory. The `take` method drains
and returns the recorded events, making it useful for assertions in
tests or examples.

```rust
use vtcode_commons::MemoryTelemetry;

let telemetry = MemoryTelemetry::new();
telemetry.record(&"event-1").unwrap();
telemetry.record(&"event-2").unwrap();

let events = telemetry.take();
assert_eq!(events, vec!["event-1", "event-2"]);
```

When no telemetry output is required, consumers can rely on the
`NoopTelemetry` type exported from the crate.

## Error Reporting

`MemoryErrorReporter` implements [`ErrorReporter`](../vtcode-commons/src/errors.rs)
by storing formatted error messages in memory. Use it to verify that
components surface recoverable errors as expected during tests.

```rust
use anyhow::Error;
use vtcode_commons::MemoryErrorReporter;

let reporter = MemoryErrorReporter::new();
reporter.capture(&Error::msg("failure"))?;

let messages = reporter.take();
assert_eq!(messages.len(), 1);
assert!(messages[0].contains("failure"));
# Ok::<_, anyhow::Error>(())
```

For production scenarios, implement the `ErrorReporter` trait to forward
to logging, paging, or other monitoring systems. If error capture is not
needed, the crate also exports `NoopErrorReporter`.

## Putting It Together

The types above are designed to work together. A minimal headless
integration could look like this:

```rust
use vtcode_commons::{MemoryErrorReporter, MemoryTelemetry, StaticWorkspacePaths};

let paths = StaticWorkspacePaths::new("/workspace", "/workspace/config");
let telemetry: MemoryTelemetry<String> = MemoryTelemetry::new();
let errors = MemoryErrorReporter::new();

// Pass the adapters into vtcode-tools or vtcode-llm builders.
```

These adapters give downstream users a sensible starting point while
still encouraging custom implementations tailored to their own
observability stacks and filesystem layouts.
