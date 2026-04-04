# ACP Registry Release Checklist

This checklist covers the exact steps to publish VTCode assets that pass
`agentclientprotocol/registry` validation.

## Preconditions

1. Merge ACP auth-method changes into `main`.
2. Confirm `vtcode-acp/src/zed/agent/handlers.rs` returns `authMethods` in `initialize`.
3. Ensure GitHub CLI is authenticated with permissions: `repo`, `workflow`.

## 1) Build and publish a macOS asset (local)

Run from repo root:

```bash
cargo build --release -p vtcode
mkdir -p dist

tar -C target/release -czf dist/vtcode-<VERSION>-aarch64-apple-darwin.tar.gz vtcode
shasum -a 256 dist/vtcode-<VERSION>-aarch64-apple-darwin.tar.gz > dist/checksums.txt
cut -d' ' -f1 dist/checksums.txt > dist/vtcode-<VERSION>-aarch64-apple-darwin.sha256
```

Create or reuse the GitHub release and upload files:

```bash
gh release create <VERSION> \
  --repo vinhnx/VTCode \
  --target <COMMIT_SHA> \
  --title <VERSION> \
  --notes "Release <VERSION>"

gh release upload <VERSION> \
  --repo vinhnx/VTCode \
  dist/vtcode-<VERSION>-aarch64-apple-darwin.tar.gz \
  dist/vtcode-<VERSION>-aarch64-apple-darwin.sha256 \
  dist/checksums.txt \
  --clobber
```

## 2) Build Linux assets (GitHub Actions)

Trigger Linux build workflow for the same tag:

```bash
gh api -X POST repos/vinhnx/VTCode/actions/workflows/236250414/dispatches \
  -f ref=main \
  -f 'inputs[tag]=<VERSION>' \
  -f 'inputs[build_windows]=false'
```

Find latest run ID and watch it:

```bash
gh api repos/vinhnx/VTCode/actions/workflows/236250414/runs?per_page=1 \
  --jq '.workflow_runs[0] | {id,status,conclusion,html_url}'

gh run watch <RUN_ID> --repo vinhnx/VTCode --exit-status
```

Download artifacts and upload to the release:

```bash
mkdir -p /tmp/vtcode-linux-assets
cd /tmp/vtcode-linux-assets

gh run download <RUN_ID> --repo vinhnx/VTCode

gh release upload <VERSION> --repo vinhnx/VTCode \
  artifacts-x86_64-unknown-linux-gnu/vtcode-<VERSION>-x86_64-unknown-linux-gnu.tar.gz \
  artifacts-x86_64-unknown-linux-gnu/vtcode-<VERSION>-x86_64-unknown-linux-gnu.sha256 \
  artifacts-x86_64-unknown-linux-musl/vtcode-<VERSION>-x86_64-unknown-linux-musl.tar.gz \
  artifacts-x86_64-unknown-linux-musl/vtcode-<VERSION>-x86_64-unknown-linux-musl.sha256 \
  --clobber
```

## 3) (Optional) Build Windows asset

Trigger with Windows enabled:

```bash
gh api -X POST repos/vinhnx/VTCode/actions/workflows/236250414/dispatches \
  -f ref=main \
  -f 'inputs[tag]=<VERSION>' \
  -f 'inputs[build_windows]=true'
```

When finished, upload Windows zip and checksum from workflow artifacts to the same release.

## 4) Update ACP registry entry

Edit `agentclientprotocol/registry/vtcode/agent.json`:

- set `version` to `<VERSION>`
- update existing archive URLs to `<VERSION>`
- add new `binary` targets only for published assets

Example target block:

```json
"linux-x86_64": {
  "archive": "https://github.com/vinhnx/VTCode/releases/download/<VERSION>/vtcode-<VERSION>-x86_64-unknown-linux-gnu.tar.gz",
  "cmd": "./vtcode",
  "args": ["acp"],
  "env": {
    "VT_ACP_ENABLED": "1",
    "VT_ACP_ZED_ENABLED": "1"
  }
}
```

## 5) Validate registry requirements locally

Run in `agentclientprotocol/registry`:

```bash
uv run --with jsonschema .github/workflows/build_registry.py
python3 .github/workflows/verify_agents.py --agent vtcode --auth-check --clean
```

## 6) Open registry PR

```bash
git checkout -b add-vtcode-agent-<VERSION>
git add vtcode/agent.json vtcode/icon.svg
git commit -m "Update VTCode registry entry to <VERSION>"
git push -u <your-fork-remote> add-vtcode-agent-<VERSION>

gh pr create --repo agentclientprotocol/registry \
  --head <your-user>:add-vtcode-agent-<VERSION> \
  --base main \
  --title "Update VTCode agent entry to <VERSION>"
```
