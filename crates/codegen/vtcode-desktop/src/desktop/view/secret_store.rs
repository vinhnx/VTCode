//! In-app API-key storage for the Provider settings group.
//!
//! When the service advertises `secret_store` support (static-key providers),
//! the settings page offers a masked input that stores the key through
//! `config/secret/store` — no Terminal round-trip and no service reload. The
//! key is wiped from UI state once accepted for delivery and never rendered
//! back; failures ask for a fresh paste.

use super::*;
use gpui_kit::component::input::InputState;

/// Cloned render inputs for the secret-store form (see
/// `NotificationSettingsProps` for the Kit-hosting rationale).
#[derive(Clone)]
pub(super) struct SecretStoreProps {
    pub(super) input: Entity<InputState>,
    pub(super) pending: bool,
    pub(super) backend_available: bool,
    pub(super) error: Option<String>,
}

impl SecretStoreProps {
    /// Snapshot the view fields the secret form reads. Both the legacy
    /// shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            input: view.secret_input.clone(),
            pending: view.secret_save_pending,
            backend_available: view.backend.is_some(),
            error: view.secret_editor_error.clone(),
        }
    }
}

impl DesktopView {
    pub(super) fn render_secret_store_form_content(
        info: &protocol::SecretStoreInfo,
        props: &SecretStoreProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let pending = props.pending;
        let value = props.input.read(cx).value().trim().to_string();
        let provider = info.provider.clone();
        let sendable = !pending && props.backend_available && !value.is_empty();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(row_title(rust_i18n::t!("app.secrets.panel_title").to_string(), &colors))
                    .child(
                        Input::new(&props.input)
                            .id("secret-input")
                            .accessibility_id("secret-api-key")
                            .aria_label(rust_i18n::t!("app.secrets.provider_aria").to_string())
                            .small()
                            .disabled(pending)
                            .when(pending, |input| input.cursor_default())
                            .when(!pending, |input| input.cursor_text())
                            .w_full(),
                    ),
            )
            .child(
                desktop_glass_secondary_primary_button("save-secret", &colors, !sendable)
                    .label(if pending {
                        rust_i18n::t!("app.secrets.saving").to_string()
                    } else {
                        rust_i18n::t!("app.secrets.save_key").to_string()
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, window: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| {
                                view.save_secret_store(provider.clone(), value.clone(), window, cx);
                            });
                        }
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(rust_i18n::t!("app.secrets.stored_as", var = info.env_var.as_str()).to_string()),
            )
            .when_some(props.error.as_ref(), |form, error| {
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
            self.secret_editor_error = Some(rust_i18n::t!("app.secrets.empty_key").to_string());
            cx.notify();
            return;
        }
        // Ignore renders that predate a provider switch; the fresh form
        // carries the new target.
        let expected = self.secret_store_info().map(|info| info.provider.as_str());
        if expected != Some(provider.as_str()) {
            return;
        }
        let params = protocol::SecretStoreParams { provider, api_key: key };
        if self.request(protocol::method::CONFIG_SECRET_STORE, serde_json::json!(params)) {
            self.secret_save_pending = true;
            self.secret_editor_error = None;
            self.secret_input.update(cx, |state, cx| state.set_value("", window, cx));
            self.status = rust_i18n::t!("app.secrets.saving_status").to_string();
        } else {
            self.secret_editor_error = Some(rust_i18n::t!(i18n::QUEUE_FULL).to_string());
        }
        cx.notify();
    }

    pub(super) fn handle_secret_store_response(&mut self, result: Value, cx: &mut Context<Self>) {
        self.secret_save_pending = false;
        let stored = match serde_json::from_value::<protocol::SecretStoreResult>(result) {
            Ok(stored) => stored,
            Err(_) => {
                self.secret_editor_error = Some(rust_i18n::t!("app.secrets.invalid_response").to_string());
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
            self.status = rust_i18n::t!("app.secrets.saved").to_string();
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
            && self.error.as_deref().is_some_and(is_provider_auth_guidance)
        {
            self.clear_error();
        }
        self.status = if self
            .configuration
            .as_ref()
            .is_some_and(|configuration| configuration.authenticated)
        {
            rust_i18n::t!("app.secrets.saved_send").to_string()
        } else {
            rust_i18n::t!("app.secrets.saved_missing").to_string()
        };
        cx.notify();
    }

    pub(super) fn handle_secret_store_error(&mut self, error: String, cx: &mut Context<Self>) {
        self.secret_save_pending = false;
        self.secret_editor_error = Some(error);
        self.status = rust_i18n::t!("app.secrets.not_saved").to_string();
        cx.notify();
    }

    pub(super) fn interrupt_secret_save(&mut self) {
        if self.secret_save_pending {
            self.secret_save_pending = false;
            self.secret_editor_error = Some(rust_i18n::t!("app.secrets.disconnected_while_saving").to_string());
        }
    }
}
