# Zed Agent Client Protocol Integration

VT Code adopts [ACP (Agent Client Protocol) by Zed](https://agentclientprotocol.com/). 

It took the reference from the official Zed implementations
([`zed-industries/claude-code-acp`](https://github.com/zed-industries/claude-code-acp),
[`cola-io/codex-acp`](https://github.com/cola-io/codex-acp)) and follows the
[Goose ACP client guidance](https://block.github.io/goose/docs/guides/acp-clients/). Use the steps
below to configure, launch, and validate the integration end to end.

## Setup overview

1. Build VT Code (release profile recommended for editor workflows).
2. Enable the ACP bridge in `vtcode.toml` or via environment overrides.
3. Wire the binary into Zed's `settings.json` under `agent_servers`.
4. Start an external agent session in Zed and confirm ACP logs report healthy traffic.

## Prerequisites

- Rust toolchain pinned by `rust-toolchain.toml`.
- VT Code configuration with provider, model, and credentials.
- Zed `v0.201` or later with the Agent Client Protocol feature flag enabled.
- An ACP client that advertises the `fs.read_text_file` capability so VT Code can proxy
  `read_file` requests. If the handshake omits it, the bridge keeps the tool disabled and reports a
  reasoning notice.

## Build VT Code

```bash
cargo build --release
```

Record the resulting binary path (`target/release/vtcode`) or add it to your `PATH`.

## Configure VT Code for ACP

Open your `vtcode.toml` (project-local copy or the default in the repo root) and enable the bridge:

```toml
[acp]
enabled = true

    [acp.zed]
    enabled = true
    transport = "stdio"
    workspace_trust = "full_auto"

        [acp.zed.tools]
        read_file = true
        list_files = true
```

Environment overrides provide the same control surface:

| Variable | Purpose |
| --- | --- |
| `VT_ACP_ENABLED` | Toggles the global ACP bridge. |
| `VT_ACP_ZED_ENABLED` | Enables the Zed transport. |
| `VT_ACP_ZED_TOOLS_READ_FILE_ENABLED` | Switches the `read_file` tool forwarding on or off. |
| `VT_ACP_ZED_TOOLS_LIST_FILES_ENABLED` | Controls whether the `list_files` bridge is available. |
| `VT_ACP_ZED_WORKSPACE_TRUST` | Forces the workspace trust mode (`full_auto` by default, `tools_policy` optional). |

When targeting models that cannot call tools (for example `openai/gpt-oss-20b:free` on OpenRouter),
disable the `read_file` bridge. VT Code emits reasoning notices and structured logs when it detects
models without function calling and automatically downgrades to plain completions.

## Manual smoke test

Run the bridge directly to ensure it starts cleanly:

```bash
./target/release/vtcode acp
```

Add `--config /absolute/path/to/vtcode.toml` if the configuration lives outside the default lookup
locations. You can also mirror Codex CLI behaviour with inline overrides such as
`--config agent.provider="openai"` when launching the bridge. Successful startup leaves the process
waiting on stdio; stop it with `Ctrl+C`.

## Register VT Code in Zed

Edit `settings.json` (Command Palette → `zed: open settings`) and add a custom agent entry:

```jsonc
{
    "agent_servers": {
        "vtcode": {
            "command": "/absolute/path/to/vtcode",
            "args": ["acp"],
            "env": {
                "VT_ACP_ENABLED": "1",
                "VT_ACP_ZED_ENABLED": "1",
                "RUST_LOG": "info"
            },
            "cwd": "/workspace/containing/vtcode"
        }
    }
}
```

- Rename the key from `vtcode` if you want a different label in Zed.
- Trim `command` to just `"vtcode"` when the binary is on `PATH`.
- Add CLI flags such as `--config` or `--log-level debug` to `args` if required.

## Use it inside Zed

1. Open the agent panel (`Cmd-?` on macOS) and choose **External Agent**.
2. Select the `vtcode` entry you added. Zed spawns VT Code and bridges ACP over stdio.
3. Chat normally. Mention files (`@src/lib.rs`) or attach buffers. When enabled, the `read_file`
   tool proxies to Zed's `fs.readTextFile` capability and streams results back into the turn, while
   `list_files` uses VT Code's workspace indexer for directory exploration.

## Package VT Code as a Zed Agent Server Extension

When you are ready to distribute VT Code to other Zed users, wrap the ACP bridge inside an Agent
Server Extension. Extensions bundle both metadata and platform-specific binaries so users can install
VT Code from Zed's marketplace without touching `settings.json`. This repository ships a ready-to-edit
manifest at `zed-extension/extension.toml`; customize it for each release and publish the directory as
the Zed extension package.

### Extension manifest layout

Add the VT Code agent definition under the `[agent_servers]` table in `extension.toml`. The copy in
`zed-extension/extension.toml` uses the latest published macOS artifacts as a baseline:

```toml
[agent_servers.vtcode]
name = "VT Code"
icon = "icons/vtcode.svg"            # Optional, 16x16 monochrome SVG recommended

[agent_servers.vtcode.env]
VT_ACP_ENABLED = "1"
VT_ACP_ZED_ENABLED = "1"

[agent_servers.vtcode.targets.darwin-aarch64]
archive = "https://github.com/vtcode-org/vtcode/releases/download/v1.2.0/vtcode-darwin-aarch64.tar.gz"
cmd = "./vtcode"
args = ["acp"]
sha256 = "replace-with-real-sha256"

[agent_servers.vtcode.targets.darwin-x86_64]
archive = "https://github.com/vtcode-org/vtcode/releases/download/v1.2.0/vtcode-darwin-x86_64.tar.gz"
cmd = "./vtcode"
args = ["acp"]
sha256 = "replace-with-real-sha256"

[agent_servers.vtcode.targets.linux-x86_64]
archive = "https://github.com/vtcode-org/vtcode/releases/download/v1.2.0/vtcode-linux-x86_64.tar.gz"
cmd = "./vtcode"
args = ["acp"]
sha256 = "replace-with-real-sha256"

[agent_servers.vtcode.targets.windows-x86_64]
archive = "https://github.com/vtcode-org/vtcode/releases/download/v1.2.0/vtcode-windows-x86_64.zip"
cmd = "./vtcode.exe"
args = ["acp"]
sha256 = "replace-with-real-sha256"
```

- `name` controls the label shown in Zed menus.
- Each `{os}-{arch}` target block supplies a download URL, the command to launch, and optional
  arguments. The example above reuses the `acp` entry-point so the extension behaves like the manual
  setup described earlier in this guide.
- The checked-in manifest currently declares macOS targets. Add Linux and Windows archives before
  publishing to cover additional platforms.
- Set `sha256` to the checksum of the published archive to harden supply-chain trust. Run
  `shasum -a 256 <archive>` on macOS/Linux or `certutil -hashfile <archive> SHA256` on Windows and
  paste the result.
- Provide an optional `[agent_servers.vtcode.env]` section when you need to carry configuration such
  as ACP toggles or provider credentials. Avoid hard-coding secrets; rely on Zed's environment
  overlays or documented setup steps instead.

### Building and publishing the archives

1. Produce release builds for every platform you intend to support (see `scripts/` for cross-compiling
   helpers). Bundle the artifacts as `.tar.gz` or `.zip` archives that include the `vtcode` binary at
   the root, plus any support files (for example `vtcode.toml.example`).
2. Create a GitHub release and upload each archive. Copy the asset URLs into `zed-extension/extension.toml`.
3. Generate SHA-256 hashes for all archives (or reuse the `dist/*.sha256` outputs) and update the manifest.
4. Commit the extension assets alongside `extension.toml`. Keep the directory structure stable so
   future updates can reuse the same icon and metadata.

### Local testing workflow

1. Use the Command Palette (`Cmd-Shift-P`) → `zed: install dev extension` to load the local
   workspace as an extension.
2. Open the Agent panel, pick the **VT Code** entry, and confirm the download succeeds on your
   platform.
3. Exercise ACP capabilities (tool calls, workspace prompts, cancellation) while watching Zed’s ACP
   logs to ensure the packaged binary behaves the same as your development build.
4. Repeat on every supported platform before publishing the extension to the marketplace.

### Keep protocol alignment

- Review the [ACP initialization contract](https://agentclientprotocol.com/protocol/initialization) when updating handshake fields so `agent_capabilities`, `agent_info`, and auth methods stay in sync with the spec. 
- Cross-check `NewSession` behaviour with Zed’s expectations outlined in the [session setup flow](https://agentclientprotocol.com/workflows/session/new) before changing session lifecycle code.
- Tool routing (for example `fs.readTextFile`) should continue to follow the [tools guidance](https://agentclientprotocol.com/protocol/tools) so capability negotiation and permission prompts remain interoperable.

## Runtime behaviour

- **Session management** – Each prompt owns a dedicated ACP session with history maintained in VT
  Code, mirroring the Claude and Codex bridges.
- **Context ingestion** – URIs such as `file://`, `zed://`, or `zed-fs://` resolve through Zed's
  `fs.readTextFile` capability, following Goose's recommended structure.
- **Embedded resources** – Inline text is wrapped in `<context>` blocks so the model can separate
  supporting material from primary instructions. Binary data is acknowledged but omitted from the
  prompt payload.
- **Streaming updates** – Token deltas and reasoning updates arrive via `session/update`
  notifications, keeping Zed's UI responsive during generation.
- **Plan tracking** – Every prompt emits an ACP plan describing analysis, optional context gathering,
  and final response drafting. VT Code updates each entry as it progresses so Zed can visualise the
  bridge's workflow in real time.
- **Tool execution** – The `read_file` tool forwards to Zed when enabled. The `list_files` tool
  uses VT Code's local workspace access, mirroring the CLI experience. When the model lacks
  function calling or the tool toggle is disabled, VT Code surfaces a reasoning notice and skips the
  invocation. Paths supplied by tools are normalised against the trusted workspace so relative
  segments stay inside the project before the request reaches the client.
- **Tool policy compatibility** – VT Code still advertises its core tool suite (for example
  `run_terminal_cmd`, `bash`, `grep_file`, `write_file`) through ACP when the model supports
  function calling. The bridge evaluates each request against the workspace's tool-policy settings
  before executing commands locally, ensuring shell access and editing tools behave the same as in
  the native CLI. Policy defaults and overrides defined under `[tools]` in `vtcode.toml` apply to
  ACP sessions just like the CLI.
- **Policy persistence** – Auto-approved tool prompts in ACP mode (for example shell execution in a
  non-interactive environment) are stored in the workspace policy file so subsequent runs reuse the
  remembered decision instead of prompting on every invocation.
- **Workspace trust** – On first launch the bridge records the workspace as fully trusted (matching
  the default `workspace_trust = "full_auto"`). Existing full auto entries are respected, and
  previously trusted workspaces aren't downgraded automatically.
- **Permission prompts** – The bridge requests explicit approval in Zed before each `read_file`
  invocation so you can confirm access to sensitive paths. If Zed cannot surface the prompt, the tool
  call is cancelled instead of executing without consent.
- **Cancellations** – When you stop a turn in Zed, VT Code stops streaming tokens, aborts pending
  tool execution with cancellation notices, and responds to the prompt with the ACP `cancelled`
  stop reason so no extra output appears after you abort the run.
- **Graceful degradation** – Unsupported payloads (images, binary blobs) emit structured
  placeholders rather than failing the prompt turn.

### Capability negotiation and safety

- VT Code inspects the Zed initialization payload before enabling each tool. When
  `fs.read_text_file` is absent, the bridge refuses to expose `read_file` and inserts a
  reasoning notice so transcripts document the downgrade.
- Every filesystem request is paired with a `session/request_permission` call so the user
  approves or rejects path access inside Zed. Denials and cancellations are surfaced as ACP
  tool updates rather than silent failures.
- Arguments are validated as absolute workspace paths prior to invoking the client method,
  preventing accidental traversal outside the project boundary.

### Telemetry and auditing

- Plan updates enumerate analysis, context gathering, and response drafting so audit trails
  show exactly how a turn progressed.
- Cancellation signals from Zed immediately cut off streaming, mark pending tool calls as
  cancelled, and end the turn with `StopReason::Cancelled`, providing a clean timeline in the
  transcript.
- Downgrades (such as models without tool calling) are emitted as explicit reasoning notices
  so reviewers can understand why a turn completed without filesystem access.

## Debugging and verification

| Symptom | Resolution |
| --- | --- |
| `Only the stdio transport is supported` | Ensure `transport = "stdio"` in `vtcode.toml`. |
| Empty responses in Zed | Confirm ACP env vars are present in the `env` map and that ACP is enabled in `vtcode.toml`. |
| `read_file` returns placeholders | Validate the referenced URI is accessible from Zed's workspace. |
| Tool calls report "Unsupported tool" | Disable the tool bridge or switch to a model that supports function calling. VT Code emits a reasoning notice when the downgrade occurs. |
| Sessions cancel unexpectedly | Inspect VT Code logs (and Zed's ACP logs) for cancellations triggered by the client. |

## Next steps

- Forward additional tools (for example MCP proxies) when the workspace requires editing or shell
  access directly from the editor.
- Advertise ACP command palettes once Zed surfaces richer UI affordances.
- File integration issues upstream so the bridge can track protocol or client changes.
