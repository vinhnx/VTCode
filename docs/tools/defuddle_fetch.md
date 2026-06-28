# Defuddle Fetch Tool

The `defuddle_fetch` tool fetches a URL through the [defuddle.md](https://defuddle.md) markdown extraction service and returns cleaned markdown inline. Use this as a fallback when `web_fetch` returns a payload that is hard to parse (heavy JS, paywalled HTML, raw RSS, etc.).

## Usage

```json
{
  "url": "https://example.com/complex-page"
}
```

Optional parameters:

| Field | Type | Default | Description |
|---|---|---|---|
| `url` | `string` | — | URL to fetch and extract |
| `max_bytes` | `number` | 256 KB | Maximum response size |

## Output

```json
{
  "url": "https://example.com/complex-page",
  "markdown": "...",
  "bytes": 12345,
  "used_this_session": 1,
  "session_cap": 1
}
```

## Guard Rails

- **Session cap**: hard-capped at **one call per session** (the service is rate-limited)
- **Max bytes**: 256 KB response limit
- **Timeout**: 30s default, max 60s
- Returns a structured error when the cap is hit, directing the agent back to `web_fetch`

## Related Tools

- `web_fetch` — fetch full page content (unlimited calls, more robust)
- `web_search` — search the web and get ranked result snippets
