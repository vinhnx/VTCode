# Zed Agent Client Protocol Integration

This guide explains how VT Code exposes the Agent Client Protocol (ACP) bridge for the Zed
editor. The bridge follows the reference implementations in
[`zed-industries/claude-code-acp`](https://github.com/zed-industries/claude-code-acp) and
[`cola-io/codex-acp`](https://github.com/cola-io/codex-acp), along with the ACP client guidance
from [Goose](https://block.github.io/goose/docs/guides/acp-clients/).

## Prerequisites

- VT Code built from source or downloaded from a release that includes the ACP module.
- Zed `v0.168` or newer with the Agent Client Protocol beta enabled.
- A valid model and API key configured in `vtcode.toml`.

## Configuration

1. Open `vtcode.toml` (or `vtcode.toml.example`) and enable the ACP bridge:

   ```toml
   [acp]
   enabled = true

   [acp.zed]
   enabled = true
   transport = "stdio"
   ```

   Environment variables override these settings at runtime:

   | Variable             | Description                          |
   | -------------------- | ------------------------------------ |
   | `VT_ACP_ENABLED`     | Enables or disables the ACP bridge.  |
   | `VT_ACP_ZED_ENABLED` | Controls the Zed-specific transport. |
   | `VT_ACP_ZED_TOOLS_READ_FILE_ENABLED` | Enables the `read_file` tool bridge for Zed. |

   Disable the tool bridge with `[acp.zed.tools] read_file = false` (or by exporting
   `VT_ACP_ZED_TOOLS_READ_FILE_ENABLED=0`) when the configured model cannot invoke tools. VT Code emits a
   reasoning notice and log entry when it detects models such as `openai/gpt-oss-20b:free` that lack
   function calling on OpenRouter.

2. Launch VT Code with the new subcommand:

   ```bash
   vtcode acp --target zed
   ```

3. In Zed, add a new Agent connection pointing at the VT Code binary. Use the `stdio` transport
   and leave the command arguments empty. Zed will manage the lifecycle of the VT Code process.

## Runtime behaviour

- **Session management** – Each prompt creates a dedicated ACP session with history stored inside
  the VT Code agent, mirroring the approach used by the Claude and Codex bridges.
- **Context ingestion** – Linked resources with `file://`, `zed://`, or `zed-fs://` URIs are
  resolved through Zed's `fs.readTextFile` capability, keeping the prompt text aligned with the
  Goose ACP client guidelines.
- **Embedded resources** – Inline text resources are wrapped in `<context>` blocks so models can
  differentiate between primary instructions and supporting context. Binary resources are noted
  but omitted from the language model input.
- **Streaming updates** – Token and reasoning deltas are streamed via `session/update`
  notifications, providing incremental feedback during generation.
- **Tool execution** – VT Code exposes a `read_file` function call that forwards to Zed's
  `fs.readTextFile` capability, enabling models with tool support to request file contents during a
  turn. When `[acp.zed.tools].read_file` is disabled or the selected model lacks function calling
  (for example, `openai/gpt-oss-20b:free` on OpenRouter), the bridge downgrades automatically and
  streams a reasoning notice explaining why tool execution was skipped.
- **Graceful degradation** – Unsupported content types (images, audio, binary resources) emit
  structured placeholders rather than failing the prompt turn, matching the behaviour in the
  reference implementations.

## Troubleshooting

| Symptom                                   | Resolution |
| ----------------------------------------- | ---------- |
| `Only the stdio transport is supported`   | Confirm `transport = "stdio"` in the config. |
| Zed shows empty responses                 | Set both env vars or enable ACP in the config file. |
| File links resolve to placeholders        | Confirm the URI is reachable and VT Code can read the workspace. |
| Tool calls report "Unsupported tool" or "Client connection unavailable" | Confirm the selected model supports function calling. VT Code now downgrades automatically and emits a reasoning notice when it detects models such as `openai/gpt-oss-20b:free` that cannot call tools over OpenRouter. |
| Prompt turns cancel unexpectedly          | Check the VT Code logs for cancellations triggered by the Zed client. |

## Next steps

- Extend the bridge with MCP tool forwarding when the workspace requires filesystem editing or
  terminal execution.
- Advertise session modes and commands once ACP clients expose richer UI affordances.
- Share feedback or issues in the VT Code repository so the integration can track upstream ACP
  improvements.
