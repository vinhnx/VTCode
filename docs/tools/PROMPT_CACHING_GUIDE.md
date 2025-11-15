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
-   `prompt_cache_retention` — optional time duration to set the Responses API server-side cache retention for prefixes (e.g., "24h"). Increasing this value can improve cache hit rates and reduce costs/latency for repeated prompt patterns on GPT-5-series.
-   `prompt_cache_retention` — optional time duration to set the Responses API server-side cache retention for prefixes (e.g., "24h"). Increasing this value can improve cache hit rates and reduce costs/latency for repeated prompt patterns on GPT-5-series.
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

### Anthropic (Claude)

```toml
[prompt_cache.providers.anthropic]
enabled = true
default_ttl_seconds = 600
extended_ttl_seconds = 3600
max_breakpoints = 6
cache_system_messages = true
cache_user_messages = true
```

-   `default_ttl_seconds` — TTL for ephemeral caches (values are emitted as `TTL"s"`).
-   `extended_ttl_seconds` — optional longer-lived TTL. When present, VT Code automatically opts into Anthropic’s extended prompt caching beta header.
-   `max_breakpoints` — maximum number of cache insertion points per request (tools, system prompt, user messages).
-   `cache_system_messages` / `cache_user_messages` — toggle cache hints for the respective message roles.

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

### xAI

```toml
[prompt_cache.providers.xai]
enabled = true
```

xAI handles caching server-side. When the override is enabled, VT Code honours the upstream behaviour and surfaces usage metrics when available.

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
-   Run `cargo nextest run` to execute all fast tests after updating configuration logic.

## Implementation Architecture

The prompt caching system is implemented as a multi-layered architecture:

1. **Global Configuration Layer**: Managed in `vtcode-core/src/config/core/prompt_cache.rs` with global and per-provider settings
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
