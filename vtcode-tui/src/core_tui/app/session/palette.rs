/// Palette management operations for Session
///
/// This module handles file palette interactions including:
/// - Loading and closing palettes
/// - Checking and handling triggers
/// - Key event handling for palette navigation
/// - Reference insertion
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

use super::{
    AppSession,
    agent_palette::{AgentEntry, AgentPalette, extract_agent_reference},
    file_palette::{FilePalette, extract_file_reference},
};
use crate::core_tui::app::session::slash;
use crate::core_tui::app::session::transient::TransientSurface;
use crate::core_tui::app::types::AgentPaletteItem;

impl AppSession {
    pub(super) fn load_agent_palette(&mut self, agents: Vec<AgentPaletteItem>) {
        let mut palette = AgentPalette::new();
        palette.load_agents(
            agents
                .into_iter()
                .map(|agent| AgentEntry {
                    display_name: format!("@agent-{}", agent.name),
                    name: agent.name,
                    description: agent.description,
                })
                .collect(),
        );
        self.agent_palette = Some(palette);
        self.agent_palette_active = false;
        self.check_agent_reference_trigger();
    }

    pub fn check_agent_reference_trigger(&mut self) {
        let cursor = self.core.input_manager.cursor();
        let content = self.core.input_manager.content();
        let trigger = extract_agent_reference(content, cursor);

        if let Some(palette) = self.agent_palette.as_mut()
            && let Some((_start, _end, query)) = trigger
        {
            palette.set_filter(query);
            if !self.agent_palette_active {
                self.ensure_inline_lists_visible_for_trigger();
                self.agent_palette_active = true;
                self.show_transient_surface(TransientSurface::AgentPalette);
                self.mark_dirty();
            }
            return;
        }

        if self.agent_palette_active {
            self.close_agent_palette();
        }
    }

    pub(super) fn close_agent_palette(&mut self) {
        self.agent_palette_active = false;
        self.close_transient_surface(TransientSurface::AgentPalette);

        if let Some(palette) = self.agent_palette.as_mut() {
            palette.set_filter(String::new());
        }
    }

    pub(super) fn handle_agent_palette_key(&mut self, key: &KeyEvent) -> bool {
        if !self.agent_palette_visible() {
            return false;
        }

        let Some(palette) = self.agent_palette.as_mut() else {
            return false;
        };

        match key.code {
            KeyCode::Up => {
                palette.move_selection_up();
                self.mark_dirty();
                true
            }
            KeyCode::Down => {
                palette.move_selection_down();
                self.mark_dirty();
                true
            }
            KeyCode::Tab => {
                palette.select_best_match();
                self.mark_dirty();
                true
            }
            KeyCode::Enter => {
                let selected_name = palette.get_selected().map(|entry| entry.name.clone());
                if let Some(name) = selected_name {
                    self.insert_agent_reference(&name);
                    self.close_agent_palette();
                    self.mark_dirty();
                    true
                } else {
                    self.close_agent_palette();
                    self.mark_dirty();
                    false
                }
            }
            KeyCode::Esc => {
                self.close_agent_palette();
                self.mark_dirty();
                true
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                palette.move_selection_down();
                self.mark_dirty();
                true
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                palette.move_selection_up();
                self.mark_dirty();
                true
            }
            _ => false,
        }
    }

    pub(crate) fn insert_agent_reference(&mut self, agent_name: &str) {
        if let Some((start, end, _)) = extract_agent_reference(
            self.core.input_manager.content(),
            self.core.input_manager.cursor(),
        ) {
            let before = &self.core.input_manager.content()[..start];
            let after = &self.core.input_manager.content()[end..];
            let reference_alias = format!("@agent-{}", agent_name);
            let new_content = format!("{}{} {}", before, reference_alias, after);
            let new_cursor = start + reference_alias.len() + 1;

            self.core.input_manager.set_content(new_content);
            self.core.input_manager.set_cursor(new_cursor);
            slash::update_slash_suggestions(self);
        }
    }

    /// Load the file palette with files from the workspace
    pub(super) fn load_file_palette(&mut self, files: Vec<String>, workspace: PathBuf) {
        let mut palette = FilePalette::new(workspace);
        palette.load_files(files);
        self.file_palette = Some(palette);
        self.file_palette_active = false;
        self.check_file_reference_trigger();
    }

    /// Check if the current input should trigger the file palette
    pub fn check_file_reference_trigger(&mut self) {
        if self.agent_palette_visible() {
            if self.file_palette_active {
                self.close_file_palette();
            }
            return;
        }

        if let Some(palette) = self.file_palette.as_mut() {
            if let Some((_start, _end, query)) = extract_file_reference(
                self.core.input_manager.content(),
                self.core.input_manager.cursor(),
            ) {
                palette.set_filter(query);
                if !self.file_palette_active {
                    self.ensure_inline_lists_visible_for_trigger();
                    self.file_palette_active = true;
                    self.show_transient_surface(TransientSurface::FilePalette);
                    self.mark_dirty();
                }
            } else if self.file_palette_active {
                self.close_file_palette();
            }
        }
    }

    /// Close the file palette and clean up resources
    pub(super) fn close_file_palette(&mut self) {
        self.file_palette_active = false;
        self.close_transient_surface(TransientSurface::FilePalette);

        // Clean up resources when closing to free memory
        if let Some(palette) = self.file_palette.as_mut() {
            palette.set_filter(String::new());
        }
    }

    /// Handle key events for the file palette
    ///
    /// Returns true if the key was handled by the palette
    pub(super) fn handle_file_palette_key(&mut self, key: &KeyEvent) -> bool {
        if !self.file_palette_visible() {
            return false;
        }

        let Some(palette) = self.file_palette.as_mut() else {
            return false;
        };

        match key.code {
            KeyCode::Up => {
                palette.move_selection_up();
                self.mark_dirty();
                true
            }
            KeyCode::Down => {
                palette.move_selection_down();
                self.mark_dirty();
                true
            }
            KeyCode::Tab => {
                palette.select_best_match();
                self.mark_dirty();
                true
            }
            KeyCode::Enter => {
                let selected_path = palette.get_selected().map(|e| e.relative_path.clone());
                if let Some(path) = selected_path {
                    self.insert_file_reference(&path);
                    self.close_file_palette();
                    self.mark_dirty();
                    true // Selection made: consume event
                } else {
                    // No selection: close palette and fall through to normal submit
                    self.close_file_palette();
                    self.mark_dirty();
                    false // Let normal Enter handling proceed
                }
            }
            KeyCode::Esc => {
                self.close_file_palette();
                self.mark_dirty();
                true
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                palette.move_selection_down();
                self.mark_dirty();
                true
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                palette.move_selection_up();
                self.mark_dirty();
                true
            }
            _ => false,
        }
    }

    /// Insert a file reference into the input at the current position
    pub(crate) fn insert_file_reference(&mut self, file_path: &str) {
        if let Some((start, end, _)) = extract_file_reference(
            self.core.input_manager.content(),
            self.core.input_manager.cursor(),
        ) {
            let before = &self.core.input_manager.content()[..start];
            let after = &self.core.input_manager.content()[end..];
            let reference_alias = format!("@{}", file_path);
            let new_content = format!("{}{} {}", before, reference_alias, after);
            let new_cursor = start + reference_alias.len() + 1;

            self.core.input_manager.set_content(new_content);
            self.core.input_manager.set_cursor(new_cursor);
            slash::update_slash_suggestions(self);
        }
    }
}
