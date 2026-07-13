# Web Search Tool

The `web_search` tool performs web searches and returns ranked results (title, URL, snippet) inline. It uses DuckDuckGo's HTML endpoint keylessly — no API key required.

## Usage

The tool accepts a `query` string and optional `max_results`:

```json
{
  "query": "Rust async performance tips"
}
```

Optional parameters:

| Field | Type | Default | Description |
|---|---|---|---|
| `query` | `string` | — | Search query (also accepts `pattern` as alias) |
| `max_results` | `number` | config default | Cap returned results (max 20) |

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

## Configuration

Configure via `vtcode.toml` under `[tools.web_search]`:

```toml
[tools.web_search]
# Provider (currently only "duckduckgo" is supported)
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

## Guard Rails

- **Cooldown** — prevents hammering the endpoint (default 3s between requests)
- **Result cache** — identical queries served from memory (default 5min TTL, no network call)
- **Session cap** — limits total outbound requests per session (default 12)
- **Timeout** — per-request timeout (default 15s, max 60s)
- **Results cap** — max 20 results per call

## Related Tools

- `web_fetch` — fetch full page content from a specific URL; pass `format="markdown"` for defuddle-style cleaned-markdown extraction (consolidates the former `defuddle_fetch` tool)
