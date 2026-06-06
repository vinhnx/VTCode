# vtcode-ghostty-core

Pure-Rust VT terminal emulator core for VT Code, inspired by [Ghostty](https://ghostty.org/)'s terminal design.

`vtcode-ghostty-core` processes VT byte streams incrementally via `Terminal::write`, maintaining a cell-based screen buffer with cursor state, styling, and terminal modes.

## Modules

| Module | Purpose |
|---|---|
| `cell` | `Cell` type representing a single character cell with style and content |
| `color` | `AnsiColor` and `Color` types for terminal color representation |
| `cursor` | `Cursor` type tracking position, shape, and visibility |
| `mode` | `CursorShape` and `MouseTracking` enums for terminal mode flags |
| `screen` | `ScreenKind` enum and screen buffer management |
| `style` | `Style` type for cell-level styling (fg, bg, effects) |
| `parser` (private) | VT escape sequence parser |
| `region` (private) | Scroll region management |
| `terminal` (private) | `Terminal` implementation orchestrating parser, screen, and cursor |

## Public entrypoints

- `Terminal` — main terminal emulator; call `write(bytes)` to process VT input
- `Cell` — character cell with content and style
- `Cursor` — cursor position and shape state
- `Color` / `AnsiColor` — terminal color types
- `Style` — cell styling (foreground, background, effects)
- `CursorShape` / `MouseTracking` — terminal mode enums
- `ScreenKind` — screen buffer type

## Usage

```rust
use vtcode_ghostty_core::Terminal;

let mut terminal = Terminal::new(80, 24);
terminal.write(b"Hello, World!\r\n");
terminal.write(b"\x1b[1;31mBold Red\x1b[0m");
```

## API reference

See [docs.rs/vtcode-ghostty-core](https://docs.rs/vtcode-ghostty-core).

## Related docs

- [Architecture overview](../docs/ARCHITECTURE.md)
