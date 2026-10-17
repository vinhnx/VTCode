# WebMCP Browser Bridge User Guide

VT Code ships a first-class, opt-in WebMCP browser bridge. It connects a browser editor to either the current VT Code
session or a bounded standalone workspace bridge:

The bridge is part of the main `vtcode` binary and the published `vtcode-webmcp` crate; it is a first-class feature. The
repository's `apps/webmcp` Vite project is the maintained WebMCP browser app used to exercise the bridge and the
browser's native WebMCP API.

```text
inspect → edit a draft → review a diff → propose → apply → check → revert
```

It has three modes:

| Mode               | Start                                    | Write boundary                       |
| ------------------ | ---------------------------------------- | ------------------------------------ |
| WebMCP app         | Open the app without pairing             | Page memory only                     |
| Headless connected | Pair `vtcode webmcp serve`               | Selected workspace; full-auto policy |
| Active connected   | `/webmcp pair <origin>` in `vtcode chat` | Session workspace; terminal policy   |

The browser never writes to the filesystem directly.

For a real agent turn, pairing has two sides:

1. **VT Code TUI:** run `/webmcp pair <origin>`. VT Code starts the active bridge and prints the WebSocket URL and
   one-time pairing code.
2. **Web app:** open **Settings → Connect or re-pair a VT Code bridge**, paste both values, and select **Pair with VT
   Code**.

The browser cannot start or approve the bridge. Keep the TUI running while the browser is connected.

`<origin>` is the exact origin of the page in the browser: scheme, hostname, and port, without the page path. The
maintained app has two published deployments:

| Deployment   | URL                                   | Origin for pairing                   |
| ------------ | ------------------------------------- | ------------------------------------ |
| ChatGPT Site | <https://vtcode.vinhnx.chatgpt.site/> | `https://vtcode.vinhnx.chatgpt.site` |
| GitHub Pages | <https://vinhnx.github.io/VTCode/>    | `https://vinhnx.github.io`           |

Do not use the `/VTCode/` path or a trailing slash in the pair command. The browser app derives this value from
`window.location.origin`, so local and custom deployments follow the same exact-origin rule.

Browser WebMCP is available only while the WebMCP app is open in a supported browser tab or webview. Chrome gates the
page API on origin isolation and the `tools` Permissions Policy; the WebMCP app reports the observed values in
`get_editor_state.webmcp_context` and keeps its normal fallback active when the API is unavailable. The authenticated VT
Code bridge is separate and can still be used for workspace operations when explicitly paired.

## Execution explanations

In an active connected session, enter `/explain --web` in the TUI to open the app's **Explanation** view. Complete the
pairing steps above if the browser is not connected. The view is available when the paired runtime advertises execution
explanations; a standalone workspace bridge or the unpaired fallback may not support it.

Select task or session scope to inspect the summary, timeline, recorded delegation, action/file relationships, plan
changes, verification, and recorded usage. Evidence controls show the recorded source; terminal navigation focuses
Transcript Review without submitting a prompt, executing a command, or editing a file. The current workspace diff is
shown separately from agent-attributed changes. Reconnects reload authoritative state, and missing evidence stays
visible.

Without an active bridge, `/explain --web` creates and opens an offline HTML report. To save one without opening it,
enter `/explain --export html`. The terminal prints its path under the session's `derived/explain/` directory. Offline
reports embed available evidence and work without pairing or a network connection; navigation stays within the report.
See [explanation usage](./commands.md#execution-explanations) for all command options and interpretation rules.

## Two integration paths

The browser client has two interfaces that are easy to confuse:

- **Browser WebMCP:** When the browser provides `document.modelContext`, the page registers eight bounded tools for
  browser/in-page agents. They can list and read visible files, search buffers, inspect editor state, open files, stage
  one exact edit in a clean browser draft, review a draft, and switch panels. Draft edits update browser memory only;
  they cannot approve or apply a filesystem change.
- **VT Code bridge:** The paired editor uses VT Code's authenticated custom WebSocket protocol. The browser sends
  workspace, patch, check, and turn requests; VT Code sends responses and runtime events back. This is the path for **VT
  CODE TURN** and terminal policy.

The WebMCP specification covers the first interface (`ModelContext` and browser-agent tool invocation), not the second.
A browser WebMCP agent cannot automatically call the Rust VT Code process, and VT Code cannot directly call
`document.modelContext.executeTool()` without a separate bridge extension. Keeping the interfaces separate preserves the
terminal approval boundary.

## Evaluate the browser tool surface

For a real browser-agent pass, use a supported WebMCP browser, enable `chrome://flags/#enable-webmcp-testing`, relaunch
Chrome, and open the deployed editor. The [Chrome WebMCP documentation](https://developer.chrome.com/docs/ai/webmcp)
links to the Model Context Tool Inspector for listing tools, executing JSON inputs, and inspecting structured output or
errors.

Use these prompts to cover direct selection, an output-dependent sequence, and a review journey:

1. “What files are available in this workspace?” → `list_project_files`.
2. “Find the file that defines the greeting and open it in the editor.” → `search_code`, then `open_file` with the path
   returned by the search.
3. “Change Hello to Hi in the greeting and prepare the draft for my review.” → `read_file`, `stage_text_edit` with the
   returned digest, then `review_draft`.
4. “Show me the current draft diff, then open the changes panel.” → `review_draft`, then `open_panel` with `changes`.

Check that long file/search results are explicitly truncated, untrusted file content is marked with
`untrustedContentHint`, `get_editor_state` reports the workflow and `webmcp_context`, page-only actions update the
editor, and no browser tool can approve, apply, or revert a filesystem change. The deterministic corpus and contract
checks live in `apps/webmcp/evals/webmcp-evals.ts` and run with `bun run test`. Model tool selection remains
probabilistic and needs the real browser-agent pass.

### Capture real Chrome or ChatGPT evidence

Open **Evidence** in the WebMCP app, select **Chrome WebMCP Tool Inspector** or **ChatGPT in-app browser**, and choose
**Start new run** before the external client begins. Use Chrome with the WebMCP testing flag (or a valid origin-trial
token), or open the deployed page in ChatGPT's in-app browser when that client exposes WebMCP. The recorder wraps the
registered callbacks, so each actual discovery and tool invocation is captured with bounded metadata, errors, elapsed
time, and a sanitized editor-state snapshot.

After the run, use **Copy JSON** or **Download JSON** and keep the export with the client tool-inspector screenshot or
screen recording. The export omits file contents, diffs, prompts, pairing codes, session tokens, and sensitive fields.
The selected client name is a human attestation; the JSON demonstrates that the page callbacks ran and should be
reviewed together with the client capture. **Run self-check** is a deterministic fallback test and is not a substitute
for this external-client evidence.

### Optional Chrome origin trial

Chrome offers WebMCP through a [time-limited origin trial](https://developer.chrome.com/blog/ai-webmcp-origin-trial). To
test the deployed WebMCP app without the testing flag, request a token for each exact origin you will use:
`https://vtcode.vinhnx.chatgpt.site` and/or `https://vinhnx.github.io`. For one artifact served at both sites, set the
repository Actions variable `WEBMCP_ORIGIN_TRIAL_TOKENS` with the tokens separated by commas or whitespace. The Pages
workflow passes it to the Vite build as `VITE_WEBMCP_ORIGIN_TRIAL_TOKENS`, which injects the tokens into the document
head before the application accesses `document.modelContext`.

The legacy singular `WEBMCP_ORIGIN_TRIAL_TOKEN` and `VITE_WEBMCP_ORIGIN_TRIAL_TOKEN` variables remain supported for a
single-origin build. Tokens are origin-specific; a production token does not enable the local Vite origin. The Pages
workflow deploys only GitHub Pages; a separately published ChatGPT Site artifact needs the equivalent build variables
from its own publisher.

No token is committed to the repository. If the variable is unset, use `chrome://flags/#enable-webmcp-testing` in a
supported Chrome version or use the normal WebMCP-unavailable fallback.

## Run the WebMCP app without a bridge

This is the quickest way to try the editor and does not require VT Code, Rust, an API key, or a network connection.

From the repository root:

```sh
cd apps/webmcp
./start.sh
```

The launcher installs browser dependencies if needed and starts the local server at `http://localhost:5173`. The header
should show **Fallback mode**. If your terminal is already in `apps/webmcp`, skip the `cd` command.

The launcher can also start a connected workflow. Use `--headless` for a workspace-only bridge or `--active` for an
interactive VT Code session:

```sh
./start.sh --headless --workspace /absolute/path/to/workspace
./start.sh --active --workspace /absolute/path/to/workspace
```

In active mode, enter `/webmcp pair http://localhost:5173` when the TUI is ready, then paste the printed URL and pairing
code into the browser. Use `--port 5174` consistently in the launcher, bridge origin, and pairing command if port `5173`
is already occupied. See the WebMCP app's [GUIDE.md](../../apps/webmcp/GUIDE.md) for the complete one-command workflow.

### Complete the fallback walkthrough

1. Select `src/config.js` in the workspace tree.
2. Change `WebMCP` to another value, such as `browser`.
3. Select **Review changes** or press `Cmd+S` on macOS / `Ctrl+S` on Windows and Linux.
4. Inspect the unified diff. The edit is still only a draft.
5. Select **Approve patch**, confirm the browser dialog, then select **Apply approved patch**.
6. Select **Run checks**. The fallback runs deterministic checks in the browser.
7. Select **Revert last change** if you want to restore the original project.

Fallback approvals and changes are deliberately simulated. The browser keeps the fallback project, drafts, open tabs,
and selected file across a page refresh. A new Vite app instance clears that browser state. Real bridge credentials are
never stored, so a refreshed page must be paired again before it can access a real workspace.

Other useful controls:

- **Reload** reads the selected file again from the current backend.
- **Discard draft** removes an unsubmitted edit from the selected file.
- **Filter files** narrows the hierarchical explorer by file name or path; `Cmd/Ctrl+K` focuses it. It does not load the
  whole project to search file contents.
- **Run self-check** performs an edit, review, approval, apply, check, and revert cycle. Start with a clean workspace
  and an open file.
- **Request turn** explains that fallback mode has no VT Code runtime; it does not call an LLM.

### Open a workspace from the web app

Select **Settings** in the header, open **Choose a workspace and setup mode**, and enter the absolute path to the
workspace. The settings dialog generates two safe, copyable choices:

1. **Active session · VT CODE TURN** starts `vtcode chat` for that workspace. After it opens, run
   `/webmcp pair <origin>` in the VT Code TUI.
2. **Workspace only** starts `vtcode webmcp serve` with the selected `--allowed-root`. This provides workspace
   inspection and proposals, but no agent turns.

The browser cannot launch a local process or grant filesystem access. Run the copied command in a terminal, keep that
process running, then select **Open pairing settings** and paste the URL and one-time code printed by VT Code.

### Manage settings

Use the header's **Settings** button for all setup and pairing controls. The dialog contains the browser origin,
workspace path, generated terminal commands, WebSocket URL, and one-time pairing code. The URL and workspace path are
remembered for this Vite app instance to make reconnecting easier; the pairing code and authenticated session token are
never stored.

After pairing, **Bridge status** is synchronized from the VT Code `status` response and heartbeat. It shows the
authenticated workspace, runtime mode, listener, pairing lease, and request limits. These are read-only values: the
active VT Code TUI and its `[webmcp]` configuration remain authoritative. To change origins, roots, policy, or the
active session, use the VT Code TUI and pair again with the newly printed values.

The WebMCP app uses an IDE-style layout rather than a page of file previews. The explorer keeps directories collapsed,
unopened files remain metadata-only, and the selected file is loaded into CodeMirror on demand. Open files appear as
tabs. The bottom panel keeps **TERMINAL**, **CHANGES**, and **VT CODE** output inside bounded scroll areas, so long
files and command output do not overflow the page.

## Connect to a real workspace

Use the local development server when pairing with a local bridge. This avoids the browser security restrictions that
commonly apply to a deployed HTTPS page connecting to a plain `ws://` endpoint.

### 1. Start the browser editor

In one terminal:

```sh
cd apps/webmcp
./start.sh
```

Open `http://localhost:5173` and leave this terminal running. The WebMCP app uses a strict port so it fails clearly if
`5173` is occupied instead of switching to an origin that does not match the bridge allowlist.

### 2. Start the bridge

In a second terminal, run the bridge from the VT Code repository root:

```sh
cargo run --locked -- webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /absolute/path/to/workspace
```

Replace `/absolute/path/to/workspace` with the project you want the browser to inspect. For an already-built source
checkout, use:

```sh
./target/debug/vtcode webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /absolute/path/to/workspace
```

The command above assumes the repository binary has already been built with `cargo build --locked`. A globally installed
binary may predate the WebMCP command; if it reports `unrecognized subcommand 'serve'`, use the `cargo run` command
above or build and run `./target/debug/vtcode` from the repository.

The server binds to loopback and normally chooses an available port. Copy the WebSocket URL and one-time pairing code
printed by the command. The URL will look like:

```text
ws://127.0.0.1:<port>/webmcp
```

The pairing code expires after five minutes and can be used once. After a successful pair, the browser sends a small
authenticated heartbeat and the session lease is refreshed while the page remains connected. Keep the bridge terminal
open while using the editor. Press `Ctrl+C` there to stop the server and revoke its in-memory sessions.

Use the URL and code printed by the same, currently running bridge process. Every restart may choose a different port
and creates a new one-time code. If the browser reports a WebSocket connection failure, it is usually using an old URL,
a stopped bridge, or a code from an earlier restart.

### 3. Pair the browser

1. Open **Settings** and expand **Connect or re-pair a VT Code bridge**.
2. Paste the printed WebSocket URL into **WebSocket URL**.
3. Paste the printed code into **One-time pairing code**.
4. Select **Pair with VT Code**.

The editor should change to **VT Code connected**, and the workspace tree will show files from the selected root instead
of the fallback files. The browser keeps the pairing code and session token only in memory.

The origin must match exactly. For example, `http://localhost:5173` and `http://127.0.0.1:5173` are different origins,
and a trailing slash is not accepted in the configured origin.

## Use the read-only remote MCP surface

`vtcode webmcp serve` can expose two MCP transports in addition to the browser WebSocket: modern Streamable HTTP at
`/mcp` and legacy HTTP+SSE at `/sse/`, with messages posted to `/messages/{session_id}`. This surface is opt-in and
offers only read-only `search` and `fetch` tools. It never adds a public file route, and citation URLs remain empty
unless you configure a citation prefix.

The listener still binds to loopback. Put a TLS-terminating proxy or identity provider in front of it. The proxy must
validate the external OAuth bearer, remove it, and inject an internal `Authorization: Bearer ...` value matching the
token stored in the environment variable configured by `proxy_token_env`. VT Code advertises the external authorization
server from `/.well-known/oauth-protected-resource`, but does not implement OAuth, token, or JWKS endpoints.

For an MCP-only server, browser origins are not required and `/webmcp` remains inaccessible. Configure it in
`vtcode.toml`:

```toml
[webmcp.remote_mcp]
enabled = true
public_url = "https://mcp.example.com/sse/"
authorization_server = "https://login.example.com"
proxy_token_env = "VTCODE_WEBMCP_MCP_PROXY_TOKEN"
allowed_origins = []
max_results = 20
max_scan_files = 256
max_scan_bytes = 16777216
session_ttl_secs = 300
```

Then export the internal token and start the loopback listener:

```sh
export VTCODE_WEBMCP_MCP_PROXY_TOKEN='proxy-injected-internal-token'
vtcode webmcp serve --mcp --allowed-root /absolute/path/to/workspace
```

The same settings can be supplied or overridden on the command line:

```sh
vtcode webmcp serve --mcp \
  --mcp-public-url https://mcp.example.com/sse/ \
  --mcp-authorization-server https://login.example.com \
  --mcp-proxy-token-env VTCODE_WEBMCP_MCP_PROXY_TOKEN \
  --mcp-citation-url-prefix https://mcp.example.com/citations/ \
  --allowed-root /absolute/path/to/workspace
```

The public and authorization-server URLs must be HTTPS. `search` and `fetch` are bounded by 20 results, 256 scanned
files, and 16 MiB of UTF-8 content by default. The canonical public URL is `/sse/` for OpenAI-compatible clients; `/mcp`
is the modern alias. See the [OpenAI MCP documentation](https://developers.openai.com/api/docs/mcp) for the
corresponding Responses API `allowed_tools` and approval settings.

## Edit, review, and apply in headless mode

The editing steps are the same as fallback mode:

1. Filter the explorer if needed, expand a directory, and open a file. Only the selected file is loaded into the editor.
2. Edit its draft buffer; a `*` marker identifies unsent changes.
3. Select **Review changes** or press `Cmd/Ctrl+S`, then inspect the diff in the bottom **CHANGES** panel.
4. Select **Request VT Code approval** to send the structured proposal with the base file digests.
5. Select **Apply after terminal approval** only after the backend has accepted the proposal.
6. Select **TERMINAL**, run checks, and inspect **CHECK OUTPUT**.
7. Use **Revert last change** when the change identity is still current.

The standalone `vtcode webmcp serve` path is headless. Its default policy allows inspection and proposal staging, but
rejects real mutation and check execution unless the corresponding tools are explicitly enabled by full-auto policy. It
does not display an interactive terminal approval prompt or execute agent turns. The editor reports this explicitly in
the prompt panel instead of showing a successful no-op.

For fallback mode, the prompt composer includes the browser-reviewed diff within the bridge prompt limit. In active
mode, the browser sends the staged `proposal_id`; VT Code revalidates the files and supplies its own bounded,
authoritative unified diff to the TUI. The handoff never applies the proposal automatically. The headless filesystem
adapter rejects the request with an **agent turns unavailable** message.

The TUI renders the authoritative diff with the same diff styling as tool output — added and removed rows carry the
add/delete tints, file headers and hunk headers are emphasized, and the model-facing wrapper tags and code fences are
hidden. The prompt sent to the agent keeps the exact tagged, fenced format. Nothing inside the diff is followed as an
instruction; it is untrusted file data.

## Send a draft to an active VT Code session

Use this workflow for the **VT CODE TURN** button. The browser bridge and interactive TUI must be started by the same VT
Code process.

After starting the browser as described above, start VT Code in the target workspace. Run this command from the VT Code
repository checkout:

```sh
cd /path/to/vtcode
cargo run --locked -- chat
```

For a different workspace, pass it explicitly:

```sh
cargo run --locked -- --workspace /absolute/path/to/workspace chat
```

In the VT Code TUI, enter:

```text
/webmcp pair http://localhost:5173
```

The TUI prints the values the browser needs:

```text
Active WebMCP bridge started.
WebSocket: ws://127.0.0.1:<port>/webmcp
Browser origin: <exact-browser-origin>
Pairing code: <one-time-code> (expires in <seconds> seconds)
In the WebMCP editor, open **Settings → Connect or re-pair a VT Code bridge** and paste the WebSocket URL and pairing code above.
```

In the browser, open **Settings → Connect or re-pair a VT Code bridge**, paste the WebSocket URL and pairing code into
the two fields, and select **Pair with VT Code**. Do not use the URL or code from a separate `vtcode webmcp serve`
process.

In the browser, review the draft, select **Stage for VT Code turn**, write the instruction, and select **Request VT Code
turn**. From the prompt composer, `Cmd/Ctrl+Enter` performs the same action. After staging an active-session proposal,
the editor switches to **VT CODE** and focuses the prompt composer. The prompt and server-authoritative diff arrive in
the active TUI as a normal agent turn. The browser diff remains a local review preview; the adapter revalidates the
proposal ID before enqueueing the handoff. Terminal tool permissions remain authoritative. The active bridge keeps
direct browser apply, check, and revert disabled; ask VT Code to perform those actions and reload the browser afterward.

### Optional: enable apply and checks for a disposable workspace

Only do this with a workspace you are comfortable changing automatically. Create a small Rust workspace so the built-in
connected check has a `Cargo.toml` and lockfile:

```sh
cargo new --bin /tmp/vtcode-webmcp-workspace
cd /tmp/vtcode-webmcp-workspace
```

Add this to `/tmp/vtcode-webmcp-workspace/vtcode.toml`:

```toml
[automation.full_auto]
enabled = true
allowed_tools = ["apply_patch", "exec_command"]
require_profile_ack = false
```

Start the server from that workspace with the full-auto flag:

```sh
vtcode --full-auto webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /tmp/vtcode-webmcp-workspace
```

For a source checkout, build or run the repository binary instead, for source checkout from the VT Code repository. Pass
`--workspace-dir` so VT Code loads the disposable workspace configuration:

```sh
cargo run --locked -- --workspace-dir /tmp/vtcode-webmcp-workspace --full-auto webmcp serve \
  --origin http://localhost:5173 \
  --allowed-root /tmp/vtcode-webmcp-workspace
```

In this mode `apply_patch` permits the real file mutation and `exec_command` permits the connected
`cargo check --locked` action. The browser confirmation is only a UI step; the server-side full-auto policy is the
authority.

## Drafts and conflicts

Draft buffers are separate from backend snapshots:

- A `*` beside a file means the browser has unsent edits.
- **Review changes** creates a diff but does not write a file.
- **Reload** refreshes a clean buffer from the backend.
- If a dirty file changed outside the editor, reload stops with an external-change conflict instead of overwriting the
  draft.
- Use **Discard draft** when the external version should win, then reload if necessary.
- A stale proposal is rejected when its base digest no longer matches the current file.
- Revert is also fail-closed if a file changed after the bridge applied it.

## Published deployments

Both public pages are static copies of the WebMCP app and start in fallback mode. The ChatGPT Site is the primary hosted
demonstration; GitHub Pages is a stable static fallback and repository reference. Neither page can access a local
workspace by itself. The production bridge is the Rust component shipped with the VT Code binary; it does not depend on
either hosting site.

### Pair the deployed page from the same machine

If either deployed page is open in a browser on the same machine as the active VT Code process, pair the active TUI with
the matching origin:

```text
/webmcp pair https://vtcode.vinhnx.chatgpt.site
/webmcp pair https://vinhnx.github.io
```

Configure both origins in `[webmcp].allowed_origins` if one listener should serve both pages. If a bridge is already
listening for another configured origin, the second `/webmcp pair <origin>` issues a new one-time code without revoking
the existing session. Use the replacement form only when you intend to revoke current sessions:

```text
/webmcp pair --replace https://vtcode.vinhnx.chatgpt.site
```

Paste the newest WebSocket URL and one-time code printed by that same TUI into **Settings → Connect or re-pair a VT Code
bridge**. A URL such as `ws://127.0.0.1:<port>/webmcp` is a loopback address, so it works only when the browser can
reach the same machine. The deployed page's `/VTCode/` path is not part of the origin allowlist value. For the ChatGPT
Site, the exact value is `https://vtcode.vinhnx.chatgpt.site`.

For a standalone workspace bridge, include the deployed origin in its explicit allowlist:

```sh
vtcode webmcp serve \
  --origin https://vtcode.vinhnx.chatgpt.site \
  --origin https://vinhnx.github.io \
  --allowed-root /absolute/path/to/workspace
```

If the page is open in a remote or sandboxed in-app browser, it may not be able to reach `127.0.0.1` on the machine
running VT Code. Use the local Vite page in the same browser, or put the loopback listener behind a TLS-terminating
reverse proxy and enter its `wss://` URL. For the standalone bridge, enable remote proxy mode with
`--allow-remote --public-url wss://<bridge-host>/webmcp`; direct non-loopback binding is rejected.

### Deployed-page origin mismatch

Starting the bridge with `http://localhost:5173` and then opening a deployed page does not work. The ChatGPT Site sends
`https://vtcode.vinhnx.chatgpt.site`; GitHub Pages sends `https://vinhnx.github.io`. The bridge rejects
`http://localhost:5173` as a different origin unless that is the page actually open. The browser may display this
rejection as the generic **VT Code WebSocket connection failed** message, while the editor remains in fallback mode.
Native browser WebMCP registration and the authenticated VT Code WebSocket bridge are separate; seeing browser tools
registered does not mean the bridge is paired.

## Troubleshooting

### “WebSocket connection failed”

Confirm that the bridge is still running and that the browser uses the exact WebSocket URL printed by VT Code, including
its port and `/webmcp` path. For active-session mode, the URL must come from the TUI where you ran `/webmcp pair`; a
standalone `webmcp serve` URL cannot receive agent turns.

For the ChatGPT Site, use `/webmcp pair https://vtcode.vinhnx.chatgpt.site`; for `https://vinhnx.github.io/VTCode/`, use
`/webmcp pair https://vinhnx.github.io`. Use `--replace` only when the active sessions should be revoked, then paste the
newly printed URL and code. Do not pair either deployed page with a bridge allowlisted only for `http://localhost:5173`.
If the browser is remote or sandboxed, a `ws://127.0.0.1:...` URL is also unreachable; use a reachable `wss://` proxy
endpoint instead.

### “WebMCP session is not authorized”

The browser lost its in-memory session token, the bridge was restarted, or the page was suspended longer than the
session lease. For an active session, run `/webmcp pair <exact-browser-origin>` again in the TUI. For a standalone
bridge, restart `vtcode webmcp serve`. Then paste the new URL and one-time code. An open page normally renews the lease
automatically.

### “Enter the WebMCP WebSocket URL and the terminal pairing code”

The browser form is missing one or both pairing values. Paste the URL into **WebSocket URL** and the code into
**One-time pairing code**. The code is not part of the URL.

### “Origin rejected”

Use the browser's actual origin in `--origin`. Check the hostname, port, scheme, and trailing slash. For the published
editor, use `https://vtcode.vinhnx.chatgpt.site` for the ChatGPT Site or `https://vinhnx.github.io` for GitHub Pages,
not the full `https://vinhnx.github.io/VTCode/` URL. `localhost` and `127.0.0.1` are not interchangeable.

### “Pairing code is invalid” or expired

Restart `vtcode webmcp serve` and enter the newly printed code. A code is one-time and is not reusable after a
successful pairing.

### The TUI prints only “Active WebMCP bridge started.”

Restart VT Code from the current source checkout, then run `/webmcp pair http://localhost:5173` again. A current build
prints the WebSocket URL, pairing code, and browser instruction immediately below the status line.

### Apply or checks are rejected

This is expected for the default headless policy. For a disposable workspace, enable `[automation.full_auto]`, include
`apply_patch` and/or `exec_command` in `allowed_tools`, and start the command with `--full-auto`. Check output uses
`cargo check --locked` in connected mode.

### A file shows an external-change conflict

Another process changed the file after the browser loaded it. Copy any needed draft text, choose **Discard draft**, and
reload the file before creating a new proposal.

## Developer commands

From `apps/webmcp`:

```sh
bun install --frozen-lockfile
bun run typecheck
bun run test
bun run build
```

The implementation details, protocol boundaries, configuration, and security model are documented in the
[WebMCP bridge development guide](../development/webmcp.md).
