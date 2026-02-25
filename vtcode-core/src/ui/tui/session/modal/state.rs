use crate::config::constants::ui;
use crate::ui::search::{fuzzy_match, normalize_query};
use crate::ui::tui::types::{
    InlineEvent, InlineListItem, InlineListSearchConfig, InlineListSelection, SecurePromptConfig,
    WizardModalMode, WizardStep,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::ListState;
use tui_popup::PopupState;

#[derive(Clone)]
pub struct ModalState {
    pub title: String,
    pub lines: Vec<String>,
    pub footer_hint: Option<String>,
    pub list: Option<ModalListState>,
    pub secure_prompt: Option<SecurePromptConfig>,
    pub is_plan_confirmation: bool,
    #[allow(dead_code)]
    pub popup_state: PopupState,
    #[allow(dead_code)]
    pub restore_input: bool,
    #[allow(dead_code)]
    pub restore_cursor: bool,
    pub search: Option<ModalSearchState>,
}

/// State for a multi-step wizard modal with tabs for navigation
#[allow(dead_code)]
#[derive(Clone)]
pub struct WizardModalState {
    pub title: String,
    pub steps: Vec<WizardStepState>,
    pub current_step: usize,
    pub search: Option<ModalSearchState>,
    pub mode: WizardModalMode,
}

/// State for a single wizard step
#[allow(dead_code)]
#[derive(Clone)]
pub struct WizardStepState {
    /// Title displayed in the tab header
    pub title: String,
    /// Question or instruction shown above the list
    pub question: String,
    /// List state for selectable items
    pub list: ModalListState,
    /// Whether this step has been completed
    pub completed: bool,
    /// The selected answer for this step
    pub answer: Option<InlineListSelection>,
    /// Optional notes for the current step (free text)
    pub notes: String,
    /// Whether notes input is active for the current step
    pub notes_active: bool,

    pub allow_freeform: bool,
    pub freeform_label: Option<String>,
    pub freeform_placeholder: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ModalKeyModifiers {
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

#[derive(Debug, Clone)]
pub enum ModalListKeyResult {
    NotHandled,
    HandledNoRedraw,
    Redraw,
    Submit(InlineEvent),
    Cancel(InlineEvent),
}

#[derive(Clone)]
pub struct ModalListState {
    pub items: Vec<ModalListItem>,
    pub visible_indices: Vec<usize>,
    pub list_state: ListState,
    pub total_selectable: usize,
    pub filter_terms: Vec<String>,
    pub filter_query: Option<String>,
    pub viewport_rows: Option<u16>,
}

#[derive(Clone)]
pub struct ModalListItem {
    pub title: String,
    pub subtitle: Option<String>,
    pub badge: Option<String>,
    pub indent: u8,
    pub selection: Option<InlineListSelection>,
    pub search_value: Option<String>,
    pub is_divider: bool,
}

#[derive(Clone)]
pub struct ModalSearchState {
    pub label: String,
    pub placeholder: Option<String>,
    pub query: String,
}

impl From<InlineListSearchConfig> for ModalSearchState {
    fn from(config: InlineListSearchConfig) -> Self {
        Self {
            label: config.label,
            placeholder: config.placeholder,
            query: String::new(),
        }
    }
}

impl ModalSearchState {
    pub fn insert(&mut self, value: &str) {
        for ch in value.chars() {
            if matches!(ch, '\n' | '\r') {
                continue;
            }
            self.query.push(ch);
        }
    }

    pub fn push_char(&mut self, ch: char) {
        self.query.push(ch);
    }

    pub fn backspace(&mut self) -> bool {
        if self.query.pop().is_some() {
            return true;
        }
        false
    }

    pub fn clear(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        self.query.clear();
        true
    }
}

impl ModalState {
    pub fn handle_list_key_event(
        &mut self,
        key: &KeyEvent,
        modifiers: ModalKeyModifiers,
    ) -> ModalListKeyResult {
        let Some(list) = self.list.as_mut() else {
            return ModalListKeyResult::NotHandled;
        };

        if let Some(search) = self.search.as_mut() {
            match key.code {
                KeyCode::Char(ch) if !modifiers.control && !modifiers.alt && !modifiers.command => {
                    search.push_char(ch);
                    list.apply_search(&search.query);
                    return ModalListKeyResult::Redraw;
                }
                KeyCode::Backspace => {
                    if search.backspace() {
                        list.apply_search(&search.query);
                        return ModalListKeyResult::Redraw;
                    }
                    return ModalListKeyResult::HandledNoRedraw;
                }
                KeyCode::Delete => {
                    if search.clear() {
                        list.apply_search(&search.query);
                        return ModalListKeyResult::Redraw;
                    }
                    return ModalListKeyResult::HandledNoRedraw;
                }
                KeyCode::Esc => {
                    if search.clear() {
                        list.apply_search(&search.query);
                        return ModalListKeyResult::Redraw;
                    }
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Up => {
                if modifiers.command {
                    list.select_first();
                } else {
                    list.select_previous();
                }
                ModalListKeyResult::Redraw
            }
            KeyCode::Down => {
                if modifiers.command {
                    list.select_last();
                } else {
                    list.select_next();
                }
                ModalListKeyResult::Redraw
            }
            KeyCode::PageUp => {
                list.page_up();
                ModalListKeyResult::Redraw
            }
            KeyCode::PageDown => {
                list.page_down();
                ModalListKeyResult::Redraw
            }
            KeyCode::Home => {
                list.select_first();
                ModalListKeyResult::Redraw
            }
            KeyCode::End => {
                list.select_last();
                ModalListKeyResult::Redraw
            }
            KeyCode::Tab => {
                // With no search active, Tab moves to first item for autocomplete behavior
                // If search is active, we already handled it above
                if self.search.is_none() && !list.visible_indices.is_empty() {
                    list.select_first();
                } else {
                    list.select_next();
                }
                ModalListKeyResult::Redraw
            }
            KeyCode::BackTab => {
                list.select_previous();
                ModalListKeyResult::Redraw
            }
            KeyCode::Right => {
                list.select_next();
                ModalListKeyResult::Redraw
            }
            KeyCode::Enter => {
                if let Some(selection) = list.current_selection() {
                    ModalListKeyResult::Submit(InlineEvent::ListModalSubmit(selection))
                } else {
                    ModalListKeyResult::HandledNoRedraw
                }
            }
            KeyCode::Esc => ModalListKeyResult::Cancel(InlineEvent::ListModalCancel),
            KeyCode::Char(ch) if modifiers.control || modifiers.alt => match ch {
                'n' | 'N' | 'j' | 'J' => {
                    list.select_next();
                    ModalListKeyResult::Redraw
                }
                'p' | 'P' | 'k' | 'K' => {
                    list.select_previous();
                    ModalListKeyResult::Redraw
                }
                _ => ModalListKeyResult::NotHandled,
            },
            _ => ModalListKeyResult::NotHandled,
        }
    }
}

impl ModalListItem {
    pub fn is_header(&self) -> bool {
        self.selection.is_none() && !self.is_divider
    }

    fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let Some(value) = self.search_value.as_ref() else {
            return false;
        };
        fuzzy_match(query, value)
    }
}

#[allow(clippy::const_is_empty)]
pub fn is_divider_title(item: &InlineListItem) -> bool {
    if item.selection.is_some() {
        return false;
    }
    if item.indent != 0 {
        return false;
    }
    if item.subtitle.is_some() || item.badge.is_some() {
        return false;
    }
    let symbol = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL;
    if symbol.is_empty() {
        return false;
    }
    item.title
        .chars()
        .all(|ch| symbol.chars().any(|needle| needle == ch))
}

impl ModalListState {
    pub fn new(items: Vec<InlineListItem>, selected: Option<InlineListSelection>) -> Self {
        let converted: Vec<ModalListItem> = items
            .into_iter()
            .map(|item| {
                let is_divider = is_divider_title(&item);
                let search_value = item
                    .search_value
                    .as_ref()
                    .map(|value| value.to_ascii_lowercase());
                ModalListItem {
                    title: item.title,
                    subtitle: item.subtitle,
                    badge: item.badge,
                    indent: item.indent,
                    selection: item.selection,
                    search_value,
                    is_divider,
                }
            })
            .collect();
        let total_selectable = converted
            .iter()
            .filter(|item| item.selection.is_some())
            .count();
        let mut modal_state = Self {
            visible_indices: (0..converted.len()).collect(),
            items: converted,
            list_state: ListState::default(),
            total_selectable,
            filter_terms: Vec::new(),
            filter_query: None,
            viewport_rows: None,
        };
        modal_state.select_initial(selected);
        modal_state
    }

    pub fn current_selection(&self) -> Option<InlineListSelection> {
        self.list_state
            .selected()
            .and_then(|index| self.visible_indices.get(index))
            .and_then(|&item_index| self.items.get(item_index))
            .and_then(|item| item.selection.clone())
    }

    pub fn get_best_matching_item(&self, query: &str) -> Option<String> {
        if query.is_empty() {
            return None;
        }

        let normalized_query = normalize_query(query);
        self.visible_indices
            .iter()
            .filter_map(|&idx| self.items.get(idx))
            .filter(|item| item.selection.is_some())
            .filter_map(|item| item.search_value.as_ref())
            .find(|search_value| fuzzy_match(&normalized_query, search_value))
            .cloned()
    }

    pub fn select_previous(&mut self) {
        if self.visible_indices.is_empty() {
            return;
        }
        let Some(mut index) = self.list_state.selected() else {
            if let Some(last) = self.last_selectable_index() {
                self.list_state.select(Some(last));
            }
            return;
        };

        while index > 0 {
            index -= 1;
            let item_index = match self.visible_indices.get(index) {
                Some(idx) => *idx,
                None => {
                    tracing::warn!("visible_indices index {index} out of bounds");
                    continue;
                }
            };
            if let Some(item) = self.items.get(item_index)
                && item.selection.is_some()
            {
                self.list_state.select(Some(index));
                return;
            }
        }

        if let Some(first) = self.first_selectable_index() {
            self.list_state.select(Some(first));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn select_next(&mut self) {
        if self.visible_indices.is_empty() {
            return;
        }
        let mut index = self.list_state.selected().unwrap_or(usize::MAX);
        if index == usize::MAX {
            if let Some(first) = self.first_selectable_index() {
                self.list_state.select(Some(first));
            }
            return;
        }
        while index + 1 < self.visible_indices.len() {
            index += 1;
            let item_index = self.visible_indices[index];
            if self.items[item_index].selection.is_some() {
                self.list_state.select(Some(index));
                break;
            }
        }
    }

    pub fn select_first(&mut self) {
        if let Some(first) = self.first_selectable_index() {
            self.list_state.select(Some(first));
        } else {
            self.list_state.select(None);
        }
        if let Some(rows) = self.viewport_rows {
            self.ensure_visible(rows);
        }
    }

    pub fn select_last(&mut self) {
        if let Some(last) = self.last_selectable_index() {
            self.list_state.select(Some(last));
        } else {
            self.list_state.select(None);
        }
        if let Some(rows) = self.viewport_rows {
            self.ensure_visible(rows);
        }
    }

    pub fn select_nth_selectable(&mut self, target_index: usize) -> bool {
        let mut count = 0usize;
        for (visible_pos, &item_index) in self.visible_indices.iter().enumerate() {
            if self.items[item_index].selection.is_some() {
                if count == target_index {
                    self.list_state.select(Some(visible_pos));
                    if let Some(rows) = self.viewport_rows {
                        self.ensure_visible(rows);
                    }
                    return true;
                }
                count += 1;
            }
        }
        false
    }

    pub fn page_up(&mut self) {
        let step = self.page_step();
        if step == 0 {
            self.select_previous();
            return;
        }
        for _ in 0..step {
            let before = self.list_state.selected();
            self.select_previous();
            if self.list_state.selected() == before {
                break;
            }
        }
    }

    pub fn page_down(&mut self) {
        let step = self.page_step();
        if step == 0 {
            self.select_next();
            return;
        }
        for _ in 0..step {
            let before = self.list_state.selected();
            self.select_next();
            if self.list_state.selected() == before {
                break;
            }
        }
    }

    pub fn set_viewport_rows(&mut self, rows: u16) {
        self.viewport_rows = Some(rows);
    }

    pub(super) fn ensure_visible(&mut self, viewport: u16) {
        let Some(selected) = self.list_state.selected() else {
            return;
        };
        if viewport == 0 {
            return;
        }
        let visible = viewport as usize;
        let offset = self.list_state.offset();
        if selected < offset {
            *self.list_state.offset_mut() = selected;
        } else if selected >= offset + visible {
            *self.list_state.offset_mut() = selected + 1 - visible;
        }
    }

    pub fn apply_search(&mut self, query: &str) {
        let preferred = self.current_selection();
        self.apply_search_with_preference(query, preferred.clone());
    }

    pub fn apply_search_with_preference(
        &mut self,
        query: &str,
        preferred: Option<InlineListSelection>,
    ) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            if self.filter_query.is_none() {
                if preferred.is_some() && self.current_selection() != preferred {
                    self.select_initial(preferred);
                }
                return;
            }
            self.visible_indices = (0..self.items.len()).collect();
            self.filter_terms.clear();
            self.filter_query = None;
            self.select_initial(preferred);
            return;
        }

        if self.filter_query.as_deref() == Some(trimmed) {
            if preferred.is_some() && self.current_selection() != preferred {
                self.select_initial(preferred);
            }
            return;
        }

        let normalized_query = normalize_query(trimmed);
        let terms = normalized_query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .map(|term| term.to_owned())
            .collect::<Vec<_>>();
        let mut indices = Vec::new();
        let mut pending_divider: Option<usize> = None;
        let mut current_header: Option<usize> = None;
        let mut header_matches = false;
        let mut header_included = false;

        for (index, item) in self.items.iter().enumerate() {
            if item.is_divider {
                pending_divider = Some(index);
                current_header = None;
                header_matches = false;
                header_included = false;
                continue;
            }

            if item.is_header() {
                current_header = Some(index);
                header_matches = item.matches(&normalized_query);
                header_included = false;
                if header_matches {
                    if let Some(divider_index) = pending_divider.take() {
                        indices.push(divider_index);
                    }
                    indices.push(index);
                    header_included = true;
                }
                continue;
            }

            let item_matches = item.matches(&normalized_query);
            let include_item = header_matches || item_matches;
            if include_item {
                if let Some(divider_index) = pending_divider.take() {
                    indices.push(divider_index);
                }
                if let Some(header_index) = current_header
                    && !header_included
                {
                    indices.push(header_index);
                    header_included = true;
                }
                indices.push(index);
            }
        }
        self.visible_indices = indices;
        self.filter_terms = terms;
        self.filter_query = Some(trimmed.to_owned());
        self.select_initial(preferred);
    }

    fn select_initial(&mut self, preferred: Option<InlineListSelection>) {
        let mut selection_index = preferred.and_then(|needle| {
            self.visible_indices
                .iter()
                .position(|&idx| self.items[idx].selection.as_ref() == Some(&needle))
        });

        if selection_index.is_none() {
            selection_index = self.first_selectable_index();
        }

        self.list_state.select(selection_index);
        *self.list_state.offset_mut() = 0;
    }

    fn first_selectable_index(&self) -> Option<usize> {
        self.visible_indices
            .iter()
            .position(|&idx| self.items[idx].selection.is_some())
    }

    fn last_selectable_index(&self) -> Option<usize> {
        self.visible_indices
            .iter()
            .rposition(|&idx| self.items[idx].selection.is_some())
    }

    pub(super) fn filter_active(&self) -> bool {
        self.filter_query
            .as_ref()
            .is_some_and(|value| !value.is_empty())
    }

    pub(super) fn filter_query(&self) -> Option<&str> {
        self.filter_query.as_deref()
    }

    pub(super) fn highlight_terms(&self) -> &[String] {
        &self.filter_terms
    }

    pub(super) fn visible_selectable_count(&self) -> usize {
        self.visible_indices
            .iter()
            .filter(|&&idx| self.items[idx].selection.is_some())
            .count()
    }

    pub(super) fn total_selectable(&self) -> usize {
        self.total_selectable
    }

    fn page_step(&self) -> usize {
        let rows = self.viewport_rows.unwrap_or(0).max(1);
        usize::from(rows)
    }
}

#[allow(dead_code)]
impl WizardModalState {
    /// Create a new wizard modal state from wizard steps
    pub fn new(
        title: String,
        steps: Vec<WizardStep>,
        current_step: usize,
        search: Option<InlineListSearchConfig>,
        mode: WizardModalMode,
    ) -> Self {
        let step_states: Vec<WizardStepState> = steps
            .into_iter()
            .map(|step| {
                let notes_active = step
                    .items
                    .first()
                    .and_then(|item| item.selection.as_ref())
                    .is_some_and(|selection| match selection {
                        InlineListSelection::RequestUserInputAnswer {
                            selected, other, ..
                        } => selected.is_empty() && other.is_some(),
                        _ => false,
                    });
                WizardStepState {
                    title: step.title,
                    question: step.question,
                    list: ModalListState::new(step.items, step.answer.clone()),
                    completed: step.completed,
                    answer: step.answer,
                    notes: String::new(),
                    notes_active,
                    allow_freeform: step.allow_freeform,
                    freeform_label: step.freeform_label,
                    freeform_placeholder: step.freeform_placeholder,
                }
            })
            .collect();

        let clamped_step = if step_states.is_empty() {
            0
        } else {
            current_step.min(step_states.len().saturating_sub(1))
        };

        Self {
            title,
            steps: step_states,
            current_step: clamped_step,
            search: search.map(ModalSearchState::from),
            mode,
        }
    }

    /// Handle key event for wizard navigation
    pub fn handle_key_event(
        &mut self,
        key: &KeyEvent,
        modifiers: ModalKeyModifiers,
    ) -> ModalListKeyResult {
        if let Some(step) = self.steps.get_mut(self.current_step)
            && step.notes_active
        {
            match key.code {
                KeyCode::Char(ch) if !modifiers.control && !modifiers.alt && !modifiers.command => {
                    step.notes.push(ch);
                    return ModalListKeyResult::Redraw;
                }
                KeyCode::Backspace => {
                    if step.notes.pop().is_some() {
                        return ModalListKeyResult::Redraw;
                    }
                    return ModalListKeyResult::HandledNoRedraw;
                }
                KeyCode::Tab | KeyCode::Esc => {
                    if !step.notes.is_empty() {
                        step.notes.clear();
                    }
                    step.notes_active = false;
                    return ModalListKeyResult::Redraw;
                }
                _ => {}
            }
        }

        // Search handling (if enabled)
        if let Some(search) = self.search.as_mut() {
            if let Some(step) = self.steps.get_mut(self.current_step) {
                match key.code {
                    KeyCode::Char(ch)
                        if !modifiers.control && !modifiers.alt && !modifiers.command =>
                    {
                        search.push_char(ch);
                        step.list.apply_search(&search.query);
                        return ModalListKeyResult::Redraw;
                    }
                    KeyCode::Backspace => {
                        if search.backspace() {
                            step.list.apply_search(&search.query);
                            return ModalListKeyResult::Redraw;
                        }
                        return ModalListKeyResult::HandledNoRedraw;
                    }
                    KeyCode::Delete => {
                        if search.clear() {
                            step.list.apply_search(&search.query);
                            return ModalListKeyResult::Redraw;
                        }
                        return ModalListKeyResult::HandledNoRedraw;
                    }
                    KeyCode::Tab => {
                        if let Some(best_match) = step.list.get_best_matching_item(&search.query) {
                            search.query = best_match;
                            step.list.apply_search(&search.query);
                            return ModalListKeyResult::Redraw;
                        }
                        return ModalListKeyResult::HandledNoRedraw;
                    }
                    KeyCode::Esc => {
                        if search.clear() {
                            step.list.apply_search(&search.query);
                            return ModalListKeyResult::Redraw;
                        }
                    }
                    _ => {}
                }
            }
        }

        if self.mode == WizardModalMode::MultiStep
            && !modifiers.control
            && !modifiers.alt
            && !modifiers.command
            && self.search.is_none()
        {
            if let KeyCode::Char(ch) = key.code
                && ch.is_ascii_digit()
                && ch != '0'
            {
                let target_index = ch.to_digit(10).unwrap_or(1).saturating_sub(1) as usize;
                if let Some(step) = self.steps.get_mut(self.current_step)
                    && step.list.select_nth_selectable(target_index)
                {
                    return self.submit_current_selection();
                }
                return ModalListKeyResult::HandledNoRedraw;
            }
        }

        match key.code {
            KeyCode::Char('n') | KeyCode::Char('N')
                if modifiers.control && self.mode == WizardModalMode::MultiStep =>
            {
                if self.current_step < self.steps.len().saturating_sub(1) {
                    self.current_step += 1;
                    ModalListKeyResult::Redraw
                } else {
                    ModalListKeyResult::HandledNoRedraw
                }
            }
            // Left arrow: go to previous step if available
            KeyCode::Left => {
                if self.current_step > 0 {
                    self.current_step -= 1;
                    ModalListKeyResult::Redraw
                } else {
                    ModalListKeyResult::HandledNoRedraw
                }
            }
            // Right arrow: go to next step if current is completed
            KeyCode::Right => {
                let can_advance = match self.mode {
                    WizardModalMode::MultiStep => self.current_step_completed(),
                    WizardModalMode::TabbedList => true,
                };

                if can_advance && self.current_step < self.steps.len().saturating_sub(1) {
                    self.current_step += 1;
                    ModalListKeyResult::Redraw
                } else {
                    ModalListKeyResult::HandledNoRedraw
                }
            }
            // Enter: select current item and mark step complete
            KeyCode::Enter => self.submit_current_selection(),
            // Escape: cancel wizard
            KeyCode::Esc => ModalListKeyResult::Cancel(InlineEvent::WizardModalCancel),
            // Up/Down/Tab: delegate to current step's list
            KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::BackTab => {
                if let Some(step) = self.steps.get_mut(self.current_step) {
                    match key.code {
                        KeyCode::Up => {
                            if modifiers.command {
                                step.list.select_first();
                            } else {
                                step.list.select_previous();
                            }
                            ModalListKeyResult::Redraw
                        }
                        KeyCode::Down => {
                            if modifiers.command {
                                step.list.select_last();
                            } else {
                                step.list.select_next();
                            }
                            ModalListKeyResult::Redraw
                        }
                        KeyCode::Tab => {
                            if self.search.is_none() && step.allow_freeform {
                                step.notes_active = !step.notes_active;
                                ModalListKeyResult::Redraw
                            } else {
                                step.list.select_next();
                                ModalListKeyResult::Redraw
                            }
                        }
                        KeyCode::BackTab => {
                            step.list.select_previous();
                            ModalListKeyResult::Redraw
                        }
                        _ => ModalListKeyResult::NotHandled,
                    }
                } else {
                    ModalListKeyResult::NotHandled
                }
            }
            _ => ModalListKeyResult::NotHandled,
        }
    }

    /// Get current selection from the active step
    fn current_selection(&self) -> Option<InlineListSelection> {
        self.steps
            .get(self.current_step)
            .and_then(|step| {
                step.list
                    .current_selection()
                    .map(|selection| (selection, step))
            })
            .map(|(selection, step)| match selection {
                InlineListSelection::RequestUserInputAnswer {
                    question_id,
                    selected,
                    other,
                } => {
                    let notes = step.notes.trim();
                    let next_other = if other.is_some() {
                        Some(notes.to_string())
                    } else if notes.is_empty() {
                        None
                    } else {
                        Some(notes.to_string())
                    };
                    InlineListSelection::RequestUserInputAnswer {
                        question_id,
                        selected,
                        other: next_other,
                    }
                }
                InlineListSelection::AskUserChoice {
                    tab_id, choice_id, ..
                } => {
                    let notes = step.notes.trim();
                    let text = if notes.is_empty() {
                        None
                    } else {
                        Some(notes.to_string())
                    };
                    InlineListSelection::AskUserChoice {
                        tab_id,
                        choice_id,
                        text,
                    }
                }
                _ => selection,
            })
    }

    /// Check if current step is completed
    fn current_step_completed(&self) -> bool {
        self.steps
            .get(self.current_step)
            .is_some_and(|step| step.completed)
    }

    fn current_step_supports_notes(&self) -> bool {
        self.steps
            .get(self.current_step)
            .and_then(|step| step.list.current_selection())
            .is_some_and(|selection| {
                matches!(
                    selection,
                    InlineListSelection::RequestUserInputAnswer { .. }
                        | InlineListSelection::AskUserChoice { .. }
                )
            })
    }

    pub fn unanswered_count(&self) -> usize {
        self.steps.iter().filter(|step| !step.completed).count()
    }

    pub fn question_header(&self) -> String {
        format!(
            "Question {}/{} ({} unanswered)",
            self.current_step.saturating_add(1),
            self.steps.len(),
            self.unanswered_count()
        )
    }

    pub fn notes_line(&self) -> Option<String> {
        let step = self.steps.get(self.current_step)?;
        if step.notes_active || !step.notes.is_empty() {
            let label = step.freeform_label.as_deref().unwrap_or("›");
            if step.notes.is_empty() {
                if let Some(placeholder) = step.freeform_placeholder.as_ref() {
                    return Some(format!("{} {}", label, placeholder));
                }
            }
            Some(format!("{} {}", label, step.notes))
        } else {
            None
        }
    }

    pub fn notes_active(&self) -> bool {
        self.steps
            .get(self.current_step)
            .is_some_and(|step| step.notes_active)
    }

    pub fn instruction_lines(&self) -> Vec<String> {
        let step = match self.steps.get(self.current_step) {
            Some(s) => s,
            None => return Vec::new(),
        };

        if self.notes_active() {
            vec!["tab or esc to clear notes | enter to submit answer".to_string()]
        } else {
            let mut lines = Vec::new();
            if step.allow_freeform {
                lines.push("tab to add notes | enter to submit answer".to_string());
            } else {
                lines.push("enter to submit answer".to_string());
            }
            lines.push("ctrl + n next question | esc to interrupt".to_string());
            lines
        }
    }

    /// Mark current step as completed with the given answer
    fn complete_current_step(&mut self, answer: InlineListSelection) {
        if let Some(step) = self.steps.get_mut(self.current_step) {
            step.completed = true;
            step.answer = Some(answer);
        }
    }

    /// Collect all answers from completed steps
    fn collect_answers(&self) -> Vec<InlineListSelection> {
        self.steps
            .iter()
            .filter_map(|step| step.answer.clone())
            .collect()
    }

    fn submit_current_selection(&mut self) -> ModalListKeyResult {
        let Some(selection) = self.current_selection() else {
            return ModalListKeyResult::HandledNoRedraw;
        };

        match self.mode {
            WizardModalMode::TabbedList => {
                ModalListKeyResult::Submit(InlineEvent::WizardModalSubmit(vec![selection]))
            }
            WizardModalMode::MultiStep => {
                self.complete_current_step(selection.clone());
                if self.current_step < self.steps.len().saturating_sub(1) {
                    self.current_step += 1;
                    ModalListKeyResult::Submit(InlineEvent::WizardModalStepComplete {
                        step: self.current_step.saturating_sub(1),
                        answer: selection,
                    })
                } else {
                    ModalListKeyResult::Submit(InlineEvent::WizardModalSubmit(
                        self.collect_answers(),
                    ))
                }
            }
        }
    }

    /// Check if all steps are completed
    pub fn all_steps_completed(&self) -> bool {
        self.steps.iter().all(|step| step.completed)
    }
}
