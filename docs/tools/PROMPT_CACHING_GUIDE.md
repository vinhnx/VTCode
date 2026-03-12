# Prompt Caching Guide

Prompt caching lets VT Code reuse validated conversation prefixes across providers to reduce latency and token consumption. This guide explains how to configure the feature globally and fine-tune the per-provider behaviour exposed in `vtcode.toml`.

## Global Settings

All prompt caching controls live under the `[prompt_cache]` section in `vtcode.toml`.

| Key                     | Type    | Description                                                                                               |
| ----------------------- | ------- | --------------------------------------------------------------------------------------------------------- |
| `enabled`               | bool    | Master switch for the caching subsystem. When disabled, per-provider overrides are ignored.               |
| `cache_dir`             | string  | Path (supports `~`) where cache entries are persisted. Relative paths resolve against the workspace root. |
| `max_entries`           | integer | Maximum entries persisted on disk before rotation.                                                        |
| `max_age_days`          | integer | Maximum age of an entry before automatic eviction.                                                        |
| `enable_auto_cleanup`   | bool    | If `true`, stale entries are purged during startup and shutdown.                                          |
| `min_quality_threshold` | float   | Minimum quality score a completion must meet before it is cached.                                         |
| `cache_friendly_prompt_shaping` | bool | Default-on prompt shaping that keeps volatile runtime context at the end of system prompts for better cache-prefix reuse. |

## Provider Overrides

Each provider exposes an override block under `[prompt_cache.providers]`. Overrides are only honoured when both the global `enabled` flag and the provider-level `enabled` flag are `true`.

### OpenAI

```toml
[prompt_cache.providers.openai]
enabled = true
min_prefix_tokens = 256
idle_expiration_seconds = 3600
surface_metrics = true
```

-   `min_prefix_tokens` — minimum number of prompt tokens before the API is asked to cache the prefix.
-   `idle_expiration_seconds` — how long (in seconds) a cached prefix can remain idle before expiry.
-   `surface_metrics` — when enabled, OpenAI usage responses expose cache-hit statistics surfaced through VT Code’s usage telemetry.
-   `prompt_cache_retention` — optional time duration to set the Responses API server-side cache retention for prefixes (e.g., "24h"). Increasing this value can improve cache hit rates and reduce costs/latency for repeated prompt patterns on OpenAI Responses models.
-   Default: `None` (opt-in) - VT Code does not set prompt_cache_retention by default; add it to `vtcode.toml` to enable it.
-   Valid formats: `<number>[s|m|h|d]` (e.g., `30s`, `5m`, `24h`, `7d`).
-   Valid range: minimum `1s`; maximum `30d`.
-   Example CLI override to enable 24h retention for Responses model:

    ```bash
    vtcode --model gpt-5 --config prompt_cache.providers.openai.prompt_cache_retention=24h ask "Explain this function"
    ```

-   To list all Response-API-enabled OpenAI models:

    ```bash
    vtcode models list --provider openai
    ```

-   Applies only to OpenAI models that use the Responses API; for other models this value is ignored.

## Prefix Stability Rules

Prompt caching on Responses-style providers only hits when the new request keeps an exact prefix match. In VT Code, the most common cache breakers are:

-   Changing `model`, `tools`, or sandbox/environment instruction blocks mid-session.
-   Reordering tools between requests.
-   Injecting new dynamic context above existing prompt items.

To reduce avoidable misses, VT Code keeps tool ordering deterministic and defers MCP `tools/list_changed` refreshes to turn boundaries so an active turn sees a stable tool catalog.
VT Code enables `prompt_cache.cache_friendly_prompt_shaping = true` by default. When it is enabled, VT Code applies provider-aware shaping:

- OpenAI, Gemini, DeepSeek, OpenRouter, Moonshot, Z.AI: move volatile counters to a trailing `[Runtime Context]` block.
- Anthropic and MiniMax: same trailing runtime block, plus Anthropic-format system prompt splitting so runtime context is sent as an uncached block.

OpenAI additionally keeps `prompt_cache_key` stable per session (unless `prompt_cache_key_mode = "off"`).

### Anthropic (Claude)

```toml
[prompt_cache.providers.anthropic]
enabled = true
tools_ttl_seconds = 3600
messages_ttl_seconds = 300
extended_ttl_seconds = 3600
max_breakpoints = 4
cache_system_messages = true
cache_user_messages = true
cache_tool_definitions = true
min_message_length_for_cache = 256
```

-   `tools_ttl_seconds` — TTL for tool definitions and system prompt cache hints.
-   `messages_ttl_seconds` — TTL for user message cache hints.
-   `extended_ttl_seconds` — optional longer-lived TTL. When present, VT Code automatically opts into Anthropic’s extended prompt caching beta header.
-   `max_breakpoints` — maximum number of cache insertion points per request (tools, system prompt, user messages).
-   `cache_system_messages` / `cache_user_messages` / `cache_tool_definitions` — toggle cache hints for each content type.
-   `min_message_length_for_cache` — avoids setting cache hints on very short user messages.

### Gemini

```toml
[prompt_cache.providers.gemini]
enabled = true
mode = "implicit"       # implicit | explicit | off
min_prefix_tokens = 128
explicit_ttl_seconds = 900
```

-   `mode` — `implicit` leverages built-in cache detection; `explicit` reserves cache slots for manual lifecycle management; `off` disables all Gemini caching.
-   `min_prefix_tokens` — minimum prompt size before requesting cache evaluation.
-   `explicit_ttl_seconds` — optional TTL when explicit mode is active.

### OpenRouter

```toml
[prompt_cache.providers.openrouter]
enabled = true
propagate_provider_capabilities = true
report_savings = true
```

-   `propagate_provider_capabilities` — pass provider cache instructions straight through to upstream models.
-   `report_savings` — surface cache-hit metrics returned by OpenRouter alongside standard usage data.

### Z.AI

```toml
[prompt_cache.providers.zai]
enabled = true
```

Z.AI handles caching server-side. When the override is enabled, VT Code honors upstream behavior and surfaces usage metrics when available.

## Usage Telemetry

When caching is active, `Usage` structs now include:

-   `cached_prompt_tokens` — tokens served from cache (OpenAI, OpenRouter).
-   `cache_creation_tokens` — tokens spent establishing a new cache entry (Anthropic, OpenRouter).
-   `cache_read_tokens` — tokens satisfied from an existing cache entry (Anthropic, OpenRouter).

These metrics flow through `vtcode-core::llm::types::Usage` and appear anywhere VT Code reports token accounting.

## Validation & Testing

-   Unit tests in `vtcode-core/src/llm/providers/anthropic.rs` validate cache control insertion and beta header composition.
-   `vtcode-core/src/llm/providers/openrouter.rs` exercises usage parsing to ensure cache metrics are preserved.
-   Local cache behavior tests in `vtcode-core/src/core/prompt_caching.rs` verify caching, eviction, and persistence.
-   Configuration loading tests ensure settings from `vtcode.toml` are applied correctly.
-   Run `cargo test` to execute all fast tests after updating configuration logic.

## Implementation Architecture

The prompt caching system is implemented as a multi-layered architecture:

1. **Global Configuration Layer**: Managed in `vtcode-config/src/core/prompt_cache.rs` with global and per-provider settings
2. **Provider Integration Layer**: Each provider has specific cache control implementation in `vtcode-core/src/llm/providers/`
3. **Local Caching Layer**: File-based caching engine in `vtcode-core/src/core/prompt_caching.rs` for optimized prompt storage
4. **Runtime Integration**: Cache configuration flows through the provider factory to ensure proper initialization

## Migration Guide

When upgrading to the new prompt caching system:

1. Add the `[prompt_cache]` section to your `vtcode.toml` if you want to customize caching behavior
2. Review provider-specific settings to optimize for your usage patterns
3. Monitor cache metrics to verify the system is performing as expected

## Troubleshooting

-   If caching isn't working as expected, verify that both global and provider-specific `enabled` flags are set to `true`
-   Check that your prompts meet the minimum token requirements for each provider
-   Enable verbose logging to see cache interaction details

By tuning these values you can balance latency, cost, and cache freshness per provider while keeping the behaviour consistent across the VT Code agent ecosystem.
