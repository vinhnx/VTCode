use crate::ui::tui::types::SecurePromptConfig;
use ratatui::prelude::*;
use ratatui_cheese::input::InputStyles;

use super::state::{ModalListState, ModalSearchState};

pub struct ModalRenderStyles {
    pub border: Style,
    pub highlight: Style,
    pub badge: Style,
    pub header: Style,
    pub selectable: Style,
    pub detail: Style,
    pub search_match: Style,
    pub title: Style,
    pub divider: Style,
    pub instruction_border: Style,
    pub instruction_title: Style,
    pub instruction_bullet: Style,
    pub instruction_body: Style,
    pub hint: Style,
}

pub struct ModalBodyContext<'a, 'b> {
    pub instructions: &'a [String],
    pub footer_hint: Option<&'a str>,
    pub list: Option<&'b mut ModalListState>,
    pub styles: &'a ModalRenderStyles,
    pub secure_prompt: Option<&'a SecurePromptConfig>,
    pub search: Option<&'a ModalSearchState>,
    pub input: &'a str,
    pub cursor: usize,
    pub input_styles: &'a InputStyles,
}

#[derive(Clone, Copy)]
pub enum ModalSection {
    Search,
    Instructions,
    Prompt,
    List,
}
