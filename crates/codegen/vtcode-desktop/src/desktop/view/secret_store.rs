//! In-app API-key storage for the Provider settings group.
//!
//! When the service advertises `secret_store` support (static-key providers),
//! the settings page offers a masked input that stores the key through
//! `config/secret/store` — no Terminal round-trip and no service reload. The
//! key is wiped from UI state once accepted for delivery and never rendered
//! back; failures ask for a fresh paste.

use super::*;

impl DesktopView {
    pub(super) fn render_secret_store_form(
        &self,
        info: &protocol::SecretStoreInfo,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let pending = self.secret_save_pending;
        let value = self.secret_input.read(cx).value().trim().to_string();
        let provider = info.provider.clone();
        let sendable = !pending && self.backend.is_some() && !value.is_empty();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div().flex().flex_col().gap_1().child(div().text_sm().child("API key")).child(
                    Input::new(&self.secret_input)
                        .id("secret-input")
                        .accessibility_id("secret-api-key")
                        .aria_label("Provider API key")
                        .small()
                        .disabled(pending)
                        .when(pending, |input| input.cursor_default())
                        .when(!pending, |input| input.cursor_text())
                        .w_full(),
                ),
            )
            .child(
                desktop_button("save-secret")
                    .label(if pending { "Saving…" } else { "Save API key" })
                    .primary()
                    .small()
                    .desktop_disabled(!sendable)
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.save_secret_store(provider.clone(), value.clone(), window, cx);
                    })),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(format!("Stored in secure storage on this device as {}.", info.env_var)),
            )
            .when_some(self.secret_editor_error.as_ref(), |form, error| {
                form.child(
                    div()
                        .id("secret-error")
                        .test_support()
                        .role(gpui_kit::Role::Alert)
                        .aria_label(error.clone())
                        .text_sm()
                        .text_color(colors.danger)
                        .child(error.clone()),
                )
            })
            .into_any_element()
    }

    pub(super) fn save_secret_store(
        &mut self,
        provider: String,
        api_key: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.secret_save_pending || self.backend.is_none() {
            return;
        }
        let key = api_key.trim().to_string();
        if key.is_empty() {
            self.secret_editor_error = Some("API key must not be empty.".into());
            cx.notify();
            return;
        }
        // Ignore renders that predate a provider switch; the fresh form
        // carries the new target.
        let expected = self
            .configuration
            .as_ref()
            .and_then(|configuration| configuration.secret_store.as_ref())
            .map(|info| info.provider.as_str());
        if expected != Some(provider.as_str()) {
            return;
        }
        let params = protocol::SecretStoreParams { provider, api_key: key };
        if self.request(protocol::method::CONFIG_SECRET_STORE, serde_json::json!(params)) {
            self.secret_save_pending = true;
            self.secret_editor_error = None;
            self.secret_input.update(cx, |state, cx| state.set_value("", window, cx));
            self.status = "Saving API key…".into();
        } else {
            self.secret_editor_error = Some(REQUEST_QUEUE_FULL_MESSAGE.into());
        }
        cx.notify();
    }

    pub(super) fn handle_secret_store_response(&mut self, result: Value, cx: &mut Context<Self>) {
        self.secret_save_pending = false;
        let stored = match serde_json::from_value::<protocol::SecretStoreResult>(result) {
            Ok(stored) => stored,
            Err(_) => {
                self.secret_editor_error = Some("Invalid credential response. Try saving again.".into());
                cx.notify();
                return;
            }
        };
        if stored.session_reset {
            self.clear_session_after_service_reset();
        }
        // A provider switch may have landed while the save was in flight;
        // only adopt snapshots for the currently displayed provider.
        let provider_matches = self
            .configuration
            .as_ref()
            .is_some_and(|configuration| configuration.provider == stored.configuration.provider);
        if !provider_matches {
            self.secret_editor_error = None;
            self.status = "API key saved.".into();
            cx.notify();
            return;
        }
        self.configuration = Some(stored.configuration);
        self.secret_editor_error = None;
        // Clear a stale auth banner only when it shows the credential
        // guidance; unrelated errors stay put.
        if self
            .configuration
            .as_ref()
            .is_some_and(|configuration| configuration.authenticated)
            && self
                .error
                .as_deref()
                .is_some_and(|error| error == provider_auth_guidance(self.configuration.as_ref()).as_str())
        {
            self.clear_error();
        }
        self.status = if self
            .configuration
            .as_ref()
            .is_some_and(|configuration| configuration.authenticated)
        {
            "API key saved. Send a message to start.".into()
        } else {
            "API key saved, but the provider still reports missing credentials.".into()
        };
        cx.notify();
    }

    pub(super) fn handle_secret_store_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.secret_save_pending = false;
        self.secret_editor_error = Some(error);
        self.status = "API key was not saved.".into();
        cx.notify();
    }

    pub(super) fn interrupt_secret_save(&mut self) {
        if self.secret_save_pending {
            self.secret_save_pending = false;
            self.secret_editor_error = Some(
                "The service disconnected while saving. The outcome is unknown; reload CLI settings before trying again."
                    .into(),
            );
        }
    }
}
