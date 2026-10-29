# Model Selection and Providers

_Part of [VT Code Configuration](./config.md)._

## Model selection

### agent.provider

The AI provider that VT Code should use.

```toml
[agent]
provider = "anthropic"  # available: openai, anthropic, google, meta, deepseek, copilot, openrouter, vercel, mimo, huggingface, zai, moonshot, minimax, mistral, qwen, stepfun, evolink, poolside, xai, nvidia, merge-gateway, ollama, lmstudio, llamacpp
default_model = "claude-sonnet-5"  # overrides the default model for the selected provider
```

### agent.provider_settings

This option lets you customize the settings for different AI providers.

For example, if you wanted to add custom API endpoints or settings for a provider:

```toml
[agent.provider_settings.openai]
name = "OpenAI"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
# Extra query params that need to be added to the URL
query_params = {}

[agent.provider_settings.anthropic]
name = "Anthropic"
base_url = "https://api.anthropic.com/v1"
env_key = "ANTHROPIC_API_KEY"

[agent.provider_settings.google]
name = "Google Gemini"
base_url = "https://generativelanguage.googleapis.com/v1beta"
env_key = "GOOGLE_GEMINI_API_KEY"
# Note: Google's API uses a different format
query_params = { key = "$GOOGLE_GEMINI_API_KEY" }

[agent.provider_settings.ollama]
name = "Ollama"
base_url = "http://localhost:11434/v1"
# No API key required for local Ollama instance

[agent.provider_settings.nvidia]
name = "NVIDIA NIM"
base_url = "https://integrate.api.nvidia.com/v1"
env_key = "NVIDIA_API_KEY"

[agent.provider_settings.merge-gateway]
name = "Merge Gateway"
base_url = "https://api-gateway.merge.dev/v1"
env_key = "MERGE_GATEWAY_API_KEY"
# Native Responses is the default; an explicit /v1/openai base URL keeps Chat Completions compatibility.
# Override the endpoint with MERGE_GATEWAY_BASE_URL when using a proxy.
# The default model is default_routing; explicit provider/model routes are also valid.

[agent.provider_settings.meta]
name = "Meta AI"
base_url = "https://api.meta.ai/v1"
env_key = "META_API_KEY"

[agent.provider_settings.vercel]
name = "Vercel AI Gateway"
base_url = "https://ai-gateway.vercel.sh/v1"
env_key = "AI_GATEWAY_API_KEY"
# OpenAI Chat Completions compatible; override the endpoint with VERCEL_AI_GATEWAY_BASE_URL.
```

Note this makes it possible to use VT Code with non-default models, so long as they are properly configured with the
correct API endpoints and authentication.

Or a third-party provider (using a distinct environment variable for the API key):

```toml
[agent.provider_settings.mistral]
name = "Mistral"
base_url = "https://api.mistral.ai/v1"
env_key = "MISTRAL_API_KEY"
```

It is also possible to configure a provider to include extra HTTP headers with a request. These can be hardcoded values
(`http_headers`) or values read from environment variables (`env_http_headers`):

```toml
[agent.provider_settings.example]
# name, base_url, ...
# This will add the HTTP header `X-Example-Header` with value `example-value`
# to each request to the model provider.
http_headers = { "X-Example-Header" = "example-value" }
# This will add the HTTP header `X-Example-Features` with the value of the
# `EXAMPLE_FEATURES` environment variable to each request to the model provider
# _if_ the environment variable is set and its value is non-empty.
env_http_headers = { "X-Example-Features" = "EXAMPLE_FEATURES" }
```

### Codex app-server sidecar

Use these settings when you want VT Code to launch the official Codex app-server locally.

```toml
[agent]
provider = "codex"
default_model = "gpt-5-codex"

[agent.codex_app_server]
command = "codex"
args = ["app-server"]
startup_timeout_secs = 10
experimental_features = false
```

- `command = "codex"` means the local `codex` CLI must be installed and available on `$PATH`.
- If your Codex binary lives elsewhere, set `command` to that executable path instead.
- `experimental_features = false` keeps experimental Codex app-server discovery and native `review/start` routing
  disabled unless you explicitly opt in.
- If the sidecar command is missing, VT Code disables the Codex runtime path early and falls back to another
  authenticated provider when available.
- In the interactive UI you can open this section directly with `/config codex` or `/config agent.codex_app_server`.

You can also enable the experimental Codex behavior for a single run:

```bash
vtcode --codex-experimental
```

### custom_providers

Use `custom_providers` for named OpenAI-compatible endpoints that are not one of VT Code's built-in providers. Each
entry has a stable `name`, a human-friendly `display_name`, a `base_url`, an optional `api_key_env`, a default `model`,
and an optional `context_window` in tokens. When omitted, the provider uses the default context window. This describes
the provider capability; the separate `context.max_context_tokens` setting can still impose a lower session budget.
Secure credentials are scoped by `(name, api_key_env)`; they are not shared with another configured endpoint that uses
the same API-key environment variable.

Custom provider definitions are trusted configuration. VT Code rejects a non-empty `custom_providers` value when its
winning value comes from a repository-controlled workspace or project layer, including command-backed `auth.command`
entries. Put custom providers in the system or user config, or select a config file explicitly with
`--config path/to/file.toml`.

New provider-level fields

```toml
[[custom_providers]]
name = "mycorp"
display_name = "MyCorporateName"
base_url = "https://llm.corp.example/v1"
api_key_env = "MYCORP_API_KEY"
model = "gpt-5.6-sol"
# context_window = 256000   # Optional context window size in tokens (provider capability)
# api_format = "auto"      # Optional provider-level API format hint: auto|openai-chat|openai-responses|anthropic-messages
```

Notes:

- `context_window` declares the provider's capability in tokens and drives the context size shown in the UI, compaction
  thresholds, and preflight token checks.
- `api_format` is a hint to VT Code about how this provider / endpoint expects model traffic. Accepted values are:
  `auto`, `openai-chat`, `openai-responses`, and `anthropic-messages`. When omitted VT Code preserves legacy behavior
  and will try to autodetect; an explicit value is honored and VT Code will not silently fallback to a different format.

Capability defaults and per-model profiles

Custom providers may expose a small, conservative set of capability defaults to use when model metadata is absent. These
are useful for gateways and aggregators that do not provide per-model descriptors. Set fields such as `supports_tools`,
`supports_vision`, `supports_structured_output`, or `supports_parallel_tool_calls` directly on the provider entry.

Providers and profiles can also pin sampling values. Available fields: `temperature` (0.0-2.0), `top_p` (0.0-1.0),
`top_k` (>= 0), `presence_penalty` / `frequency_penalty` (-2.0-2.0), `max_tokens` (> 0; overrides the agent loop's
built-in per-task limits), and `reasoning_effort`. Pinning `reasoning_effort` on a profile implies effort support for
that model.

For fine-grained overrides you can declare sparse per-model profiles. Profiles live in
`custom_providers.profiles."<model-id>"` and only modify runtime defaults for that specific model identifier. IMPORTANT:
profiles do not add or enable models in the picker — `model` / `models` remain the allowlist/default. A profile only
changes how VT Code treats an already-selected model at runtime (capabilities, context window, api_format, sampling
values, etc.).

Example per-model profile:

```toml
[custom_providers.profiles."corp-model"]
api_format = "openai-responses"
context_window = 131072
temperature = 0.2            # sampling temperature (0.0-2.0)
# top_p = 0.9                # nucleus sampling (0.0-1.0)
# top_k = 40                 # top-k cutoff (>= 0)
# presence_penalty = 0.1     # (-2.0-2.0)
# frequency_penalty = -0.5   # (-2.0-2.0)
# max_tokens = 8192          # overrides built-in per-task limits (800/2000)
reasoning_effort = "low"     # implies effort support for this model
supports_tools = true
supports_vision = false
supports_structured_output = true
supports_parallel_tool_calls = true
supports_context_caching = false
supports_prompt_cache_options = false # explicit opt-in for openai-responses cache TTL
supports_responses_compaction = true
supports_context_edits = false
```

Precedence and semantics

When determining a model's runtime shape VT Code applies values in the following order (highest wins):

1. per-model profile (`custom_providers.profiles."<model-id>"`)
2. provider-level defaults (fields on the `[[custom_providers]]` entry)
3. model metadata discovered from the provider (or autodetection)
4. conservative built-in fallback defaults

Sampling values resolve on the same chain, with one extra global layer beneath the provider: profile → provider default
→ `agent.temperature` / `agent.reasoning_effort` globals → built-in per-task limits (`max_tokens` only). Two built-in
behaviors sit above the profile chain: simple sub-tasks force `reasoning_effort = "minimal"` regardless of a profile
pin, and Anthropic Messages backends (native Anthropic/MiniMax, or custom profiles with
`api_format = "anthropic-messages"`) drop `temperature`, `top_p`, and `top_k` on every request. Anthropic API version
`2026-10-21` rejects those fields outright, even at their default values, so a pinned value never reaches the wire and
`agent.temperature` has no effect on Anthropic routes.

Additional rules:

- An explicit boolean `false` in any overriding layer is honored and prevents a higher-level implicit `true` from taking
  effect.
- Omitting `api_format` preserves legacy autodetection behavior; explicitly setting `api_format` to a value instructs VT
  Code to use this API shape and not silently fall back.
- Profiles do not make a model available in the picker — use `model` or `models` to control availability.
- Wire delivery depends on the backend's API format. The OpenAI Chat shape sends `temperature`, `top_p`, and both
  penalties; the OpenAI Responses shape sends them inside a nested `sampling_parameters` object that some compatible
  endpoints ignore, and currently does not emit `max_output_tokens` for non-native endpoints; `top_k` is accepted in
  configuration but not serialized for these shapes today (it applies only to backends whose own request builders expose
  it).
- Name-based OpenAI sampling gates apply to custom endpoints too, by bare model-name match: models named `gpt`,
  `gpt-5.2`, `gpt-5.4`, `gpt-5.5*` accept sampling only while reasoning effort resolves to `none` (values are silently
  omitted otherwise), and `gpt-5`/`gpt-5-mini`/`gpt-5-nano` never receive sampling parameters. Prefer neutral model IDs
  on custom gateways if you need pinned values on such names.

Store a custom provider key with the same explicit identity used by the configuration:

```bash
vtcode secret add mycorp --key-name MYCORP_API_KEY
vtcode secret status mycorp --key-name MYCORP_API_KEY
```

`api_key_env` defaults to a derived `NAME_API_KEY` value when omitted. Process environment variables and workspace
`.env` entries take precedence over secure storage. Unambiguous legacy provider-only entries are migrated only for a
provider's default key; use `--key-name` for every non-default profile. Ambiguous legacy entries require a new key.
Workspace `.env` values are read only for credential lookup and are not added to the process environment. Older
key-scoped secure-storage entries use an ambiguous account format and are not imported automatically; add those keys
again with `vtcode secret add`.

These entries are editable from `/config`, and they show up in the model picker using `display_name` so you can toggle
between multiple custom endpoints without losing track of the active one.

### providers (model list overrides)

Use `[providers.<name>]` to extend a built-in provider's model list with additional custom models. This is useful when
you want to add models to an existing provider (e.g., OpenCode Zen, OpenCode Go) without creating an entirely new custom
provider.

```toml
[providers.opencode-zen]
models = ["gpt-5.6-sol", "claude-sonnet-5", "glm-5.1"]
base_url = "https://custom-endpoint.example.com"   # optional
api_key_env = "MY_CUSTOM_KEY"                        # optional
```

Each `[providers.<name>]` section supports. Its `api_key_env` override also becomes the credential identity used by
model availability, the picker, and the runtime:

For the same trust-boundary reason, endpoint and credential overrides are accepted only from system/user configuration,
an explicitly selected config file, or explicit runtime flags. A repository-controlled workspace or project file cannot
set `base_url` or `api_key_env`; model-list-only overrides remain available there.

| Field         | Type       | Required | Description                                                      |
| ------------- | ---------- | -------- | ---------------------------------------------------------------- |
| `models`      | `string[]` | Yes      | List of model identifiers to add to the provider's model picker. |
| `base_url`    | `string`   | No       | Override the provider's default API endpoint.                    |
| `api_key_env` | `string`   | No       | Override the provider's default API key environment variable.    |

The provider key must match a built-in provider (e.g., `opencode-zen`, `opencode-go`, `openai`, `anthropic`). Custom
models appear in the `/model` picker alongside the provider's built-in entries.

#### Example: Adding fine-tuned models to OpenAI

```toml
[providers.openai]
models = ["ft:gpt-5.4:my-org:custom-model:v1"]
```

#### Example: Overriding OpenCode Zen endpoint

```toml
[providers.opencode-zen]
models = ["gpt-5.6-sol", "claude-sonnet-5"]
base_url = "https://my-proxy.example.com/v1"
api_key_env = "MY_PROXY_KEY"
```

### Provider whitelisting

Use `providers_whitelist` to restrict which providers VT Code may access. This is a governance control for environments
where only approved inference endpoints should be reachable — for example, a corporate gateway or an air-gapped setup.

```toml
# Allow only corporate gateways + Gemini
providers_whitelist = ["opencode-zen", "opencode-go", "gemini"]
```

When `providers_whitelist` is non-empty:

- The `/model` picker shows only the listed providers.
- The first-run wizard offers only the listed providers.
- The startup validator rejects `agent.provider` values not in the list.
- Saving a model selection that falls outside the list is blocked.

When `providers_whitelist` is empty (the default), all built-in providers and `[[custom_providers]]` entries are
available — this is the backward-compatible default.

Whitelist entries may be:

- A built-in provider key (`openai`, `anthropic`, `gemini`, `opencode-zen`, `opencode-go`, `ollama`, `lmstudio`,
  `llamacpp`, `copilot`, `deepseek`, `openrouter`, `moonshot`, `zai`, `minimax`, `mimo`, `mistral`, `huggingface`,
  `qwen`, `stepfun`, `evolink`, `poolside`).
- A `name` from a `[[custom_providers]]` entry.

Matching is case-insensitive.

### Model-specific settings

You can also configure model-specific behavior:

```toml
[agent.model_settings]
context_window = 128000    # Context window size in tokens
max_output_tokens = 4096   # Maximum tokens for model output
temperature = 0.7          # Model temperature (0.0-2.0)
top_p = 0.9                # Top-P sampling parameter
```

## Native Claude speed

`provider.anthropic.speed` accepts `"standard"` or `"fast"` (unset uses standard speed). `/fast [on|off]` and
`/model` save this preference independently of the OpenAI service tier. Fast mode applies only to the native
Claude API with Opus 5.5, 5, or explicitly selected 4.8; other routes keep the preference dormant. An explicit
request speed overrides configuration. Fast uses premium pricing, and switching speeds invalidates prompt-cache
reuse. See [fast mode](../development/fast-mode.md) for eligibility, endpoint precedence, and retry behavior.
