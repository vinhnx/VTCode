# Web Search Tool

The `web_search` tool performs web searches and returns ranked results (title, URL, snippet) inline. Two providers are
supported, selected in `vtcode.toml`:

- **`duckduckgo`** (default) — keyless DuckDuckGo HTML endpoint, no API key required. Best-effort; may be rate-limited
  or anti-bot challenged.
- **`youcom`** — the [You.com Search API](https://you.com/docs). Opt-in; requires a `YDC_API_KEY` environment variable.

## Usage

The tool accepts a `query` string and optional `max_results`:

```json
{
  "query": "Rust async performance tips"
}
```

Optional parameters:

| Field         | Type     | Default        | Description                                    |
| ------------- | -------- | -------------- | ---------------------------------------------- |
| `query`       | `string` | —              | Search query (also accepts `pattern` as alias) |
| `max_results` | `number` | config default | Cap returned results (max 20)                  |

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

The `provider` field reports which backend served the query (`"duckduckgo"` or `"youcom"`).

## Configuration

Configure via `vtcode.toml` under `[tools.web_search]`:

```toml
[tools.web_search]
# Provider: "auto" (default; aliases the keyless DuckDuckGo backend), "duckduckgo"
# (keyless), or "youcom" (requires YDC_API_KEY)
provider = "duckduckgo"

# Default results per call (hard cap: 20)
max_results = 5

# Request timeout in seconds (max: 60)
timeout_secs = 15

# Minimum gap between requests in milliseconds (default: 3000)
cooldown_ms = 3000

# How long results are cached in seconds (default: 300)
cache_ttl_secs = 300

# Session-wide request cap (default: 12)
session_max_requests = 12
```

### You.com provider

Set `provider = "youcom"` to route `web_search` through the You.com Search API (`POST https://ydc-index.io/v1/search`,
the search service host used by the official You.com SDKs):

```toml
[tools.web_search]
provider = "youcom"
```

Then export your API key (get one at [you.com/platform/api-keys](https://you.com/platform/api-keys)):

```bash
export YDC_API_KEY="***"
```

The key is read at request time and sent as the `X-API-Key` header; it is never written to logs or error messages. The
request is sent to the fixed `https://ydc-index.io/v1/search` endpoint with redirects disabled, so the `X-API-Key`
header can never be forwarded to a different origin or scheme by an upstream redirect; a 3xx response is surfaced as a
structured error instead of followed. If the key is missing or rejected, the tool returns a structured error telling the
agent how to fix the setup — no silent fallback, no crash. A missing key is reported before any network activity, so it
does not consume the session request cap or trigger the cooldown.

Result shape is identical to the DuckDuckGo provider, so downstream tool usage (`web_fetch` on a promising URL) works
the same either way.

## Guard Rails

- **Cooldown** — prevents hammering the endpoint (default 3s between requests)
- **Result cache** — identical queries served from memory (default 5min TTL, no network call)
- **Session cap** — limits total outbound requests per session (default 12)
- **Timeout** — per-request timeout (default 15s, max 60s)
- **Results cap** — max 20 results per call

## Related Tools

- `web_fetch` — fetch full page content from a specific URL; pass `format="markdown"` for defuddle-style
  cleaned-markdown extraction (consolidates the former `defuddle_fetch` tool)
