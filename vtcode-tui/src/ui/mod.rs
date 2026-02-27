pub mod file_colorizer;
pub mod markdown;
pub mod search;
pub mod syntax_highlight;
pub mod theme;

pub use file_colorizer::FileColorizer;

pub mod tui {
    pub use crate::core_tui::*;
}
