/// Palette management operations for Session
///
/// This module handles file palette and prompt palette interactions including:
/// - Loading and closing palettes
/// - Checking and handling triggers
/// - Key event handling for palette navigation
/// - Reference insertion
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;

use super::{
    Session,
    file_palette::{FilePalette, extract_file_reference},
    prompt_palette::{PromptPalette, extract_prompt_reference},
};
use crate::prompts::CustomPromptRegistry;
use crate::ui::tui::session::slash;

const PROMPT_COMMAND_PREFIX: &str = "/prompt:";

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
    pub(super) fn check_file_reference_trigger(&mut self) {
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
            let new_content = format!("{}{} {}", before, file_path, after);
            let new_cursor = start + file_path.len() + 1;

            self.input_manager.set_content(new_content);
            self.input_manager.set_cursor(new_cursor);
            slash::update_slash_suggestions(self);
        }
    }

    /// Set custom prompts and initialize the prompt palette if enabled
    pub fn set_custom_prompts(&mut self, custom_prompts: CustomPromptRegistry) {
        // Initialize prompt palette when custom prompts are loaded
        if custom_prompts.enabled() && !custom_prompts.is_empty() {
            let mut palette = PromptPalette::new();
            palette.load_prompts(custom_prompts.iter());
            self.prompt_palette = Some(palette);
        }

        self.custom_prompts = Some(custom_prompts);
        // Update slash palette if we're currently viewing slash commands
        if self.input_manager.content().starts_with('/') {
            slash::update_slash_suggestions(self);
        }
    }

    /// Check if the current input should trigger the prompt palette
    pub(super) fn check_prompt_reference_trigger(&mut self) {
        // Initialize prompt palette on-demand if it doesn't exist
        if self.prompt_palette.is_none()
            && let Some(registry) = &self.custom_prompts
            && registry.enabled()
            && !registry.is_empty()
        {
            let mut palette = PromptPalette::new();
            palette.load_prompts(registry.iter());
            self.prompt_palette = Some(palette);
        }

        if let Some(palette) = self.prompt_palette.as_mut() {
            if let Some((_start, _end, query)) =
                extract_prompt_reference(self.input_manager.content(), self.input_manager.cursor())
            {
                palette.set_filter(query);
                if !self.prompt_palette_active {
                    self.prompt_palette_active = true;
                }
            } else if self.prompt_palette_active {
                self.close_prompt_palette();
            }
        }
    }

    /// Close the prompt palette and clean up resources
    pub(super) fn close_prompt_palette(&mut self) {
        self.prompt_palette_active = false;

        // Clean up resources when closing to free memory
        if let Some(palette) = self.prompt_palette.as_mut() {
            palette.set_filter(String::new());
        }
    }

    /// Handle key events for the prompt palette
    ///
    /// Returns true if the key was handled by the palette
    pub(super) fn handle_prompt_palette_key(&mut self, key: &KeyEvent) -> bool {
        if !self.prompt_palette_active {
            return false;
        }

        let Some(palette) = self.prompt_palette.as_mut() else {
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
            KeyCode::Enter => {
                let selected_name = palette.get_selected().map(|e| e.name.clone());
                if let Some(name) = selected_name {
                    self.insert_prompt_reference(&name);
                    self.close_prompt_palette();
                    self.mark_dirty();
                }
                true
            }
            KeyCode::Esc => {
                self.close_prompt_palette();
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

    /// Insert a prompt reference as a slash command into the input
    pub(super) fn insert_prompt_reference(&mut self, prompt_name: &str) {
        let mut command = String::from(PROMPT_COMMAND_PREFIX);
        command.push_str(prompt_name);
        command.push(' ');

        self.input_manager.set_content(command);
        self.input_manager.move_cursor_to_end();
        slash::update_slash_suggestions(self);
    }
}
