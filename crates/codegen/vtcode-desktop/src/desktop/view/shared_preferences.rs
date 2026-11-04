//! CLI preference inheritance stays behind the typed local-service boundary.
use super::*;

fn known_theme(id: &str) -> bool {
    id == themes::SYSTEM_LIGHT_ID || id == themes::SYSTEM_DARK_ID || themes::find(id).is_some()
}

impl DesktopView {
    fn has_unresolved_submission(&self) -> bool {
        self.retained_submission.is_some() || self.pending_submission.is_some() || self.first_submission.is_some()
    }

    pub(super) fn can_sync_theme_to_cli(&self) -> bool {
        let Some(cli_id) = themes::cli_id_for(&self.theme_id) else {
            return false;
        };
        self.session_change_idle()
            && !self.has_unresolved_submission()
            && !self.attachments_picker_open
            && self.attachments_task.is_none()
            && !self.workspace_picker_open
            && self
                .configuration
                .as_ref()
                .and_then(|configuration| configuration.shared_ui.as_ref())
                .is_some_and(|preferences| preferences.theme != cli_id)
    }

    pub(super) fn sync_theme_to_cli(&mut self, cx: &mut Context<Self>) {
        if !self.can_sync_theme_to_cli() {
            return;
        }
        let Some(cli_id) = themes::cli_id_for(&self.theme_id) else {
            return;
        };
        let params = protocol::ThemePreferenceSelectParams { theme: cli_id.to_string() };
        if self.request(protocol::method::CONFIG_THEME, serde_json::json!(params)) {
            self.session_request = Some(protocol::method::CONFIG_THEME);
            self.status = rust_i18n::t!("app.settings.cli_sync.saving_theme").to_string();
            self.error = None;
        } else {
            self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
        }
        cx.notify();
    }

    pub(super) fn can_reload_cli_settings(&self) -> bool {
        self.workspace.is_some()
            && self.workspace_switch_idle()
            && self.interaction.is_none()
            && !self.has_unresolved_submission()
            && !self.attachments_picker_open
            && self.attachments_task.is_none()
            && !self.workspace_picker_open
    }

    pub(super) fn reload_cli_settings(&mut self, cx: &mut Context<Self>) {
        if !self.can_reload_cli_settings() {
            self.error = Some(rust_i18n::t!("app.settings.cli_sync.reload_blocked").to_string());
            cx.notify();
            return;
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        if let Some(backend) = self.backend.as_ref() {
            // Wait for the supervised transport's terminal event before replacing it.
            if backend.commands.try_send(BackendCommand::Stop).is_err() {
                self.error = Some(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
            } else {
                self.pending_workspace = Some(workspace);
                self.status = rust_i18n::t!("app.settings.cli_sync.reloading").to_string();
            }
        } else {
            self.start_workspace(workspace, cx);
        }
        cx.notify();
    }

    pub(super) fn reload_cli_settings_from_ui(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reload_cli_settings(cx);
        self.restore_focus_after_review_close(window, cx);
    }

    pub(super) fn sync_cli_theme(&mut self, cx: &mut Context<Self>) {
        if !self.preferences.follow_cli_theme.unwrap_or(self.preferences.theme.is_none()) {
            return;
        }
        let theme = self
            .configuration
            .as_ref()
            .and_then(|configuration| configuration.shared_ui.as_ref())
            .map(|preferences| preferences.theme.clone());
        let Some(theme) = theme else {
            return;
        };
        if !known_theme(&theme) {
            self.error = Some(rust_i18n::t!("app.settings.cli_sync.theme_unavailable").to_string());
            return;
        }
        self.preferences.follow_cli_theme = Some(true);
        self.apply_theme_state(theme, None, cx);
    }

    pub(super) fn render_shared_preferences(&self, cx: &Context<Self>) -> AnyElement {
        let shared = self
            .configuration
            .as_ref()
            .and_then(|configuration| configuration.shared_ui.as_ref());
        let theme = shared.map(|preferences| preferences.theme.clone());
        let follows = self.preferences.follow_cli_theme.unwrap_or(self.preferences.theme.is_none());
        let sync_label = rust_i18n::t!("app.settings.cli_sync.sync_theme").to_string();
        let reload_label = rust_i18n::t!("app.settings.cli_sync.reload_settings").to_string();
        let use_theme_label = rust_i18n::t!("app.settings.cli_sync.use_theme").to_string();
        let group = settings_group_box(rust_i18n::t!("app.settings.cli_sync.group").to_string(), &cx.theme().colors)
            .child(div().text_sm().child(match theme.as_ref() {
                Some(theme) => {
                    let mode = if follows {
                        rust_i18n::t!("app.settings.cli_sync.following").to_string()
                    } else {
                        rust_i18n::t!("app.settings.cli_sync.desktop_override").to_string()
                    };
                    rust_i18n::t!("app.settings.cli_sync.theme_status", theme = theme, mode = mode).to_string()
                }
                None => rust_i18n::t!("app.settings.cli_sync.disconnected").to_string(),
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        desktop_default_button("sync-cli-theme")
                            .label(sync_label.as_str())
                            .tooltip_with_action(sync_label.as_str(), &SyncCliTheme, Some(key_context::DESKTOP))
                            .desktop_disabled(!self.can_sync_theme_to_cli())
                            .dispatches_desktop_action(SyncCliTheme, self.focus_handle.clone()),
                    )
                    .child(
                        desktop_default_button("reload-cli-settings")
                            .label(reload_label.as_str())
                            .tooltip_with_action(reload_label.as_str(), &ReloadCliSettings, Some(key_context::DESKTOP))
                            .desktop_disabled(!self.can_reload_cli_settings())
                            .dispatches_desktop_action(ReloadCliSettings, self.focus_handle.clone()),
                    )
                    .child(
                        desktop_default_button("use-cli-theme")
                            .label(use_theme_label.as_str())
                            .desktop_disabled(theme.as_deref().is_none_or(|theme| !known_theme(theme)))
                            .on_click(cx.listener(|view, _, window, cx| {
                                let theme = view
                                    .configuration
                                    .as_ref()
                                    .and_then(|configuration| configuration.shared_ui.as_ref())
                                    .map(|preferences| preferences.theme.clone());
                                if let Some(theme) = theme.filter(|theme| known_theme(theme)) {
                                    view.preferences.follow_cli_theme = Some(true);
                                    view.apply_theme_state(theme, Some(window), cx);
                                }
                            })),
                    ),
            )
            .footer(rust_i18n::t!("app.settings.cli_sync.footer").to_string());
        accessible_group_box("cli-preferences-group", rust_i18n::t!("app.settings.cli_sync.group").to_string(), group)
    }

    pub(super) fn import_cli_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(shared) = self
            .configuration
            .as_ref()
            .and_then(|configuration| configuration.shared_ui.as_ref())
        else {
            return;
        };
        if shared.shortcuts.is_empty() {
            self.shortcut_message = Some(if shared.shortcut_warnings.is_empty() {
                rust_i18n::t!("app.shortcuts.no_cli_overrides").to_string()
            } else {
                shared.shortcut_warnings.join("; ")
            });
            cx.notify();
            return;
        }
        let mut preferences = self.preferences.clone();
        preferences.shortcuts = self.shortcut_draft.clone();
        for (action, keys) in &shared.shortcuts {
            let _previous = preferences
                .shortcuts
                .insert(action.clone(), keys.first().cloned().unwrap_or_default());
        }
        let message = match shortcuts::bindings(&preferences.shortcuts, cx) {
            Err(error) => error,
            Ok(_) => match preferences.try_save() {
                Err(error) => error,
                Ok(()) => {
                    bootstrap::reinstall_shortcuts(cx, &self.preferences.shortcuts, &preferences.shortcuts);
                    self.shortcut_draft = shortcuts::draft_from(&preferences.shortcuts);
                    self.shortcut_capture = None;
                    self.preferences = preferences;
                    self.shortcuts_editing = false;
                    self.settings_navigation_focus.focus(window, cx);
                    if shared.shortcut_warnings.is_empty() {
                        rust_i18n::t!("app.shortcuts.cli_imported").to_string()
                    } else {
                        rust_i18n::t!(
                            "app.shortcuts.cli_imported_warnings",
                            warnings = shared.shortcut_warnings.join("; ")
                        )
                        .to_string()
                    }
                }
            },
        };
        self.shortcut_message = Some(message);
        cx.notify();
    }
}
