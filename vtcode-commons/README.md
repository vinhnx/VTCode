# vtcode-commons

Shared traits for paths, telemetry, and error reporting reused across VT Code
component extractions.

Keeps thin downstream crates like `vtcode-llm` and `vtcode-tools` decoupled
from VT Code's internal configuration and telemetry wiring while sharing
common contracts.

## Modules

| Module | Purpose |
|---|---|
| `ansi`, `ansi_capabilities`, `ansi_codes` | ANSI escape helpers and terminal capability detection |
| `async_utils` | Async convenience wrappers |
| `colors`, `color_policy`, `color256_theme` | Color utilities and theming |
| `diff`, `diff_paths`, `diff_preview`, `diff_theme` | Unified diff rendering |
| `errors`, `error_category` | Error formatting, reporting, and retry classification |
| `formatting` | Text formatting helpers |
| `fs` | Filesystem utilities |
| `http` | HTTP client helpers |
| `image` | Image processing utilities |
| `llm` | Core LLM types (`BackendKind`, `LLMError`, `LLMResponse`, `Usage`) |
| `paths` | Path resolution traits and helpers |
| `sanitizer` | Input sanitization |
| `serde_helpers` | Custom serde (de)serializers |
| `telemetry` | Telemetry sink trait |
| `tokens` | Token estimation and truncation |
| `unicode` | Unicode width monitoring |
| `styling`, `preview`, `ui_protocol` | Rendering and UI protocol types |

## Public entrypoints

### Traits

- `ErrorFormatter` / `ErrorReporter` – format and report errors
- `PathResolver` / `PathScope` / `WorkspacePaths` – workspace-aware path resolution
- `TelemetrySink` – telemetry event sink

### Reference implementations

- `DisplayErrorFormatter`, `NoopErrorReporter`, `MemoryErrorReporter`
- `NoopTelemetry`, `MemoryTelemetry`
- `StaticWorkspacePaths`

### LLM types

- `BackendKind` – provider enum (OpenAI, Anthropic, Gemini, …)
- `Usage` – token usage tracking

### Error classification

- `ErrorCategory`, `BackoffStrategy`, `Retryability`

## Usage

```rust
use vtcode_commons::{PathResolver, ErrorReporter, TelemetrySink};

// Implement the traits for your application, or use the reference
// implementations for testing:
use vtcode_commons::{StaticWorkspacePaths, NoopTelemetry, NoopErrorReporter};
```

## Related docs

- [`docs/modules/vtcode_commons_reference.md`](../docs/modules/vtcode_commons_reference.md)

## API reference

<https://docs.rs/vtcode-commons>
