# LM Studio Client Quick Reference

Quick API lookup for VT Code's LM Studio integration.

## Module

| Item | Type | Purpose |
|------|------|---------|
| `lmstudio::LMStudioClient` | Struct | Main client for server interaction |

## Common Tasks

### Connect to LM Studio Server

```rust
use vtcode_core::llm::providers::lmstudio::LMStudioClient;

let client = LMStudioClient::try_from_base_url("http://localhost:1234").await?;
// Server is reachable
```

### List Available Models

```rust
let models = client.fetch_models().await?;
for model in models {
    println!("{}", model);
}
```

### Load a Model

Pre-load a model into memory (faster inference):

```rust
client.load_model("openai/gpt-oss-20b").await?;
```

### Download a Model

Download using the `lms` CLI tool:

```rust
client.download_model("openai/gpt-oss-20b").await?;
```

## API Reference

### `LMStudioClient`

```rust
pub async fn try_from_base_url(base_url: &str) -> io::Result<Self>
```
Create a client and verify server is reachable.

```rust
pub async fn fetch_models(&self) -> io::Result<Vec<String>>
```
Get list of available model IDs.

```rust
pub async fn load_model(&self, model: &str) -> io::Result<()>
```
Pre-load model into memory via minimal request.

```rust
pub async fn download_model(&self, model: &str) -> io::Result<()>
```
Download model using `lms` CLI tool.

## Error Handling

### Connection Error
```
LM Studio is not responding. Install from https://lmstudio.ai/download 
and run 'lms server start'.
```

### Model Not Found in Response
```
No 'data' array in response
```

### Missing `lms` CLI Tool
```
LM Studio not found. Please install LM Studio from https://lmstudio.ai/
```

## CLI Tool Discovery

The `lms` command is searched in this order:

1. **PATH** - Standard system PATH
2. **Unix fallback** - `~/.lmstudio/bin/lms`
3. **Windows fallback** - `%USERPROFILE%\.lmstudio\bin\lms.exe`

## Default Server

```
http://localhost:1234
```

## Environment Variables

- `LMSTUDIO_BASE_URL` - Override server URL (optional)

## Endpoints Used

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/models` | GET | List available models |
| `/responses` | POST | Load model (minimal request) |

## Testing

```bash
# All LM Studio tests
cargo test --lib lmstudio

# With output
cargo test --lib lmstudio -- --nocapture
```

## Type Details

```rust
#[derive(Clone)]
pub struct LMStudioClient {
    client: reqwest::Client,
    base_url: String,
}
```

## See Also

- [LM Studio Integration Guide](./lmstudio-codex-integration.md)
- [LM Studio Provider Guide](./providers/lmstudio.md)
- [Ollama Integration](./ollama-codex-integration.md) (similar pattern)
