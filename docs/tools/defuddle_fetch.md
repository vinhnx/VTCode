# Defuddle Fetch (now `web_fetch` `format=markdown`)

> **Consolidated.** The standalone `defuddle_fetch` tool was merged into
> [`web_fetch`](./web_fetch.md). Clean-markdown extraction is now a fetch *mode*,
> not a separate tool, so the model has one fewer tool to choose between.

Fetch cleaned markdown for a URL by calling `web_fetch` with `format="markdown"`:

```json
{
  "url": "https://example.com/complex-page",
  "format": "markdown"
}
```

This routes through the [defuddle.md](https://defuddle.md) markdown extraction
service and returns the cleaned markdown inline. Use it as a fallback when
`web_fetch` (default `format="summary"`) returns a payload that is hard to parse
(heavy JS, paywalled HTML, raw RSS, etc.).

Optional parameters:

| Field | Type | Default | Description |
|---|---|---|---|
| `url` | `string` | — | URL to fetch and extract |
| `max_bytes` | `number` | 256 KB | Maximum response size |

## Guard Rails

- **Session cap**: hard-capped at **one call per session** (the service is rate-limited)
- **Max bytes**: 256 KB response limit
- **Timeout**: 30s default, max 60s
- Returns a structured error when the cap is hit, directing the agent back to `web_fetch`

## Related Tools

- `web_fetch` — fetch full page content (unlimited calls, more robust); `format="markdown"` gives defuddle extraction
- `web_search` — search the web and get ranked result snippets
