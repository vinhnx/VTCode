# VT Code Zed Agent Server Extension

This directory packages VT Code as a Zed Agent Server Extension so users can install the binary directly from Zed's marketplace or as a local dev extension. It also ships lightweight VT-branded icon and color themes.

## Quick Start

1. Install the extension from Zed's marketplace or load it as a development extension
2. Enable **VT Code** under _Settings → Agents_
3. Use the integrated slash commands to interact with VT Code from the Zed Assistant

## Required Settings & Configuration

### Binary Configuration

- **Binary selection**: By default, the extension uses the bundled VT Code executable, but you can override this by setting the `VT_CODE_BINARY` environment variable to point to a custom VT Code executable.
  - Example: `VT_CODE_BINARY=/path/to/your/vtcode/binary`

### Environment Variables

The extension automatically sets the following environment variables for proper MCP (Model Context Protocol) connectivity:
- `VT_ACP_ENABLED=1` - Enables ACP mode
- `VT_ACP_ZED_ENABLED=1` - Enables Zed-specific ACP features
- `VT_ACP_ZED_TOOLS_READ_FILE_ENABLED=1` - Enables file reading tools
- `VT_ACP_ZED_TOOLS_LIST_FILES_ENABLED=1` - Enables file listing tools

## Available Slash Commands

All slash commands are available from the Assistant input:

- `/logs` – Lists absolute paths to `trajectory.jsonl`, `sandbox.jsonl`, and `agent.log` so you can open them quickly.
- `/status` – Summarizes ACP enablement, the log directory, and the resolved launch binary.
- `/doctor` – Performs basic diagnostics (binary path, MCP connectivity, log directory health, context server status) to help troubleshoot issues.

## Contents

-   `extension.toml` – Manifest that registers the VT Code agent server with Zed (top-level `schema_version`, `id`, `name`, `version`, and metadata fields).
-   `icons/` – Brand assets for the agent server (`vtcode.svg`).
-   `languages/` – `vt-trajectory/` configures syntax highlighting for VT trajectory logs (`trajectory.jsonl`, `sandbox.jsonl`).
-   `slash_commands` are registered in `extension.toml` (`/logs`, `/status`, `/doctor`) with behavior implemented in `src/lib.rs`.
-   `context_servers` – `vtcode` MCP server entry launches the VT Code binary in ACP mode, preferring `VT_CODE_BINARY` and falling back to the packaged executable (`./vtcode`).

## Troubleshooting with /doctor

Use the `/doctor` slash command to diagnose common issues:

1. Run `/doctor` in the Zed Assistant
2. The command will check:
   - VT Code binary availability and location
   - Log directory status
   - Context server configuration
   - MCP connectivity status

## Updating for a New Release

1. Build and upload platform archives via `./scripts/release.sh` (or manually produce the `dist/` artifacts).
2. Update the `version` field in `extension.toml` to match the new tag.
3. Replace the `archive` URLs so they point at the freshly published GitHub release assets.
4. Run `./scripts/release.sh` to execute the automated release workflow. It rebuilds binaries, uploads release assets, and rewrites `extension.toml` with fresh SHA-256 checksums for every available target.
5. Commit the updated files and include them in the release PR.

## Local Testing

1. From Zed, run the Command Palette command `zed: install dev extension` and select this directory.
2. Choose **VT Code** from the Agent panel and confirm the download succeeds.
3. Exercise ACP features (tool calls, cancellations) to verify the packaged binary works as expected.
4. Open `.vtcode/logs/trajectory.jsonl` (or `sandbox.jsonl`) to confirm the VT Trajectory Log language is detected with JSON-derived highlighting.
5. In the Assistant, run `/logs`, `/status`, and `/doctor` to ensure all slash commands return VT-specific context.
6. Open the Agent panel, choose **VT Code** (context server) and confirm the binary starts successfully (set `VT_CODE_BINARY` or rely on the packaged `./vtcode` fallback).

After verification, push the manifest changes and publish the release so the extension can be listed publicly.

## Next Steps

-   When you add Linux or Windows builds, extend `extension.toml` with the appropriate target tables and rerun the release script so their checksums are captured automatically.
-   Re-run `zed: install dev extension` after each release to confirm download, checksum validation, and ACP negotiation succeed with the updated manifest.


## Troubleshooting Development Installation

If you encounter build errors when installing the development extension in Zed:

1. Make sure you have the correct Rust version installed (check rust-toolchain.toml):
   ```bash
   rustup show
   ```

2. Ensure the required WASM target is installed:
   ```bash
   rustup target add wasm32-wasip2
   ```

3. Build the extension for the WASM target:
   ```bash
   cd zed-extension
   cargo build --target wasm32-wasip2 --release
   ```

4. Verify the extension.wasm file exists and is up-to-date:
   ```bash
   ls -la extension.wasm
   ```

5. If needed, copy the built WASM file to the extension root:
   ```bash
   cp target/wasm32-wasip2/release/vtcode_zed_extension.wasm extension.wasm
   ```

6. Clean any cached build artifacts if you continue to have issues:
   ```bash
   cargo clean
   ```

