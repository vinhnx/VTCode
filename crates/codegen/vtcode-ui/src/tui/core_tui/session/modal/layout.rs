use crate::tui::ui::tui::types::{InlineStatus, SecurePromptConfig};
use ratatui::prelude::*;
use ratatui_cheese::input::InputStyles;

use super::state::{ModalListState, ModalSearchState};

pub struct ModalRenderStyles {
    pub(crate) border: Style,
    pub(crate) highlight: Style,
    pub(crate) badge: Style,
    pub(crate) header: Style,
    pub(crate) selectable: Style,
    pub(crate) detail: Style,
    pub(crate) search_match: Style,
    pub(crate) title: Style,
    pub(crate) divider: Style,
    /// Area background for the modal chrome. Must stay modifier-free: ratatui's
    /// `Cell::set_style` only *inserts* `add_modifier`, so a DIM painted here
    /// sticks to every glyph later drawn inside the modal and mutes
    /// otherwise-bright text (see `render_modal_background`).
    pub(crate) background: Style,
    pub(crate) instruction_border: Style,
    pub(crate) instruction_title: Style,
    pub(crate) instruction_bullet: Style,
    pub(crate) instruction_body: Style,
    pub(crate) hint: Style,
    /// Semantic tones for badges, values, and the status strip.
    pub(crate) success: Style,
    pub(crate) warning: Style,
    pub(crate) danger: Style,
    /// Accent emphasis that does not imply selection (values, Current badges).
    pub(crate) accent: Style,
}

pub struct ModalBodyContext<'a, 'b> {
    pub(crate) instructions: &'a [String],
    pub(crate) footer_hint: Option<&'a str>,
    /// Latest apply/save feedback for the status strip.
    pub(crate) status: Option<&'a InlineStatus>,
    pub(crate) list: Option<&'b mut ModalListState>,
    pub(crate) styles: &'a ModalRenderStyles,
    pub(crate) secure_prompt: Option<&'a SecurePromptConfig>,
    pub(crate) search: Option<&'a ModalSearchState>,
    pub(crate) input: &'a str,
    pub(crate) cursor: usize,
    pub(crate) input_styles: &'a InputStyles,
}

#[derive(Clone, Copy)]
pub enum ModalSection {
    Search,
    Instructions,
    Prompt,
    List,
}
