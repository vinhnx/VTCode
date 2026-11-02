# DuckDuckGo web search integration

The built-in `web_search` implementation lives in
`crates/codegen/vtcode-core/src/tools/web_search/mod.rs`. It uses the keyless
DuckDuckGo HTML endpoint and maps ranked result links into the stable
`title`/`url`/`snippet` contract. No search API key or interactive secret is
required. Full-page retrieval remains the separate `web_fetch` tool and still
uses its own approval, domain, and network-safety checks.

The tool facade owns arguments, session admission, cache state, and response
classification. Private `parser.rs` owns pure HTML extraction and result URL
validation; `transport.rs` owns HTTP dispatch, redirect policy, and bounded body
reading. Their tests live in matching private test modules. Both builtin
registration paths use `web_search_parameter_schema()` from the facade, and
`parse_duckduckgo_html` remains available through its existing public path.

The provider configuration accepts `auto` (the default compatibility alias)
and `duckduckgo`. Removed provider values fail during configuration loading
with the generic supported-values error instead of silently selecting another
backend. Both configuration values report the canonical runtime backend ID
`duckduckgo`; the network allowlist includes `html.duckduckgo.com` for the
HTML endpoint.

DuckDuckGo does not publish a dependable numeric quota for its HTML endpoint.
The 3-second cooldown, 5-minute successful-result cache, 12-request session
budget, and 1,000,000-byte response-body limit are conservative VTCode safeguards, not
claims about a vendor quota. They remain configurable and are scoped to the
tool instance; cache hits do not consume cooldown or session budget.

Each tool instance admits requests under one lock, checking the cooldown and
session cap before reserving the outbound attempt. Successful payloads,
including an explicit no-result response, are cached for the configured TTL;
cache hits consume neither cooldown nor session quota. There are no automatic
provider retries or fallbacks. HTTP 202, 403, and 429 responses, plus
challenge-shaped HTML, are returned as `antiban_blocked` errors with
`retryable: false` and are never cached. Timeout and 5xx failures carry
`retryable: true` for at most one explicit retry after `next_action`.

Cooldown and session-cap responses carry the canonical `provider`, stable
`error_type` (`cooldown` or `session_cap`), `retryable`, and `next_action`;
cooldowns also report a positive `retry_after_ms`, while a session cap reports
`request_budget_remaining: 0`. Agents should not retry an anti-bot response
immediately from the same network or session. They should use `web_fetch` for
a known result URL under its separate approval and domain rules, or ask the
user about a future alternate backend.

The response reader rejects bodies over 1,000,000 bytes before parsing. The streaming
HTML parser limits results to 20, truncates titles and snippets, and accepts
only HTTP(S) result URLs with a host. Parser, URL-validation, body-limit,
challenge, cache, cooldown, cap, and concurrent-admission behavior are covered
by the `vtcode-core` web-search tests.

See the [web search tool reference](../tools/web_search.md) for configuration
and agent-facing output details.
