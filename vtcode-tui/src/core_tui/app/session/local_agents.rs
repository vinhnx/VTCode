use crate::core_tui::session::list_navigator::ListNavigator;
use crate::core_tui::types::LocalAgentEntry;
use hashbrown::HashSet;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{AppSession, InlineEvent, transient::TransientSurface};

#[derive(Clone, Debug, Default)]
pub(super) struct LocalAgentsState {
    entries: Vec<LocalAgentEntry>,
    navigator: ListNavigator,
    active_ids: HashSet<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LocalAgentsUpdate {
    pub(super) has_new_delegated_entries: bool,
}

impl LocalAgentsState {
    pub(super) fn set_entries(&mut self, entries: Vec<LocalAgentEntry>) -> LocalAgentsUpdate {
        let previous_id = self.selected_entry().map(|entry| entry.id.clone());
        let next_active_ids = entries
            .iter()
            .map(|entry| entry.id.clone())
            .collect::<HashSet<_>>();
        let has_new_delegated_entries = entries.iter().any(|entry| {
            entry.kind == crate::core_tui::types::LocalAgentKind::Delegated
                && !self.active_ids.contains(entry.id.as_str())
        });
        self.entries = entries;
        self.navigator.set_item_count(self.entries.len());
        self.active_ids = next_active_ids;

        if self.entries.is_empty() {
            return LocalAgentsUpdate {
                has_new_delegated_entries,
            };
        }

        if let Some(previous_id) = previous_id
            && let Some(index) = self
                .entries
                .iter()
                .position(|entry| entry.id == previous_id)
        {
            self.navigator.select_index(index);
            return LocalAgentsUpdate {
                has_new_delegated_entries,
            };
        }

        self.navigator.select_first();
        LocalAgentsUpdate {
            has_new_delegated_entries,
        }
    }

    pub(super) fn entries(&self) -> &[LocalAgentEntry] {
        &self.entries
    }

    pub(super) fn has_entries(&self) -> bool {
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

    pub(super) fn page_up(&mut self, step: usize) -> bool {
        self.navigator.page_up(step)
    }

    pub(super) fn page_down(&mut self, step: usize) -> bool {
        self.navigator.page_down(step)
    }

    pub(super) fn set_visible_rows(&mut self, rows: usize) {
        self.navigator.set_visible_rows(rows);
    }

    pub(super) fn visible_rows(&self) -> usize {
        self.navigator.visible_rows()
    }

    pub(super) fn scroll_offset(&self) -> usize {
        self.navigator.scroll_offset()
    }

    pub(super) fn selected_entry(&self) -> Option<&LocalAgentEntry> {
        self.selected().and_then(|index| self.entries.get(index))
    }
}

impl AppSession {
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

    pub(super) fn handle_local_agents_key(&mut self, key: &KeyEvent) -> Option<InlineEvent> {
        if !self.local_agents_visible() {
            return None;
        }

        let input_empty = self.core.input_manager.content().trim().is_empty();
        match key.code {
            KeyCode::Up if input_empty => {
                self.local_agents_state.move_selection_up();
                self.mark_dirty();
                None
            }
            KeyCode::Down if input_empty => {
                self.local_agents_state.move_selection_down();
                self.mark_dirty();
                None
            }
            KeyCode::PageUp if input_empty => {
                let step = self.local_agents_state.visible_rows().max(1);
                self.local_agents_state.page_up(step);
                self.mark_dirty();
                None
            }
            KeyCode::PageDown if input_empty => {
                let step = self.local_agents_state.visible_rows().max(1);
                self.local_agents_state.page_down(step);
                self.mark_dirty();
                None
            }
            KeyCode::Char('n') if input_empty && key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.local_agents_state.move_selection_down();
                self.mark_dirty();
                None
            }
            KeyCode::Char('p') if input_empty && key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.local_agents_state.move_selection_up();
                self.mark_dirty();
                None
            }
            KeyCode::Char('o') | KeyCode::Char('O')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.selected_local_agent_transcript_event()
            }
            KeyCode::Char('k') | KeyCode::Char('K')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.selected_local_agent_stop_event()
            }
            KeyCode::Char('x') | KeyCode::Char('X')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.selected_local_agent_force_cancel_event()
            }
            KeyCode::Enter if input_empty => self.selected_local_agent_inspect_event(),
            KeyCode::Esc => {
                self.close_transient_surface(TransientSurface::LocalAgents);
                self.mark_dirty();
                None
            }
            _ => None,
        }
    }

    fn selected_local_agent_inspect_event(&mut self) -> Option<InlineEvent> {
        let entry = self.local_agents_state.selected_entry()?.clone();
        self.mark_dirty();
        Some(InlineEvent::Submit(match entry.kind {
            crate::core_tui::types::LocalAgentKind::Delegated => {
                format!("/agent inspect {}", entry.id)
            }
            crate::core_tui::types::LocalAgentKind::Background => {
                format!("/subprocesses inspect {}", entry.id)
            }
        }))
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
        Some(InlineEvent::Submit(match entry.kind {
            crate::core_tui::types::LocalAgentKind::Delegated => {
                format!("/agent close {}", entry.id)
            }
            crate::core_tui::types::LocalAgentKind::Background => {
                format!("/subprocesses stop {}", entry.id)
            }
        }))
    }

    fn selected_local_agent_force_cancel_event(&mut self) -> Option<InlineEvent> {
        let entry = self.local_agents_state.selected_entry()?.clone();
        self.mark_dirty();
        Some(InlineEvent::Submit(match entry.kind {
            crate::core_tui::types::LocalAgentKind::Delegated => {
                format!("/agent close {}", entry.id)
            }
            crate::core_tui::types::LocalAgentKind::Background => {
                format!("/subprocesses cancel {}", entry.id)
            }
        }))
    }
}
