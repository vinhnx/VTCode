# vtcode-lmstudio

LM Studio OSS provider integration for VT Code.

This crate provides a clean, modular integration with [LM Studio](https://lmstudio.ai) for running local open-source language models. It handles model discovery, downloading, and loading operations with a simple async API.

## Features

- **Model Discovery**: Query available models from a running LM Studio server
- **Auto-Download**: Automatically download missing models using the `lms` CLI
- **Model Preloading**: Load models in the background to reduce inference latency
- **Error Handling**: Graceful handling of server unavailability with contextual error messages
- **Cross-Platform**: Works on macOS, Linux, and Windows with fallback PATH detection for the `lms` binary

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
vtcode-lmstudio = { path = "../vtcode-lmstudio" }
vtcode-config = { path = "../vtcode-config" }
```

## Quick Start

```rust
use vtcode_config::ConfigManager;
use vtcode_lmstudio::ensure_oss_ready;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manager = ConfigManager::load_from_workspace(".")?;
    let config = manager.config();
    
    // Ensure LM Studio is ready (check connectivity, download models if needed)
    ensure_oss_ready(config).await?;
    
    Ok(())
}
```

## Usage

### Basic Client Creation

```rust
use vtcode_config::ConfigManager;
use vtcode_lmstudio::LMStudioClient;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let manager = ConfigManager::load_from_workspace(".")?;
    let config = manager.config();
    
    // Create client from configuration (requires LMSTUDIO_URL env var or defaults to localhost:1234)
    let client = LMStudioClient::try_from_provider(config).await?;
    
    // Fetch available models
    let models = client.fetch_models().await?;
    println!("Available models: {:?}", models);
    
    Ok(())
}
```

### Loading and Downloading Models

```rust
use vtcode_lmstudio::LMStudioClient;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let client = LMStudioClient::try_from_provider(&config).await?;
    
    // Check if model exists
    let models = client.fetch_models().await?;
    let model_name = "openai/gpt-oss-20b";
    
    if !models.iter().any(|m| m == model_name) {
        // Download if missing
        client.download_model(model_name).await?;
    }
    
    // Load into memory
    client.load_model(model_name).await?;
    
    Ok(())
}
```

## Configuration

The crate uses the following configuration sources (in order of precedence):

1. **Environment Variable**: `LMSTUDIO_URL` (e.g., `http://localhost:1234`)
2. **Default**: `http://localhost:1234`

Example `.env`:

```bash
LMSTUDIO_URL=http://localhost:1234
```

Or in `vtcode.toml` (future enhancement):

```toml
[lmstudio]
base_url = "http://localhost:1234"
```

## Architecture

### Components

- **`LMStudioClient`**: HTTP client for communicating with the LM Studio server
  - `try_from_provider()`: Create from VTCodeConfig
  - `fetch_models()`: Get list of available models
  - `load_model()`: Preload a model into VRAM
  - `download_model()`: Download a model using the `lms` CLI

- **`ensure_oss_ready()`**: High-level convenience function that:
  1. Determines the model to use (from config or default)
  2. Verifies server connectivity
  3. Downloads missing models
  4. Preloads the model asynchronously

### Error Handling

The crate uses `std::io::Result<T>` for all operations to maintain compatibility with system-level concerns:

```rust
// Connection errors
LMStudioClient::try_from_provider(&config).await?
// -> io::Error with helpful messages

// Model operations
client.fetch_models().await?
// -> io::Error with status codes or JSON parse errors

// Non-critical failures
ensure_oss_ready(&config).await
// -> Logs warnings but doesn't fail for transient issues
```

## Testing

Run unit tests:

```bash
cargo test --package vtcode-lmstudio
```

Tests include:

- Model fetching (happy path, error handling, JSON validation)
- Server connectivity checks
- LMS binary discovery (PATH and fallback locations)
- Base URL configuration

Mock server testing with [wiremock](https://docs.rs/wiremock/).

## Design Notes

This integration follows the architectural pattern established in [OpenAI's Codex](https://github.com/openai/codex/tree/main/codex-rs/lmstudio):

- **Minimal dependencies**: Only requires `reqwest`, `serde_json`, and `tokio`
- **Modular**: Can be used independently of VT Code's core
- **Async-first**: All I/O operations are async
- **Provider-agnostic**: Works with any OpenAI-compatible API endpoint

## References

- [LM Studio Official](https://lmstudio.ai)
- [LM Studio CLI Docs](https://lmstudio.ai/docs/app/cli)
- [OpenAI Codex LM Studio Integration](https://github.com/openai/codex/tree/main/codex-rs/lmstudio)
