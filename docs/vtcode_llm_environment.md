# `vtcode-llm` Environment Configuration Guide

This guide explains how the `vtcode-llm` crate discovers provider credentials, maps
feature flags to environment variables, and offers lightweight mocks for downstream
integration tests. It is intended for consumers who want to adopt the crate without
bringing in VT Code's full configuration system.

## Provider environment variables

Each provider feature corresponds to one or more environment variables. Keys should
be populated before constructing a client so the `ProviderConfig` trait can surface
the secret values.

| Feature flag | Primary variable     | Aliases          | Notes                                                                                                             |
| ------------ | -------------------- | ---------------- | ----------------------------------------------------------------------------------------------------------------- |
| `google`     | `GEMINI_API_KEY`     | `GOOGLE_API_KEY` | Gemini clients accept either variable; the first non-empty value wins.                                            |
| `openai`     | `OPENAI_API_KEY`     | –                | Required for GPT models served by OpenAI.                                                                         |
| `anthropic`  | `ANTHROPIC_API_KEY`  | –                | Required for Claude models.                                                                                       |
| `deepseek`   | `DEEPSEEK_API_KEY`   | –                | Required for DeepSeek models.                                                                                     |
| `openrouter` | `OPENROUTER_API_KEY` | –                | Required for OpenRouter routing.                                                                                  |
| `xai`        | `XAI_API_KEY`        | –                | Required for xAI Grok models.                                                                                     |
| `zai`        | `ZAI_API_KEY`        | –                | Required for Zhipu AI (Z.AI) models.                                                                              |
| `moonshot`   | `MOONSHOT_API_KEY`   | –                | Required for Moonshot AI models.                                                                                  |
| `lmstudio`   | `LMSTUDIO_API_KEY`   | –                | Optional; provide when the LM Studio developer server enforces auth. Override host/port with `LMSTUDIO_BASE_URL`. |
| `ollama`     | _N/A_                | –                | Ollama uses a local runtime and does not require an API key.                                                      |

When multiple providers are enabled, populate the variables you plan to use. Downstream
applications can surface their own configuration UX but should forward the resolved
secrets to the `ProviderConfig` implementor.

## Loading keys with `ProviderConfig`

Implementors of [`config::ProviderConfig`](../vtcode-llm/src/config.rs) decide where
credentials originate. A common pattern is to read from environment variables and then
pass the owned values to `OwnedProviderConfig` before building a client:

```rust
use std::env;
use vtcode_llm::config::{as_factory_config, OwnedProviderConfig};

fn gemini_from_env() -> anyhow::Result<vtcode_core::llm::factory::ProviderConfig> {
    let key = env::var("GEMINI_API_KEY")
        .or_else(|_| env::var("GOOGLE_API_KEY"))
        .map_err(|_| anyhow::anyhow!("Set GEMINI_API_KEY or GOOGLE_API_KEY"))?;

    let config = OwnedProviderConfig::new()
        .with_api_key(key)
        .with_model("gemini-2.0-flash-exp".to_string());

    Ok(as_factory_config(&config))
}
```

Because the trait only exposes borrowed data, callers can also point to secrets stored
in files, KMS-backed fetchers, or other secret managers.

## Wiring workspace paths and telemetry

When prompt caching is enabled, use
[`config::AdapterHooks`](../vtcode-llm/src/config.rs) to resolve relative directories
against your workspace implementation and surface telemetry or error information using
`vtcode-commons` traits:

```rust
use vtcode_commons::{NoopErrorReporter, NoopTelemetry, WorkspacePaths};
use vtcode_llm::config::{as_factory_config_with_hooks, AdapterHooks, OwnedProviderConfig};

struct MyWorkspacePaths;

impl WorkspacePaths for MyWorkspacePaths {
    fn workspace_root(&self) -> &std::path::Path {
        std::path::Path::new("/srv/workspace")
    }

    fn config_dir(&self) -> std::path::PathBuf {
        std::path::PathBuf::from("/srv/workspace/config")
    }
}

let workspace_paths = MyWorkspacePaths;
let telemetry = NoopTelemetry;
let reporter = NoopErrorReporter;
let formatter = vtcode_commons::DisplayErrorFormatter;
let hooks = AdapterHooks::new(&workspace_paths, &telemetry, &reporter, &formatter);

let provider_config = OwnedProviderConfig::new().with_prompt_cache(Default::default());
let core_config = as_factory_config_with_hooks(&provider_config, &hooks);
```

The adapter records prompt-cache resolution events, normalizes cache directories to
absolute paths, and reports hook failures through the supplied telemetry and error
reporting implementations.

## Using the optional mock client

Enable the `mock` feature to access `mock::StaticResponseClient`, a lightweight
implementation of the `LLMClient` trait designed for deterministic tests:

```toml
# Cargo.toml
vtcode-llm = { version = "0.0.1", features = ["mock", "openai"] }
```

```rust
use vtcode_llm::mock::StaticResponseClient;
use vtcode_core::llm::types::{BackendKind, LLMResponse};

let mut client = StaticResponseClient::new("gpt-5-nano", BackendKind::OpenAI)
    .with_response(LLMResponse {
        content: "Hello from a test".into(),
        model: "gpt-5-nano".into(),
        usage: None,
        reasoning: None,
    });

let response = futures::executor::block_on(client.generate("ignored prompt"))?;
assert_eq!(response.content, "Hello from a test");
```

Queue as many responses (or errors) as needed. When the queue runs dry, the client
returns an `LLMError::InvalidRequest`, helping tests detect unexpected extra calls.

## Next steps

-   Add additional provider-specific environment documentation as new integrations land.
-   Share runnable examples that combine `ProviderConfig` implementors with the mock
    client to showcase end-to-end integration tests.
