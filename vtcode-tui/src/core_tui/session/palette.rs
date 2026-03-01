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
    Session,
    file_palette::{FilePalette, extract_file_reference},
};
use crate::ui::tui::session::slash;

impl Session {
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
        if let Some(palette) = self.file_palette.as_mut() {
            if let Some((_start, _end, query)) =
                extract_file_reference(self.input_manager.content(), self.input_manager.cursor())
            {
                palette.set_filter(query);
                if !self.file_palette_active {
                    self.file_palette_active = true;
                }
            } else if self.file_palette_active {
                self.close_file_palette();
            }
        }
    }

    /// Close the file palette and clean up resources
    pub(super) fn close_file_palette(&mut self) {
        self.file_palette_active = false;

        // Clean up resources when closing to free memory
        if let Some(palette) = self.file_palette.as_mut() {
            palette.set_filter(String::new());
        }
    }

    /// Handle key events for the file palette
    ///
    /// Returns true if the key was handled by the palette
    pub(super) fn handle_file_palette_key(&mut self, key: &KeyEvent) -> bool {
        if !self.file_palette_active {
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
                }
                true
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
    pub(super) fn insert_file_reference(&mut self, file_path: &str) {
        if let Some((start, end, _)) =
            extract_file_reference(self.input_manager.content(), self.input_manager.cursor())
        {
            let before = &self.input_manager.content()[..start];
            let after = &self.input_manager.content()[end..];
            let reference_alias = format!("@{}", file_path);
            let new_content = format!("{}{} {}", before, reference_alias, after);
            let new_cursor = start + reference_alias.len() + 1;

            self.input_manager.set_content(new_content);
            self.input_manager.set_cursor(new_cursor);
            slash::update_slash_suggestions(self);
        }
    }
}
