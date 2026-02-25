# Ollama Module Quick Reference

Quick lookup for the Ollama provider submodules added from OpenAI Codex.

## Modules

| Module | Purpose | Key Types |
|--------|---------|-----------|
| `ollama::client` | Server interaction | `OllamaClient` |
| `ollama::pull` | Progress reporting | `OllamaPullEvent`, `OllamaPullProgressReporter` |
| `ollama::parser` | Event parsing | `pull_events_from_value()` |
| `ollama::url` | URL utilities | `is_openai_compatible_base_url()`, `base_url_to_host_root()` |

## Common Tasks

### Connect to Ollama Server

```rust
use vtcode_core::llm::providers::ollama::OllamaClient;

let client = OllamaClient::try_from_base_url("http://localhost:11434").await?;
```

### List Models

```rust
let models = client.fetch_models().await?;
for model in models {
    println!("{}", model);
}
```

### Pull a Model

```rust
use futures::StreamExt;
use vtcode_core::llm::providers::ollama::CliPullProgressReporter;

let mut stream = client.pull_model_stream("llama2").await?;
let mut reporter = CliPullProgressReporter::new();

while let Some(event) = stream.next().await {
    reporter.on_event(&event)?;
}
```

### Parse Ollama API Response

```rust
use vtcode_core::llm::providers::ollama::pull_events_from_value;
use serde_json::json;

let response = json!({"status": "pulling", "digest": "sha256:abc", "total": 1000, "completed": 500});
let events = pull_events_from_value(&response);
// events contains OllamaPullEvent variants
```

### Detect URL Type

```rust
use vtcode_core::llm::providers::ollama::{is_openai_compatible_base_url, base_url_to_host_root};

assert!(is_openai_compatible_base_url("http://localhost:11434/v1"));
let host = base_url_to_host_root("http://localhost:11434/v1");
assert_eq!(host, "http://localhost:11434");
```

## Type Hierarchy

```
OllamaPullEvent (enum)
├── Status(String)
├── ChunkProgress { digest, total, completed }
├── Success
└── Error(String)

OllamaPullProgressReporter (trait)
├── CliPullProgressReporter
└── TuiPullProgressReporter
```

## API Methods

### `OllamaClient`

```rust
pub async fn try_from_base_url(base_url: &str) -> io::Result<Self>
pub async fn fetch_models(&self) -> io::Result<Vec<String>>
pub async fn pull_model_stream(&self, model: &str) -> io::Result<BoxStream<'static, OllamaPullEvent>>
```

### `OllamaPullProgressReporter`

```rust
pub trait OllamaPullProgressReporter {
    fn on_event(&mut self, event: &OllamaPullEvent) -> io::Result<()>;
}
```

## Error Handling

Connection errors include helpful messages:

```
No running Ollama server detected. Start it with: `ollama serve` (after installing)
Install instructions: https://github.com/ollama/ollama?tab=readme-ov-file
```

## Testing

```bash
# Test URL parsing
cargo test --lib ollama::url

# Test event parsing
cargo test --lib ollama::parser

# All Ollama tests
cargo test --lib ollama
```

## Files Location

- `vtcode-core/src/llm/providers/ollama/client.rs`
- `vtcode-core/src/llm/providers/ollama/pull.rs`
- `vtcode-core/src/llm/providers/ollama/parser.rs`
- `vtcode-core/src/llm/providers/ollama/url.rs`

## Source

Adapted from https://github.com/openai/codex/tree/main/codex-rs/ollama
