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

## Moonshot AI

- **Guide:** [Moonshot Integration](./providers/moonshot.md)
- **Official docs:**
  - [API overview](https://platform.moonshot.ai/docs/overview)
  - [Kimi K2 Turbo promotion](https://platform.moonshot.ai/docs/promotion#kimi-k2-model-limited-time-promotion)
- Default model: `moonshot-v1-32k` (override via `vtcode.toml` or CLI `--model`).

## OpenRouter Marketplace

- **Guide:** [OpenRouter Integration](./providers/openrouter.md)
- **Official docs:**
  - [API overview](https://openrouter.ai/docs/api-reference/overview/llms)
  - [Streaming](https://openrouter.ai/docs/api-reference/streaming/llms)
  - [Model catalog](https://openrouter.ai/docs/llms)
- Default models: `x-ai/grok-code-fast-1`, `qwen/qwen3-coder` (override via `vtcode.toml` or CLI `--model`).

> ℹ️ Additional provider-specific guides will be added as new integrations land in VT Code.
