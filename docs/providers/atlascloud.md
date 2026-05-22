# Atlas Cloud Integration Guide

Atlas Cloud exposes an OpenAI-compatible chat API, so VT Code can use it through
the existing `[[custom_providers]]` support without adding a dedicated runtime
provider.

## Prerequisites

1. Create an Atlas Cloud API key.
2. Store the key in a local `.env` file:

```bash
cat <<'ENV' > .env
ATLASCLOUD_API_KEY=your-atlascloud-key
ENV
```

## Quickstart

Add Atlas Cloud to your workspace `vtcode.toml`:

```toml
[agent]
provider = "atlascloud"
default_model = "deepseek-ai/DeepSeek-V3-0324"
reasoning_effort = "low"

[[custom_providers]]
name = "atlascloud"
display_name = "Atlas Cloud"
base_url = "https://api.atlascloud.ai/v1"
api_key_env = "ATLASCLOUD_API_KEY"
model = "deepseek-ai/DeepSeek-V3-0324"
```

Then run VT Code normally:

```bash
vtcode ask "Summarize this repository"
```

## Why This Works

- Atlas Cloud's LLM endpoint is OpenAI-compatible.
- VT Code registers `[[custom_providers]]` as OpenAI-compatible providers.
- Custom providers bypass the built-in model catalog check, so Atlas-specific
  model IDs work as long as the upstream endpoint accepts them.

## Model Selection Notes

- Atlas Cloud's docs often show the alias `deepseek-v3`.
- In live testing, the exact slug `deepseek-ai/DeepSeek-V3-0324` was returned by
  `GET /v1/models` and worked for `POST /v1/chat/completions`.
- If your account enables different models, update both `agent.default_model`
  and `[[custom_providers]].model` to the slug returned by Atlas Cloud.

## CLI Examples

Run a one-off prompt with the workspace config:

```bash
vtcode ask "Explain the current module layout"
```

Override the model for a single invocation:

```bash
vtcode ask --model qwen/qwen3-32b "Review this patch at a high level"
```

## Troubleshooting

| Symptom | Resolution |
| --- | --- |
| `Unknown provider: atlascloud` | Use a VT Code build that includes custom provider registration in CLI flows. |
| `API key not found for custom provider 'atlascloud'` | Confirm `ATLASCLOUD_API_KEY` is present in your shell or local `.env`. |
| `403` from chat completions | Verify the model slug against `GET /v1/models` and confirm the Atlas account can access chat generation. |
| Model not found | Replace the sample model with the exact model ID returned by Atlas Cloud for your account. |

## References

- [Atlas Cloud LLM / Chat](https://www.atlascloud.ai/docs/models/llm)
- [Atlas Cloud FAQ](https://www.atlascloud.ai/docs/en/faq)
