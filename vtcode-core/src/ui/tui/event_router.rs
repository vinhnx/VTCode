//! Event routing system for TUI interactions
//!
//! Routes events to the appropriate handler based on current UI state.
//! This implements the "message passing" pattern recommended in Ratatui docs.

use super::modern_tui::Event;
use super::session::Session;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

/// Determines which component should handle an event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventRoute {
    /// Event should be handled by the main session
    Session,
    /// Event should be handled by an active modal
    Modal,
    /// Event should be handled by the file palette
    FilePalette,
    /// Event should be handled by the prompt palette
    PromptPalette,
    /// Event should be handled by the slash command palette
    SlashPalette,
}

/// Routes events to appropriate handlers based on UI state
pub struct EventRouter;

impl EventRouter {
    /// Determine which component should handle the given event
    ///
    /// Priority order:
    /// 1. Modal (if active)
    /// 2. File/Prompt/Slash Palettes (if active)
    /// 3. Main session
    pub fn route(session: &Session, event: &Event) -> EventRoute {
        // Modals take highest priority
        if session.has_modal() {
            return EventRoute::Modal;
        }

        // Palettes are next priority
        if session.is_file_palette_active() {
            return EventRoute::FilePalette;
        }

        if session.is_prompt_palette_active() {
            return EventRoute::PromptPalette;
        }

        if session.is_slash_palette_active() {
            return EventRoute::SlashPalette;
        }

        // Default to session handling
        EventRoute::Session
    }

    /// Check if an event should be globally handled (e.g., Ctrl+C for quit)
    pub fn is_global_event(event: &Event) -> bool {
        matches!(
            event,
            Event::Quit
                | Event::Key(KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: ratatui::crossterm::event::KeyModifiers::CONTROL,
                    ..
                })
        )
    }

    /// Route and handle an event
    pub fn handle(session: &mut Session, event: Event) -> anyhow::Result<bool> {
        // Check for global events first
        if Self::is_global_event(&event) {
            session.request_exit();
            return Ok(true);
        }

        // Route to appropriate handler
        match Self::route(session, &event) {
            EventRoute::Modal => {
                // Modal-specific event handling
                // session.handle_modal_event(event)
                Ok(false)
            }
            EventRoute::FilePalette => {
                // File palette event handling
                // session.handle_file_palette_event(event)
                Ok(false)
            }
            EventRoute::PromptPalette => {
                // Prompt palette event handling
                // session.handle_prompt_palette_event(event)
                Ok(false)
            }
            EventRoute::SlashPalette => {
                // Slash palette event handling
                // session.handle_slash_palette_event(event)
                Ok(false)
            }
            EventRoute::Session => {
                // Main session event handling
                // session.handle_session_event(event)
                Ok(false)
            }
        }
    }
}

// Add helper methods to Session for router to use
impl Session {
    pub(crate) fn has_modal(&self) -> bool {
        self.modal.is_some()
    }

    pub(crate) fn is_file_palette_active(&self) -> bool {
        self.file_palette_active
    }

    pub(crate) fn is_prompt_palette_active(&self) -> bool {
        self.prompt_palette_active
    }

    pub(crate) fn is_slash_palette_active(&self) -> bool {
        // Check if slash palette has suggestions
        false // Placeholder - implement based on SlashPalette API
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_priority() {
        // Test that routing follows priority order
        // Modal > Palette > Session
    }

    #[test]
    fn test_global_events() {
        // Test that global events are always handled
    }
}
