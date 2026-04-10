# vtcode-ghostty-vt-sys

Safe Ghostty VT snapshot wrapper for VT Code.

Uses `libloading` to dynamically load the native Ghostty VT library at runtime, providing a safe Rust interface for rendering terminal snapshots. Supported on **Linux** and **macOS**; other platforms return an unavailable error.

## Usage

```rust
use vtcode_ghostty_vt_sys::{GhosttyRenderRequest, render_terminal_snapshot};

let request = GhosttyRenderRequest {
    cols: 80,
    rows: 24,
    scrollback_lines: 1000,
};

let output = render_terminal_snapshot(request, b"hello world\r\nline two")?;
println!("screen: {}", output.screen_contents);
println!("scrollback: {}", output.scrollback);
```

## API Reference

- **`GhosttyRenderRequest`** — Terminal dimensions for rendering: `cols`, `rows`, `scrollback_lines`.
- **`GhosttyRenderOutput`** — Rendered result containing `screen_contents` and `scrollback`.
- **`render_terminal_snapshot(request, vt_stream: &[u8]) -> Result<GhosttyRenderOutput>`** — Feed raw VT byte stream through the Ghostty terminal emulator and return the rendered text.

## Dependencies

- `anyhow` — error handling
- `libloading` — runtime dynamic library loading
