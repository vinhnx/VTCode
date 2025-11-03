# VT Code Zed Agent Server Extension

This directory packages VT Code as a Zed Agent Server Extension so users can install the binary directly from Zed's marketplace or as a local dev extension. It also ships lightweight VT-branded icon and color themes.

## Contents

-   `extension.toml` – Manifest that registers the VT Code agent server with Zed (top-level `schema_version`, `id`, `name`, `version`, and metadata fields).
-   `icons/` – Brand assets for the agent server (`vtcode.svg`) and icon theme (`vtcode-file.svg`, `vtcode-folder.svg`, `vtcode-folder-open.svg`, `vtcode-rust.svg`).
-   `icon_themes/` – `vtcode-icon-theme.json` registers the bundled icon theme so Zed can display VT-styled directory and Rust file icons.
-   `themes/` – `vtcode-theme.json` defines the VT-branded editor color theme.
-   `languages/` – `vt-trajectory/` configures syntax highlighting for VT trajectory logs (`trajectory.jsonl`, `sandbox.jsonl`).
-   `slash_commands` are registered in `extension.toml` (`/logs`, `/status`) with behavior implemented in `src/lib.rs`.
-   `context_servers` – `vtcode` MCP server entry launches the bundled VT Code binary in ACP mode.

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
4. Open **Settings → Appearance → Icons** and select **VT Code Icons** to validate the bundled icon theme renders correctly.
5. Open **Settings → Appearance → Themes** and select **VT Code Dark** to verify the color theme.
6. Open `.vtcode/logs/trajectory.jsonl` (or `sandbox.jsonl`) to confirm the VT Trajectory Log language is detected with JSON-derived highlighting.
7. In the Assistant, run `/logs` and `/status` to ensure the slash commands return VT-specific context.
8. Open the Agent panel, choose **VT Code** (context server) and confirm the binary starts successfully (requires `vtcode` binary on the PATH or configured via `VT_CODE_BINARY`).

After verification, push the manifest changes and publish the release so the extension can be listed publicly.

## Next Steps

-   When you add Linux or Windows builds, extend `extension.toml` with the appropriate target tables and rerun the release script so their checksums are captured automatically.
-   Re-run `zed: install dev extension` after each release to confirm download, checksum validation, and ACP negotiation succeed with the updated manifest.
