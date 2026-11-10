//! Session-option flows: permission/mode pickers, workspace permission persistence, and agent cycling.
use super::composer_controls::capitalize_agent_label;
use super::*;

impl DesktopView {
    pub(super) fn can_select_session_options(&self) -> bool {
        self.session_change_allowed(None) && !self.attachments_picker_open && self.session_options().is_some()
    }

    pub(super) fn select_session_options(
        &mut self,
        options: protocol::SessionOptionsSelectParams,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.can_select_session_options() {
            return false;
        }
        if self.session_option_is_current(&options) {
            if let Some(choice) = options.permission_mode {
                self.save_workspace_permission(choice);
            }
            return true;
        }
        if self.request_session_change(protocol::method::CONFIG_OPTIONS, json!(options), cx) {
            self.pending_permission_choice = options.permission_mode;
            self.status = rust_i18n::t!("app.permissions.updating_runtime").to_string();
            true
        } else {
            false
        }
    }

    fn permission_workspace_key(&self) -> Option<String> {
        self.service_info
            .as_ref()
            .map(|info| info.workspace.clone())
            .or_else(|| self.workspace.as_ref().map(|path| path.to_string_lossy().into_owned()))
    }

    pub(super) fn save_workspace_permission(&mut self, choice: protocol::PermissionMode) {
        if choice == protocol::PermissionMode::FullAccess {
            return;
        }
        if let Some(workspace) = self.permission_workspace_key() {
            self.preferences.touch_workspace(&workspace);
            let _previous = self.preferences.workspace_permissions.insert(workspace, choice);
            if let Err(error) = self.preferences.try_save() {
                self.error = Some(rust_i18n::t!("app.permissions.save_failed", error = error.to_string()).to_string());
            }
        }
    }

    pub(super) fn restore_workspace_permission(&mut self, cx: &mut Context<Self>) {
        let Some(choice) = self
            .permission_workspace_key()
            .and_then(|workspace| self.preferences.workspace_permissions.get(&workspace).copied())
        else {
            return;
        };
        let options = protocol::SessionOptionsSelectParams {
            permission_mode: Some(choice),
            ..Default::default()
        };
        if !self.session_option_is_current(&options) && !self.select_session_options(options, cx) {
            self.error = Some(rust_i18n::t!("app.permissions.restore_failed").to_string());
        }
    }

    pub(super) fn workspace_permission_ready(&self) -> bool {
        if self
            .session_options()
            .is_some_and(|options| options.permission_mode == protocol::PermissionMode::FullAccess)
        {
            return true;
        }
        self.permission_workspace_key()
            .and_then(|workspace| self.preferences.workspace_permissions.get(&workspace))
            .is_none_or(|saved| self.session_options().is_some_and(|options| options.permission_mode == *saved))
    }

    pub(super) fn session_option_is_current(&self, options: &protocol::SessionOptionsSelectParams) -> bool {
        self.session_options().is_some_and(|current| {
            options.permission_mode.is_none_or(|mode| current.permission_mode == mode)
                && options.mode.as_ref().is_none_or(|mode| current.mode == *mode)
                && options
                    .reasoning_effort
                    .as_ref()
                    .is_none_or(|effort| current.reasoning_effort == *effort)
                && options.fast_mode.is_none_or(|enabled| current.fast_mode_enabled == enabled)
        })
    }

    pub(super) fn open_permissions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggle_palette_kind(PaletteKind::Permissions, window, cx) {
            return;
        }
        if !self.can_select_session_options() {
            return;
        }
        let entries = [
            (
                protocol::PermissionMode::Ask,
                permission_mode_label(protocol::PermissionMode::Ask),
                rust_i18n::t!("app.permissions.desc_ask").to_string(),
            ),
            (
                protocol::PermissionMode::AutoApproveCommands,
                permission_mode_label(protocol::PermissionMode::AutoApproveCommands),
                rust_i18n::t!("app.permissions.desc_auto").to_string(),
            ),
            (
                protocol::PermissionMode::FullAccess,
                permission_mode_label(protocol::PermissionMode::FullAccess),
                rust_i18n::t!("app.permissions.desc_full").to_string(),
            ),
        ]
        .into_iter()
        .map(|(permission_mode, label, description)| {
            let mut entry = command(
                label,
                PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams {
                    permission_mode: Some(permission_mode),
                    ..Default::default()
                }),
            );
            entry.description = Some(description);
            entry
        })
        .collect();
        self.show_palette(
            PaletteKind::Permissions,
            vec![(rust_i18n::t!("app.permissions.group_next_session").to_string(), entries)],
            window,
            cx,
        );
    }

    /// Cycle the primary agent forward (CLI Shift+Tab parity). The select
    /// path enforces the mode-switch lock, so mid-run presses are dropped
    /// exactly like the CLI busy notice.
    pub(super) fn cycle_primary_agent(&mut self, cx: &mut Context<Self>) {
        let Some(options) = self.session_options() else {
            return;
        };
        if options.available_modes.is_empty() {
            return;
        }
        // Unknown current mode (stale/custom value) starts at the first mode
        // instead of skipping it via `unwrap_or(0)`-then-increment.
        let next = options
            .available_modes
            .iter()
            .position(|mode| mode.name == options.mode)
            .map_or(0, |current| (current + 1) % options.available_modes.len());
        let Some(mode) = options.available_modes.get(next).map(|mode| mode.name.clone()) else {
            return;
        };
        // CLI parity: a lock-dropped cycle surfaces a busy notice instead of
        // silently swallowing the press (`MODE_SWITCH_BUSY_NOTICE`).
        if !self
            .select_session_options(protocol::SessionOptionsSelectParams { mode: Some(mode), ..Default::default() }, cx)
        {
            self.status = rust_i18n::t!("app.composer.mode_switch_busy").to_string();
            cx.notify();
        }
    }

    pub(super) fn open_modes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.toggle_palette_kind(PaletteKind::Modes, window, cx) {
            return;
        }
        if !self.can_select_session_options() {
            return;
        }
        let entries = self
            .session_options()
            .into_iter()
            .flat_map(|options| &options.available_modes)
            .map(|mode| {
                let mut entry = command(
                    capitalize_agent_label(&mode.display_name),
                    PaletteAction::SelectSessionOptions(protocol::SessionOptionsSelectParams {
                        mode: Some(mode.name.clone()),
                        ..Default::default()
                    }),
                );
                entry.description = Some(mode.description.clone());
                entry
            })
            .collect();
        self.show_palette(
            PaletteKind::Modes,
            vec![(rust_i18n::t!("app.permissions.group_default_agent").to_string(), entries)],
            window,
            cx,
        );
    }

    pub(super) fn render_mode_button(&self, _cx: &Context<Self>) -> AnyElement {
        let mode = self
            .active_primary_agent_display
            .as_ref()
            .filter(|active| self.selected_session_id.as_deref() == Some(active.session_id.as_str()))
            .map(|active| active.display_name.as_str())
            .or_else(|| {
                self.session_options().map(|options| {
                    options
                        .available_modes
                        .iter()
                        .find(|mode| mode.name == options.mode)
                        .map_or(options.mode.as_str(), |mode| mode.display_name.as_str())
                })
            })
            .map(capitalize_agent_label)
            .unwrap_or_else(|| rust_i18n::t!(i18n::CONFIGURATION_UNAVAILABLE).to_string());
        desktop_button("composer-mode")
            .open(self.palette_open_for(&[PaletteKind::Modes]))
            .child(div().id("composer-mode-label").test_support().truncate().child(mode.clone()))
            .max_w(px(180.))
            .overflow_hidden()
            .accessibility_label(rust_i18n::t!("app.composer.agent_mode_aria", mode = mode.as_str()).to_string())
            .tooltip_with_action(
                rust_i18n::t!("app.composer.choose_agent_mode").to_string(),
                &OpenModePicker,
                Some(key_context::DESKTOP),
            )
            .dropdown_caret(true)
            .ghost()
            .small()
            .desktop_disabled(!self.can_select_session_options())
            .dispatches_desktop_action(OpenModePicker, self.focus_handle.clone())
            .into_any_element()
    }
}

/// Short label for a session permission mode, shared by the composer
/// permission picker and the curated configuration row.
pub(super) fn permission_mode_label(mode: protocol::PermissionMode) -> String {
    match mode {
        protocol::PermissionMode::Ask => rust_i18n::t!(i18n::COMPOSER_PERMISSION_ASK).to_string(),
        protocol::PermissionMode::AutoApproveCommands => rust_i18n::t!(i18n::COMPOSER_PERMISSION_AUTO).to_string(),
        protocol::PermissionMode::FullAccess => rust_i18n::t!(i18n::COMPOSER_PERMISSION_FULL).to_string(),
    }
}
