# Merge Gateway Quick Reference

| Setting           | Value                                                                                                                                                                                 |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Provider key      | `merge-gateway`                                                                                                                                                                       |
| API key           | `MERGE_GATEWAY_API_KEY`                                                                                                                                                               |
| Native endpoint   | `https://api-gateway.merge.dev/v1`                                                                                                                                                    |
| Endpoint override | `MERGE_GATEWAY_BASE_URL`                                                                                                                                                              |
| Default model     | `default_routing`                                                                                                                                                                     |
| Curated routes    | See the full list below; all are available in the model picker                                                                                                                        |
| Default transport | Merge Responses API (`POST /responses`)                                                                                                                                               |
| Legacy transport  | Explicit base URLs ending in `/v1/openai` use Chat Completions                                                                                                                        |
| Catalog           | Authenticated `GET /models`, paginated and cache-backed                                                                                                                               |
| Authentication    | **\*\***                                                                                                                                                                              |
| Tool calls        | Supported                                                                                                                                                                             |
| Streaming usage   | Supported via native Responses SSE                                                                                                                                                    |
| Reasoning effort  | Forwarded per route: `reasoning_effort` (OpenAI/xAI/Moonshot/ZAI) or `thinking.budget_tokens` (Anthropic/Gemini/DeepSeek/Qwen/MiniMax/Thinking Machines); unknown routes omitted |

Curated routes:

```text
openai/gpt-5.5
anthropic/claude-opus-5
anthropic/claude-opus-5-5
anthropic/claude-sonnet-5
anthropic/claude-sonnet-5-5
google/gemini-3.6-flash
google/gemini-3.7-flash
google/gemini-4-argon
deepseek/deepseek-v4-pro-0813
deepseek/deepseek-v4-flash-0731
xai/grok-4.6
qwen/qwen3.8-max
minimax/minimax-h3
moonshot/kimi-k3
thinkingmachines/inkling
zai/glm-5.3-flash
zai/glm-5.3-flashx
openai/gpt-5.6-luna
openai/gpt-5.6-sol
openai/gpt-5.6-terra
openai/gpt-6-astra
openai/gpt-6-sol
openai/gpt-6.1-sol
openai/gpt-6-luna
xiaomimimo/mimo-v2.6-pro
xiaomimimo/mimo-v2.6-flash
```

## Minimal setup

```bash
export MERGE_GATEWAY_API_KEY="your-merge-api-key"
vtcode --provider merge-gateway --model default_routing
```

For arbitrary valid Merge routes:

```bash
vtcode --provider merge-gateway --model deepseek/deepseek-v4-pro
```

Create keys in the [Merge dashboard](https://dashboard.merge.dev/). See the
[full Merge Gateway guide](./merge-gateway.md) for configuration, limitations, and troubleshooting.
