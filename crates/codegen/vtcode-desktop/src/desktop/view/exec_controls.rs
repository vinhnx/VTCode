//! Foreground exec-session controls: background + force-cancel requests.
use super::*;

impl DesktopView {
    pub(super) fn background_foreground_session(&mut self, cx: &mut Context<Self>) {
        self.request_exec_command(protocol::method::EXEC_BACKGROUND, cx);
    }

    pub(super) fn force_cancel_foreground_sessions(&mut self, cx: &mut Context<Self>) {
        self.request_exec_command(protocol::method::EXEC_FORCE_CANCEL, cx);
    }

    fn request_exec_command(&mut self, method: &'static str, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session_id.clone() else {
            return;
        };
        if !self.request(method, json!({ keys::SESSION_ID: session_id })) {
            self.error = Some(rust_i18n::t!(i18n::QUEUE_FULL).to_string());
        }
        cx.notify();
    }

    pub(super) fn apply_exec_command_response(&mut self, method: &str) {
        self.clear_error();
        self.status = if method == protocol::method::EXEC_FORCE_CANCEL {
            rust_i18n::t!("app.service.force_cancel_accepted").to_string()
        } else {
            rust_i18n::t!("app.service.background_accepted").to_string()
        };
    }
}
