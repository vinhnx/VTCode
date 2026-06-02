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

# Optional: list available models for the model picker.
# models = [
#     "deepseek-ai/DeepSeek-V3-0324",
#     "deepseek-ai/deepseek-r1-0528",
#     "moonshotai/Kimi-K2-Instruct",
#     "Qwen/Qwen3-Coder",
#     "google/gemini-2.5-flash",
#     "openai/gpt-5.2-chat",
#     "anthropic/claude-opus-4.5-20251101",
#     "zai-org/glm-4.7",
#     "minimaxai/minimax-m2.1",
#     "xai/grok-4-0709",
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
  The validated chat pool used in our rollout contains 50 cross-provider model
  IDs that have already been smoke-tested through Atlas Cloud's OpenAI-compatible
  route.
- The recommended default is `deepseek-ai/DeepSeek-V3-0324`, because it is a
  stable model slug that is already verified across multiple Atlas integrations.
- Use the `models` field in `[[custom_providers]]` to populate the model picker
  with the exact slugs you have access to. Run `GET /v1/models` against your
  API key to see your full catalog.
- If your account enables different models, update both `agent.default_model`
  and `[[custom_providers]].model` to the slug returned by Atlas Cloud.

## Validated 50-model pool

The following examples mirror the validated Atlas chat pool used across recent
Atlas provider rollouts:

- `deepseek-ai/DeepSeek-V3-0324`, `deepseek-ai/deepseek-r1-0528`, `moonshotai/Kimi-K2-Instruct`, `Qwen/Qwen3-Coder`, `Qwen/Qwen3-235B-A22B-Instruct-2507`
- `deepseek-ai/DeepSeek-V3.1`, `moonshotai/Kimi-K2-Instruct-0905`, `Qwen/Qwen3-Next-80B-A3B-Instruct`, `Qwen/Qwen3-Next-80B-A3B-Thinking`, `Qwen/Qwen3-30B-A3B-Instruct-2507`
- `deepseek-ai/DeepSeek-V3.1-Terminus`, `deepseek-ai/DeepSeek-V3.2-Exp`, `zai-org/GLM-4.6`, `MiniMaxAI/MiniMax-M2`, `Qwen/Qwen3-VL-235B-A22B-Instruct`
- `moonshotai/Kimi-K2-Thinking`, `google/gemini-2.5-flash`, `google/gemini-2.5-flash-lite`, `openai/gpt-5.1`, `openai/gpt-5.1-chat`
- `openai/gpt-4o`, `openai/gpt-4o-mini`, `openai/gpt-4.1`, `openai/gpt-4.1-mini`, `openai/gpt-4.1-nano`
- `openai/o1`, `openai/o3`, `openai/o3-mini`, `openai/o4-mini`, `anthropic/claude-sonnet-4.5-20250929`
- `deepseek-ai/deepseek-v3.2`, `openai/gpt-5`, `openai/gpt-5-chat`, `openai/gpt-5-mini`, `openai/gpt-5-nano`
- `openai/gpt-5.2`, `openai/gpt-5.2-chat`, `google/gemini-2.5-pro`, `anthropic/claude-opus-4.5-20251101`, `google/gemini-3-flash-preview`
- `zai-org/glm-4.7`, `minimaxai/minimax-m2.1`, `google/gemini-2.0-flash`, `qwen/qwen3-8b`, `qwen/qwen3-235b-a22b-thinking-2507`
- `qwen/qwen3-vl-235b-a22b-thinking`, `qwen/qwen3-30b-a3b`, `qwen/qwen3-30b-a3b-thinking-2507`, `deepseek-ai/deepseek-ocr`, `xai/grok-4-0709`

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
