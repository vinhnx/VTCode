use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::core_tui::log::LogEntry;
use crate::core_tui::runner::TuiSessionDriver;
use crate::core_tui::types::{InlineCommand, InlineEvent};
use crate::options::FullscreenInteractionSettings;

use super::Session;

impl TuiSessionDriver for Session {
    type Command = InlineCommand;
    type Event = InlineEvent;

    fn handle_command(&mut self, command: Self::Command) {
        self.handle_command(command);
    }

    fn handle_event(
        &mut self,
        event: crossterm::event::Event,
        events: &UnboundedSender<Self::Event>,
        callback: Option<&(dyn Fn(&Self::Event) + Send + Sync + 'static)>,
    ) {
        self.handle_event(event, events, callback);
    }

    fn handle_tick(&mut self) {
        self.handle_tick();
    }

    fn render(&mut self, frame: &mut ratatui::Frame<'_>) {
        self.render(frame);
    }

    fn take_redraw(&mut self) -> bool {
        self.take_redraw()
    }

    fn use_steady_cursor(&self) -> bool {
        self.use_steady_cursor()
    }

    fn should_exit(&self) -> bool {
        self.should_exit()
    }

    fn request_exit(&mut self) {
        self.request_exit();
    }

    fn mark_dirty(&mut self) {
        self.mark_dirty();
    }

    fn update_terminal_title(&mut self) {
        self.update_terminal_title();
    }

    fn clear_terminal_title(&mut self) {
        self.clear_terminal_title();
    }

    fn is_running_activity(&self) -> bool {
        self.is_running_activity()
    }

    fn has_status_spinner(&self) -> bool {
        self.has_status_spinner()
    }

    fn thinking_spinner_active(&self) -> bool {
        self.thinking_spinner.is_active
    }

    fn has_active_navigation_ui(&self) -> bool {
        self.has_active_overlay()
    }

    fn apply_coalesced_scroll(&mut self, line_delta: i32, page_delta: i32) {
        self.apply_coalesced_scroll(line_delta, page_delta);
    }

    fn set_show_logs(&mut self, show: bool) {
        self.show_logs = show;
    }

    fn set_active_pty_sessions(
        &mut self,
        sessions: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    ) {
        self.active_pty_sessions = sessions;
    }

    fn set_workspace_root(&mut self, root: Option<std::path::PathBuf>) {
        self.set_workspace_root(root);
    }

    fn set_log_receiver(&mut self, receiver: UnboundedReceiver<LogEntry>) {
        self.set_log_receiver(receiver);
    }

    fn set_fullscreen_active(&mut self, active: bool) {
        self.set_fullscreen_active(active);
    }

    fn set_fullscreen_interaction(&mut self, config: FullscreenInteractionSettings) {
        self.set_fullscreen_interaction(config);
    }
}
