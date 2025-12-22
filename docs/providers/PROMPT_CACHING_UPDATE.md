# Prompt Caching Implementation Update (September 26, 2025)

## Overview

VT Code now includes comprehensive prompt caching support across multiple LLM providers, enabling significant cost savings and reduced latency for repeated conversation patterns. This update introduces both global caching settings and provider-specific configurations.

## New Configuration Structure

The prompt caching functionality is controlled through the `[prompt_cache]` section in `vtcode.toml`:

```toml
[prompt_cache]
enabled = true
cache_dir = ".vtcode/cache/prompts"
max_entries = 1000
max_age_days = 30
enable_auto_cleanup = true
min_quality_threshold = 0.7

[prompt_cache.providers.openai]
enabled = true
min_prefix_tokens = 1024
idle_expiration_seconds = 3600
surface_metrics = true

[prompt_cache.providers.anthropic]
enabled = true
default_ttl_seconds = 300
extended_ttl_seconds = 3600
max_breakpoints = 4
cache_system_messages = true
cache_user_messages = true

[prompt_cache.providers.gemini]
enabled = true
mode = "implicit"       # implicit | explicit | off
min_prefix_tokens = 1024
explicit_ttl_seconds = 3600

[prompt_cache.providers.openrouter]
enabled = true
propagate_provider_capabilities = true
report_savings = true

[prompt_cache.providers.xai]
enabled = true

[prompt_cache.providers.deepseek]
enabled = true
surface_metrics = true
```

## Provider-Specific Implementations

### OpenAI
- Automatic caching for GPT-4o, GPT-4o mini, o1-preview/mini, and fine-tunes
- Caching applies to prompts of 1,024+ tokens
- Reports cache hits via `prompt_tokens_details.cached_tokens`
- No additional request parameters needed

### Anthropic (Claude)
- Explicit cache control via `cache_control` blocks
- Supports both 5-minute (ephemeral) and 1-hour (persistent) TTL options
- Up to 4 cache breakpoints per request
- Applies to tools, system messages, and user messages
- Requires beta headers: `prompt-caching-2024-07-31` and `extended-cache-ttl-2025-04-11`

### Google Gemini
- Implicit caching automatically enabled for 2.5 models
- Explicit caching available via `client.caches` APIs
- Minimum 1,024 tokens (2.5 Flash) / 4,096 tokens (2.5 Pro) for implicit hits
- Supports both implicit and explicit modes

### OpenRouter
- Pass-through provider caching with savings reporting
- Automatically propagates provider cache instructions
- Reports cache savings via `cache_discount`

### xAI (Grok)
- Automatic platform-level caching
- Cache behavior managed server-side
- Usage metrics reported in response

### DeepSeek
- Context caching on disk enabled by default
- Cache stored in 64-token units
- Reports cache hit/miss metrics

## Usage Metrics

The new implementation includes enhanced usage metrics:

```rust
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_prompt_tokens: Option<u32>,      // Tokens served from cache
    pub cache_creation_tokens: Option<u32>,     // Tokens spent creating cache entry
    pub cache_read_tokens: Option<u32>,         // Tokens read from cache
}
```

## Implementation Details

### Core Caching Engine
- Local caching system to store optimized prompts
- Quality threshold to determine which responses to cache
- Automatic cleanup of expired entries
- Configurable cache directory with home directory expansion

### Integration Points
- CLI commands: `vtcode ask` and interactive mode
- Provider factory with cache configuration injection
- LLM request/response pipeline with cache-aware processing
- Router component with cache configuration support

## Configuration Hierarchy

The prompt caching system follows this configuration hierarchy:

1. Global `enabled` flag (master switch)
2. Provider-specific `enabled` flag
3. Provider-specific settings (TTL, breakpoints, etc.)

Caching is only active when both the global and provider-specific flags are enabled.

## Performance Benefits

- Reduced API costs through repeated prompt caching
- Lower response latency for cached content
- Improved token efficiency across conversation threads
- Automatic cache invalidation prevents stale content issues

## Testing and Validation

The implementation includes comprehensive unit tests for:
- Cache control insertion for Anthropic
- Usage parsing for cache metrics
- Configuration loading and validation
- Cache lifecycle management

To run the tests:
```bash
cargo test
```

## Migration Considerations

- Existing configurations will use default prompt caching settings
- Users may need to update their `vtcode.toml` to take advantage of provider-specific settings
- Cache metrics will now be included in usage reports

## Security and Privacy

- Local cache entries are stored in user's home directory by default
- Cache entries include content but no API keys or sensitive information
- Automatic cleanup prevents unbounded storage growth

## Troubleshooting

- If cache performance is poor, adjust the `min_quality_threshold` setting
- For debugging, verify the cache directory is writable and not blocked by security software
- Check provider-specific token requirements are met for caching to activate
- Monitor cache statistics using VT Code's built-in metrics