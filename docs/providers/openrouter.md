# OpenRouter Integration Guide

OpenRouter expands VT Code with access to the full model marketplace. This guide covers configuration, CLI usage, and tips for
working with custom OpenRouter model IDs.

## Prerequisites

1. [Create an OpenRouter account](https://openrouter.ai) and generate an API key.
2. Export the API key in your shell or add it to a local `.env` file:

```bash
export OPENROUTER_API_KEY="your-openrouter-key"
# or
cat <<'ENV' > .env
OPENROUTER_API_KEY=your-openrouter-key
ENV
```

## Quickstart

Run VT Code against the Qwen3 Coder model optimised for IDE workflows:

```bash
vtcode --provider openrouter --model qwen/qwen3-coder chat
```

Both commands stream responses using the OpenRouter Responses API and support VT Code tooling out of the box.

## Persisting configuration

Add OpenRouter to your workspace `vtcode.toml`:

```toml
[agent]
provider = "openrouter"
default_model = "qwen/qwen3-coder"
```

Custom model IDs are accepted. If you reference a model not listed in `docs/models.json`, ensure it is enabled for your
OpenRouter account.

## Runtime behaviour

- **Tool calling:** VT Code maps OpenRouter conversations to the OpenAI-compatible function calling format.
- **Streaming:** Streaming is fully supported for OpenRouter providers (VT Code uses the standard streaming interface).
- **Prompt refinement:** The prompt refiner automatically reuses your OpenRouter key and respects any custom model overrides.
- **Routing:** When the LLM router is enabled, VT Code honours the configured provider and model combination for routing tasks.

## Troubleshooting

| Symptom | Resolution |
| --- | --- |
| `HTTP 403` or `401` errors | Confirm `OPENROUTER_API_KEY` is set and active for the chosen model. |
| Model not found | Double-check the model slug in the [OpenRouter catalog](https://openrouter.ai/docs/llms) and your workspace config. |
| Tool calls ignored | Ensure the model you selected advertises tool support. Many third-party providers expose read-only models. |

For additional details, consult the [OpenRouter API reference](https://openrouter.ai/docs/api-reference/overview/llms) and the
[streaming documentation](https://openrouter.ai/docs/api-reference/streaming/llms).
