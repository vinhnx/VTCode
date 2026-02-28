use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, List, ListItem, Paragraph, Widget, Wrap},
};

use crate::config::constants::ui;
use crate::ui::tui::session::{
    modal::{
        ModalListLayout, ModalRenderStyles, ModalSearchState, ModalSection, WizardModalState,
        compute_modal_area,
    },
    terminal_capabilities,
};

/// Generic modal widget for buffer-based overlay rendering
///
/// Supports multiple modal types:
/// - List modals (file browser, prompt browser, etc.)
/// - Wizard modals (guided workflows with tabs)
/// - Text modals (informational messages)
/// - Search modals (with filtering)
/// - Secure prompt modals (password input)
///
/// # Example
/// ```ignore
/// ModalWidget::new(title, viewport)
///     .modal_state(modal_state)
///     .styles(styles)
///     .render(area, buf);
/// ```
pub struct ModalWidget<'a> {
    title: String,
    viewport: Rect,
    modal_type: ModalType<'a>,
    styles: ModalRenderStyles,
    input_content: Option<&'a str>,
    cursor_position: Option<usize>,
}

/// Different types of modals that can be rendered
pub enum ModalType<'a> {
    /// Simple text modal with instructions
    Text { lines: &'a [String] },
    /// List modal with selectable items
    List {
        lines: &'a [String],
        list_state: &'a mut crate::ui::tui::session::modal::ModalListState,
    },
    /// Wizard modal with tabs and steps
    Wizard { wizard_state: &'a WizardModalState },
    /// Search modal with input field
    Search {
        lines: &'a [String],
        search_state: &'a ModalSearchState,
        list_state: Option<&'a mut crate::ui::tui::session::modal::ModalListState>,
    },
    /// Secure prompt modal for password input
    SecurePrompt {
        lines: &'a [String],
        prompt_config: &'a crate::ui::tui::types::SecurePromptConfig,
    },
}

impl<'a> ModalWidget<'a> {
    /// Create a new ModalWidget with required parameters
    pub fn new(title: String, viewport: Rect) -> Self {
        Self {
            title,
            viewport,
            modal_type: ModalType::Text { lines: &[] },
            styles: ModalRenderStyles {
                border: Style::default(),
                highlight: Style::default(),
                badge: Style::default(),
                header: Style::default(),
                selectable: Style::default(),
                detail: Style::default(),
                search_match: Style::default(),
                title: Style::default().add_modifier(Modifier::BOLD),
                divider: Style::default(),
                instruction_border: Style::default(),
                instruction_title: Style::default(),
                instruction_bullet: Style::default(),
                instruction_body: Style::default(),
                hint: Style::default(),
            },
            input_content: None,
            cursor_position: None,
        }
    }

    /// Set the modal type and content
    #[must_use]
    pub fn modal_type(mut self, modal_type: ModalType<'a>) -> Self {
        self.modal_type = modal_type;
        self
    }

    /// Set the render styles
    #[must_use]
    pub fn styles(mut self, styles: ModalRenderStyles) -> Self {
        self.styles = styles;
        self
    }

    /// Set input content for secure prompts
    #[must_use]
    pub fn input_content(mut self, content: &'a str) -> Self {
        self.input_content = Some(content);
        self
    }

    /// Set cursor position for secure prompts
    #[must_use]
    pub fn cursor_position(mut self, position: usize) -> Self {
        self.cursor_position = Some(position);
        self
    }

    /// Calculate the modal area based on content
    fn calculate_modal_area(&self) -> Rect {
        let (text_lines, prompt_lines, search_lines, has_list) = match &self.modal_type {
            ModalType::Text { lines } => (lines.len(), 0, 0, false),
            ModalType::List { lines, .. } => (lines.len(), 0, 0, true),
            ModalType::Wizard { wizard_state: _ } => {
                let height = 10; // Default height for wizard modals
                (height, 0, 0, true)
            }
            ModalType::Search {
                lines, list_state, ..
            } => (lines.len(), 0, 3, list_state.is_some()),
            ModalType::SecurePrompt { lines, .. } => (lines.len(), 2, 0, false),
        };

        compute_modal_area(
            self.viewport,
            text_lines,
            prompt_lines,
            search_lines,
            has_list,
        )
    }
}

impl<'a> Widget for ModalWidget<'a> {
    fn render(self, _area: Rect, buf: &mut Buffer) {
        if self.viewport.height == 0 || self.viewport.width == 0 {
            return;
        }

        let area = self.calculate_modal_area();

        // Render clear background
        Clear.render(area, buf);

        // Render border block
        let block = Block::bordered()
            .title(Line::styled(self.title.clone(), self.styles.title))
            .border_type(terminal_capabilities::get_border_type())
            .border_style(self.styles.border);
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        // Render modal content based on type
        match self.modal_type {
            ModalType::Text { lines } => {
                self.render_text_modal(inner, buf, lines);
            }
            ModalType::List {
                lines,
                ref list_state,
            } => {
                self.render_list_modal(inner, buf, lines, list_state);
            }
            ModalType::Wizard { wizard_state } => {
                self.render_wizard_modal(inner, buf, wizard_state);
            }
            ModalType::Search {
                lines,
                search_state,
                list_state: ref list_state_opt,
            } => {
                // Handle the Option<&mut T> by creating a new Option with the reference
                let list_state_ref = list_state_opt.as_ref().map(|s| &**s);
                self.render_search_modal(inner, buf, lines, search_state, list_state_ref);
            }
            ModalType::SecurePrompt {
                lines,
                prompt_config,
            } => {
                self.render_secure_prompt_modal(inner, buf, lines, prompt_config);
            }
        }
    }
}

impl<'a> ModalWidget<'a> {
    fn render_text_modal(&self, area: Rect, buf: &mut Buffer, lines: &[String]) {
        if lines.is_empty() {
            return;
        }

        let paragraph = Paragraph::new(
            lines
                .iter()
                .map(|line| Line::from(line.as_str()))
                .collect::<Vec<_>>(),
        )
        .wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }

    fn render_list_modal(
        &self,
        area: Rect,
        buf: &mut Buffer,
        lines: &[String],
        list_state: &crate::ui::tui::session::modal::ModalListState,
    ) {
        let layout = ModalListLayout::new(area, lines.len());

        // Render instructions if present
        if let Some(text_area) = layout.text_area
            && !lines.is_empty()
        {
            self.render_instructions(text_area, buf, lines);
        }

        // Render list
        self.render_modal_list(layout.list_area, buf, list_state);
    }

    fn render_wizard_modal(&self, area: Rect, buf: &mut Buffer, wizard_state: &WizardModalState) {
        // Layout: [Tabs Header (1 row)] [Question text] [List]
        let chunks = Layout::vertical([
            Constraint::Length(1), // Tabs
            Constraint::Length(2), // Question with padding
            Constraint::Min(3),    // List
        ])
        .split(area);

        // Render tabs
        self.render_wizard_tabs(
            chunks[0],
            buf,
            &wizard_state.steps,
            wizard_state.current_step,
        );

        // Render question for current step
        if let Some(step) = wizard_state.steps.get(wizard_state.current_step) {
            let question = Paragraph::new(Line::from(Span::styled(
                step.question.clone(),
                self.styles.header,
            )));
            question.render(chunks[1], buf);

            // Note: We can't render the list here because we don't have mutable access
            // This is a limitation of the current design - wizard modals should be
            // handled differently or the list state should be passed separately
        }
    }

    fn render_search_modal(
        &self,
        area: Rect,
        buf: &mut Buffer,
        lines: &[String],
        search_state: &ModalSearchState,
        list_state: Option<&crate::ui::tui::session::modal::ModalListState>,
    ) {
        let mut sections = Vec::new();
        let has_instructions = lines.iter().any(|line| !line.trim().is_empty());

        if has_instructions {
            sections.push(ModalSection::Instructions);
        }
        sections.push(ModalSection::Search);
        if list_state.is_some() {
            sections.push(ModalSection::List);
        }

        let mut constraints = Vec::new();
        for section in &sections {
            match section {
                ModalSection::Instructions => {
                    let visible_rows = lines.len().max(1) as u16;
                    let height = visible_rows.saturating_add(2);
                    constraints.push(Constraint::Length(height.min(area.height)));
                }
                ModalSection::Search => {
                    constraints.push(Constraint::Length(3.min(area.height)));
                }
                ModalSection::List => {
                    constraints.push(Constraint::Min(3));
                }
                _ => {}
            }
        }

        let chunks = Layout::vertical(constraints).split(area);
        let mut chunk_iter = chunks.iter();

        for section in &sections {
            if let Some(chunk) = chunk_iter.next() {
                match section {
                    ModalSection::Instructions => {
                        if chunk.height > 0 && has_instructions {
                            self.render_instructions(*chunk, buf, lines);
                        }
                    }
                    ModalSection::Search => {
                        self.render_modal_search(*chunk, buf, search_state);
                    }
                    ModalSection::List => {
                        if let Some(list_state) = list_state {
                            self.render_modal_list(*chunk, buf, list_state);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn render_secure_prompt_modal(
        &self,
        area: Rect,
        buf: &mut Buffer,
        lines: &[String],
        prompt_config: &crate::ui::tui::types::SecurePromptConfig,
    ) {
        let mut sections = Vec::new();
        let has_instructions = lines.iter().any(|line| !line.trim().is_empty());

        if has_instructions {
            sections.push(ModalSection::Instructions);
        }
        sections.push(ModalSection::Prompt);

        let mut constraints = Vec::new();
        for section in &sections {
            match section {
                ModalSection::Instructions => {
                    let visible_rows = lines.len().max(1) as u16;
                    let height = visible_rows.saturating_add(2);
                    constraints.push(Constraint::Length(height.min(area.height)));
                }
                ModalSection::Prompt => {
                    constraints.push(Constraint::Length(3.min(area.height)));
                }
                _ => {}
            }
        }

        let chunks = Layout::vertical(constraints).split(area);
        let mut chunk_iter = chunks.iter();

        for section in &sections {
            if let Some(chunk) = chunk_iter.next() {
                match section {
                    ModalSection::Instructions => {
                        if chunk.height > 0 && has_instructions {
                            self.render_instructions(*chunk, buf, lines);
                        }
                    }
                    ModalSection::Prompt => {
                        self.render_secure_prompt(
                            *chunk,
                            buf,
                            prompt_config,
                            self.input_content.unwrap_or(""),
                            self.cursor_position.unwrap_or(0),
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    fn render_instructions(&self, area: Rect, buf: &mut Buffer, instructions: &[String]) {
        let items: Vec<ListItem> = instructions
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    ListItem::new(Line::default())
                } else if i == 0 {
                    // First line gets header style
                    ListItem::new(Line::from(Span::styled(
                        trimmed.to_owned(),
                        self.styles.header,
                    )))
                } else {
                    // Subsequent lines get bullet points
                    let bullet_prefix = format!("{} ", ui::MODAL_INSTRUCTIONS_BULLET);
                    ListItem::new(Line::from(vec![
                        Span::styled(bullet_prefix, self.styles.instruction_bullet),
                        Span::styled(trimmed.to_owned(), self.styles.instruction_body),
                    ]))
                }
            })
            .collect();

        let block = Block::bordered()
            .title(Span::styled(
                ui::MODAL_INSTRUCTIONS_TITLE.to_owned(),
                self.styles.instruction_title,
            ))
            .border_type(terminal_capabilities::get_border_type())
            .border_style(self.styles.instruction_border);

        let widget = List::new(items)
            .block(block)
            .style(self.styles.instruction_body)
            .highlight_symbol("")
            .repeat_highlight_symbol(false);
        widget.render(area, buf);
    }

    fn render_modal_list(
        &self,
        area: Rect,
        buf: &mut Buffer,
        list_state: &crate::ui::tui::session::modal::ModalListState,
    ) {
        use crate::ui::tui::session::modal::modal_list_items;

        // Note: Since we're working with Buffer (not Frame), we can't do stateful rendering
        // This is a simplified version that just renders the current state
        if list_state.visible_indices.is_empty() {
            let message = Paragraph::new(Line::from(Span::styled(
                ui::MODAL_LIST_NO_RESULTS_MESSAGE.to_owned(),
                self.styles.detail,
            )))
            .wrap(Wrap { trim: true });
            message.render(area, buf);
            return;
        }

        let content_width = area.width.saturating_sub(4) as usize;
        let items = modal_list_items(list_state, &self.styles, content_width);
        let widget = List::new(items)
            .highlight_style(self.styles.highlight)
            .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
            .repeat_highlight_symbol(true);

        widget.render(area, buf);
    }

    fn render_wizard_tabs(
        &self,
        area: Rect,
        buf: &mut Buffer,
        steps: &[crate::ui::tui::session::modal::WizardStepState],
        current_step: usize,
    ) {
        // Simple tab rendering - just show the current step title
        if let Some(step) = steps.get(current_step) {
            let icon = if step.completed { "✔" } else { "☐" };
            let text = format!("{} {}", icon, step.title);
            let tabs = Paragraph::new(Line::from(text).style(self.styles.highlight));
            tabs.render(area, buf);
        }
    }

    fn render_modal_search(&self, area: Rect, buf: &mut Buffer, search_state: &ModalSearchState) {
        let mut spans = Vec::new();
        if search_state.query.is_empty() {
            if let Some(placeholder) = &search_state.placeholder {
                spans.push(Span::styled(placeholder.clone(), self.styles.detail));
            }
        } else {
            spans.push(Span::styled(
                search_state.query.clone(),
                self.styles.selectable,
            ));
        }
        spans.push(Span::styled("▌".to_owned(), self.styles.highlight));

        let block = Block::bordered()
            .title(Span::styled(search_state.label.clone(), self.styles.header))
            .border_type(terminal_capabilities::get_border_type())
            .border_style(self.styles.border);

        let paragraph = Paragraph::new(Line::from(spans))
            .block(block)
            .wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }

    fn render_secure_prompt(
        &self,
        area: Rect,
        buf: &mut Buffer,
        config: &crate::ui::tui::types::SecurePromptConfig,
        input: &str,
        _cursor: usize,
    ) {
        // For buffer-based rendering, we'll render a simple password field
        // The full tui-prompts integration requires a Frame, not just a Buffer
        let grapheme_count = input.chars().count();
        let sanitized: String = std::iter::repeat_n('•', grapheme_count).collect();

        let mut spans = vec![Span::styled(config.label.clone(), self.styles.header)];
        spans.push(Span::raw(" "));
        spans.push(Span::styled(sanitized, self.styles.selectable));
        spans.push(Span::styled("▌".to_owned(), self.styles.highlight));

        let block = Block::bordered()
            .border_type(terminal_capabilities::get_border_type())
            .border_style(self.styles.border);

        let paragraph = Paragraph::new(Line::from(spans))
            .block(block)
            .wrap(Wrap { trim: true });
        paragraph.render(area, buf);
    }
}
