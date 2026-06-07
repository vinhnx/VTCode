# vtcode-vim

Reusable Vim-style prompt editing engine for VT Code surfaces. Implements normal/insert modes, motions, and text objects.

## Conventions

- The editor engine is a pure state machine with no I/O. Input is `crossterm::event::KeyEvent`, output is editor actions.
- All editing operations are undo-able. The undo stack is managed internally.
- Keep the engine decoupled from any TUI framework -- it operates on plain `String` buffers.
- Benchmarks live in `benches/vim_engine.rs`. Run with `cargo bench -p vtcode-vim`.

## Dependencies

- `crossterm` (key event types only, no terminal I/O)
