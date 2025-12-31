# LM Studio Integration for VT Code

## Overview

VT Code now includes native support for [LM Studio](https://lmstudio.ai), an open-source desktop application for running large language models locally. This integration is based on the architecture proven in [OpenAI's Codex](https://github.com/openai/codex/tree/main/codex-rs/lmstudio).

**Key capabilities:**

- Run local OSS models without cloud dependencies
- Automatic model discovery and downloading
- Background model preloading for faster inference
- Cross-platform support (macOS, Linux, Windows)

## Quick Start

### 1. Install LM Studio

Visit [https://lmstudio.ai](https://lmstudio.ai) and download the appropriate version for your OS.

### 2. Start the Local Server

In LM Studio:
1. Select a model (e.g., "gpt-oss-20b")
2. Click "Start Server"
3. Note the default URL: `http://localhost:1234`

### 3. Configure VT Code

Set the provider in your config:

```toml
# vtcode.toml
[agent]
provider = "lmstudio"
default_model = "openai/gpt-oss-20b"
```

Or via environment variable:

```bash
export VTCODE_PROVIDER=lmstudio
export LMSTUDIO_URL=http://localhost:1234
```

### 4. Use It

```rust
use vtcode_config::ConfigManager;
use vtcode_lmstudio::ensure_oss_ready;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manager = ConfigManager::load_from_workspace(".")?;
    let config = manager.config();
    
    // Automatically ensure LM Studio is ready
    ensure_oss_ready(config).await?;
    
    // Model is now preloaded and ready for inference
    Ok(())
}
```

## Architecture

### Module Structure

```
vtcode-lmstudio/
├── src/
│   ├── lib.rs          # High-level API (ensure_oss_ready, constants)
│   └── client.rs       # Low-level HTTP client (LMStudioClient)
├── Cargo.toml
└── README.md
```

### Component Interactions

```
┌─────────────────────────────────┐
│     ensure_oss_ready()          │  High-level orchestration
│  (Public API entry point)       │
└────────────┬────────────────────┘
             │
             ▼
┌─────────────────────────────────┐
│    LMStudioClient               │  HTTP communication
│  - try_from_provider()          │  with LM Studio server
│  - fetch_models()               │
│  - download_model()             │
│  - load_model()                 │
└─────────────────────────────────┘
             │
             ▼
┌─────────────────────────────────┐
│   LM Studio Server              │  Running locally
│   (OpenAI-compatible API)       │  on http://localhost:1234
└─────────────────────────────────┘
```

## API Reference

### Public Functions

#### `ensure_oss_ready(config: &VTCodeConfig) -> io::Result<()>`

High-level convenience function that:

1. Determines which model to use (from config or DEFAULT_OSS_MODEL)
2. Creates a client and verifies server connectivity
3. Checks if the model exists locally
4. Downloads the model if missing (using `lms get`)
5. Loads the model into VRAM asynchronously

**Non-fatal behavior**: Transient failures (e.g., server unreachable, model list fetch fails) are logged as warnings but don't cause the function to fail, allowing higher layers to handle errors.

**Example:**

```rust
ensure_oss_ready(&config).await?;
// Model is now ready for inference
```

### Public Types

#### `LMStudioClient`

HTTP client for communicating with LM Studio servers.

**Methods:**

- `async fn try_from_provider(config: &VTCodeConfig) -> io::Result<Self>`
  - Create client from configuration
  - Verifies server is reachable before returning
  - Uses `LMSTUDIO_URL` env var or defaults to `http://localhost:1234`

- `async fn fetch_models(&self) -> io::Result<Vec<String>>`
  - Get list of available models from server
  - Returns model IDs (e.g., `["openai/gpt-oss-20b"]`)

- `async fn load_model(&model: &str) -> io::Result<()>`
  - Preload a model into VRAM
  - Sends minimal inference request to trigger loading
  - Useful for reducing latency on first use

- `async fn download_model(&model: &str) -> io::Result<()>`
  - Download a model using the `lms` CLI tool
  - Automatically finds `lms` in PATH or fallback locations
  - Prints progress to stderr

**Platform-specific `lms` discovery:**

- **macOS/Linux**: Checks PATH, then `~/.lmstudio/bin/lms`
- **Windows**: Checks PATH, then `%USERPROFILE%\.lmstudio\bin\lms.exe`

### Constants

```rust
pub const DEFAULT_OSS_MODEL: &str = "openai/gpt-oss-20b";
```

Used when provider is `lmstudio` and `default_model` isn't explicitly set in config.

## Configuration

### Environment Variables

| Variable | Purpose | Default |
|----------|---------|---------|
| `LMSTUDIO_URL` | LM Studio server endpoint | `http://localhost:1234` |
| `VTCODE_PROVIDER` | Active LLM provider | (varies by config) |

### TOML Configuration

**Future enhancement** (planned):

```toml
[lmstudio]
base_url = "http://localhost:1234"  # Server endpoint
```

Currently use environment variables instead.

## Error Handling

All client operations return `std::io::Result<T>` for system-level error compatibility:

### Connection Errors

```rust
// Server not reachable
-> io::Error with message: "LM Studio is not responding. Install from https://lmstudio.ai/download and run 'lms server start'."
```

### Model Errors

```rust
// Model not found in list
-> Downloaded via `lms get` CLI

// JSON parse error
-> io::Error(InvalidData, "JSON parse error: ...")

// Model download failed
-> io::Error with exit code from `lms` command
```

### LMS Binary Errors

```rust
// `lms` not in PATH or fallback location
-> io::Error(NotFound, "LM Studio not found. Please install LM Studio from https://lmstudio.ai/")
```

## Testing

Run tests with:

```bash
cargo test --package vtcode-lmstudio
```

**Test Coverage:**

- ✓ Model fetching (happy path, error handling)
- ✓ Server connectivity checks (200 OK, 404/500 errors)
- ✓ JSON response parsing
- ✓ LMS binary discovery (PATH and fallback locations)
- ✓ Base URL configuration

Uses [wiremock](https://docs.rs/wiremock/) for mock HTTP server testing.

## Performance Considerations

### Model Preloading

The `ensure_oss_ready()` function spawns a background task to load the model:

```rust
tokio::spawn({
    let client = lmstudio_client.clone();
    async move {
        if let Err(e) = client.load_model(&model).await {
            tracing::warn!("Failed to load model {}: {}", model, e);
        }
    }
});
```

**Benefits:**
- Doesn't block the initialization flow
- Reduces latency on first inference
- Failures are logged but non-fatal

### Network Considerations

- **Connection timeout**: 5 seconds (configurable via reqwest Client builder)
- **Server reachability**: Checked once per client creation
- **Model list**: Cached locally once fetched

## Troubleshooting

### "LM Studio is not responding"

**Cause**: LM Studio server isn't running on the configured URL.

**Solution**:
1. Start LM Studio desktop application
2. Click "Start Server" 
3. Verify URL matches `LMSTUDIO_URL` env var (default: `http://localhost:1234`)

### "LM Studio not found"

**Cause**: `lms` CLI tool not in PATH.

**Solution**:
1. Install LM Studio from https://lmstudio.ai
2. Add `~/.lmstudio/bin` to PATH, or
3. Reinstall and ensure "Add to PATH" is checked

### Model Download Hangs

**Cause**: Large model or slow network.

**Solution**:
1. Check internet connection
2. Download model manually in LM Studio UI
3. Or use `lms get <model-name>` in terminal separately

## Design Rationale

### Why Modular?

The `vtcode-lmstudio` crate is a standalone module that:

- Can be used independently of VT Code
- Doesn't pull in vtcode-core dependencies
- Follows Rust packaging best practices
- Mirrors OpenAI's Codex architecture

### Why `std::io::Result`?

Using `std::io::Result<T>` instead of `anyhow::Result<T>`:

- **Compatibility**: System-level error reporting
- **Simplicity**: No external error types needed
- **Clarity**: Signals "I/O operation" to callers
- **Precedent**: Matches OpenAI's Codex pattern

### Why Async-First?

- **Non-blocking**: Large model downloads don't freeze the app
- **Scalability**: Can manage multiple concurrent connections
- **Integration**: Works seamlessly with tokio runtime

## References

- **LM Studio**: https://lmstudio.ai
- **LMS CLI**: https://lmstudio.ai/docs/app/cli
- **OpenAI Codex**: https://github.com/openai/codex/tree/main/codex-rs/lmstudio
- **OpenAI PR #2312**: "LM Studio OSS Support" (implementation reference)

## Future Enhancements

Potential improvements (not yet implemented):

- [ ] TOML configuration support (move from env vars)
- [ ] Model metadata caching (avoid repeated API calls)
- [ ] Concurrent model downloads
- [ ] Resource monitoring (VRAM usage)
- [ ] Web UI integration (display models, manage downloads)
- [ ] Streaming responses support
