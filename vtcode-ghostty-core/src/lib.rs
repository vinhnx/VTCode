//! Pure-Rust VT terminal emulator core for VT Code.
//!
//! Inspired by [Ghostty](https://ghostty.org/)'s terminal design.
//! Processes VT byte streams incrementally via [`Terminal::write`].

pub mod cell;
pub mod color;
pub mod cursor;
pub mod mode;
pub mod screen;
pub mod style;

mod parser;
mod region;
mod terminal;

pub use cell::Cell;
pub use color::{AnsiColor, Color};
pub use cursor::Cursor;
pub use mode::{CursorShape, MouseTracking};
pub use screen::ScreenKind;
pub use style::Style;
pub use terminal::Terminal;
