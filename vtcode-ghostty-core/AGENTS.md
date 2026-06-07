# vtcode-ghostty-core

Pure-Rust VT terminal emulator core for VT Code, inspired by Ghostty. Implements VT parser and state machine without FFI.

## Conventions

- No `unsafe` code. All terminal emulation is pure Rust.
- The parser is a streaming state machine -- feed bytes incrementally, not in bulk.
- Terminal state is snapshot-able via `TerminalState::snapshot()` for PTY session capture.
- Character width calculations use `unicode-width`.

## Dependencies

- `unicode-width` (character width)
