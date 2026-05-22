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
default_model = "deepseek-ai/deepseek-v4-flash"
reasoning_effort = "low"

[[custom_providers]]
name = "atlascloud"
display_name = "Atlas Cloud"
base_url = "https://api.atlascloud.ai/v1"
api_key_env = "ATLASCLOUD_API_KEY"
model = "deepseek-ai/deepseek-v4-flash"

# Optional: list available models for the model picker.
# models = [
#     "deepseek-ai/deepseek-v4-flash",
#     "deepseek-ai/deepseek-v4-pro",
#     "deepseek-ai/DeepSeek-V3-0324",
#     "deepseek-ai/DeepSeek-V3.1",
#     "deepseek-ai/deepseek-v3.2",
#     "deepseek-ai/deepseek-r1-0528",
#     "qwen/qwen3.6-35b-a3b",
#     "qwen/qwen3.6-plus",
#     "qwen/qwen3.5-122b-a10b",
#     "qwen/qwen3-coder-next",
#     "moonshotai/kimi-k2.6",
#     "moonshotai/kimi-k2.5",
#     "zai-org/glm-5.1",
#     "zai-org/glm-5-turbo",
#     "minimaxai/minimax-m2.7",
#     "minimaxai/minimax-m2.5",
#     "kwaipilot/kat-coder-pro-v2",
# ]
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

- Atlas Cloud hosts 300+ models spanning LLM, image, video, audio, and 3D.
  The complete LLM catalog includes DeepSeek, Qwen, Kimi, GLM, MiniMax,
  and many more.
- The recommended default is `deepseek-ai/deepseek-v4-flash` — DeepSeek's
  latest flash model with a 1M context window and competitive pricing
  ($0.14/M input tokens).
- Use the `models` field in `[[custom_providers]]` to populate the model picker
  with the exact slugs you have access to. Run `GET /v1/models` against your
  API key to see your full catalog.
- If your account enables different models, update both `agent.default_model`
  and `[[custom_providers]].model` to the slug returned by Atlas Cloud.

## CLI Examples

Run a one-off prompt with the workspace config:

```bash
vtcode ask "Explain the current module layout"
```

Override the model for a single invocation:

```bash
vtcode ask --model moonshotai/kimi-k2.6 "Review this patch at a high level"
```

## Troubleshooting

| Symptom | Resolution |
| --- | --- |
| `Unknown provider: atlascloud` | Use a VT Code build that includes custom provider registration in CLI flows. |
| `API key not found for custom provider 'atlascloud'` | Confirm `ATLASCLOUD_API_KEY` is present in your shell or local `.env`. |
| `403` from chat completions | Verify the model slug against `GET /v1/models` and confirm the Atlas account can access chat generation. |
| Model not found | Replace the sample model with the exact model ID returned by Atlas Cloud for your account. |

## References

- [Atlas Cloud LLM Catalog](https://www.atlascloud.ai/models/list/llm)
- [Atlas Cloud API Docs](https://docs.atlascloud.ai)
- [Atlas Cloud FAQ](https://www.atlascloud.ai/docs/en/faq)
