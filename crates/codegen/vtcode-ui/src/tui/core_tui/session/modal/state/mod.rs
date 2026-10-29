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
    pub(super) viewport_rows: Option<u16>,
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

/// Typed selections whose modals block until the user responds (approvals,
/// permissions, HITL questions/input, file conflicts, session limits).
fn attends_to_user(selection: &InlineListSelection) -> bool {
    matches!(
        selection,
        InlineListSelection::ToolApproval(_)
            | InlineListSelection::ToolApprovalDenyOnce
            | InlineListSelection::ToolApprovalSession
            | InlineListSelection::ToolApprovalPermanent
            | InlineListSelection::ToolApprovalEnable
            | InlineListSelection::PlanApprovalExecute
            | InlineListSelection::PlanApprovalFreshContext
            | InlineListSelection::PlanApprovalEditPlan
            | InlineListSelection::PlanApprovalDiscuss
            | InlineListSelection::PlanApprovalAutoAccept
            | InlineListSelection::PlanApprovalSwitchBuild
            | InlineListSelection::PlanApprovalSwitchAuto
            | InlineListSelection::AskUserChoice { .. }
            | InlineListSelection::RequestUserInputAnswer { .. }
            | InlineListSelection::FileConflictReload
            | InlineListSelection::FileConflictViewDiff
            | InlineListSelection::FileConflictAbort
            | InlineListSelection::SessionLimitIncrease(_)
    )
}

mod list_state;
mod wizard_state;

impl ModalState {
    /// Whether the open modal gates the agent on the user: an approval or
    /// permission prompt, a HITL question/input, a file-conflict decision, or
    /// a session-limit confirmation. Typed selections are authoritative so a
    /// reworded title cannot hide the gate; wording-only fallbacks stay at
    /// the caller.
    pub(crate) fn attention_required(&self) -> bool {
        self.secure_prompt.is_some()
            || self.list.as_ref().is_some_and(|list| {
                list.items
                    .iter()
                    .any(|item| item.selection.as_ref().is_some_and(attends_to_user))
            })
    }

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
