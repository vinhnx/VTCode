# Model Profiles and Provider APIs

Model profiles and provider API format

This short development note describes the TOML shape used by VT Code to express provider-level API format hints,
capability defaults, and sparse per-model profiles. It is intended for maintainers and integrators; end-users should
prefer the higher-level guidance in the Configuration guide.

Provider-level fields (in `[[custom_providers]]`)

- api_format = "auto" | "openai-chat" | "openai-responses" | "anthropic-messages"
  - Optional hint that describes the provider's preferred API shape.
  - `auto` or omitted: preserve legacy autodetection and existing metadata flows.
  - Explicit value: instructs VT Code to use the specified API shape. VT Code will not silently fallback to another
    format when an explicit value is given.

- context_window = `<integer tokens>` (optional)
  - Provider capability in tokens. Drives UI context sizing, compaction thresholds, and preflight token checks.
  - When omitted VT Code uses the provider/model default (commonly 128000 for custom OpenAI-compatible endpoints unless
    otherwise discovered).

- `supports_tools`, `supports_reasoning`, `supports_reasoning_effort`, `supports_vision`, `supports_structured_output`,
  `supports_parallel_tool_calls`, `supports_context_caching`, `supports_responses_compaction`, and
  `supports_context_edits` (optional booleans)
  - Provider-level conservative defaults applied when per-model metadata is unavailable.

Per-model profiles (sparse overrides)

Profiles live under a top-level table keyed by the literal path `custom_providers.profiles."<model-id>"`. Each profile
is intentionally sparse — list only the overrides you need.

Example:

[custom_providers.profiles."gpt-5.6-sol"] api_format = "openai-responses" context_window = 131072 supports_tools = true
supports_vision = false supports_structured_output = true supports_parallel_tool_calls = true

Notes and semantics

- model / models remain the allowlist/defaults used to control the `/model` picker and what models are available.
  Profiles do NOT make a model available; they only modify runtime defaults for a model identifier that is already
  selectable.
- Precedence (highest wins): profile > provider defaults > model metadata / autodetect > conservative fallback.
- Explicit boolean `false` is honored and may override an implicit `true` from a lower-precedence layer.
- Omitting `api_format` preserves legacy behavior. Setting `api_format` explicitly instructs VT Code to treat the model
  with that API shape; it does not cause silent fallbacks.

Keep examples small and conservative: prefer to declare only the fields you need to correct autodetection or to provide
conservative capability signals for gateways that omit detailed model descriptors.
