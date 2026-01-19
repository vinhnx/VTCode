# Prompt Caching Research (September 26, 2025)

This note captures verified prompt caching behavior across providers required by VT Code.

| Provider | Support Status | Minimum Prompt / Prefix | TTL & Expiration | Enablement Notes | References |
| --- | --- | --- | --- | --- | --- |
| OpenAI | Automatic for GPT-4o, GPT-4o mini, o1-preview/mini, and fine-tunes | 1,024 tokens (prefix cached in 128-token increments) | Cache cleared after 5–10 minutes idle; always removed within 1 hour | No additional parameters; cache usage reported via `prompt_tokens_details.cached_tokens` | [OpenAI API Prompt Caching](https://openai.com/index/api-prompt-caching/) |
| Anthropic Claude | Available via `cache_control` blocks; supports 5-minute & 1-hour caches with per-breakpoint TTL | 1,024 tokens for Opus/Sonnet tiers; 2,048 tokens for Haiku tiers | Per-breakpoint TTL: 1h for tools/system, 5m for messages (default); optional 1h for all | Up to 4 cache breakpoints; caches cover tools → system → messages hierarchy; supports mixed TTLs | [Anthropic Prompt Caching Guide](https://docs.anthropic.com/id/docs/build-with-claude/prompt-caching) |
| Google Gemini API | Implicit caching automatically enabled for Gemini 2.5 models; explicit caching via `client.caches` APIs | 1,024 tokens (2.5 Flash) / 4,096 tokens (2.5 Pro) minimum for implicit hits | Implicit caches live ~3–5 minutes; explicit caches default to 1 hour TTL (configurable) | Implicit requires no code changes; explicit caching needs cache creation & reuse identifiers | [Gemini API Context Caching](https://ai.google.dev/gemini-api/docs/caching/) |
| OpenRouter | Passes through provider caching and reports savings via `cache_discount` | Provider-specific (e.g., 1,024 tokens for OpenAI, 1,028 for Gemini Flash) | Tracks provider TTLs (e.g., 5-minute best effort) | Automatic where providers support; Anthropic/Gemini require `cache_control` in payloads | [OpenRouter Prompt Caching](https://openrouter.ai/docs/features/prompt-caching) |
| xAI (Grok) | Cached prompt tokens automatically enabled | Follows model limits (large contexts up to 2M tokens on Grok 4 Fast) | Managed by platform; cached usage visible in response `usage` | Toggle available in account settings; no per-request fields required | [xAI Models & Pricing – Cached Prompt Tokens](https://docs.x.ai/docs/models) |
| DeepSeek | Context caching on disk enabled by default for all requests | Cache stored in 64-token units; repeated prefix reused | Automatic eviction after hours/days when unused | Usage reports include `prompt_cache_hit_tokens` and `prompt_cache_miss_tokens` | [DeepSeek Context Caching Guide](https://api-docs.deepseek.com/guides/kv_cache) |

## Key Observations

- **Provider variance:** OpenAI, xAI, and DeepSeek provide automatic caching; Anthropic and OpenRouter-mediated Gemini require explicit cache control markers.
- **Minimum lengths:** All providers enforce lower bounds (≥1,024 tokens) before caching applies; Gemini Pro and Anthropic Haiku demand longer prefixes.
- **TTL flexibility:** Anthropic offers 5-minute and 1-hour options per breakpoint; Google explicit caches accept custom TTLs; other providers manage eviction internally.
- **Billing signals:** Each provider exposes cache metrics (`cached_tokens`, `cache_discount`, `prompt_cache_hit_tokens`, etc.) that VT Code should surface for observability.

## Anthropic Claude Implementation Details

VT Code implements enhanced Anthropic prompt caching with the following features:

### Per-Breakpoint TTL Control

VT Code supports **mixed TTL caching** for optimal cost/performance tradeoffs:

```toml
[prompt_cache]
enabled = true

[prompt_cache.providers.anthropic]
# TTL for tools and system prompts (typically stable content)
# Default: 3600 (1 hour) - recommended for stable tool definitions
tools_ttl_seconds = 3600

# TTL for messages (typically dynamic content)
# Default: 300 (5 minutes) - recommended for frequently changing content
messages_ttl_seconds = 300

# Maximum cache breakpoints (max 4 per Anthropic spec)
max_breakpoints = 4

# Apply cache control to different content types
cache_tool_definitions = true   # Cache tool definitions
cache_system_messages = true    # Cache system prompts
cache_user_messages = true      # Cache user messages
# Minimum message length before applying cache control
min_message_length_for_cache = 256  # ~64 tokens
```

### Cache Metrics Helpers

VT Code provides helper methods for cache performance analysis:

```rust
if let Some(usage) = &response.usage {
    // Cache hit rate as percentage (0-100)
    let hit_rate = usage.cache_hit_rate(); // e.g., Some(75.0)
    
    // Check if this was a cache hit
    let is_hit = usage.is_cache_hit(); // Some(true/false)
    
    // Total tokens from cache (read + creation)
    let total_cache = usage.total_cache_tokens();
    
    // Ratio of prompt tokens served from cache
    let savings = usage.cache_savings_ratio(); // e.g., Some(0.8) for 80% savings
}
```

### Recommended Configuration Patterns

**For Coding Agents** (stable tools, changing context):
```toml
[prompt_cache.providers.anthropic]
tools_ttl_seconds = 3600       # 1h for stable tools
messages_ttl_seconds = 300     # 5m for changing messages
cache_tool_definitions = true
cache_system_messages = true
cache_user_messages = true
min_message_length_for_cache = 512  # Only cache substantial messages
```

**For Long-Running Conversations**:
```toml
[prompt_cache.providers.anthropic]
tools_ttl_seconds = 3600       # 1h for tools
messages_ttl_seconds = 3600    # 1h for conversation history
cache_tool_definitions = true
cache_system_messages = true
cache_user_messages = true
min_message_length_for_cache = 128
```

## Implementation Status

The prompt caching research has been successfully implemented in VT Code with the following features:

- **Global configuration**: Controlled through `[prompt_cache]` section in `vtcode.toml`
- **Per-provider settings**: Individual configuration for OpenAI, Anthropic, Gemini, OpenRouter, xAI, and DeepSeek
- **Anthropic mixed TTL**: Per-breakpoint TTL control (1h for tools/system, 5m for messages by default)
- **Runtime integration**: Cache configuration flows through the provider factory to all LLM providers
- **Local caching**: File-based storage for optimized prompts with automatic cleanup
- **Usage tracking**: Enhanced usage metrics with cache-specific fields and helper methods
- **Cache analytics**: `cache_hit_rate()`, `is_cache_hit()`, `total_cache_tokens()`, `cache_savings_ratio()`

The implementation respects all per-provider capabilities identified in this research and provides a unified configuration interface for users.
