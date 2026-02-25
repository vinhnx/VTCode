# Ollama Integration Documentation Index

Comprehensive guide to the OpenAI Codex Ollama integration in VT Code.

## Quick Links

| Document | Purpose | Audience |
|----------|---------|----------|
| [OLLAMA_CODEX_SUMMARY.md](./OLLAMA_CODEX_SUMMARY.md) | Project completion overview | Project Leads |
| [ollama-codex-integration.md](./ollama-codex-integration.md) | Deep dive into implementation | Architects |
| [ollama-quick-reference.md](./ollama-quick-reference.md) | API reference and lookup | Developers |
| [ollama-integration-examples.md](./ollama-integration-examples.md) | 8 practical code examples | Integrators |
| [providers/ollama.md](./providers/ollama.md) | User-facing Ollama setup guide | End Users |

## Modules Overview

### Client Module (`ollama/client.rs`)

**Purpose**: High-level interface for Ollama server operations

```rust
pub struct OllamaClient { ... }

impl OllamaClient {
    pub async fn try_from_base_url(base_url: &str) -> io::Result<Self>
    pub async fn fetch_models(&self) -> io::Result<Vec<String>>
    pub async fn pull_model_stream(&self, model: &str) -> io::Result<BoxStream<OllamaPullEvent>>
}
```

**Key Features**:
- Server health checking with 5s timeout
- Connection error with setup instructions
- Streaming model pulls
- Model enumeration

### Pull Module (`ollama/pull.rs`)

**Purpose**: Progress reporting for model downloads

```rust
pub enum OllamaPullEvent {
    Status(String),
    ChunkProgress { digest, total, completed },
    Success,
    Error(String),
}

pub trait OllamaPullProgressReporter {
    fn on_event(&mut self, event: &OllamaPullEvent) -> io::Result<()>;
}
```

**Implementations**:
- `CliPullProgressReporter` - Terminal output with speed/percentage
- `TuiPullProgressReporter` - TUI stub for future integration

### Parser Module (`ollama/parser.rs`)

**Purpose**: Parse Ollama's JSON-lines streaming protocol

```rust
pub fn pull_events_from_value(value: &JsonValue) -> Vec<OllamaPullEvent>
```

**Handles**:
- Status messages
- Progress chunks
- Success signals
- Error messages

### URL Module (`ollama/url.rs`)

**Purpose**: URL handling for base URL normalization

```rust
pub fn is_openai_compatible_base_url(base_url: &str) -> bool
pub fn base_url_to_host_root(base_url: &str) -> String
```

**Features**:
- Detects OpenAI-compatible endpoints (`/v1`)
- Extracts host from URL
- Handles trailing slashes

## Data Flow

```
┌─────────────────────────────────────────────────────────────┐
│                    Your Application                          │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
         ┌─────────────────────────────┐
         │    OllamaClient             │
         ├─────────────────────────────┤
         │ • try_from_base_url()       │
         │ • fetch_models()            │
         │ • pull_model_stream()       │ ◄── Health check + error handling
         └────────┬────────────────────┘
                  │
         ┌────────┴─────────────────────┐
         │                              │
         ▼                              ▼
    ┌─────────────┐           ┌──────────────────┐
    │HTTP Client  │           │JSON Parser       │
    │(Tokio)      │           │(JSON-lines)      │
    └─────────────┘           └────────┬─────────┘
         │                             │
         │                             ▼
         │                   ┌──────────────────┐
         │                   │pull_events_      │
         │                   │from_value()      │
         │                   └────────┬─────────┘
         │                            │
         └────────────┬───────────────┘
                      │
                      ▼
          ┌──────────────────────┐
          │  OllamaPullEvent     │
          │  (enum)              │
          └──────────┬───────────┘
                     │
                     ▼
    ┌────────────────────────────┐
    │OllamaPullProgressReporter  │
    │(trait)                     │
    └────┬─────────────────────┬─┘
         │                     │
         ▼                     ▼
    ┌──────────────┐   ┌──────────────┐
    │CliProgress  │   │TuiProgress   │
    │Reporter     │   │Reporter      │
    └──────────────┘   └──────────────┘
```

## Integration Roadmap

### Phase 1: Core Infrastructure (✅ Complete)
- [x] `OllamaClient` with health checks
- [x] `OllamaPullEvent` and reporters
- [x] Event parsing for JSON-lines
- [x] URL utilities

### Phase 2: Startup Integration (🔄 Ready)
- [ ] Health check on startup
- [ ] Warn if Ollama unavailable
- [ ] Log available models

### Phase 3: Model Selection (🔄 Ready)
- [ ] Show progress when selecting missing models
- [ ] Auto-pull on demand
- [ ] Progress window in TUI

### Phase 4: Configuration (🔄 Ready)
- [ ] Add `[ollama]` section to `vtcode.toml`
- [ ] `auto_pull` option
- [ ] Model whitelist
- [ ] Timeout configuration

## Common Patterns

### Pattern 1: Health Check

```rust
let client = OllamaClient::try_from_base_url("http://localhost:11434").await?;
// Server is reachable
```

### Pattern 2: List Models

```rust
let models = client.fetch_models().await?;
for model in models {
    println!("{}", model);
}
```

### Pattern 3: Pull with Progress

```rust
let mut stream = client.pull_model_stream("model").await?;
let mut reporter = CliPullProgressReporter::new();

while let Some(event) = stream.next().await {
    reporter.on_event(&event)?;
}
```

### Pattern 4: Custom Reporter

```rust
impl OllamaPullProgressReporter for MyReporter {
    fn on_event(&mut self, event: &OllamaPullEvent) -> io::Result<()> {
        match event {
            OllamaPullEvent::Status(s) => { /* ... */ }
            OllamaPullEvent::ChunkProgress { .. } => { /* ... */ }
            _ => {}
        }
        Ok(())
    }
}
```

## Error Handling

### Connection Error
```
No running Ollama server detected. Start it with: `ollama serve` (after installing)
Install instructions: https://github.com/ollama/ollama?tab=readme-ov-file
```

### Model Not Found
```
No remote models found. Ensure Ollama server is running or set OLLAMA_BASE_URL
```

## Testing

```bash
# All Ollama tests
cargo test --lib ollama

# Specific module
cargo test --lib ollama::url
cargo test --lib ollama::parser

# With output
cargo test --lib ollama -- --nocapture
```

## File Locations

```
vtcode/
├── vtcode-core/src/llm/providers/
│   ├── ollama.rs                  (main provider)
│   └── ollama/                    (new modules)
│       ├── client.rs              (high-level API)
│       ├── pull.rs                (progress reporting)
│       ├── parser.rs              (event parsing)
│       └── url.rs                 (URL utilities)
│
└── docs/
    ├── providers/
    │   └── ollama.md              (user guide)
    │
    ├── ollama-codex-integration.md    (architecture)
    ├── ollama-quick-reference.md      (developer ref)
    ├── ollama-integration-examples.md (code examples)
    ├── OLLAMA_CODEX_SUMMARY.md        (project summary)
    └── OLLAMA_INDEX.md                (this file)
```

## Key Concepts

### OllamaPullEvent
Structured event emitted during model pulling:
- **Status**: Human-readable update (e.g., "verifying")
- **ChunkProgress**: Byte-level progress for a layer
- **Success**: Pull completed
- **Error**: Error occurred

### OllamaPullProgressReporter
Trait for handling events. Allows multiple implementations (CLI, TUI, logging, etc.)

### URL Normalization
Handles both:
- Native Ollama: `http://localhost:11434`
- OpenAI-compatible: `http://localhost:11434/v1`

## Performance Notes

- Connection timeout: 5 seconds
- No polling - stream-based events
- Memory-efficient string buffering
- JSON-lines parsing (one line at a time)

## Compatibility

- Rust 1.93.0+
- Tokio async runtime
- Ollama 0.1+
- OpenAI-compatible endpoints
- Cloud and local deployments

## References

- **Codex Source**: https://github.com/openai/codex/tree/main/codex-rs/ollama
- **Ollama Docs**: https://ollama.com/docs
- **Ollama API**: https://github.com/ollama/ollama/blob/main/docs/api.md

## Document Relations

```
OLLAMA_INDEX.md (this file)
├── OLLAMA_CODEX_SUMMARY.md
│   └── (overview of completed work)
│
├── ollama-codex-integration.md
│   └── (detailed patterns and design)
│
├── ollama-quick-reference.md
│   └── (developer lookup)
│
└── ollama-integration-examples.md
    └── (8 practical scenarios)
```

## Questions?

Refer to the appropriate document:
- **"How do I...?"** → `ollama-integration-examples.md`
- **"What's the API for...?"** → `ollama-quick-reference.md`
- **"Why this design...?"** → `ollama-codex-integration.md`
- **"What was completed...?"** → `OLLAMA_CODEX_SUMMARY.md`
- **"How do I use Ollama...?"** → `providers/ollama.md`

---

**Last Updated**: December 31, 2025  
**Status**: Production-ready  
**Quality**: Comprehensive documentation + tests
