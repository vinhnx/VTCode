# vtcode-ghostty-vt-sys

[Root AGENTS.md](../AGENTS.md) | FFI bindings to Ghostty VT runtime for terminal snapshot rendering.

## Entry Point

`render_terminal_snapshot(GhosttyRenderRequest, &[u8]) -> Result<GhosttyRenderOutput>`

## Rules

- Loads `libghostty-vt.{dylib,so}` at runtime via `libloading::Library`.
- All `unsafe` FFI blocks must have `// SAFETY:` comments.
- `GhosttyApi` is a `OnceLock` singleton — load once, reuse forever.
- Platform support: macOS + Linux only. Other platforms return `unavailable_error()` (fallback to `legacy_vt100`).
- Runtime library search: `exe_dir/ghostty-vt/` then `exe_dir/`.

## Key Types

`GhosttyRenderRequest` (cols, rows, scrollback_lines) | `GhosttyRenderOutput` (screen_contents, scrollback)

## Gotchas

- `unavailable_error()` is `#[cold]` — signals fallback, not crash.
- Tests require `VTCODE_GHOSTTY_VT_TEST_ASSET_DIR` env var pointing to compiled library. Tests skip silently if unavailable.
- `sized()` helper writes struct size to first field — matches upstream `libghostty-rs` layout.
- `build.rs` + `ghostty-vt-manifest.toml` manage the packaged runtime binary.
