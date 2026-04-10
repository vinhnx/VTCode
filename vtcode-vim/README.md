# vtcode-vim

Reusable Vim-style prompt editing engine for VT Code surfaces.

Implement the `Editor` trait on any text buffer and call `handle_key` to get
Normal/Insert mode editing, motions, operators, and dot-repeat out of the box.

## Usage

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vtcode_vim::{Editor, VimState, handle_key};

struct MyEditor {
    buf: String,
    cursor: usize,
}

impl Editor for MyEditor {
    fn content(&self) -> &str { &self.buf }
    fn cursor(&self) -> usize { self.cursor }
    fn set_cursor(&mut self, pos: usize) { self.cursor = pos.min(self.buf.len()); }

    fn move_left(&mut self) {
        if self.cursor > 0 { self.cursor -= 1; }
    }
    fn move_right(&mut self) {
        if self.cursor < self.buf.len() { self.cursor += 1; }
    }
    fn delete_char_forward(&mut self) {
        if self.cursor < self.buf.len() { self.buf.remove(self.cursor); }
    }
    fn insert_text(&mut self, text: &str) {
        self.buf.insert_str(self.cursor, text);
        self.cursor += text.len();
    }
    fn replace(&mut self, content: String, cursor: usize) {
        self.buf = content;
        self.cursor = cursor.min(self.buf.len());
    }
}

// Drive editing with key events:
let mut state = VimState::new(true);
let mut editor = MyEditor { buf: "hello".into(), cursor: 0 };
let mut clipboard = String::new();

let outcome = handle_key(
    &mut state,
    &mut editor,
    &mut clipboard,
    &KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
);
assert!(outcome.handled);
```

## API reference

| Item | Description |
|---|---|
| `Editor` (trait) | Minimal text-surface interface: `content`, `cursor`, `set_cursor`, `move_left`, `move_right`, `delete_char_forward`, `insert_text`, `replace` |
| `handle_key(state, editor, clipboard, key) -> HandleKeyOutcome` | Route a single `crossterm::KeyEvent` through the Vim engine |
| `HandleKeyOutcome` | Result struct with `handled` and `clear_selection` flags |
| `VimMode` | `Normal` or `Insert` |
| `VimState` | Tracks current mode, pending operator, and repeat state |
