use super::*;

impl WizardModalState {
    /// Whether digit shortcuts submit the current step's option: MultiStep
    /// wizards only, no search box, and a scannable option count.
    /// TabbedList navigates by tabs, so its digits stay unbound — and the
    /// badge renderer uses this same gate, so unbound digits never show
    /// dead numbers.
    pub(crate) fn numbered_shortcuts(&self) -> bool {
        self.mode == WizardModalMode::MultiStep
            && self.search.is_none()
            && self
                .steps
                .get(self.current_step)
                .is_some_and(|step| step.list.numbered_shortcuts())
    }

    /// Create a new wizard modal state from wizard steps
    pub(crate) fn new(
        title: String,
        steps: Vec<WizardStep>,
        current_step: usize,
        search: Option<InlineListSearchConfig>,
        mode: WizardModalMode,
    ) -> Self {
        let step_states: Vec<WizardStepState> = steps
            .into_iter()
            .map(|step| {
                let notes_active = step.items.first().and_then(|item| item.selection.as_ref()).is_some_and(
                    |selection| match selection {
                        InlineListSelection::RequestUserInputAnswer { selected, other, .. } => {
                            selected.is_empty() && other.is_some()
                        }
                        _ => false,
                    },
                );
                let checked_indices = if step.multi_select {
                    step.items
                        .iter()
                        .enumerate()
                        .filter_map(|(index, item)| match (step.answer.as_ref(), item.selection.as_ref()) {
                            (
                                Some(InlineListSelection::RequestUserInputAnswer { selected, .. }),
                                Some(InlineListSelection::RequestUserInputAnswer { selected: values, .. }),
                            ) if !values.is_empty() && values.iter().all(|value| selected.contains(value)) => {
                                Some(index)
                            }
                            _ => None,
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let notes = if step.multi_select {
                    match step.answer.as_ref() {
                        Some(InlineListSelection::RequestUserInputAnswer { other, .. }) => {
                            other.clone().unwrap_or_default()
                        }
                        _ => String::new(),
                    }
                } else {
                    String::new()
                };
                WizardStepState {
                    multi_select: step.multi_select,
                    checked_indices,
                    title: step.title,
                    question: step.question,
                    list: ModalListState::new(step.items, step.answer.clone()),
                    completed: step.completed,
                    answer: step.answer,
                    notes,
                    notes_active,
                    allow_freeform: step.allow_freeform,
                    freeform_label: step.freeform_label,
                    freeform_placeholder: step.freeform_placeholder,
                    mask_input: step.mask_input,
                    freeform_default: step.freeform_default,
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

    pub(crate) fn paste_notes(&mut self, content: &str) -> bool {
        let Some(step) = self.steps.get_mut(self.current_step) else {
            return false;
        };
        if !step.notes_active && Self::step_selected_custom_note_item_index(step).is_none() {
            return false;
        }
        step.notes_active = true;
        // Sensitive input keeps pasted whitespace intact for literal validation.
        if step.mask_input {
            step.notes.push_str(content);
        } else {
            step.notes.extend(content.chars().filter(|ch| !matches!(ch, '\n' | '\r')));
        }
        true
    }

    /// Handle key event for wizard navigation
    pub(crate) fn handle_key_event(&mut self, key: &KeyEvent, modifiers: ModalKeyModifiers) -> ModalListKeyResult {
        if let Some(step) = self.steps.get_mut(self.current_step)
            && step.notes_active
        {
            match key.code {
                KeyCode::Char(ch) if !modifiers.control && !modifiers.alt && !modifiers.command => {
                    let mut state = InputState::new();
                    state.set_value(step.notes.clone());
                    state.end();
                    state.insert_char(ch);
                    step.notes = state.value().to_owned();
                    return ModalListKeyResult::Redraw;
                }
                KeyCode::Backspace => {
                    if !step.notes.is_empty() {
                        let mut state = InputState::new();
                        state.set_value(step.notes.clone());
                        state.end();
                        state.delete_before();
                        step.notes = state.value().to_owned();
                        return ModalListKeyResult::Redraw;
                    }
                    return ModalListKeyResult::HandledNoRedraw;
                }
                KeyCode::Esc if step.mask_input => {
                    return ModalListKeyResult::Cancel(InlineEvent::Overlay(OverlayEvent::Cancelled));
                }
                KeyCode::Tab | KeyCode::Esc => {
                    if !step.multi_select && !step.notes.is_empty() {
                        step.notes.clear();
                    }
                    step.notes_active = false;
                    return ModalListKeyResult::Redraw;
                }
                _ => {}
            }
        }

        if let Some(step) = self.steps.get_mut(self.current_step)
            && !step.notes_active
            && Self::step_selected_custom_note_item_index(step).is_some()
        {
            match key.code {
                KeyCode::Char(ch) if !modifiers.control && !modifiers.alt && !modifiers.command => {
                    step.notes_active = true;
                    let mut state = InputState::new();
                    state.set_value(step.notes.clone());
                    state.end();
                    state.insert_char(ch);
                    step.notes = state.value().to_owned();
                    return ModalListKeyResult::Redraw;
                }
                KeyCode::Backspace if !step.notes.is_empty() => {
                    step.notes_active = true;
                    let mut state = InputState::new();
                    state.set_value(step.notes.clone());
                    state.end();
                    state.delete_before();
                    step.notes = state.value().to_owned();
                    return ModalListKeyResult::Redraw;
                }
                _ => {}
            }
        }

        // Search handling (if enabled)
        if let Some(search) = self.search.as_mut()
            && let Some(step) = self.steps.get_mut(self.current_step)
        {
            match key.code {
                KeyCode::Char(ch) if !modifiers.control && !modifiers.alt && !modifiers.command => {
                    search.push_char(ch);
                    step.list.apply_search(&search.query, search.fuzzy);
                    return ModalListKeyResult::Redraw;
                }
                KeyCode::Backspace => {
                    if search.backspace() {
                        step.list.apply_search(&search.query, search.fuzzy);
                        return ModalListKeyResult::Redraw;
                    }
                    return ModalListKeyResult::HandledNoRedraw;
                }
                KeyCode::Delete => {
                    if search.clear() {
                        step.list.apply_search(&search.query, search.fuzzy);
                        return ModalListKeyResult::Redraw;
                    }
                    return ModalListKeyResult::HandledNoRedraw;
                }
                KeyCode::Tab => {
                    if let Some(best_match) = step.list.get_best_matching_item(&search.query) {
                        search.query = best_match;
                        step.list.apply_search(&search.query, search.fuzzy);
                        return ModalListKeyResult::Redraw;
                    }
                    return ModalListKeyResult::HandledNoRedraw;
                }
                KeyCode::Esc if search.clear() => {
                    step.list.apply_search(&search.query, search.fuzzy);
                    return ModalListKeyResult::Redraw;
                }
                _ => {}
            }
        }

        if self.mode == WizardModalMode::MultiStep
            && !modifiers.control
            && !modifiers.alt
            && !modifiers.command
            && self.search.is_none()
            && let KeyCode::Char(ch) = key.code
            && ch.is_ascii_digit()
            && ch != '0'
        {
            // Same scannability gate as plain modals: crowded steps offer
            // no digits rather than dead ones.
            let target_index = ch.to_digit(10).unwrap_or(1).saturating_sub(1) as usize;
            if self.numbered_shortcuts()
                && let Some(step) = self.steps.get_mut(self.current_step)
                && step.list.select_nth_selectable(target_index)
            {
                if step.multi_select {
                    return self.toggle_current_option();
                }
                return self.submit_current_selection();
            }
            return ModalListKeyResult::HandledNoRedraw;
        }

        match key.code {
            KeyCode::Char('n') | KeyCode::Char('N') if modifiers.control && self.mode == WizardModalMode::MultiStep => {
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
            KeyCode::Char(' ') if self.search.is_none() => self.toggle_current_option(),
            // Escape or Ctrl+C: cancel wizard
            KeyCode::Esc => ModalListKeyResult::Cancel(InlineEvent::Overlay(OverlayEvent::Cancelled)),
            KeyCode::Char('c') | KeyCode::Char('C') if modifiers.control => {
                ModalListKeyResult::Cancel(InlineEvent::Interrupt)
            }
            KeyCode::Char('\u{3}') => ModalListKeyResult::Cancel(InlineEvent::Interrupt),
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
                            if self.search.is_none()
                                && (step.allow_freeform || Self::step_selected_custom_note_item_index(step).is_some())
                            {
                                step.notes_active.toggle();
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

    pub(crate) fn handle_mouse_click(&mut self, visible_index: usize) -> ModalListKeyResult {
        let submit_after_click = {
            let Some(step) = self.steps.get_mut(self.current_step) else {
                return ModalListKeyResult::NotHandled;
            };
            let Some(&item_index) = step.list.visible_indices.get(visible_index) else {
                return ModalListKeyResult::HandledNoRedraw;
            };
            let Some(item) = step.list.items.get(item_index) else {
                return ModalListKeyResult::HandledNoRedraw;
            };
            let Some(selection) = item.selection.as_ref() else {
                return ModalListKeyResult::HandledNoRedraw;
            };

            if step.multi_select {
                step.list.list_state.select(Some(visible_index));
                return self.toggle_current_option();
            }
            let clicked_custom_note = is_custom_note_selection(selection);
            let already_selected = step.list.list_state.selected() == Some(visible_index);

            if self.mode == WizardModalMode::TabbedList {
                step.list.list_state.select(Some(visible_index));
                if let Some(rows) = step.list.viewport_rows {
                    step.list.ensure_visible(rows);
                }

                if clicked_custom_note && step.notes.trim().is_empty() && !step.has_freeform_default() {
                    step.notes_active = true;
                    return ModalListKeyResult::Redraw;
                }

                true
            } else if already_selected {
                true
            } else {
                step.list.list_state.select(Some(visible_index));
                if let Some(rows) = step.list.viewport_rows {
                    step.list.ensure_visible(rows);
                }
                return ModalListKeyResult::Redraw;
            }
        };

        if submit_after_click {
            return self.submit_current_selection();
        }

        ModalListKeyResult::Redraw
    }

    pub(crate) fn handle_mouse_scroll(&mut self, down: bool) -> ModalListKeyResult {
        let Some(step) = self.steps.get_mut(self.current_step) else {
            return ModalListKeyResult::NotHandled;
        };

        let before = step.list.list_state.selected();
        if down {
            step.list.select_next();
        } else {
            step.list.select_previous();
        }

        if step.list.list_state.selected() == before {
            ModalListKeyResult::HandledNoRedraw
        } else {
            ModalListKeyResult::Redraw
        }
    }

    fn toggle_current_option(&mut self) -> ModalListKeyResult {
        let Some(step) = self.steps.get_mut(self.current_step) else {
            return ModalListKeyResult::HandledNoRedraw;
        };
        let Some(index) = step
            .list
            .list_state
            .selected()
            .and_then(|i| step.list.visible_indices.get(i))
            .copied()
        else {
            return ModalListKeyResult::HandledNoRedraw;
        };
        if !step.multi_select {
            return ModalListKeyResult::Redraw;
        }
        if step
            .list
            .items
            .get(index)
            .and_then(|item| item.selection.as_ref())
            .is_some_and(is_custom_note_selection)
        {
            step.notes_active = true;
            return ModalListKeyResult::Redraw;
        }
        if let Some(position) = step.checked_indices.iter().position(|i| *i == index) {
            step.checked_indices.remove(position);
        } else if step.list.items.get(index).and_then(|item| item.selection.as_ref()).is_some_and(|selection| matches!(selection, InlineListSelection::RequestUserInputAnswer { selected, .. } if !selected.is_empty())) {
            step.checked_indices.push(index);
        }
        step.checked_indices.sort_unstable();
        ModalListKeyResult::Redraw
    }

    /// Get current selection from the active step
    pub(super) fn current_selection(&self) -> Option<InlineListSelection> {
        if let Some(step) = self.steps.get(self.current_step)
            && step.multi_select
        {
            let question_id = step.list.items.iter().find_map(|item| match item.selection.as_ref()? {
                InlineListSelection::RequestUserInputAnswer { question_id, .. } => Some(question_id.clone()),
                _ => None,
            })?;
            let selected = step
                .checked_indices
                .iter()
                .filter_map(|i| step.list.items.get(*i))
                .filter_map(|item| match item.selection.as_ref()? {
                    InlineListSelection::RequestUserInputAnswer { selected, .. } => Some(selected.clone()),
                    _ => None,
                })
                .flatten()
                .collect::<Vec<_>>();
            let other = step.submitted_freeform_value();
            if selected.is_empty() && other.is_none() {
                return None;
            }
            return Some(InlineListSelection::RequestUserInputAnswer { question_id, selected, other });
        }
        self.steps
            .get(self.current_step)
            .and_then(|step| step.list.current_selection().map(|selection| (selection, step)))
            .map(|(selection, step)| match selection {
                InlineListSelection::RequestUserInputAnswer { question_id, selected, other } => {
                    let next_other = if other.is_some() {
                        step.submitted_freeform_value()
                    } else if step.notes.trim().is_empty() {
                        None
                    } else {
                        Some(step.notes.trim().to_string())
                    };
                    InlineListSelection::RequestUserInputAnswer { question_id, selected, other: next_other }
                }
                InlineListSelection::AskUserChoice { tab_id, choice_id, .. } => {
                    let notes = step.notes.trim();
                    let text = if notes.is_empty() {
                        None
                    } else {
                        Some(notes.to_string())
                    };
                    InlineListSelection::AskUserChoice { tab_id, choice_id, text }
                }
                _ => selection,
            })
    }

    /// Check if current step is completed
    fn current_step_completed(&self) -> bool {
        self.steps.get(self.current_step).is_some_and(|step| step.completed)
    }

    fn step_selected_custom_note_item_index(step: &WizardStepState) -> Option<usize> {
        let selected_visible = step.list.list_state.selected()?;
        let item_index = *step.list.visible_indices.get(selected_visible)?;
        let item = step.list.items.get(item_index)?;
        item.selection
            .as_ref()
            .filter(|selection| is_custom_note_selection(selection))
            .map(|_| item_index)
    }

    fn current_step_selected_custom_note_item_index(&self) -> Option<usize> {
        self.steps
            .get(self.current_step)
            .and_then(Self::step_selected_custom_note_item_index)
    }

    fn current_step_requires_custom_note_input(&self) -> bool {
        self.current_step_selected_custom_note_item_index().is_some()
    }

    fn current_step_supports_notes(&self) -> bool {
        self.steps
            .get(self.current_step)
            .and_then(|step| step.list.current_selection())
            .is_some_and(|selection| {
                matches!(
                    selection,
                    InlineListSelection::RequestUserInputAnswer { .. } | InlineListSelection::AskUserChoice { .. }
                )
            })
    }

    fn current_step_has_freeform_default(&self) -> bool {
        self.steps
            .get(self.current_step)
            .is_some_and(WizardStepState::has_freeform_default)
    }

    fn unanswered_count(&self) -> usize {
        self.steps.iter().filter(|step| !step.completed).count()
    }

    pub(crate) fn question_header(&self) -> String {
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
            if step.notes.is_empty()
                && let Some(placeholder) = step.freeform_placeholder.as_ref()
            {
                return Some(format!("{label} {placeholder}"));
            }
            Some(format!("{} {}", label, step.display_notes()))
        } else {
            None
        }
    }

    pub(crate) fn notes_active(&self) -> bool {
        self.steps.get(self.current_step).is_some_and(|step| step.notes_active)
    }

    pub(crate) fn instruction_lines(&self) -> Vec<String> {
        let step = match self.steps.get(self.current_step) {
            Some(s) => s,
            None => return Vec::new(),
        };
        if step.multi_select && !step.notes_active {
            return vec!["space to toggle | enter to continue | tab to add notes | esc to cancel".to_string()];
        }
        let custom_note_selected = self.current_step_requires_custom_note_input();

        if self.notes_active() {
            if custom_note_selected {
                vec![if self.current_step_has_freeform_default() {
                    "type custom note | enter to submit or accept default | esc to clear".to_string()
                } else {
                    "type custom note | enter to continue | esc to clear".to_string()
                }]
            } else {
                vec!["tab or esc to clear notes | enter to submit answer".to_string()]
            }
        } else {
            let mut lines = Vec::new();
            if custom_note_selected {
                lines.push(if self.current_step_has_freeform_default() {
                    "type custom note | enter to accept default".to_string()
                } else {
                    "type custom note | enter to continue".to_string()
                });
            } else if step.allow_freeform {
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
        self.steps.iter().filter_map(|step| step.answer.clone()).collect()
    }

    fn submit_current_selection(&mut self) -> ModalListKeyResult {
        if !self.steps.get(self.current_step).is_some_and(|step| step.multi_select)
            && self.current_step_requires_custom_note_input()
            && let Some(step) = self.steps.get_mut(self.current_step)
            && step.notes.trim().is_empty()
            && !step.has_freeform_default()
        {
            step.notes_active = true;
            return ModalListKeyResult::Redraw;
        }

        let Some(selection) = self.current_selection() else {
            return ModalListKeyResult::HandledNoRedraw;
        };

        match self.mode {
            WizardModalMode::TabbedList => ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
                OverlaySubmission::Wizard(vec![selection]),
            ))),
            WizardModalMode::MultiStep => {
                self.complete_current_step(selection);
                if self.current_step < self.steps.len().saturating_sub(1) {
                    self.current_step += 1;
                    ModalListKeyResult::Redraw
                } else {
                    ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
                        OverlaySubmission::Wizard(self.collect_answers()),
                    )))
                }
            }
        }
    }

    /// Check if all steps are completed
    pub fn all_steps_completed(&self) -> bool {
        self.steps.iter().all(|step| step.completed)
    }
}

impl WizardStepState {
    pub(in super::super) fn display_notes(&self) -> std::borrow::Cow<'_, str> {
        if self.mask_input {
            "•".repeat(self.notes.chars().count()).into()
        } else {
            self.notes.as_str().into()
        }
    }

    fn has_freeform_default(&self) -> bool {
        self.freeform_default.is_some()
    }

    fn submitted_freeform_value(&self) -> Option<String> {
        let notes = self.notes.trim();
        if !notes.is_empty() {
            return Some(notes.to_string());
        }

        self.freeform_default.clone()
    }
}
