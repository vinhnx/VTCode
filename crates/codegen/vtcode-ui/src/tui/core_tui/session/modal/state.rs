use crate::tui::config::constants::ui;
use crate::tui::ui::search::{FuzzyQuery, exact_terms_match, normalize_query};
use crate::tui::ui::tui::types::{
    InlineEvent, InlineItemKind, InlineListItem, InlineListSearchConfig, InlineListSelection, InlineStatus, InlineTone,
    OverlayEvent, OverlayHotkey, OverlayHotkeyAction, OverlayHotkeyKey, OverlaySelectionChange, OverlaySubmission,
    SecurePromptConfig, WizardModalMode, WizardStep,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::widgets::ListState;
use ratatui_cheese::input::InputState;

#[derive(Clone)]
pub struct ModalState {
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
    pub(crate) footer_hint: Option<String>,
    /// Latest apply/save/cancel feedback shown as a toned status strip.
    pub(crate) status: Option<InlineStatus>,
    pub(crate) hotkeys: Vec<OverlayHotkey>,
    pub(crate) list: Option<ModalListState>,
    pub(crate) secure_prompt: Option<SecurePromptConfig>,
    pub(crate) restore_input: bool,
    pub(crate) restore_cursor: bool,
    pub(crate) search: Option<ModalSearchState>,
    pub(crate) is_help_modal: bool,
}

/// State for a multi-step wizard modal with tabs for navigation
#[derive(Clone)]
pub struct WizardModalState {
    pub(crate) title: String,
    pub(crate) steps: Vec<WizardStepState>,
    pub(crate) current_step: usize,
    pub(crate) search: Option<ModalSearchState>,
    pub(crate) mode: WizardModalMode,
}

/// State for a single wizard step
#[derive(Clone)]
pub struct WizardStepState {
    /// Title displayed in the tab header
    pub(crate) title: String,
    /// Question or instruction shown above the list
    pub(crate) question: String,
    /// List state for selectable items
    pub(crate) list: ModalListState,
    /// Whether this step has been completed
    pub(crate) completed: bool,
    /// The selected answer for this step
    answer: Option<InlineListSelection>,
    /// Optional notes for the current step (free text)
    pub(crate) notes: String,
    /// Whether notes input is active for the current step
    pub(crate) notes_active: bool,

    allow_freeform: bool,
    pub(crate) freeform_label: Option<String>,
    pub(crate) freeform_placeholder: Option<String>,
    freeform_default: Option<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ModalKeyModifiers {
    pub(crate) control: bool,
    pub(crate) alt: bool,
    pub(crate) command: bool,
}

#[derive(Debug, Clone)]
pub enum ModalListKeyResult {
    NotHandled,
    HandledNoRedraw,
    Redraw,
    Emit(InlineEvent),
    Submit(InlineEvent),
    Cancel(InlineEvent),
}

#[derive(Clone)]
pub struct ModalListState {
    pub(crate) items: Vec<ModalListItem>,
    pub(crate) visible_indices: Vec<usize>,
    pub(crate) list_state: ListState,
    total_selectable: usize,
    filter_terms: Vec<String>,
    filter_query: Option<String>,
    viewport_rows: Option<u16>,
    density_behavior: ModalListDensityBehavior,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ModalListDensityBehavior {
    Adjustable,
    FixedComfortable,
}

fn config_list_navigation_hint() -> String {
    crate::design::keys::list_hint()
}

#[derive(Clone)]
pub struct ModalListItem {
    pub(crate) title: String,
    pub(crate) subtitle: Option<String>,
    pub(crate) badge: Option<String>,
    pub(crate) indent: u8,
    pub(crate) selection: Option<InlineListSelection>,
    search_value: Option<String>,
    pub(crate) is_divider: bool,
    pub(crate) value: Option<String>,
    pub(crate) badge_tone: InlineTone,
    pub(crate) kind: InlineItemKind,
}

#[derive(Clone)]
pub struct ModalSearchState {
    pub(crate) label: String,
    pub(crate) placeholder: Option<String>,
    pub(crate) query: String,
    pub(crate) fuzzy: bool,
}

impl From<InlineListSearchConfig> for ModalSearchState {
    fn from(config: InlineListSearchConfig) -> Self {
        Self {
            label: config.label,
            placeholder: config.placeholder,
            query: String::new(),
            fuzzy: config.fuzzy,
        }
    }
}

impl ModalSearchState {
    pub(crate) fn insert(&mut self, value: &str) {
        for ch in value.chars() {
            if matches!(ch, '\n' | '\r') {
                continue;
            }
            self.query.push(ch);
        }
    }

    fn push_char(&mut self, ch: char) {
        self.query.push(ch);
    }

    fn backspace(&mut self) -> bool {
        if self.query.pop().is_some() {
            return true;
        }
        false
    }

    fn clear(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        self.query.clear();
        true
    }
}

impl ModalState {
    pub(crate) fn hotkey_action(&self, key: &KeyEvent, modifiers: ModalKeyModifiers) -> Option<OverlayHotkeyAction> {
        self.hotkeys.iter().find_map(|hotkey| match hotkey.key {
            OverlayHotkeyKey::CtrlChar(ch)
                if modifiers.control
                    && !modifiers.alt
                    && !modifiers.command
                    && matches!(key.code, KeyCode::Char(key_ch) if key_ch.eq_ignore_ascii_case(&ch)) =>
            {
                Some(hotkey.action)
            }
            OverlayHotkeyKey::Char(ch)
                if !modifiers.control
                    && !modifiers.alt
                    && !modifiers.command
                    && matches!(key.code, KeyCode::Char(key_ch) if key_ch.eq_ignore_ascii_case(&ch)) =>
            {
                Some(hotkey.action)
            }
            _ => None,
        })
    }

    pub(crate) fn handle_list_key_event(&mut self, key: &KeyEvent, modifiers: ModalKeyModifiers) -> ModalListKeyResult {
        let Some(list) = self.list.as_mut() else {
            return ModalListKeyResult::NotHandled;
        };

        if let Some(search) = self.search.as_mut() {
            match key.code {
                KeyCode::Char(ch) if !modifiers.control && !modifiers.alt && !modifiers.command => {
                    let previous = list.current_selection();
                    search.push_char(ch);
                    list.apply_search(&search.query, search.fuzzy);
                    if let Some(event) = selection_change_event(list, previous) {
                        return ModalListKeyResult::Emit(event);
                    }
                    return ModalListKeyResult::Redraw;
                }
                KeyCode::Backspace => {
                    if search.backspace() {
                        let previous = list.current_selection();
                        list.apply_search(&search.query, search.fuzzy);
                        if let Some(event) = selection_change_event(list, previous) {
                            return ModalListKeyResult::Emit(event);
                        }
                        return ModalListKeyResult::Redraw;
                    }
                    return ModalListKeyResult::HandledNoRedraw;
                }
                KeyCode::Delete => {
                    if search.clear() {
                        let previous = list.current_selection();
                        list.apply_search(&search.query, search.fuzzy);
                        if let Some(event) = selection_change_event(list, previous) {
                            return ModalListKeyResult::Emit(event);
                        }
                        return ModalListKeyResult::Redraw;
                    }
                    return ModalListKeyResult::HandledNoRedraw;
                }
                KeyCode::Esc if search.clear() => {
                    let previous = list.current_selection();
                    list.apply_search(&search.query, search.fuzzy);
                    if let Some(event) = selection_change_event(list, previous) {
                        return ModalListKeyResult::Emit(event);
                    }
                    return ModalListKeyResult::Redraw;
                }
                _ => {}
            }
        }

        let previous_selection = list.current_selection();
        match key.code {
            // Row density is fixed at construction (comfortable for all lists);
            // swallow the legacy toggle chord so it never leaks into the
            // composer behind the modal.
            KeyCode::Char('d') | KeyCode::Char('D') if modifiers.alt => ModalListKeyResult::HandledNoRedraw,
            // Numbered shortcuts (1-9) for search-less modals: two-step
            // select, Enter confirms. The search block above already consumed
            // character keys while a search box is open, so reaching here
            // with a digit means no search is active; the guard restates it
            // for robustness. Every digit outcome is consumed so shortcuts
            // never leak into the composer behind the modal.
            KeyCode::Char(ch)
                if !modifiers.control && !modifiers.alt && !modifiers.command && self.search.is_none() =>
            {
                if !ch.is_ascii_digit() {
                    ModalListKeyResult::NotHandled
                } else if matches!(ch, '1'..='9')
                    && list.numbered_shortcuts()
                    && list.select_nth_selectable((ch as u8 - b'1') as usize)
                {
                    if let Some(event) = selection_change_event(list, previous_selection) {
                        ModalListKeyResult::Emit(event)
                    } else {
                        ModalListKeyResult::Redraw
                    }
                } else {
                    ModalListKeyResult::HandledNoRedraw
                }
            }
            KeyCode::Up => {
                if modifiers.command {
                    list.select_first();
                } else {
                    list.select_previous();
                }
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::Down => {
                if modifiers.command {
                    list.select_last();
                } else {
                    list.select_next();
                }
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::PageUp => {
                list.page_up();
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::PageDown => {
                list.page_down();
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::Home => {
                list.select_first();
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::End => {
                list.select_last();
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::Tab => {
                // With no search active, Tab moves to first item for autocomplete behavior
                // If search is active, we already handled it above
                if self.search.is_none() && !list.visible_indices.is_empty() {
                    list.select_first();
                } else {
                    list.select_next();
                }
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::BackTab => {
                list.select_previous();
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::Left => {
                if let Some(selection) = list.current_selection()
                    && let Some(adjusted) = map_config_selection_for_arrow(&selection, true)
                {
                    return ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
                        OverlaySubmission::Selection(adjusted),
                    )));
                }
                list.select_previous();
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::Right => {
                if let Some(selection) = list.current_selection()
                    && let Some(adjusted) = map_config_selection_for_arrow(&selection, false)
                {
                    return ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
                        OverlaySubmission::Selection(adjusted),
                    )));
                }
                list.select_next();
                if let Some(event) = selection_change_event(list, previous_selection) {
                    ModalListKeyResult::Emit(event)
                } else {
                    ModalListKeyResult::Redraw
                }
            }
            KeyCode::Enter => {
                if let Some(selection) = list.current_selection() {
                    ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
                        OverlaySubmission::Selection(selection),
                    )))
                } else {
                    ModalListKeyResult::HandledNoRedraw
                }
            }
            KeyCode::Esc => ModalListKeyResult::Cancel(InlineEvent::Overlay(OverlayEvent::Cancelled)),
            KeyCode::Char(ch) if modifiers.control || modifiers.alt => match ch {
                'c' | 'C' if modifiers.control => ModalListKeyResult::Cancel(InlineEvent::Interrupt),
                'n' | 'N' | 'j' | 'J' => {
                    list.select_next();
                    if let Some(event) = selection_change_event(list, previous_selection) {
                        ModalListKeyResult::Emit(event)
                    } else {
                        ModalListKeyResult::Redraw
                    }
                }
                'p' | 'P' | 'k' | 'K' => {
                    list.select_previous();
                    if let Some(event) = selection_change_event(list, previous_selection) {
                        ModalListKeyResult::Emit(event)
                    } else {
                        ModalListKeyResult::Redraw
                    }
                }
                _ => ModalListKeyResult::NotHandled,
            },
            KeyCode::Char('\u{3}') => ModalListKeyResult::Cancel(InlineEvent::Interrupt),
            _ => ModalListKeyResult::NotHandled,
        }
    }

    pub(crate) fn handle_list_mouse_click(&mut self, visible_index: usize) -> ModalListKeyResult {
        let Some(list) = self.list.as_mut() else {
            return ModalListKeyResult::NotHandled;
        };
        let Some(&item_index) = list.visible_indices.get(visible_index) else {
            return ModalListKeyResult::HandledNoRedraw;
        };
        if list.items.get(item_index).and_then(|item| item.selection.as_ref()).is_none() {
            return ModalListKeyResult::HandledNoRedraw;
        }

        let previous_selection = list.current_selection();
        if list.list_state.selected() == Some(visible_index) {
            if let Some(selection) = list.current_selection() {
                return ModalListKeyResult::Submit(InlineEvent::Overlay(OverlayEvent::Submitted(
                    OverlaySubmission::Selection(selection),
                )));
            }
            return ModalListKeyResult::HandledNoRedraw;
        }

        list.list_state.select(Some(visible_index));
        if let Some(rows) = list.viewport_rows {
            list.ensure_visible(rows);
        }
        if let Some(event) = selection_change_event(list, previous_selection) {
            ModalListKeyResult::Emit(event)
        } else {
            ModalListKeyResult::Redraw
        }
    }

    pub(crate) fn handle_list_mouse_scroll(&mut self, down: bool) -> ModalListKeyResult {
        let Some(list) = self.list.as_mut() else {
            return ModalListKeyResult::NotHandled;
        };

        let previous_selection = list.current_selection();
        if down {
            list.select_next();
        } else {
            list.select_previous();
        }

        if let Some(event) = selection_change_event(list, previous_selection) {
            ModalListKeyResult::Emit(event)
        } else {
            ModalListKeyResult::Redraw
        }
    }
}

fn selection_change_event(list: &ModalListState, previous: Option<InlineListSelection>) -> Option<InlineEvent> {
    let current = list.current_selection();
    if current == previous {
        return None;
    }
    current
        .map(|selection| InlineEvent::Overlay(OverlayEvent::SelectionChanged(OverlaySelectionChange::List(selection))))
}

fn is_custom_note_selection(selection: &InlineListSelection) -> bool {
    matches!(
        selection,
        InlineListSelection::RequestUserInputAnswer {
            selected,
            other,
            ..
        } if selected.is_empty() && other.is_some()
    )
}

fn map_config_selection_for_arrow(selection: &InlineListSelection, is_left: bool) -> Option<InlineListSelection> {
    let InlineListSelection::ConfigAction(action) = selection else {
        return None;
    };

    if action.ends_with(":cycle") {
        if is_left {
            let key = action.trim_end_matches(":cycle");
            return Some(InlineListSelection::ConfigAction(format!("{key}:cycle_prev")));
        }
        return Some(selection.clone());
    }

    if action.ends_with(":inc") {
        if is_left {
            let key = action.trim_end_matches(":inc");
            return Some(InlineListSelection::ConfigAction(format!("{key}:dec")));
        }
        return Some(selection.clone());
    }

    if action.ends_with(":dec") {
        if is_left {
            return Some(selection.clone());
        }
        let key = action.trim_end_matches(":dec");
        return Some(InlineListSelection::ConfigAction(format!("{key}:inc")));
    }

    if action.ends_with(":toggle") {
        let _ = is_left;
        return Some(selection.clone());
    }

    None
}

impl ModalListItem {
    /// Group-header rows get bold titles and a blank gap above. Explicit
    /// `Hint` rows are dimmed notes and must not be treated as headers.
    pub(crate) fn is_header(&self) -> bool {
        !self.is_divider && self.selection.is_none() && self.kind != InlineItemKind::Hint
    }

    /// Dimmed note row (non-selectable, not a group header).
    pub(crate) fn is_hint(&self) -> bool {
        !self.is_divider && self.kind == InlineItemKind::Hint
    }

    /// Lowercased match corpus: caller `search_value` when present, else
    /// title + subtitle so labels and descriptions stay searchable.
    fn search_haystack(&self) -> std::borrow::Cow<'_, str> {
        if let Some(value) = self.search_value.as_deref() {
            return std::borrow::Cow::Borrowed(value);
        }
        let mut parts = vec![self.title.as_str()];
        if let Some(subtitle) = self.subtitle.as_deref() {
            parts.push(subtitle);
        }
        std::borrow::Cow::Owned(normalize_query(&parts.join(" ")))
    }

    fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        // `search_value` is already lowercased at construction; avoid
        // re-normalizing a query or allocating a filter per row.
        exact_terms_match(query, &self.search_haystack())
    }
}

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
    item.title.chars().all(|ch| symbol.chars().any(|needle| needle == ch))
}

impl ModalListState {
    pub(crate) fn new(items: Vec<InlineListItem>, selected: Option<InlineListSelection>) -> Self {
        let converted: Vec<ModalListItem> = items
            .into_iter()
            .map(|item| {
                let is_divider = is_divider_title(&item);
                let search_value = item.search_value.as_ref().map(|value| value.to_ascii_lowercase());
                ModalListItem {
                    title: item.title,
                    subtitle: item.subtitle,
                    badge: item.badge,
                    indent: item.indent,
                    selection: item.selection,
                    search_value,
                    is_divider,
                    value: item.value,
                    badge_tone: item.badge_tone,
                    kind: item.kind,
                }
            })
            .collect();
        let total_selectable = converted.iter().filter(|item| item.selection.is_some()).count();
        let density_behavior = Self::density_behavior_for_items(&converted);
        let mut modal_state = Self {
            visible_indices: (0..converted.len()).collect(),
            items: converted,
            list_state: ListState::default(),
            total_selectable,
            filter_terms: Vec::new(),
            filter_query: None,
            viewport_rows: None,
            density_behavior,
        };
        modal_state.select_initial(selected);
        modal_state
    }

    fn density_behavior_for_items(items: &[ModalListItem]) -> ModalListDensityBehavior {
        if items
            .iter()
            .any(|item| matches!(item.selection, Some(InlineListSelection::ConfigAction(_))))
        {
            ModalListDensityBehavior::FixedComfortable
        } else {
            ModalListDensityBehavior::Adjustable
        }
    }

    pub(crate) fn current_selection(&self) -> Option<InlineListSelection> {
        self.list_state
            .selected()
            .and_then(|index| self.visible_indices.get(index))
            .and_then(|&item_index| self.items.get(item_index))
            .and_then(|item| item.selection.clone())
    }

    fn get_best_matching_item(&self, query: &str) -> Option<String> {
        if query.is_empty() {
            return None;
        }

        let normalized_query = normalize_query(query);
        self.visible_indices
            .iter()
            .filter_map(|&idx| self.items.get(idx))
            .filter(|item| item.selection.is_some())
            .filter_map(|item| item.search_value.as_ref())
            .find(|search_value| exact_terms_match(&normalized_query, search_value))
            .cloned()
    }

    fn select_previous(&mut self) {
        if self.visible_indices.is_empty() {
            return;
        }
        let Some(mut index) = self.list_state.selected() else {
            if let Some(last) = self.last_selectable_index() {
                self.list_state.select(Some(last));
                if let Some(rows) = self.viewport_rows {
                    self.ensure_visible(rows);
                }
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
                if let Some(rows) = self.viewport_rows {
                    self.ensure_visible(rows);
                }
                return;
            }
        }

        if let Some(first) = self.first_selectable_index() {
            self.list_state.select(Some(first));
        } else {
            self.list_state.select(None);
        }
        if let Some(rows) = self.viewport_rows {
            self.ensure_visible(rows);
        }
    }

    pub(crate) fn select_next(&mut self) {
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
                if let Some(rows) = self.viewport_rows {
                    self.ensure_visible(rows);
                }
                break;
            }
        }
    }

    fn select_first(&mut self) {
        if let Some(first) = self.first_selectable_index() {
            self.list_state.select(Some(first));
        } else {
            self.list_state.select(None);
        }
        if let Some(rows) = self.viewport_rows {
            self.ensure_visible(rows);
        }
    }

    pub(crate) fn select_last(&mut self) {
        if let Some(last) = self.last_selectable_index() {
            self.list_state.select(Some(last));
        } else {
            self.list_state.select(None);
        }
        if let Some(rows) = self.viewport_rows {
            self.ensure_visible(rows);
        }
    }

    pub(crate) fn selected_is_last(&self) -> bool {
        let Some(selected) = self.list_state.selected() else {
            return false;
        };
        self.last_selectable_index().is_some_and(|last| selected == last)
    }

    fn select_nth_selectable(&mut self, target_index: usize) -> bool {
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

    /// Max numbered shortcut: digit keys 1-9 jump to the nth visible option.
    pub(super) const MAX_NUMBERED_SHORTCUT: usize = 9;

    /// Whether digit shortcuts are offered for this list: a scannable number
    /// of visible selectable options. Key routing ANDs the modal search
    /// state (digits filter while a search box is open); render callers
    /// pass their own search knowledge — one shared count gate so the two
    /// sides can never disagree.
    pub(super) fn numbered_shortcuts(&self) -> bool {
        let count = self.visible_selectable_count();
        count > 0 && count <= Self::MAX_NUMBERED_SHORTCUT
    }

    /// 1-based shortcut number for a visible row: its position among visible
    /// selectable options, skipping dividers and other non-selectable rows.
    /// Returns `None` unless the whole list qualifies (`numbered_shortcuts`),
    /// so crowded lists never show dead numbers. Mirrors
    /// `select_nth_selectable` so badges and digit keys agree.
    pub(super) fn shortcut_number(&self, visible_pos: usize) -> Option<usize> {
        if !self.numbered_shortcuts() {
            return None;
        }
        let &item_index = self.visible_indices.get(visible_pos)?;
        self.items.get(item_index)?.selection.as_ref()?;
        let ordinal = self
            .visible_indices
            .iter()
            .take(visible_pos + 1)
            .filter(|&&idx| self.items.get(idx).is_some_and(|item| item.selection.is_some()))
            .count();
        (ordinal <= Self::MAX_NUMBERED_SHORTCUT).then_some(ordinal)
    }

    fn page_up(&mut self) {
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

    fn page_down(&mut self) {
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

    pub(crate) fn set_viewport_rows(&mut self, rows: u16) {
        self.viewport_rows = Some(rows);
        if rows == 0 {
            *self.list_state.offset_mut() = 0;
            return;
        }
        let max_offset = self.max_scroll_offset();
        if self.list_state.offset() > max_offset {
            *self.list_state.offset_mut() = max_offset;
        }
    }

    pub(crate) fn max_scroll_offset(&self) -> usize {
        let Some(rows) = self.viewport_rows else {
            return self.visible_indices.len().saturating_sub(1);
        };
        if rows == 0 {
            return 0;
        }
        self.visible_indices.len().saturating_sub(rows as usize)
    }

    pub(super) fn ensure_visible(&mut self, viewport: u16) {
        let Some(selected) = self.list_state.selected() else {
            return;
        };
        if viewport == 0 {
            return;
        }
        let visible = viewport as usize;
        let max_offset = self.visible_indices.len().saturating_sub(visible.max(1));
        let offset = self.list_state.offset().min(max_offset);
        *self.list_state.offset_mut() = if selected < offset {
            selected
        } else if selected >= offset + visible {
            (selected + 1 - visible).min(max_offset)
        } else {
            offset
        };
    }

    pub(crate) fn apply_search(&mut self, query: &str, fuzzy: bool) {
        let preferred = self.current_selection();
        self.apply_search_with_preference(query, preferred, fuzzy);
    }

    pub(crate) fn apply_search_with_preference(
        &mut self,
        query: &str,
        preferred: Option<InlineListSelection>,
        fuzzy: bool,
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

        if fuzzy {
            let mut fuzzy_query = FuzzyQuery::new(&normalized_query);
            self.visible_indices = self.fuzzy_visible_indices(&mut fuzzy_query);
            self.filter_terms = terms;
            self.filter_query = Some(trimmed.to_owned());
            self.select_initial(preferred);
            return;
        }

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

    /// Fuzzy filter with relevance ranking. Items are grouped into
    /// divider/header/children blocks (selectables without a preceding header
    /// rank individually); blocks are ordered by their best fuzzy score while
    /// a matching header keeps its whole group, preserving curated order
    /// within blocks.
    fn fuzzy_visible_indices(&self, query: &mut FuzzyQuery) -> Vec<usize> {
        /// One child item of a header block: list index plus its fuzzy score
        /// (`None` when the item does not match the query).
        struct ScoredIndex {
            index: usize,
            score: Option<u32>,
        }

        /// A divider/header/children run ranked as one unit; a matching header
        /// keeps all of its children, otherwise only matching children show.
        struct FuzzySearchBlock {
            divider_index: Option<usize>,
            header_index: Option<usize>,
            header_score: Option<u32>,
            child_scores: Vec<ScoredIndex>,
        }

        impl FuzzySearchBlock {
            fn best_score(&self) -> Option<u32> {
                self.header_score
                    .into_iter()
                    .chain(self.child_scores.iter().filter_map(|child| child.score))
                    .max()
            }
        }

        let mut blocks: Vec<FuzzySearchBlock> = Vec::new();
        let mut open_block: Option<FuzzySearchBlock> = None;
        let mut pending_divider: Option<usize> = None;

        for (index, item) in self.items.iter().enumerate() {
            if item.is_divider {
                if let Some(block) = open_block.take() {
                    blocks.push(block);
                }
                pending_divider = Some(index);
                continue;
            }

            let haystack = item.search_haystack();
            let score = query.score(&haystack);

            if item.is_header() {
                if let Some(block) = open_block.take() {
                    blocks.push(block);
                }
                open_block = Some(FuzzySearchBlock {
                    divider_index: pending_divider.take(),
                    header_index: Some(index),
                    header_score: score,
                    child_scores: Vec::new(),
                });
                continue;
            }

            match open_block.as_mut() {
                // Selectables directly after a header belong to its block.
                Some(block) if block.header_index.is_some() => {
                    block.child_scores.push(ScoredIndex { index, score });
                }
                _ => {
                    if let Some(block) = open_block.take() {
                        blocks.push(block);
                    }
                    open_block = Some(FuzzySearchBlock {
                        divider_index: pending_divider.take(),
                        header_index: None,
                        header_score: None,
                        child_scores: vec![ScoredIndex { index, score }],
                    });
                }
            }
        }
        if let Some(block) = open_block.take() {
            blocks.push(block);
        }

        blocks.retain(|block| block.best_score().is_some());
        blocks.sort_by_key(|block| std::cmp::Reverse(block.best_score()));

        let mut indices = Vec::new();
        for block in blocks {
            if let Some(divider_index) = block.divider_index {
                indices.push(divider_index);
            }
            if let Some(header_index) = block.header_index {
                indices.push(header_index);
            }
            let header_matched = block.header_score.is_some();
            for child in block.child_scores {
                if header_matched || child.score.is_some() {
                    indices.push(child.index);
                }
            }
        }
        indices
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
        self.visible_indices.iter().position(|&idx| self.items[idx].selection.is_some())
    }

    fn last_selectable_index(&self) -> Option<usize> {
        self.visible_indices
            .iter()
            .rposition(|&idx| self.items[idx].selection.is_some())
    }

    pub(super) fn filter_active(&self) -> bool {
        self.filter_query.as_ref().is_some_and(|value| !value.is_empty())
    }

    #[cfg(test)]
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

    pub(super) fn non_filter_summary_text(&self, footer_hint: Option<&str>) -> Option<String> {
        if !self.has_non_filter_summary(footer_hint) {
            return None;
        }
        match self.density_behavior {
            ModalListDensityBehavior::FixedComfortable => Some(config_list_navigation_hint()),
            ModalListDensityBehavior::Adjustable => footer_hint.filter(|hint| !hint.is_empty()).map(ToOwned::to_owned),
        }
    }

    /// Summary rows above the list: filter/keyboard hint, plus one extra row
    /// when a status strip is shown (`has_status`).
    pub(crate) fn summary_line_rows(&self, footer_hint: Option<&str>, has_status: bool) -> usize {
        let mut rows = 0;
        if has_status {
            rows += 1;
        }
        if self.filter_active() || self.has_non_filter_summary(footer_hint) {
            rows += 1;
        }
        rows
    }

    fn has_non_filter_summary(&self, footer_hint: Option<&str>) -> bool {
        match self.density_behavior {
            ModalListDensityBehavior::FixedComfortable => true,
            ModalListDensityBehavior::Adjustable => footer_hint.is_some_and(|hint| !hint.is_empty()),
        }
    }

    fn page_step(&self) -> usize {
        let rows = self.viewport_rows.unwrap_or(0).max(1);
        usize::from(rows)
    }
}

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

    /// Get current selection from the active step
    fn current_selection(&self) -> Option<InlineListSelection> {
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
            Some(format!("{} {}", label, step.notes))
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
        if self.current_step_requires_custom_note_input()
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
