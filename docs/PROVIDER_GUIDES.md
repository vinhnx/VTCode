# Provider Guides

This index collects provider-specific guides for configuring VT Code with different LLM backends.

## Google Gemini

- Configuration details are covered in the main [Getting Started guide](./user-guide/getting-started.md#configure-your-llm-provider).
- Models and constants are defined in [`vtcode-core/src/config/constants.rs`](../vtcode-core/src/config/constants.rs).

## OpenAI GPT

- Follow the [Getting Started guide](./user-guide/getting-started.md#configure-your-llm-provider) for API key setup.
- See [`vtcode-core/src/config/constants.rs`](../vtcode-core/src/config/constants.rs) for the latest supported models.

## Anthropic Claude

- Key management and defaults mirror the Gemini/OpenAI flow in [Getting Started](./user-guide/getting-started.md#configure-your-llm-provider).
- Supported model IDs live in [`vtcode-core/src/config/constants.rs`](../vtcode-core/src/config/constants.rs).

## OpenRouter Marketplace

- **Guide:** [OpenRouter Integration](./providers/openrouter.md)
- **Official docs:**
  - [API overview](https://openrouter.ai/docs/api-reference/overview/llms)
  - [Streaming](https://openrouter.ai/docs/api-reference/streaming/llms)
  - [Model catalog](https://openrouter.ai/docs/llms)
- Default models: `x-ai/grok-code-fast-1`, `qwen/qwen3-coder` (override via `vtcode.toml` or CLI `--model`).

## Ollama Local & Cloud Models

- **Setup:** Install and run Ollama locally ([official install](https://ollama.com/download))
- **Configuration:** Local usage needs no key; set `OLLAMA_API_KEY` to access Ollama Cloud
- **Default model:** Any locally available model (e.g., `llama3:8b`, `mistral:7b`, `qwen3:1.7b`)
- **Cloud models:** Use IDs like `gpt-oss:120b-cloud` with `OLLAMA_BASE_URL=https://ollama.com`
- **Custom Models:** Use the `custom-ollama` option in the model picker to enter any locally or cloud-available model ID
- **Base URL:** Configurable via `OLLAMA_BASE_URL` environment variable (defaults to `http://localhost:11434`)
- **Features:** Streaming, structured tool calling (including Ollama's web search tools), and thinking traces when `reasoning_effort` is enabled

> ℹ️ Additional provider-specific guides will be added as new integrations land in VT Code.
