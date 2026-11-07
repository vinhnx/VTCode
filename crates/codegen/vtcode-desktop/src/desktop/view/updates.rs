//! Cached updater status: refresh requests, snapshot storage, and notifications.

use super::*;

impl DesktopView {
    pub(super) fn request_update_status(&mut self) {
        if self.update_request_pending {
            return;
        }
        self.update_request_pending = true;
        if !self.request(protocol::method::UPDATE_STATUS, json!({})) {
            self.update_request_pending = false;
        }
    }

    pub(super) fn handle_update_status_response(&mut self, result: Value) {
        self.update_request_pending = false;
        match serde_json::from_value::<protocol::UpdateStatusSnapshot>(result) {
            Ok(status) => self.update_status = Some(status),
            Err(_) => self.error = Some(rust_i18n::t!("app.service.invalid_updates").to_string()),
        }
    }

    /// Announce a newly reported release exactly once per version. Runs ahead
    /// of [`DesktopView::handle_backend_event`] so the system banner has a
    /// window, mirroring [`DesktopView::notify_agent_activity`].
    pub(super) fn maybe_notify_update_available(
        &mut self,
        event: &BackendEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let BackendEvent::Response { method, result: Some(result), error: None } = event else {
            return;
        };
        if method != protocol::method::UPDATE_STATUS {
            return;
        }
        let Ok(status) = serde_json::from_value::<protocol::UpdateStatusSnapshot>(result.clone()) else {
            return;
        };
        if !status.update_available {
            return;
        }
        let Some(latest) = status.latest_version.as_deref() else {
            return;
        };
        let notifications = &self.preferences.notifications;
        if !notifications.enabled() || !notifications.updates() {
            return;
        }
        if notifications.last_notified_update.as_deref() == Some(latest) {
            return;
        }
        let system = !window.is_window_active();
        let weak = cx.entity().downgrade();
        super::super::notifications::update_available(
            window,
            latest,
            system,
            deeplinks::click_handler(deeplinks::NotificationTarget::UpdateSettings, weak),
            cx,
        );
        self.preferences.notifications.last_notified_update = Some(latest.to_string());
        self.preferences.save();
        cx.notify();
    }
}
