# VT Code Zed Agent Server Extension

This directory packages VT Code as a Zed Agent Server Extension so users can install the binary directly from Zed's marketplace or as a local dev extension.

## Contents

- `extension.toml` – Manifest that registers the VT Code agent server with Zed.
- `icons/vtcode.svg` – Monochrome icon displayed in Zed's menus.

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

After verification, push the manifest changes and publish the release so the extension can be listed publicly.

## Next Steps

- When you add Linux or Windows builds, extend `extension.toml` with the appropriate target tables and rerun the release script so their checksums are captured automatically.
- Re-run `zed: install dev extension` after each release to confirm download, checksum validation, and ACP negotiation succeed with the updated manifest.
