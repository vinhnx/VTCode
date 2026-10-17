# WebMCP deployment reference

VT Code's maintained WebMCP browser app is published at two URLs. Both pages use the same browser tool surface and start
in safe in-memory fallback mode; the ChatGPT Site is the primary hosted demonstration, while GitHub Pages is a static
fallback and reference deployment.

| Deployment   | URL                                   | Exact browser origin                 | Active pairing command                            |
| ------------ | ------------------------------------- | ------------------------------------ | ------------------------------------------------- |
| ChatGPT Site | <https://vtcode.vinhnx.chatgpt.site/> | `https://vtcode.vinhnx.chatgpt.site` | `/webmcp pair https://vtcode.vinhnx.chatgpt.site` |
| GitHub Pages | <https://vinhnx.github.io/VTCode/>    | `https://vinhnx.github.io`           | `/webmcp pair https://vinhnx.github.io`           |

The page derives its origin from `window.location.origin`. The GitHub Pages `/VTCode/` path is a URL path, not part of
the origin; do not include it in a pairing command or bridge allowlist. The same rule applies to ports on local or
custom deployments.

## Select a deployment

Use the [ChatGPT Site](https://vtcode.vinhnx.chatgpt.site/) when demonstrating the browser WebMCP experience. Use
[GitHub Pages](https://vinhnx.github.io/VTCode/) when a stable static fallback or repository-hosted reference is more
useful. Neither page hosts the Rust WebSocket bridge. Without an explicit bridge pairing, the editor remains page-local
and cannot read or write a local workspace.

Browser WebMCP and the VT Code bridge are separate interfaces:

- Browser WebMCP registers eight bounded, page-scoped tools through `document.modelContext` when the browser context
  supports it.
- The authenticated `/webmcp` WebSocket connects the page to VT Code for workspace reads, reviewed proposals, runtime
  events, and active agent turns.

The page can demonstrate the first interface without pairing. A real `VT CODE TURN` requires the second interface and
the same VT Code TUI session that owns terminal approval.

## Pair either published page

Configure both exact production origins if the same active TUI should serve both sites:

```toml
[webmcp]
allowed_origins = [
  "https://vtcode.vinhnx.chatgpt.site",
  "https://vinhnx.github.io",
]
```

Then run the command matching the open page:

```text
/webmcp pair https://vtcode.vinhnx.chatgpt.site
/webmcp pair https://vinhnx.github.io
```

When a bridge is already running, pairing another configured origin issues a new one-time code and keeps existing
authenticated sessions active. The pending code is still single-use. Use `--replace` only when intentionally revoking
current browser sessions:

```text
/webmcp pair --replace https://vtcode.vinhnx.chatgpt.site
```

Paste the newest WebSocket URL and code into **Settings → Connect or re-pair a VT Code bridge**. A loopback
`ws://127.0.0.1:...` endpoint is reachable only from a browser that can reach the same machine. A remote or sandboxed
browser needs a reachable `wss://` endpoint behind a TLS-terminating reverse proxy.

For a workspace-only headless bridge, repeat `--origin` for the exact origins that should be accepted:

```sh
vtcode webmcp serve \
  --origin https://vtcode.vinhnx.chatgpt.site \
  --origin https://vinhnx.github.io \
  --allowed-root /absolute/path/to/workspace
```

The headless adapter does not execute active VT Code agent turns. Use an active TUI pairing when the browser must send
`VT CODE TURN` prompts.

## Origin trial and browser support

Chrome origin-trial tokens are bound to exact origins. For a build served at both public URLs, provide one token for
each origin through `VITE_WEBMCP_ORIGIN_TRIAL_TOKENS`, separated by commas or whitespace. The legacy singular
`VITE_WEBMCP_ORIGIN_TRIAL_TOKEN` remains supported for a single-origin build. The GitHub Pages workflow reads the
corresponding `WEBMCP_ORIGIN_TRIAL_TOKENS` Actions variable and injects the tokens into the document head at build time;
no token belongs in source control. If the ChatGPT Site is built or uploaded separately, pass the same build variables
to that publisher as well; the Pages workflow does not deploy the ChatGPT Site.

If no token is available, use a supported browser's WebMCP testing flag or the normal fallback. WebMCP availability also
depends on the page's origin-isolated browsing context and `tools` Permissions Policy. A successful HTTP response or an
active VT Code pairing does not, by itself, prove that native browser WebMCP is available.

## Verification checklist

For either deployment:

1. Open the page directly and confirm the expected URL and fallback editor.
2. In a supported WebMCP browser, inspect the page and confirm the eight registered tools, bounded results, and
   `get_editor_state.webmcp_context`.
3. Start **Evidence** before a real inspector or ChatGPT in-app-browser run; keep the sanitized JSON beside the
   screenshot or recording.
4. For workspace access, pair using the exact origin table above and paste the URL/code printed by the same TUI.
5. Confirm browser edits remain proposals and that approval, apply, checks, and revert stay behind the VT Code terminal
   or configured headless policy.

The implementation details and test commands are in the [WebMCP development guide](../development/webmcp.md), the
[browser bridge user guide](../user-guide/webmcp.md), and the [WebMCP app guide](../../apps/webmcp/GUIDE.md).
