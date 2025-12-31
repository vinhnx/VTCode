# OpenAI Codex LM Studio Integration Applied to VT Code

## Summary

Applied design patterns from OpenAI's Codex `codex-rs/lmstudio` module to enhance VT Code's LM Studio provider with better error handling, model management, and server health checking.

**Reference**: https://github.com/openai/codex/tree/main/codex-rs/lmstudio

## Key Differences from Ollama

| Aspect | Ollama | LM Studio |
|--------|--------|-----------|
| **API** | Native Ollama `/api/*` | OpenAI-compatible `/models`, `/responses` |
| **Model Download** | Streaming `/api/pull` | CLI tool `lms get --yes` |
| **Model Loading** | Implicit on demand | Explicit via minimal `/responses` request |
| **Installation** | `ollama serve` | `lms server start` |
| **Setup** | Online-first | Desktop app with CLI |

## What Was Applied

### 1. **High-Level Client** (`lmstudio/client.rs`)

`LMStudioClient` provides a unified interface for server interaction:

```rust
pub struct LMStudioClient { ... }

impl LMStudioClient {
    pub async fn try_from_base_url(base_url: &str) -> io::Result<Self>
    pub async fn fetch_models(&self) -> io::Result<Vec<String>>
    pub async fn load_model(&self, model: &str) -> io::Result<()>
    pub async fn download_model(&self, model: &str) -> io::Result<()>
}
```

**Features**:
- Server health checking with 5-second timeout
- Model listing via `/models` endpoint
- Model loading via minimal `/responses` request
- Model downloading using `lms` CLI tool
- Cross-platform CLI tool location (PATH + fallback paths)

### 2. **Improved Error Handling** (`fetch_lmstudio_models`)

**Enhanced error messages**:
```
LM Studio is not responding. Install from https://lmstudio.ai/download 
and run 'lms server start'.
```

**Changes**:
- Added connection timeout (5s) to fail fast on unreachable servers
- Helpful error context with setup instructions
- Debug logging for troubleshooting
- Actionable guidance for specific HTTP errors

### 3. **CLI Tool Discovery**

Intelligent discovery of `lms` command:

```rust
// First checks PATH (standard)
// Then checks platform-specific installation paths:
// - Unix: ~/.lmstudio/bin/lms
// - Windows: %USERPROFILE%\.lmstudio\bin\lms.exe
```

## Files Created

1. **`vtcode-core/src/llm/providers/lmstudio/client.rs`** (220 lines)
   - `LMStudioClient` with full server interaction
   - Model fetching, loading, and downloading
   - CLI tool discovery logic
   - Comprehensive unit tests

## Files Modified

1. **`vtcode-core/src/llm/providers/lmstudio.rs`**
   - Added `pub mod client`
   - Exported `LMStudioClient`
   - Improved `fetch_lmstudio_models()` error handling with timeout

## Usage Examples

### Example 1: Connect to LM Studio

```rust
use vtcode_core::llm::providers::lmstudio::LMStudioClient;

let client = LMStudioClient::try_from_base_url("http://localhost:1234").await?;
println!("Connected to LM Studio");
```

### Example 2: List Available Models

```rust
let models = client.fetch_models().await?;
for model in models {
    println!("- {}", model);
}
```

### Example 3: Download and Load Model

```rust
let model = "openai/gpt-oss-20b";

// Download if missing
client.download_model(model).await?;

// Load into memory
client.load_model(model).await?;
println!("Model ready!");
```

## Architecture

```
LMStudioClient (entrypoint)
├── try_from_base_url() → server health check
├── fetch_models() → GET /models
├── load_model() → POST /responses (minimal)
└── download_model() → exec `lms get --yes`
    ├── find_lms() → PATH + fallback locations
    └── std::process::Command execution
```

## Design Patterns

### 1. **Server Health Checking**
Connection timeout prevents hanging on unavailable servers.

### 2. **CLI Tool Discovery**
Searches multiple locations for `lms` command, supporting different installation patterns.

### 3. **Helpful Error Messages**
Provides actionable guidance for common setup issues.

### 4. **Cloneable Client**
`#[derive(Clone)]` allows sharing across async tasks.

## Testing

Unit tests cover:
- Model fetching (happy path, missing data array, server errors)
- Server health checks (success and error)
- CLI tool discovery (with mock home directories)
- Model loading requests

```bash
cargo test --lib lmstudio
```

## Integration Points for Future Work

1. **Auto-download on missing model** - Check before use
2. **Health check on startup** - Warn if LM Studio unavailable
3. **Background model loading** - Pre-load into memory
4. **Progress tracking** - Monitor `lms` download progress
5. **Configuration** - Add `[lmstudio]` to `vtcode.toml`

## Code Quality

✅ **Compilation**: cargo c -p vtcode-core - CLEAN  
✅ **Clippy**: cargo clippy -p vtcode-core - CLEAN  
✅ **Tests**: All passing  
✅ **Architecture**: Follows Codex patterns + VT Code standards  

## References

- **Codex Source**: https://github.com/openai/codex/tree/main/codex-rs/lmstudio
- **LM Studio**: https://lmstudio.ai
- **OpenAI OSS Models**: https://github.com/openai/gpt-oss
- **VT Code LM Studio Provider**: `vtcode-core/src/llm/providers/lmstudio.rs`
