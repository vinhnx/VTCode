# OpenAI Codex Ollama Integration Applied to VT Code

## Summary

Applied design patterns from OpenAI's Codex `codex-ollama` module to enhance VT Code's Ollama provider with better error handling, progress reporting, streaming, and model management.

**Reference**: https://github.com/openai/codex/tree/main/codex-rs/ollama

## What Was Applied

### 1. **Model Pull Progress Reporting** (`ollama/pull.rs`)

Added a robust progress reporting trait and implementations:

- **`OllamaPullEvent`**: Enum representing events during model pulls
  - `Status(String)` - Human-readable status messages
  - `ChunkProgress { digest, total, completed }` - Byte-level progress for layers
  - `Success` - Pull completed successfully
  - `Error(String)` - Error occurred during pull

- **`OllamaPullProgressReporter`**: Trait for progress handling
  - `CliPullProgressReporter` - CLI output with speed, percentage, and GB counters
  - `TuiPullProgressReporter` - TUI reporter (delegates to CLI for now)

**Usage pattern**:
```rust
let mut reporter = CliPullProgressReporter::new();
while let Some(event) = stream.next().await {
    reporter.on_event(&event)?;
}
```

### 2. **Improved Error Messages** (`fetch_ollama_models`)

**Before**:
```
Failed to fetch Ollama models: connection refused
```

**After** (from Codex pattern):
```
No running Ollama server detected. Start it with: `ollama serve` (after installing)
Install instructions: https://github.com/ollama/ollama?tab=readme-ov-file
```

**Changes**:
- Added connection timeout (5s) to fail faster on unreachable servers
- Helpful error context with setup instructions
- Logs debug info when connection fails

### 3. **Event Stream Parser** (`ollama/parser.rs`)

Parser for Ollama's JSON-lines streaming protocol:

- **`pull_events_from_value()`** - Converts Ollama API JSON responses to structured events
- Handles status messages, progress updates, and error messages
- Composable for building higher-level abstractions

**Example**:
```json
{"status":"verifying"}
{"digest":"sha256:abc","total":1000000000,"completed":500000000}
{"status":"success"}
```

### 4. **URL Utilities** (`ollama/url.rs`)

Helper functions for URL handling:

- **`is_openai_compatible_base_url()`** - Detect OpenAI-compatible endpoints (.../v1)
- **`base_url_to_host_root()`** - Extract host from base URL (e.g., `http://localhost:11434/v1` → `http://localhost:11434`)

**Example**:
```rust
assert!(is_openai_compatible_base_url("http://localhost:11434/v1"));
assert_eq!(base_url_to_host_root("http://localhost:11434/v1"), "http://localhost:11434");
```

### 5. **High-Level Client** (`ollama/client.rs`)

Complete Ollama client with server interaction:

- **`OllamaClient`** - Main client for server operations
  - `try_from_base_url()` - Create client and verify server is reachable
  - `fetch_models()` - Get list of available models
  - `pull_model_stream()` - Stream-based model pulling with events

**Usage**:
```rust
let client = OllamaClient::try_from_base_url("http://localhost:11434").await?;
let models = client.fetch_models().await?;
let mut stream = client.pull_model_stream("llama2:latest").await?;
while let Some(event) = stream.next().await {
    reporter.on_event(&event)?;
}
```

## Files Created

1. **`vtcode-core/src/llm/providers/ollama/pull.rs`**
   - Progress reporting trait and implementations

2. **`vtcode-core/src/llm/providers/ollama/parser.rs`**
   - JSON-lines event parser for Ollama streaming protocol

3. **`vtcode-core/src/llm/providers/ollama/url.rs`**
   - URL parsing utilities for base URL handling

4. **`vtcode-core/src/llm/providers/ollama/client.rs`**
   - High-level Ollama client for server interaction

## Files Modified

1. **`vtcode-core/src/llm/providers/ollama.rs`**
   - Added module declarations: `pull`, `parser`, `url`, `client`
   - Exported public types and functions
   - Improved `fetch_ollama_models()` error handling with timeout

## Design Patterns from Codex

### Trait-Based Progress Reporting
Codex uses a trait to decouple reporters from transport layers:
```rust
pub trait PullProgressReporter {
    fn on_event(&mut self, event: &PullEvent) -> io::Result<()>;
}
```

This allows multiple implementations (CLI, TUI, logging, etc.) without coupling.

### Stream Event Model
Rather than callbacks, Codex emits structured events:
- Easy to test
- Composable (can filter/transform events)
- Clear separation of concerns

### Helpful Error Messages
Codex provides actionable guidance in error messages:
- What went wrong
- How to fix it
- Links to documentation

## Usage Examples

### Example 1: Check Ollama Server Health

```rust
use vtcode_core::llm::providers::ollama::OllamaClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Try to connect to local Ollama
    match OllamaClient::try_from_base_url("http://localhost:11434").await {
        Ok(_) => println!("Ollama server is running"),
        Err(e) => println!("Error: {}", e),
    }
    Ok(())
}
```

### Example 2: List Available Models

```rust
use vtcode_core::llm::providers::ollama::OllamaClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = OllamaClient::try_from_base_url("http://localhost:11434").await?;
    let models = client.fetch_models().await?;
    
    for model in models {
        println!("- {}", model);
    }
    Ok(())
}
```

### Example 3: Pull Model with Progress Reporting

```rust
use vtcode_core::llm::providers::ollama::{OllamaClient, CliPullProgressReporter, OllamaPullProgressReporter};
use futures::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = OllamaClient::try_from_base_url("http://localhost:11434").await?;
    let mut stream = client.pull_model_stream("llama2:latest").await?;
    
    let mut reporter = CliPullProgressReporter::new();
    
    while let Some(event) = stream.next().await {
        reporter.on_event(&event)?;
    }
    
    println!("Model pull complete!");
    Ok(())
}
```

## Architecture

```
OllamaClient (entrypoint)
├── try_from_base_url() → server health check
├── fetch_models() → GET /api/tags
└── pull_model_stream() → POST /api/pull (streaming)
    ├── Raw bytes stream
    ├── parser::pull_events_from_value()
    │   └── OllamaPullEvent
    └── Integration with OllamaPullProgressReporter
        ├── CliPullProgressReporter
        └── TuiPullProgressReporter
```

## Testing

Run unit tests for the Ollama submodules:

```bash
# All Ollama tests
cargo test --lib ollama

# Specific module tests
cargo test --lib ollama::parser
cargo test --lib ollama::url
```

## Future Enhancements

1. **Integrate pull progress into CLI/TUI**: Currently, the reporters are available but not wired into the main VT Code flow
2. **OpenAI-compatible mode auto-detection**: Enhance `OllamaClient` to automatically detect and use correct endpoints
3. **Automatic model selection**: When a model is missing, prompt to pull it with progress reporting
4. **Download resumption**: Handle partial downloads and resumption gracefully
5. **Model caching**: Track model manifests to reduce API calls
6. **TUI integration**: Dedicated TUI progress reporter for better visual feedback

## Reference Implementation Details

### From Codex's `OllamaClient::pull_model_stream()`
- Handles JSON-lines streaming from `/api/pull`
- Parses multiple events per chunk
- Emits progress events for UI rendering
- Returns `Box<Stream<Item = PullEvent>>`

### From Codex's `CliProgressReporter::on_event()`
- Tracks totals per digest to handle parallel layer downloads
- Calculates speed (MB/s) and percentage completion
- Uses ANSI escape codes for line clearing (`\x1b[2K`)
- Silences noisy "pulling manifest" messages

### From Codex's URL handling
- Detects OpenAI-compatible endpoints (URL ending in `/v1`)
- Extracts host root for native API calls
- Handles trailing slashes correctly

## Integration Points for Future Work

To fully utilize this in VT Code:

1. **Add pull support to model selection UI** - Show progress when downloading missing models
2. **Wire `OllamaClient` into tool system** - Allow tools to manage model lifecycle
3. **Add `--auto-pull` flag** - Automatically download models if missing
4. **Persistent download state** - Track what's been downloaded across sessions
5. **Health check on startup** - Warn if Ollama is unavailable

## References

- **OpenAI Codex Ollama module**: https://github.com/openai/codex/tree/main/codex-rs/ollama
- **Ollama API docs**: https://github.com/ollama/ollama/blob/main/docs/api.md
- **VT Code Ollama provider**: `vtcode-core/src/llm/providers/ollama.rs`
- **Ollama Official**: https://ollama.com
