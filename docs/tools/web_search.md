# Web search

`web_search` uses the Octen Search API to return ranked **excerpts**. It never requests full page content. Reading a
result with `web_fetch` remains subject to separate approval, domain restrictions, and network safety checks.
Changing the search provider does not resolve a fetch policy denial.

## Secure setup

Run `/secret add octen` in the interactive session and paste the key into the masked prompt. Keys are stored using
`agent.credential_storage_mode` (OS keyring or encrypted file), with verified read-back. No paid validation request
is sent when saving. Escape cancels without replacing the previous key; blank input is rejected.

Use `/secret status octen` to see availability and source, and `/secret delete octen` to remove the saved key.
A nonblank `OCTEN_API_KEY` environment override takes precedence over secure storage. Deleting a stored key does not
remove that override; the command explains when it still supplies credentials. Changes apply to the next uncached
search without restarting. Keys do not belong in `vtcode.toml`, command arguments, or conversation messages.

Missing keys produce a structured `setup_error` pointing to `/secret add octen` or `OCTEN_API_KEY`. Tool calls and
headless runs never open a credential prompt. Secure-storage errors are reported separately from missing keys.
Octen is a search service, separate from the LLM provider/model registry.

## Arguments and results

```json
{ "query": "Rust ownership reference", "max_results": 5 }
```

`query` must contain 1–500 characters after trimming. `max_results` defaults to 8 and is clamped to 1–20.

```json
{
  "query": "Rust ownership reference",
  "provider": "octen",
  "count": 1,
  "cached": false,
  "results": [
    { "title": "Ownership", "url": "https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html", "snippet": "An excerpt from the result highlight." }
  ]
}
```

Octen highlights become snippets (at most 400 characters plus an ellipsis); titles are limited to 200 characters
plus an ellipsis. Results preserve rank and reject non-HTTP(S), missing-host, or credential-bearing URLs.
Successful nonempty results are cached; empty results and failures are not. Malformed responses, API errors,
rejected account access, rate limits, timeouts, network failures, redirects, and oversized bodies are distinguished.
Upstream error bodies and transport details are omitted to protect credentials.

## Configuration and migration

```toml
[tools.web_search]
provider = "octen"
max_results = 8
timeout_secs = 20
cooldown_ms = 3000
cache_ttl_secs = 300
session_max_requests = 12
```

`auto` remains a compatibility alias for Octen. Explicit `duckduckgo` and `youcom` settings fail with migration
guidance: change the provider to `octen` and configure an Octen key. Legacy search integrations are removed;
historical user credentials are not deleted. If low-level registry setup cannot load the configuration, web tools
remain disabled even when approved; repair the configuration before starting a new registry.

Requests use the fixed `https://api.octen.ai/search` endpoint with `x-api-key`. Redirects are disabled and response
reads are bounded to 1 MB. Timeout is clamped to 1–60 seconds. There are no automatic retries or fallback providers.
Every outbound attempt, including failures and cancellations, consumes session quota; missing credentials and cache
hits consume none. Concurrent calls share atomic cooldown/cap admission. The existing cooldown, cache TTL, and cap
defaults remain unchanged.

## Cost and pilot rationale

On 2026-10-22, [Octen pricing](https://octen.ai/pricing) advertised a promotional $1 per 1,000 web searches. Pricing
can change; consult the current page. Vendor latency and search benchmark claims are selection context, not VT Code
validation or evidence of coding-query quality.

A small acceptance pilot uses at most five coding-documentation queries (Rust ownership, Tokio cancellation,
reqwest redirects, Serde enum compatibility, and Cargo locked builds). Record observed latency, retrieval of useful
primary sources, and failures. Run only with existing credentials; do not create accounts or purchase credits.
Without credentials, live quality and latency remain unverified. See the [API contract](https://docs.octen.ai/api-reference/search).
