use ratatui::prelude::*;

use crate::config::constants::ui;

use super::{Session, file_palette::FilePalette};

mod history_picker;
mod palettes;

pub(super) use history_picker::{render_history_picker, split_inline_history_picker_area};
pub(super) use palettes::{render_file_palette, split_inline_file_palette_area};

fn default_style(session: &Session) -> Style {
    session.core.styles.default_style()
}

fn accent_style(session: &Session) -> Style {
    session.core.styles.accent_style()
}

fn modal_list_highlight_style(session: &Session) -> Style {
    session.core.styles.modal_list_highlight_style()
}
