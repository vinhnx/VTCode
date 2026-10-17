# Vercel AI Gateway Quick Reference

| Setting           | Value                                                                       |
| ----------------- | --------------------------------------------------------------------------- |
| Provider          | `vercel` (aliases: `vercel-ai-gateway`, `ai-gateway`)                       |
| API key           | `AI_GATEWAY_API_KEY`                                                        |
| Default endpoint  | `https://ai-gateway.vercel.sh/v1`                                           |
| Endpoint override | `VERCEL_AI_GATEWAY_BASE_URL`                                                |
| Default model     | `anthropic/claude-sonnet-5`                                                 |
| Context           | 1,000,000 tokens (Claude Sonnet 5)                                          |
| Compaction        | Native `/responses/compact` for `openai/*` routes; local fallback otherwise |
| Official docs     | [vercel.com/docs/ai-gateway](https://vercel.com/docs/ai-gateway)            |

```bash
export AI_GATEWAY_API_KEY="..."
vtcode --provider vercel --model anthropic/claude-sonnet-5 ask "Summarize this repository"
```

One key unlocks hundreds of models from OpenAI, Anthropic, Google, DeepSeek, Moonshot, Alibaba, MiniMax, Mistral, and
others with zero token markup and automatic failover. Model IDs use the gateway's native `vendor/model` format (e.g.
`openai/gpt-5.6-sol`); unlisted gateway model IDs are accepted.

Popular curated models:

- `anthropic/claude-sonnet-5` — default, 1M context
- `openai/gpt-5.6-sol` — 1.05M context
- `google/gemini-3.8-flash` — 1M context
- `deepseek/deepseek-v4-flash` — 1M context
- `moonshotai/kimi-k3` — 1M context

Note: some IDs (e.g. `anthropic/claude-sonnet-5`, `moonshotai/kimi-k3`) also exist in OpenRouter's catalog — set
`provider = "vercel"` explicitly when configuring the model string. Selecting from the `/model` picker always routes to
Vercel.

See the [full Vercel AI Gateway provider guide](./vercel-ai-gateway.md).
