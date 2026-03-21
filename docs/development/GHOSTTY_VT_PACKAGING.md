# Ghostty VT Packaging

VT Code does not need the full Ghostty source tree in Git.

## Layout

Provide prebuilt Ghostty VT assets in one of these locations:

- `dist/ghostty-vt/<target-triple>/`
- `$VTCODE_GHOSTTY_VT_ASSET_DIR`

Each target directory must contain:

- `include/ghostty/vt.h`
- `lib/`

The `lib/` directory must contain the platform runtime library:

- Linux: `libghostty-vt*.so*`
- macOS: `libghostty-vt*.dylib`
- Windows: `*.dll`

## Runtime

At runtime, VT Code searches for the Ghostty helper in this order:

1. `$VTCODE_GHOSTTY_VT_HOST`
2. `$VTCODE_GHOSTTY_VT_DIR/ghostty_vt_host`
3. `<vtcode executable dir>/ghostty-vt/ghostty_vt_host`
4. `<vtcode executable dir>/ghostty_vt_host`
5. build-time helper path for local development builds

If no helper is available, VT Code falls back to `pty.emulation_backend = "legacy_vt100"`.

## Release Packaging

Use:

```bash
bash scripts/stage-ghostty-vt-assets.sh <target-triple> <release-dir>
```

The script stages sidecar assets into `<release-dir>/ghostty-vt/`.
Release archives include that directory when it exists.
Installers copy the sidecar when present, but VT Code installation remains successful if the sidecar is missing or cannot be installed.

## Local Debug Runs

For local `./scripts/run-debug.sh` sessions, you can bootstrap and stage a matching sidecar with:

```bash
VTCODE_GHOSTTY_VT_AUTO_SETUP=1 ./scripts/run-debug.sh
```

`run-debug.sh` now:

- downloads and builds the pinned Ghostty VT assets on demand when `VTCODE_GHOSTTY_VT_AUTO_SETUP=1`
- stages the sidecar into `target/debug/ghostty-vt/`
- exports `VTCODE_GHOSTTY_VT_DIR` when the sidecar is available

When Ghostty is active, PTY snapshot logs include `Ghostty VT snapshot rendered successfully`.
If setup or staging fails, VT Code logs a fallback warning and continues with `legacy_vt100`.
