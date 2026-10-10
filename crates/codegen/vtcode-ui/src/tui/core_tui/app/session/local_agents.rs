use crate::tui::core_tui::session::list_navigator::ListNavigator;
use crate::tui::core_tui::types::{ExecSessionAction, LocalAgentEntry, LocalAgentKind};
use hashbrown::HashSet;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{AppSession, InlineEvent};

#[derive(Clone, Debug, Default)]
pub(super) struct LocalAgentsState {
    entries: Vec<LocalAgentEntry>,
    navigator: ListNavigator,
    active_ids: HashSet<String>,
    /// List body rect from the last inline bottom-dock paint (mouse hit-testing).
    list_area: Option<ratatui::layout::Rect>,
    /// Outer inline panel rect from the last paint (mouse hit-testing).
    window_area: Option<ratatui::layout::Rect>,
    expanded: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LocalAgentsUpdate {
    pub(super) has_new_delegated_entries: bool,
}

pub(super) enum LocalAgentsKeyResult {
    NotHandled,
    Handled,
    Emit(InlineEvent),
}

impl LocalAgentsState {
    pub(super) fn set_entries(&mut self, entries: Vec<LocalAgentEntry>) -> LocalAgentsUpdate {
        let previous_id = self.selected_entry().map(|entry| entry.id.clone());
        let next_active_ids = entries.iter().map(|entry| entry.id.clone()).collect::<HashSet<_>>();
        let has_new_delegated_entries = entries
            .iter()
            .any(|entry| entry.kind == LocalAgentKind::Delegated && !self.active_ids.contains(entry.id.as_str()));
        self.entries = entries;
        self.navigator.set_item_count(self.entries.len());
        self.active_ids = next_active_ids;

        if self.entries.is_empty() {
            return LocalAgentsUpdate { has_new_delegated_entries };
        }

        if let Some(previous_id) = previous_id
            && let Some(index) = self.entries.iter().position(|entry| entry.id == previous_id)
        {
            self.navigator.select_index(index);
            return LocalAgentsUpdate { has_new_delegated_entries };
        }

        self.navigator.select_first();
        LocalAgentsUpdate { has_new_delegated_entries }
    }

    pub(super) fn entries(&self) -> &[LocalAgentEntry] {
        &self.entries
    }

    /// Number of entries still in a loading state (running subagents,
    /// background subprocesses, or retained exec sessions). Single source for
    /// both the drawer shimmer and the global background loading signal.
    pub(super) fn loading_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.is_loading()).count()
    }

    /// Retained terminal rows (completed / failed / stopped / exited).
    pub(super) fn finished_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.is_finished()).count()
    }

    pub(super) fn set_list_area(&mut self, area: Option<ratatui::layout::Rect>) {
        self.list_area = area;
    }

    pub(super) fn list_area(&self) -> Option<ratatui::layout::Rect> {
        self.list_area
    }

    pub(super) fn set_window_area(&mut self, area: Option<ratatui::layout::Rect>) {
        self.window_area = area;
    }

    pub(super) fn window_area(&self) -> Option<ratatui::layout::Rect> {
        self.window_area
    }

    pub(super) fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub(super) fn toggle_expanded(&mut self) -> bool {
        self.expanded = !self.expanded;
        self.expanded
    }

    fn has_entries(&self) -> bool {
        !self.entries.is_empty()
    }

    pub(super) fn selected(&self) -> Option<usize> {
        self.navigator.selected()
    }

    pub(super) fn select_index(&mut self, index: usize) -> bool {
        self.navigator.select_index(index)
    }

    pub(super) fn move_selection_up(&mut self) -> bool {
        self.navigator.move_up()
    }

    pub(super) fn move_selection_down(&mut self) -> bool {
        self.navigator.move_down()
    }

    fn page_up(&mut self, step: usize) -> bool {
        self.navigator.page_up(step)
    }

    fn page_down(&mut self, step: usize) -> bool {
        self.navigator.page_down(step)
    }

    pub(super) fn set_visible_rows(&mut self, rows: usize) {
        self.navigator.set_visible_rows(rows);
    }

    fn visible_rows(&self) -> usize {
        self.navigator.visible_rows()
    }

    pub(super) fn scroll_offset(&self) -> usize {
        self.navigator.scroll_offset()
    }

    pub(super) fn set_scroll_offset(&mut self, offset: usize) {
        self.navigator.set_scroll_offset(offset);
    }

    fn selected_entry(&self) -> Option<&LocalAgentEntry> {
        self.selected().and_then(|index| self.entries.get(index))
    }
}

impl AppSession {
    pub(crate) fn local_agents_is_expanded(&self) -> bool {
        self.local_agents_state.is_expanded()
    }

    pub(crate) fn local_agents_entry_count(&self) -> usize {
        self.local_agents_state.entries().len()
    }

    pub(super) fn should_open_local_agents_with_down(
        &self,
        key: &KeyEvent,
        has_control: bool,
        has_alt: bool,
        has_command: bool,
    ) -> bool {
        matches!(key.code, KeyCode::Down)
            && !has_control
            && !has_alt
            && !has_command
            && !self.local_agents_visible()
            && !self.has_active_overlay()
            && self.core.input_manager.content().trim().is_empty()
            && self.core.input_manager.history_index().is_none()
            && self.local_agents_state.has_entries()
    }

    pub(super) fn handle_local_agents_key(&mut self, key: &KeyEvent) -> LocalAgentsKeyResult {
        if !self.local_agents_visible() {
            return LocalAgentsKeyResult::NotHandled;
        }

        match key.code {
            KeyCode::Up => {
                self.local_agents_state.move_selection_up();
                self.mark_dirty();
                LocalAgentsKeyResult::Handled
            }
            KeyCode::Down => {
                self.local_agents_state.move_selection_down();
                self.mark_dirty();
                LocalAgentsKeyResult::Handled
            }
            KeyCode::PageUp => {
                let step = self.local_agents_state.visible_rows().max(1);
                self.local_agents_state.page_up(step);
                self.mark_dirty();
                LocalAgentsKeyResult::Handled
            }
            KeyCode::PageDown => {
                let step = self.local_agents_state.visible_rows().max(1);
                self.local_agents_state.page_down(step);
                self.mark_dirty();
                LocalAgentsKeyResult::Handled
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.local_agents_state.move_selection_down();
                self.mark_dirty();
                LocalAgentsKeyResult::Handled
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(event) = self.selected_exec_session_action_event(ExecSessionAction::Preview) {
                    LocalAgentsKeyResult::Emit(event)
                } else {
                    self.local_agents_state.move_selection_up();
                    self.mark_dirty();
                    LocalAgentsKeyResult::Handled
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') if key.modifiers.contains(KeyModifiers::CONTROL) => self
                .selected_exec_session_action_event(ExecSessionAction::Focus)
                .map_or(LocalAgentsKeyResult::NotHandled, LocalAgentsKeyResult::Emit),
            KeyCode::Char('o') | KeyCode::Char('O') if key.modifiers.contains(KeyModifiers::ALT) => self
                .selected_local_agent_transcript_event()
                .map_or(LocalAgentsKeyResult::Handled, LocalAgentsKeyResult::Emit),
            KeyCode::Char('k') | KeyCode::Char('K') if key.modifiers.contains(KeyModifiers::CONTROL) => self
                .selected_local_agent_stop_event()
                .map_or(LocalAgentsKeyResult::Handled, LocalAgentsKeyResult::Emit),
            KeyCode::Char('x') | KeyCode::Char('X') if key.modifiers.contains(KeyModifiers::CONTROL) => self
                .selected_local_agent_force_cancel_event()
                .map_or(LocalAgentsKeyResult::Handled, LocalAgentsKeyResult::Emit),
            KeyCode::Char('e') | KeyCode::Char('E') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.local_agents_state.toggle_expanded();
                self.mark_dirty();
                LocalAgentsKeyResult::Handled
            }
            KeyCode::Enter => self
                .selected_local_agent_inspect_event()
                .map_or(LocalAgentsKeyResult::Handled, LocalAgentsKeyResult::Emit),
            KeyCode::Esc => {
                self.close_local_agents_drawer(true);
                self.mark_dirty();
                LocalAgentsKeyResult::Handled
            }
            _ => LocalAgentsKeyResult::NotHandled,
        }
    }

    fn selected_local_agent_inspect_event(&mut self) -> Option<InlineEvent> {
        let entry = self.local_agents_state.selected_entry()?.clone();
        self.mark_dirty();
        Some(InlineEvent::Submit(
            match entry.kind {
                LocalAgentKind::Delegated => {
                    format!("/agent inspect {}", entry.id)
                }
                LocalAgentKind::Background => {
                    format!("/subprocesses inspect {}", entry.id)
                }
                LocalAgentKind::ExecSession => {
                    return Some(InlineEvent::ExecSessionAction { id: entry.id, action: ExecSessionAction::Inspect });
                }
            }
            .into(),
        ))
    }

    fn selected_local_agent_transcript_event(&mut self) -> Option<InlineEvent> {
        let path = self
            .local_agents_state
            .selected_entry()?
            .transcript_path
            .as_ref()?
            .display()
            .to_string();
        self.mark_dirty();
        Some(InlineEvent::OpenFileInEditor(path))
    }

    fn selected_local_agent_stop_event(&mut self) -> Option<InlineEvent> {
        let entry = self.local_agents_state.selected_entry()?.clone();
        self.mark_dirty();
        Some(InlineEvent::Submit(
            match entry.kind {
                LocalAgentKind::Delegated => {
                    format!("/agent close {}", entry.id)
                }
                LocalAgentKind::Background => {
                    format!("/subprocesses stop {}", entry.id)
                }
                LocalAgentKind::ExecSession => {
                    return Some(InlineEvent::ExecSessionAction {
                        id: entry.id,
                        action: ExecSessionAction::GracefulTerminate,
                    });
                }
            }
            .into(),
        ))
    }

    fn selected_local_agent_force_cancel_event(&mut self) -> Option<InlineEvent> {
        let entry = self.local_agents_state.selected_entry()?.clone();
        self.mark_dirty();
        Some(InlineEvent::Submit(
            match entry.kind {
                LocalAgentKind::Delegated => {
                    format!("/agent close {}", entry.id)
                }
                LocalAgentKind::Background => {
                    format!("/subprocesses cancel {}", entry.id)
                }
                LocalAgentKind::ExecSession => {
                    return Some(InlineEvent::ExecSessionAction {
                        id: entry.id,
                        action: ExecSessionAction::ForceTerminateOrClose,
                    });
                }
            }
            .into(),
        ))
    }

    fn selected_exec_session_action_event(&mut self, action: ExecSessionAction) -> Option<InlineEvent> {
        let entry = self.local_agents_state.selected_entry()?.clone();
        if entry.kind != LocalAgentKind::ExecSession {
            return None;
        }
        self.mark_dirty();
        Some(InlineEvent::ExecSessionAction { id: entry.id, action })
    }
}
