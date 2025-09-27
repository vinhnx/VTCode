use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
};
use termion::event::Event as TermionEvent;
use tokio::sync::mpsc::UnboundedSender;

use crate::ui::tui::{
    action::{Action, ScrollAction},
    prompt::PromptBar,
    transcript::TranscriptView,
    types::{RatatuiCommand, RatatuiEvent, RatatuiTheme},
};

pub struct Session {
    transcript: TranscriptView,
    prompt: PromptBar,
    needs_redraw: bool,
    should_exit: bool,
}

impl Session {
    pub fn new(theme: RatatuiTheme, placeholder: Option<String>) -> Self {
        Self {
            transcript: TranscriptView::new(theme.clone()),
            prompt: PromptBar::new(theme, placeholder),
            needs_redraw: true,
            should_exit: false,
        }
    }

    pub fn should_exit(&self) -> bool {
        self.should_exit
    }

    pub fn request_exit(&mut self) {
        self.should_exit = true;
    }

    pub fn take_redraw(&mut self) -> bool {
        if self.needs_redraw {
            self.needs_redraw = false;
            true
        } else {
            false
        }
    }

    pub fn handle_command(&mut self, command: RatatuiCommand) {
        let mut updated = true;
        match command {
            RatatuiCommand::AppendLine { kind, segments } => {
                self.transcript.push_line(kind, segments);
            }
            RatatuiCommand::Inline { kind, segment } => {
                self.transcript.append_inline(kind, segment);
            }
            RatatuiCommand::ReplaceLast { count, kind, lines } => {
                self.transcript.replace_last(count, kind, lines);
            }
            RatatuiCommand::SetPrompt { prefix, style } => {
                self.prompt.set_prompt(prefix, style);
            }
            RatatuiCommand::SetPlaceholder { hint, style } => {
                self.prompt.set_placeholder(hint, style);
            }
            RatatuiCommand::SetMessageLabels { agent, user } => {
                self.transcript.set_labels(agent, user);
            }
            RatatuiCommand::SetTheme { theme } => {
                self.transcript.set_theme(theme.clone());
                self.prompt.set_theme(theme);
            }
            RatatuiCommand::SetCursorVisible(value) => {
                self.prompt.set_cursor_visible(value);
            }
            RatatuiCommand::SetInputEnabled(value) => {
                self.prompt.set_input_enabled(value);
            }
            RatatuiCommand::ClearInput => {
                self.prompt.clear_input();
            }
            RatatuiCommand::ForceRedraw => {
                updated = true;
            }
            RatatuiCommand::Shutdown => {
                self.request_exit();
                updated = false;
            }
        }

        if updated {
            self.mark_dirty();
        }
    }

    pub fn handle_event(&mut self, event: TermionEvent, events: &UnboundedSender<RatatuiEvent>) {
        if let TermionEvent::Key(key) = event {
            let action = self.prompt.handle_key(key);
            self.process_action(action, events);
        }
    }

    pub fn draw(&mut self, frame: &mut Frame<'_>) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(frame.area());
        self.transcript.render(frame, layout[0]);
        self.prompt.render(frame, layout[1]);
    }

    fn process_action(&mut self, action: Action, events: &UnboundedSender<RatatuiEvent>) {
        match action {
            Action::None => {}
            Action::Redraw => self.mark_dirty(),
            Action::Submit(text) => {
                let _ = events.send(RatatuiEvent::Submit(text));
                self.mark_dirty();
            }
            Action::Cancel => {
                let _ = events.send(RatatuiEvent::Cancel);
                self.mark_dirty();
            }
            Action::Exit => {
                let _ = events.send(RatatuiEvent::Exit);
                self.mark_dirty();
            }
            Action::Interrupt => {
                let _ = events.send(RatatuiEvent::Interrupt);
                self.mark_dirty();
            }
            Action::Scroll(direction) => {
                self.transcript.scroll(direction);
                let event = match direction {
                    ScrollAction::LineUp => RatatuiEvent::ScrollLineUp,
                    ScrollAction::LineDown => RatatuiEvent::ScrollLineDown,
                    ScrollAction::PageUp => RatatuiEvent::ScrollPageUp,
                    ScrollAction::PageDown => RatatuiEvent::ScrollPageDown,
                };
                let _ = events.send(event);
                self.mark_dirty();
            }
        }
    }

    fn mark_dirty(&mut self) {
        self.needs_redraw = true;
    }
}
