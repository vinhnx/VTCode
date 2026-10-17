# Vercel AI Gateway Integration

The Vercel AI Gateway gives VT Code access to hundreds of models from OpenAI, Anthropic, Google, DeepSeek, Moonshot,
Alibaba, MiniMax, Mistral, and others behind a single OpenAI Chat Completions-compatible endpoint. A single API key
works across vendors with zero token markup and automatic failover.

## Setup

1. Create an API key in the [Vercel dashboard](https://vercel.com/dashboard) under AI Gateway. See the official
   [getting started guide](https://vercel.com/docs/ai-gateway/getting-started).
2. Export it before starting VT Code:

   ```bash
   export AI_GATEWAY_API_KEY="your-vercel-ai-gateway-key"
   ```

3. Select the provider in `vtcode.toml`:

   ```toml
   [agent]
   provider = "vercel"
   model = "anthropic/claude-sonnet-5"
   api_key_env = "AI_GATEWAY_API_KEY"

   # Optional: make the endpoint and credential identity explicit
   [agent.provider_settings.vercel]
   name = "Vercel AI Gateway"
   base_url = "https://ai-gateway.vercel.sh/v1"
   env_key = "AI_GATEWAY_API_KEY"
   ```

The default endpoint is:

```text
https://ai-gateway.vercel.sh/v1
```

VT Code posts OpenAI Chat Completions requests to `/chat/completions`. Set `VERCEL_AI_GATEWAY_BASE_URL` when a proxy is
required.

## Quick start

```bash
export AI_GATEWAY_API_KEY="your-vercel-ai-gateway-key"
vtcode --provider vercel --model anthropic/claude-sonnet-5 chat
```

You can also verify the endpoint directly with `curl`:

```bash
curl https://ai-gateway.vercel.sh/v1/chat/completions \
  -H "Authorization: Bearer $AI_GATEWAY_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "anthropic/claude-sonnet-5",
    "messages": [{ "role": "user", "content": "Hello!" }]
  }'
```

## Curated models

| Model ID                        | Display                |   Context | Input $/M | Output $/M | Notes                                              |
| ------------------------------- | ---------------------- | --------: | --------: | ---------: | -------------------------------------------------- |
| `anthropic/claude-sonnet-5`     | Claude Sonnet 5        | 1,000,000 |        $2 |        $10 | Default                                            |
| `anthropic/claude-opus-5`       | Claude Opus 5          | 1,000,000 |        $5 |        $25 |                                                    |
| `anthropic/claude-opus-5-5`     | Claude Opus 5.5        | 1,000,000 |        $4 |        $20 | Adaptive thinking always on, default effort medium |
| `anthropic/claude-haiku-4.5`    | Claude Haiku 4.5       |   200,000 |        $1 |         $5 |                                                    |
| `openai/gpt-5.6-sol`            | GPT-5.6 Sol            | 1,050,000 |        $2 |        $10 |                                                    |
| `openai/gpt-6-astra`            | GPT-6 Astra            | 1,050,000 |       $10 |        $50 |                                                    |
| `openai/gpt-5.6-luna`           | GPT-5.6 Luna           | 1,050,000 |     $0.20 |      $1.20 |                                                    |
| `openai/gpt-5.3-codex`          | GPT-5.3 Codex          |   400,000 |     $1.75 |        $14 |                                                    |
| `google/gemini-3.1-pro-preview` | Gemini 3.1 Pro Preview | 1,000,000 |        $2 |        $12 |                                                    |
| `google/gemini-3.8-flash`       | Gemini 3.8 Flash       | 1,000,000 |     $0.75 |      $3.75 |                                                    |
| `deepseek/deepseek-v4-pro`      | DeepSeek V4 Pro        | 1,000,000 |     $0.66 |      $1.98 |                                                    |
| `deepseek/deepseek-v4-flash`    | DeepSeek V4 Flash      | 1,000,000 |     $0.13 |      $0.26 |                                                    |
| `moonshotai/kimi-k3`            | Kimi K3                | 1,000,000 |        $3 |        $15 |                                                    |
| `moonshotai/kimi-k2.7-code`     | Kimi K2.7 Code         |   256,000 |     $0.95 |         $4 |                                                    |
| `alibaba/qwen3.8-max`           | Qwen3.8 Max            | 1,000,000 |        $2 |         $6 |                                                    |
| `alibaba/qwen3-coder-next`      | Qwen3 Coder Next       |   256,000 |     $0.50 |      $1.20 | No reasoning traces                                |
| `minimax/minimax-m3`            | MiniMax M3             |   512,000 |     $0.30 |      $1.20 |                                                    |

All curated models emit reasoning traces except `alibaba/qwen3-coder-next`.

Model IDs use the gateway's native `vendor/model` format. Unlisted gateway model IDs are accepted: VT Code validates the
request shape only, not the model allowlist.

## Persisting configuration

Add Vercel AI Gateway to your workspace `vtcode.toml`:

```toml
[agent]
provider = "vercel"
model = "anthropic/claude-sonnet-5"
api_key_env = "AI_GATEWAY_API_KEY"
```

## Shared model IDs and provider precedence

Some curated IDs (`anthropic/claude-sonnet-5`, `openai/gpt-6-astra`, `google/gemini-3.8-flash`,
`deepseek/deepseek-v4-pro`, `deepseek/deepseek-v4-flash`, `moonshotai/kimi-k3`, `moonshotai/kimi-k2.7-code`) also exist
in OpenRouter's catalog. When you configure the model string directly, set `provider = "vercel"` explicitly — bare
model-string auto-detection keeps OpenRouter precedence for shared IDs. Selecting a model from the `/model` picker
always routes to Vercel.

## Runtime behaviour

- **API surface:** VT Code sends standard OpenAI Chat Completions requests to `POST /chat/completions`.
- **Tool calling:** Function calling uses the OpenAI-compatible format.
- **Streaming:** Streaming is fully supported.
- **Reasoning:** Reasoning traces are surfaced for all curated models except `alibaba/qwen3-coder-next`.
- **Structured output:** Supported.
- **Compaction:** `openai/*` routes compact via the gateway's native `POST /v1/responses/compact` endpoint; all other
  routes use VT Code's universal local summarization. See the [compaction engine guide](../development/compaction.md).

## Troubleshooting

| Symptom                                      | Resolution                                                                                                                                                                          |
| -------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `HTTP 401` errors                            | Confirm `AI_GATEWAY_API_KEY` is set and points to a key created in the [Vercel dashboard](https://vercel.com/dashboard).                                                            |
| Requests hit the wrong endpoint              | Set `VERCEL_AI_GATEWAY_BASE_URL` to override the base URL (for example, when routing through a proxy).                                                                              |
| Model routes to OpenRouter instead of Vercel | Some model IDs are shared with OpenRouter's catalog; set `provider = "vercel"` explicitly in `vtcode.toml` or pick the model from the `/model` picker.                              |
| Model not found                              | Double-check the `vendor/model` ID in the [Vercel AI Gateway docs](https://vercel.com/docs/ai-gateway). Unlisted gateway model IDs are accepted as long as the gateway serves them. |

See the [Vercel AI Gateway quick reference](./vercel-ai-gateway-quick-reference.md) for the shortest setup checklist.
