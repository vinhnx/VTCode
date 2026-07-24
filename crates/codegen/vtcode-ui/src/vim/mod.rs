//! Vim-style prompt editing engine for VT Code terminal surfaces.

mod engine;
mod text;
mod types;

pub(crate) use engine::{Editor, HandleKeyOutcome, handle_key};
pub(crate) use text::{next_char_boundary, prev_char_boundary};
pub(crate) use types::VimState;
