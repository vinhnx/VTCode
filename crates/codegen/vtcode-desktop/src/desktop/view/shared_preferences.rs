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
        self.session_change_idle()
            && !self.has_unresolved_submission()
            && !self.attachments_picker_open
            && self.attachments_task.is_none()
            && !self.workspace_picker_open
            && themes::find(&self.theme_id).is_some()
            && self
                .configuration
                .as_ref()
                .and_then(|configuration| configuration.shared_ui.as_ref())
                .is_some_and(|preferences| preferences.theme != self.theme_id)
    }

    pub(super) fn sync_theme_to_cli(&mut self, cx: &mut Context<Self>) {
        if !self.can_sync_theme_to_cli() {
            return;
        }
        let params = protocol::ThemePreferenceSelectParams { theme: self.theme_id.clone() };
        if self.request(protocol::method::CONFIG_THEME, serde_json::json!(params)) {
            self.session_request = Some(protocol::method::CONFIG_THEME);
            self.status = "Saving the selected theme to CLI configuration…".into();
            self.error = None;
        } else {
            self.error = Some(rust_i18n::t!("app.errors.request_queue_unavailable").to_string());
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
            self.error =
                Some("Finish the active operation or restore the unsent message before reloading CLI settings.".into());
            cx.notify();
            return;
        }
        let Some(workspace) = self.workspace.clone() else {
            return;
        };
        if let Some(backend) = self.backend.as_ref() {
            // Wait for the supervised transport's terminal event before replacing it.
            if backend.commands.try_send(BackendCommand::Stop).is_err() {
                self.error = Some(rust_i18n::t!("app.errors.request_queue_unavailable").to_string());
            } else {
                self.pending_workspace = Some(workspace);
                self.status = "Reloading CLI settings…".into();
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
            self.error =
                Some("The CLI theme is not available in VT Code Desktop. Choose a desktop theme in Settings.".into());
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
        let group = GroupBox::new()
            .flex_shrink_0()
            .fill()
            .title(group_box_title("CLI preferences", &cx.theme().colors))
            .child(div().text_sm().child(match theme.as_ref() {
                Some(theme) => format!("CLI theme: {theme} · {}", if follows { "Following CLI" } else { "Desktop override" }),
                None => "Connect to the current desktop service to load CLI preferences.".into(),
            }))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        desktop_button("sync-cli-theme")
                            .label("Sync current theme to CLI")
                            .ghost()
                            .tooltip_with_action("Sync current theme to CLI", &SyncCliTheme, Some("VTCodeDesktop"))
                            .desktop_disabled(!self.can_sync_theme_to_cli())
                            .dispatches_desktop_action(SyncCliTheme, self.focus_handle.clone()),
                    )
                    .child(
                        desktop_button("reload-cli-settings")
                            .label("Reload CLI settings")
                            .ghost()
                            .tooltip_with_action("Reload CLI settings", &ReloadCliSettings, Some("VTCodeDesktop"))
                            .desktop_disabled(!self.can_reload_cli_settings())
                            .dispatches_desktop_action(ReloadCliSettings, self.focus_handle.clone()),
                    )
                    .child(
                        desktop_button("use-cli-theme")
                            .label("Use CLI theme")
                            .ghost()
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
            .footer("Sync saves the selected built-in theme through the CLI config writer; new CLI sessions use it. Reload reads all CLI configuration layers through a fresh local service. Reload starts a new conversation, keeps the editable draft and attached files, and leaves previous sessions in history.");
        accessible_group_box("cli-preferences-group", "CLI preferences", group)
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
                "No equivalent CLI shortcut overrides are configured.".into()
            } else {
                shared.shortcut_warnings.join("; ")
            });
            cx.notify();
            return;
        }
        let mut preferences = self.preferences.clone();
        preferences.shortcuts = self.shortcut_values(cx);
        for (action, keys) in &shared.shortcuts {
            let _previous = preferences
                .shortcuts
                .insert(action.clone(), keys.first().cloned().unwrap_or_default());
        }
        let message = match shortcuts::bindings(&preferences.shortcuts) {
            Err(error) => error,
            Ok(_) => match preferences.try_save() {
                Err(error) => error,
                Ok(()) => {
                    self.shortcut_inputs = shortcuts::inputs(&preferences.shortcuts, window, cx);
                    self.preferences = preferences;
                    self.shortcuts_editing = false;
                    self.settings_navigation_focus.focus(window, cx);
                    if shared.shortcut_warnings.is_empty() {
                        "CLI shortcuts imported. Restart VT Code Desktop to apply.".into()
                    } else {
                        format!("CLI shortcuts imported; {}. Restart to apply.", shared.shortcut_warnings.join("; "))
                    }
                }
            },
        };
        self.shortcut_message = Some(message);
        cx.notify();
    }
}
