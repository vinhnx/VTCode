use ratatui::prelude::*;

use crate::config::constants::ui;

use super::{Session, agent_palette::AgentPalette, file_palette::FilePalette};

mod history_picker;
mod local_agents;
mod palettes;

pub(super) use history_picker::{
    history_picker_panel_layout, render_history_picker, split_inline_history_picker_area,
};
pub(super) use local_agents::{
    local_agents_panel_layout, render_local_agents, split_inline_local_agents_area,
};
pub(super) use palettes::{
    agent_palette_panel_layout, file_palette_panel_layout, render_agent_palette,
    render_file_palette, split_inline_agent_palette_area, split_inline_file_palette_area,
};

fn default_style(session: &Session) -> Style {
    session.core.styles.default_style()
}

fn accent_style(session: &Session) -> Style {
    session.core.styles.accent_style()
}

fn modal_list_highlight_style(session: &Session) -> Style {
    session.core.styles.modal_list_highlight_style()
}
