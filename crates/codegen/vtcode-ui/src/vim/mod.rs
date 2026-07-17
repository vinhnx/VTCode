//! Vim-style prompt editing engine for VT Code terminal surfaces.

mod engine;
mod text;
mod types;

pub use engine::{Editor, HandleKeyOutcome, handle_key};
pub use text::{next_char_boundary, prev_char_boundary};
pub(crate) use types::VimState;
