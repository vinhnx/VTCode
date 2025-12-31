# Ollama Integration Examples

Practical examples for integrating the new Ollama client and progress reporting into VT Code's systems.

## Example 1: Add Auto-Pull Support to Model Selection

When a user selects a model that isn't installed, automatically pull it:

```rust
// In model selection logic
use vtcode_core::llm::providers::ollama::{OllamaClient, CliPullProgressReporter, OllamaPullProgressReporter};
use futures::StreamExt;

pub async fn ensure_model_available(model: &str, base_url: &str) -> anyhow::Result<()> {
    let client = OllamaClient::try_from_base_url(base_url).await?;
    
    // Check if model is available
    let models = client.fetch_models().await?;
    if models.iter().any(|m| m == model) {
        return Ok(());
    }
    
    // Model not found, pull it
    println!("Model not found locally. Downloading {}...", model);
    let mut stream = client.pull_model_stream(model).await?;
    let mut reporter = CliPullProgressReporter::new();
    
    while let Some(event) = stream.next().await {
        reporter.on_event(&event)?;
    }
    
    Ok(())
}
```

## Example 2: Health Check on Startup

Add a startup check to warn if Ollama isn't available:

```rust
// In main initialization
use vtcode_core::llm::providers::ollama::OllamaClient;

pub async fn check_ollama_availability() {
    match OllamaClient::try_from_base_url("http://localhost:11434").await {
        Ok(_) => {
            tracing::info!("Ollama server is available");
        }
        Err(e) => {
            tracing::warn!("Ollama not available: {}", e);
            // Continue anyway - user might use cloud models
        }
    }
}
```

## Example 3: Tool That Lists Available Models

Create a tool that agents can use to list Ollama models:

```rust
// In vtcode-tools or similar
use vtcode_core::llm::providers::ollama::OllamaClient;

pub async fn list_ollama_models(base_url: Option<String>) -> anyhow::Result<String> {
    let url = base_url.as_deref().unwrap_or("http://localhost:11434");
    
    match OllamaClient::try_from_base_url(url).await {
        Ok(client) => {
            let models = client.fetch_models().await?;
            let model_list = models
                .iter()
                .map(|m| format!("  - {}", m))
                .collect::<Vec<_>>()
                .join("\n");
            
            Ok(format!("Available Ollama models:\n{}", model_list))
        }
        Err(e) => Err(anyhow::anyhow!("Failed to connect to Ollama: {}", e))
    }
}
```

## Example 4: TUI Progress Window

Show pull progress in a TUI window:

```rust
// Pseudocode for TUI integration
use vtcode_core::llm::providers::ollama::{OllamaClient, TuiPullProgressReporter};
use futures::StreamExt;

pub async fn show_pull_progress_in_tui(model: &str) -> anyhow::Result<()> {
    let client = OllamaClient::try_from_base_url("http://localhost:11434").await?;
    let mut stream = client.pull_model_stream(model).await?;
    let mut reporter = TuiPullProgressReporter::default();
    
    // Create TUI window
    // let mut window = tui::create_progress_window("Downloading model...")?;
    
    while let Some(event) = stream.next().await {
        reporter.on_event(&event)?;
        // window.update_from_event(&event)?;
    }
    
    Ok(())
}
```

## Example 5: Configuration-Based Model Auto-Pull

Read from `vtcode.toml` to auto-pull required models:

```rust
// In config loading
use vtcode_core::config::Config;
use vtcode_core::llm::providers::ollama::OllamaClient;

pub async fn auto_pull_configured_models(config: &Config) -> anyhow::Result<()> {
    // Hypothetical: config.ollama.auto_pull_models = ["llama2", "mistral"]
    
    let client = OllamaClient::try_from_base_url("http://localhost:11434").await?;
    let available = client.fetch_models().await?;
    
    for model in config.ollama.auto_pull_models.iter() {
        if !available.contains(model) {
            println!("Pulling required model: {}", model);
            let mut stream = client.pull_model_stream(model).await?;
            let mut reporter = CliPullProgressReporter::new();
            
            while let Some(event) = stream.next().await {
                reporter.on_event(&event)?;
            }
        }
    }
    
    Ok(())
}
```

## Example 6: Custom Progress Reporter

Implement a custom reporter for logging:

```rust
use vtcode_core::llm::providers::ollama::{OllamaPullEvent, OllamaPullProgressReporter};
use std::io;

pub struct LoggingPullReporter;

impl OllamaPullProgressReporter for LoggingPullReporter {
    fn on_event(&mut self, event: &OllamaPullEvent) -> io::Result<()> {
        match event {
            OllamaPullEvent::Status(s) => {
                tracing::info!("Pull status: {}", s);
            }
            OllamaPullEvent::ChunkProgress { digest, total, completed } => {
                if let (Some(t), Some(c)) = (total, completed) {
                    let pct = (c * 100) / t;
                    tracing::debug!("Progress for {}: {}%", digest, pct);
                }
            }
            OllamaPullEvent::Success => {
                tracing::info!("Model pull completed successfully");
            }
            OllamaPullEvent::Error(e) => {
                tracing::error!("Pull error: {}", e);
            }
        }
        Ok(())
    }
}

// Usage:
let mut reporter = LoggingPullReporter;
while let Some(event) = stream.next().await {
    reporter.on_event(&event)?;
}
```

## Example 7: Multi-Model Pull with Progress Tracking

Pull multiple models and track overall progress:

```rust
use vtcode_core::llm::providers::ollama::{OllamaClient, CliPullProgressReporter};
use futures::StreamExt;

pub async fn pull_multiple_models(models: &[&str]) -> anyhow::Result<()> {
    let client = OllamaClient::try_from_base_url("http://localhost:11434").await?;
    
    for (i, model) in models.iter().enumerate() {
        println!("\n[{}/{}] Pulling {}...", i + 1, models.len(), model);
        
        let mut stream = client.pull_model_stream(model).await?;
        let mut reporter = CliPullProgressReporter::new();
        
        while let Some(event) = stream.next().await {
            reporter.on_event(&event)?;
        }
    }
    
    Ok(())
}

// Usage:
pull_multiple_models(&["llama2", "mistral", "neural-chat"]).await?;
```

## Example 8: Health Check with Automatic Fallback

Try primary Ollama server, fall back to cloud:

```rust
use vtcode_core::llm::providers::ollama::OllamaClient;

pub async fn get_ollama_client() -> anyhow::Result<OllamaClient> {
    // Try local first
    match OllamaClient::try_from_base_url("http://localhost:11434").await {
        Ok(client) => {
            tracing::info!("Connected to local Ollama");
            return Ok(client);
        }
        Err(_) => {
            tracing::warn!("Local Ollama not available, trying cloud");
        }
    }
    
    // Fall back to cloud (requires API key)
    if let Ok(api_key) = std::env::var("OLLAMA_API_KEY") {
        if let Ok(client) = OllamaClient::try_from_base_url("https://api.ollama.com").await {
            tracing::info!("Connected to Ollama Cloud");
            return Ok(client);
        }
    }
    
    Err(anyhow::anyhow!(
        "No Ollama server available. Start local with `ollama serve` or set OLLAMA_API_KEY"
    ))
}
```

## Integration Checklist

- [ ] Add `OllamaClient` to your initialization code
- [ ] Hook `pull_model_stream()` into model selection UI
- [ ] Implement a custom `OllamaPullProgressReporter` for your UI framework
- [ ] Add health check on startup
- [ ] Create a tool that lists available models
- [ ] Add configuration for auto-pull behavior
- [ ] Test with various Ollama server configurations
- [ ] Document Ollama setup in user guide

## Testing These Examples

All examples can be tested with a running Ollama server:

```bash
# Start Ollama
ollama serve

# In another terminal, pull a small test model
ollama pull mistral

# Then run your integration code
cargo run -- your-integration-test
```

## See Also

- [Ollama Integration Guide](./ollama-codex-integration.md)
- [Ollama Quick Reference](./ollama-quick-reference.md)
- [Ollama Provider Guide](./providers/ollama.md)
