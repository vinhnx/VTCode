# Web Search Tool

The `web_search` tool performs web searches and returns ranked results (title, URL, snippet) inline. The only built-in
provider is keyless DuckDuckGo HTML search, selected by default through `auto` or explicitly with `duckduckgo`:

- **`duckduckgo`** — keyless DuckDuckGo HTML endpoint, no API key required. Best-effort; may be rate-limited
  or anti-bot challenged.

DuckDuckGo does not publish a dependable numeric quota for this HTML endpoint. Its responses can depend on
request fingerprints and headers, so VTCode's limits are local safeguards rather than vendor guarantees: the
default 3-second cooldown, 5-minute result cache, 12-request session budget, and 1 MiB response-body cap.

## Usage

The tool accepts a `query` string and optional `max_results`:

```json
{
  "query": "Rust async performance tips"
}
```

Optional parameters:

| Field         | Type      | Default        | Description                                 |
| ------------- | --------- | -------------- | ------------------------------------------- |
| `query`       | `string`  | —              | Nonblank search query, up to 500 characters |
| `max_results` | `integer` | config default | Cap returned results (max 20)               |

## Output

```json
{
  "query": "Rust async performance tips",
  "provider": "duckduckgo",
  "count": 5,
  "cached": false,
  "results": [
    { "title": "...", "url": "https://...", "snippet": "..." }
  ]
}
```

The `provider` field reports the backend as `"duckduckgo"`.

### Errors and anti-bot responses

Admission and provider failures use a stable structured contract. For example, a cooldown response includes
`error_type: "cooldown"`, `retryable: true`, `retry_after_ms`, `provider`, and `next_action`; an exhausted
session includes `error_type: "session_cap"`, `retryable: false`, `request_budget_remaining: 0`, and
`next_action`.

HTTP 202, 403, or 429 responses, and HTML containing a challenge or CAPTCHA, are reported as
`error_type: "antiban_blocked"` with `retryable: false`. Do not retry those responses immediately from the
same network or session. Use a known result URL with `web_fetch` under its separate approval and domain rules,
or ask the user whether to configure another backend in a future provider extension. Ordinary timeout and 5xx
failures are marked retryable for at most one explicit retry after the supplied `next_action`; the tool does not
automatically retry DuckDuckGo requests.

## Configuration

Configure via `vtcode.toml` under `[tools.web_search]`:

```toml
[tools.web_search]
# Provider: "auto" (default; aliases the keyless DuckDuckGo backend), "duckduckgo"
# (keyless)
provider = "duckduckgo"

# Default results per call (hard cap: 20)
max_results = 5

# Request timeout in seconds (max: 60)
timeout_secs = 20

# Minimum gap between requests in milliseconds (default: 3000)
cooldown_ms = 3000

# How long results are cached in seconds (default: 300)
cache_ttl_secs = 300

# Session-wide request cap (default: 12)
session_max_requests = 12
```

Anti-bot challenges and non-success responses are reported as structured errors rather than cached as empty results.

## Guard Rails

- **Cooldown** — prevents hammering the endpoint (default 3s between requests)
- **Result cache** — identical queries served from memory (default 5min TTL, no network call)
- **Session cap** — limits total outbound requests per session (default 12)
- **Timeout** — per-request timeout (default 20s, max 60s)
- **Results cap** — max 20 results per call

The cooldown, cache, session budget, and body cap are VTCode policy controls; none should be interpreted as a
published DuckDuckGo quota.

## Related Tools

- `web_fetch` — fetch full page content from a specific URL; pass `format="markdown"` for defuddle-style
  cleaned-markdown extraction (consolidates the former `defuddle_fetch` tool)
