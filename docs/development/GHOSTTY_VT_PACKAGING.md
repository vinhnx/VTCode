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

At runtime, VT Code loads the Ghostty runtime library from one of these locations:

1. `<vtcode executable dir>/ghostty-vt/`
2. `<vtcode executable dir>/`

On macOS, VT Code looks for `libghostty-vt*.dylib`. On Linux, it looks for `libghostty-vt*.so*`.
If no compatible runtime library is available, VT Code falls back to `pty.emulation_backend = "legacy_vt100"`.

## Release Packaging

Use:

```bash
bash scripts/stage-ghostty-vt-assets.sh <target-triple> <release-dir>
```

The script stages runtime libraries into `<release-dir>/ghostty-vt/`.
Official macOS/Linux release archives are expected to include that directory by default.
Native installers copy the runtime libraries during release installs, but VT Code installation remains successful if they are missing or cannot be installed.

## Local Debug Runs

For local `./scripts/run.sh` and `./scripts/run-debug.sh` sessions, VT Code bootstraps and stages matching runtime libraries automatically. You can also pre-stage them manually with:

```bash
bash scripts/setup-ghostty-vt-dev.sh "$(rustc -vV | sed -n 's/^host: //p')"
```

`run.sh` and `run-debug.sh` now:

- download and build the pinned Ghostty VT assets on demand when they are missing
- stage the runtime libraries into `target/<profile>/ghostty-vt/`
- run with Ghostty by default and fall back to `legacy_vt100` if the runtime libraries are unavailable

Set `VTCODE_GHOSTTY_VT_AUTO_SETUP=0` to skip the automatic bootstrap step if you want to exercise the fallback path or avoid the download.

When Ghostty is active, PTY snapshot logs include `Ghostty VT snapshot rendered successfully`.
If setup or staging fails, VT Code logs a fallback warning and continues with `legacy_vt100`.
