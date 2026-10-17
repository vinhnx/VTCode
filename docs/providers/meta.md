# Meta AI Integration Guide

VT Code has a first-class Meta AI provider for Meta's Muse models. Prefer this provider when you want to call Meta
directly; use the [OpenRouter guide](./openrouter.md#meta-muse-models) when you specifically want OpenRouter billing,
routing, or fallback options.

## Prerequisites

Create a Meta AI API key using the [Meta AI developer documentation](https://dev.meta.ai/docs/llms.txt). Meta's
documentation uses `MODEL_API_KEY`; VT Code accepts that variable and also supports the provider-specific `META_API_KEY`
variable.

```bash
export MODEL_API_KEY="your-meta-api-key"
# Or: export META_API_KEY="your-meta-api-key"
```

## Quickstart

Use the official provider and its default Muse model:

```bash
vtcode --provider meta --model muse-spark-1.3 chat
```

The equivalent persistent configuration is:

```toml
[agent]
provider = "meta"
default_model = "muse-spark-1.3"
```

The endpoint defaults to `https://api.meta.ai/v1`. Set `META_BASE_URL` when a compatible gateway or test endpoint is
required.

## Curated models

| Model                        | Notes                                                                                   |
| ---------------------------- | --------------------------------------------------------------------------------------- |
| `muse-wonder-full-1.0`       | Most advanced Muse Wonder Full model for critical workflows (Muse Code subscription)   |
| `muse-wonder-1.0`            | Frontier Muse Wonder model for coding, long-running agents, and advanced computer use |
| `muse-spark-1.3`             | Default curated Standard-tier Muse Spark model                                          |
| `muse-spark-1.3-contributor` | Opt-in Contributor-tier variant; review Meta's data-use terms before use                |
| `muse-spark-1.1`             | Previous curated Standard-tier Muse Spark model                                         |

All five models are exposed through the `/model` picker and support streaming, function tools, structured output,
multimodal input, and reasoning effort controls through Meta's OpenAI-compatible Chat Completions API.

See the [Meta model catalog](https://developer.meta.com/ai/models), [Muse Wonder](https://developer.meta.com/ai/models/muse-wonder/)
([announcement](https://research.meta.ai/blog/muse-wonder)),
[Chat Completions protocol](https://dev.meta.ai/docs/protocols/chat-completions.md), and
[authentication guide](https://dev.meta.ai/docs/authentication.md) for provider-side details.

## Troubleshooting

| Symptom                 | Resolution                                                                                                                  |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Missing credentials     | Set `META_API_KEY` or the documented `MODEL_API_KEY` variable.                                                              |
| Model rejected          | Use one of `muse-wonder-1.0`, `muse-wonder-full-1.0`, `muse-spark-1.1`, `muse-spark-1.3`, or `muse-spark-1.3-contributor` for the official Meta provider. |
| Custom endpoint failure | Confirm the endpoint implements Meta's `/v1/chat/completions` protocol and set `META_BASE_URL`.                             |
