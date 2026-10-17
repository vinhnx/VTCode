# Merge Gateway Integration

Merge Gateway gives VT Code one native Responses API endpoint for routing
requests to Merge's supported model vendors. VT Code also retains the
OpenAI-compatible Chat Completions surface for explicitly configured legacy
`/v1/openai` endpoints.

## Setup

1. Create a Merge API key in the [Merge dashboard](https://dashboard.merge.dev/).
2. Export it before starting VT Code:

   ```bash
   export MERGE_GATEWAY_API_KEY="your-merge-api-key"
   ```

3. Select the provider in `vtcode.toml`:

   ```toml
   [agent]
   provider = "merge-gateway"
   default_model = "default_routing"
   api_key_env = "MERGE_GATEWAY_API_KEY"
   ```

The default endpoint is the native Merge API:

```text
https://api-gateway.merge.dev/v1
```

VT Code posts native requests to `/responses` and discovers authenticated model
metadata from `/models`. Set `MERGE_GATEWAY_BASE_URL` when a proxy is required.
To keep an existing OpenAI-compatible deployment, explicitly set the base URL
to a path ending in `/v1/openai`; that selects the legacy Chat Completions
transport and appends `/chat/completions`.

## Quick start

```bash
export MERGE_GATEWAY_API_KEY="your-merge-api-key"
vtcode --provider merge-gateway --model default_routing
```

Use an explicit route when you want to select a vendor model:

```bash
vtcode --provider merge-gateway --model anthropic/claude-opus-5
```

## Curated models

| Model ID | Context | Vision metadata | Notes |
| --- | ---: | :---: | --- |
| `default_routing` | 128k baseline | No | Merge chooses the route |
| `openai/gpt-5.5` | 1.1M | No | OpenAI route |
| `anthropic/claude-opus-5` | 1M | Yes | Anthropic route |
| `anthropic/claude-opus-5-5` | 1M | Yes | Anthropic route |
| `anthropic/claude-sonnet-5` | 1M | Yes | Anthropic route |
| `anthropic/claude-sonnet-5-5` | 1M | Yes | Anthropic route, adaptive thinking on by default, forced `tool_choice` rejected |
| `google/gemini-3.6-flash` | 1M | Yes | Google route |
| `google/gemini-3.7-flash` | 1M | Yes | Google route |
| `google/gemini-4-argon` | 1.05M | Yes | Google route, flagship reasoning model |
| `deepseek/deepseek-v4-pro-0813` | 1M | No | DeepSeek route |
| `deepseek/deepseek-v4-flash-0731` | 1M | No | DeepSeek route |
| `xai/grok-4.6` | 500k | No | xAI route |
| `qwen/qwen3.8-max` | 1M | Yes | Qwen route |
| `minimax/minimax-h3` | 131k | No | MiniMax route |
| `moonshot/kimi-k3` | 1M | Yes | Moonshot route |
| `thinkingmachines/inkling` | 1M | No | Thinking Machines route |
| `zai/glm-5.3-flash` | 1.31M | Yes | Z.AI route (320B/18B hybrid attention, native vision) |
| `zai/glm-5.3-flashx` | 1M | Yes | Z.AI high-speed route (Flash stack, up to 200 tok/s, native vision) |
| `openai/gpt-5.6-luna` | 1.1M | Yes | OpenAI route |
| `openai/gpt-5.6-sol` | 1.1M | Yes | OpenAI route |
| `openai/gpt-5.6-terra` | 1.1M | Yes | OpenAI route |
| `openai/gpt-6-astra` | 1.05M | Yes | OpenAI route |
| `openai/gpt-6-sol` | 1.05M | Yes | OpenAI route |
| `openai/gpt-6.1-sol` | 1.05M | Yes | OpenAI route |
| `openai/gpt-6-luna` | 1.05M | Yes | OpenAI route |
| `xiaomimimo/mimo-v2.6-pro` | 1M | Yes | Xiaomi MiMo route; reasoning controls omitted (gateway has no vendor serving reasoning jointly with tools) |
| `xiaomimimo/mimo-v2.6-flash` | 1M | Yes | Xiaomi MiMo route; reasoning controls omitted (gateway has no vendor serving reasoning jointly with tools) |
| `mistral/mistral-large-4-0` | 1M | Yes | Mistral route (Public Preview); reasoning controls omitted pending catalog confirmation |
| `meta/muse-spark-open` | 1M | Yes | Meta route (open-weight 816B-A42B flagship for coding/agentic work; `reasoning_effort` low/medium/high/xhigh) |

These are the models shown in VT Code's picker. Merge model IDs are not a
closed local allowlist: any valid explicit `provider/model` route can be used
with `provider = "merge-gateway"`, even when it is not in `docs/models.json`.

## Configuration examples

Provider settings can make the endpoint and credential identity explicit:

```toml
[agent.provider_settings.merge-gateway]
name = "Merge Gateway"
base_url = "https://api-gateway.merge.dev/v1"
env_key = "MERGE_GATEWAY_API_KEY"
```

The environment variables are:

| Variable | Purpose |
| --- | --- |
| `MERGE_GATEWAY_API_KEY` | Bearer token used for Merge Gateway requests |
| `MERGE_GATEWAY_BASE_URL` | Optional native `/v1` override; ending in `/v1/openai` selects legacy Chat Completions |

## Responses and catalog behavior

Native requests preserve text, images/documents, tool calls, tool results,
structured-output schemas, streaming, and the model selected by Merge. Tool
results are sent as top-level `tool_result` input items, matching Merge's
multi-turn contract.

### Streaming

VT Code's native `stream` and `stream_normalized` paths send
`POST /v1/responses` with `"stream": true`. Merge returns SSE-style `data:`
frames: native `response.stream` frames are cumulative snapshots,
`response.done` is the successful terminal frame, and `response.error` is an
in-band provider failure. VT Code converts cumulative snapshots into text and
tool deltas as they arrive. It retains the first snapshot until the next frame
confirms that the response is cumulative, preserving the ability to discard a
pre-output `fallback_restart`; a stream with only one snapshot necessarily
emits that snapshot at the terminal frame. Usage is emitted from the final
response snapshot.

The parser also accepts the frame kind from the JSON `object` field, which is
the native Merge form, and resets the buffered snapshot when Merge emits a
`fallback_restart` frame. See the [Merge streaming contract](https://docs.merge.dev/merge-gateway/streaming)
for the upstream response and error behavior.

When the Merge provider is selected and `MERGE_GATEWAY_API_KEY` is available,
VT Code fetches the authenticated `GET /v1/models` catalog with cursor
pagination. Catalog data is cached per provider and reused when refresh fails;
unknown explicit `provider/model` route IDs remain valid and use conservative
capabilities. Deprecated catalog routes are not added to the picker.

The supplied [`models/catalog/llms.txt`](https://docs.merge.dev/merge-gateway/models/catalog/llms.txt)
file is a documentation index, not a runtime model dataset. Runtime discovery
uses the authenticated `/v1/models` endpoint.

Merge's reasoning behavior is vendor- and route-specific. VT Code discovers each
route's reasoning capability from the authenticated `/v1/models` catalog and
applies the configured reasoning effort using the route's advertised control:
routes advertising a provider-native `reasoning_effort` (OpenAI, xAI, Moonshot,
Meta, Z.AI prefixes) receive a `reasoning_effort` string, while routes advertising a
Gateway-managed thinking budget (Anthropic, Gemini, DeepSeek, Qwen, MiniMax,
Thinking Machines prefixes) receive a top-level `thinking` block with a
`budget_tokens` value derived from the effort level and clamped below
`max_tokens`. Unclassified routes such as `default_routing` and unknown explicit
route IDs never receive reasoning controls. Merge routing metadata and billed
cost remain provider-side metadata; VT Code reports normalized token usage
through its existing response contract.

## Troubleshooting

- `401` or authentication errors: confirm `MERGE_GATEWAY_API_KEY` is set and
  points to a key created in Merge.
- `404` errors: verify the native base URL ends at `/v1`, or that an explicitly
  configured legacy base URL ends at `/v1/openai`; do not include
  `/responses` or `/chat/completions` in the configured base URL.
- No dynamic models appear: confirm the API key is available in
  `MERGE_GATEWAY_API_KEY`; VT Code keeps curated/static models available when
  catalog discovery is unavailable.
- A model is rejected by Merge: confirm the exact vendor-prefixed route ID in
  Merge's catalog. VT Code deliberately does not reject unknown Merge IDs
  locally.
- `capability_unavailable` (no vendor supports `streaming_tools`/`tools`):
  brand-new routes may have no vendor serving streaming tool use yet. VT Code
  retries streaming tool requests once without streaming; if the route has no
  tool vendor at all the request fails closed — use `default_routing` or
  another model until the route gains tool vendors. A proven no-tool-vendor
  verdict is cached per session so later turns fail fast without burning
  calls; tool-free requests always bypass the cache. A rejection naming
  `reasoning` alongside `tools` blames that combination rather than tools
  alone, so it is never cached as a no-tool-vendor verdict.
- `capability_unavailable` naming `reasoning` (e.g. `(['reasoning', 'tools'])`
  on `xiaomimimo/mimo-v2.6-*`): no vendor serves reasoning jointly with tools
  for the route. VT Code omits reasoning controls for such routes
  (`xiaomimimo/` is unclassified); explicitly configuring an effort for them
  blocks the turn up front ("Choose a supported reasoning effort...") instead
  of sending a doomed request.
- `service_tier` `422`/`400`: Merge only accepts `standard`/`flex`/`priority`,
  and serves `flex` solely on routes priced for it (GPT-5.4/5.5/5.6 and Gemini
  flash routes); `priority` is priced nowhere and always fails closed. VT Code
  maps OpenAI tiers onto this vocabulary, drops unmapped ones (e.g.
  `ultrafast`) with a warning, and retries a priced-out tier once without it
  (serving standard) instead of failing.
- No explicit `prompt_cache_breakpoint` markers: Merge's Responses-compatible
  routes reject the field with 400 `invalid_parameter`, so VT Code sends
  explicit cache breakpoints only to the native api.openai.com backend and
  never through Merge Gateway. Caching still applies via session-key routing
  and the upstream implicit latest-message breakpoint.
- Reasoning output is absent: Merge reasoning controls are route-specific and
  are not projected into the generic VT Code reasoning fields; the reasoning
  effort is still honored on reasoning-capable routes.

See the [Merge Gateway quick reference](./merge-gateway-quick-reference.md) for
the shortest setup checklist.
