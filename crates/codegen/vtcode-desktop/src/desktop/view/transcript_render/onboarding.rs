//! First-workspace onboarding panel: guided permission and agent setup.
//!
//! Rendered inside the welcome prelude until the workspace has a saved
//! permission choice. The triggers mirror the composer's own picker buttons
//! (current value plus caret, same actions and tooltips), so no new actions
//! or protocol calls are introduced.

use super::super::composer_controls::{capitalize_agent_label, permission_indicator};
use super::super::session_options::permission_mode_label;
use super::*;

impl DesktopView {
    /// Show once per workspace: connected, no sessions yet, and no saved
    /// permission choice. Gated on `session_list_received` so the panel does
    /// not flash while the session list is still loading.
    pub(super) fn should_show_onboarding(&self) -> bool {
        if self.workspace.is_none() || !self.session_list_received || !self.sessions.is_empty() {
            return false;
        }
        self.permission_workspace_key()
            .and_then(|key| self.preferences.workspace_permissions.get(&key))
            .is_none()
    }

    pub(super) fn render_onboarding_panel(&self, cx: &App) -> AnyElement {
        let colors = cx.theme().colors;
        let title = rust_i18n::t!("app.welcome.onboarding_title").to_string();

        let mut panel = div()
            .id("onboarding-panel")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(title.clone())
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .mt_4()
            .child(
                div()
                    .id("onboarding-title")
                    .test_support()
                    .role(gpui_kit::Role::Heading)
                    .aria_label(title.clone())
                    .text_sm()
                    .font_semibold()
                    .text_color(colors.foreground)
                    .child(title),
            )
            .child(self.render_onboarding_row(
                "onboarding-permissions",
                rust_i18n::t!("app.welcome.onboarding_permissions_label").to_string(),
                rust_i18n::t!("app.welcome.onboarding_permissions_detail").to_string(),
                self.render_onboarding_permission_trigger(cx),
                cx,
            ))
            .child(self.render_onboarding_row(
                "onboarding-agent",
                rust_i18n::t!("app.welcome.onboarding_agent_label").to_string(),
                rust_i18n::t!("app.welcome.onboarding_agent_detail").to_string(),
                self.render_onboarding_agent_trigger(),
                cx,
            ));

        if let Some(auth) = self.render_onboarding_auth(cx) {
            panel = panel.child(auth);
        }

        panel
            .child(
                div()
                    .id("onboarding-trust-note")
                    .test_support()
                    .text_xs()
                    .text_color(colors.muted_foreground)
                    .child(rust_i18n::t!("app.welcome.onboarding_trust_note").to_string()),
            )
            .into_any_element()
    }

    fn render_onboarding_row(
        &self,
        id: &'static str,
        label: String,
        detail: String,
        trigger: AnyElement,
        cx: &App,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        div()
            .id(id)
            .test_support()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .child(div().text_sm().font_medium().text_color(colors.foreground).child(label))
                    .child(div().text_xs().text_color(colors.muted_foreground).child(detail)),
            )
            .child(trigger)
            .into_any_element()
    }

    fn render_onboarding_permission_trigger(&self, cx: &App) -> AnyElement {
        let mode = self.session_options().map(|options| options.permission_mode);
        let label = mode
            .map(permission_mode_label)
            .unwrap_or_else(|| rust_i18n::t!(i18n::CONFIGURATION_LOADING_VALUE).to_string());
        let mut trigger = desktop_ghost_button("onboarding-permissions-button")
            .open(self.palette_open_for(&[PaletteKind::Permissions]));
        if let Some(mode) = mode {
            let (icon, icon_color) = permission_indicator(Some(mode), cx.theme().colors);
            trigger = trigger.icon(Icon::new(icon).text_color(icon_color));
        }
        trigger
            .child(
                div()
                    .id("onboarding-permissions-label")
                    .test_support()
                    .truncate()
                    .font_medium()
                    .child(label.clone()),
            )
            .dropdown_caret(true)
            .small()
            .min_w_0()
            .accessibility_label(rust_i18n::t!("app.composer.execution_policy", policy = label.as_str()).to_string())
            .tooltip_with_action(
                rust_i18n::t!("app.composer.execution_policy", policy = label.as_str()).to_string(),
                &OpenPermissions,
                Some(key_context::DESKTOP),
            )
            .desktop_disabled(!self.can_select_session_options())
            .dispatches_desktop_action(OpenPermissions, self.focus_handle.clone())
            .into_any_element()
    }

    fn render_onboarding_agent_trigger(&self) -> AnyElement {
        let mode = self
            .session_options()
            .map(|options| {
                options
                    .available_modes
                    .iter()
                    .find(|mode| mode.name == options.mode)
                    .map_or(options.mode.as_str(), |mode| mode.display_name.as_str())
            })
            .map(capitalize_agent_label)
            .unwrap_or_else(|| rust_i18n::t!(i18n::CONFIGURATION_LOADING_VALUE).to_string());
        desktop_ghost_button("onboarding-agent-button")
            .open(self.palette_open_for(&[PaletteKind::Modes]))
            .child(
                div()
                    .id("onboarding-agent-label")
                    .test_support()
                    .truncate()
                    .font_medium()
                    .child(mode.clone()),
            )
            .dropdown_caret(true)
            .small()
            .min_w_0()
            .accessibility_label(rust_i18n::t!("app.composer.agent_mode_aria", mode = mode.as_str()).to_string())
            .tooltip_with_action(
                rust_i18n::t!("app.composer.choose_agent_mode").to_string(),
                &OpenModePicker,
                Some(key_context::DESKTOP),
            )
            .desktop_disabled(!self.can_select_session_options())
            .dispatches_desktop_action(OpenModePicker, self.focus_handle.clone())
            .into_any_element()
    }

    fn render_onboarding_auth(&self, cx: &App) -> Option<AnyElement> {
        let configuration = self.configuration.as_ref()?;
        let colors = cx.theme().colors;
        let text = if configuration.authenticated {
            rust_i18n::t!(
                "app.welcome.onboarding_auth_ready",
                provider = configuration.provider.as_str(),
                model = configuration.model.as_str()
            )
            .to_string()
        } else {
            format!(
                "{} {}",
                rust_i18n::t!("app.welcome.onboarding_auth_setup_prefix"),
                configuration.credential_setup_command
            )
        };
        Some(
            div()
                .id("onboarding-auth")
                .test_support()
                .text_xs()
                .text_color(colors.muted_foreground)
                .child(text)
                .into_any_element(),
        )
    }
}
