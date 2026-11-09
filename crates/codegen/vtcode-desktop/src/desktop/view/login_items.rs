//! Launch-at-login through the main-app ServiceManagement login item.
//!
//! The OS owns the truth: the settings toggle registers/unregisters, and
//! launch reconciles the preference with the OS status, because the user
//! can flip the item in System Settings behind our back. Test builds swap
//! the native calls for a `cx`-scoped stub (see [`test_override`]), so
//! parallel headless tests never share stub state.

use super::*;

#[cfg(test)]
use gpui_kit::Global;
#[cfg(not(test))]
use objc2_service_management::{SMAppService, SMAppServiceStatus};

/// What the settings toggle understands. `NotRegistered` and `NotFound`
/// both read as [`LoginItemStatus::Disabled`]: nothing launches at login.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LoginItemStatus {
    Enabled,
    Disabled,
    RequiresApproval,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
struct LoginItemStub {
    status: Option<LoginItemStatus>,
    settings_opened: bool,
}

#[cfg(test)]
impl Global for LoginItemStub {}

/// Current OS status.
#[cfg_attr(
    not(test),
    expect(
        unsafe_code,
        reason = "SMAppService calls have no preconditions; unsafe by binding convention"
    )
)]
pub(super) fn status(cx: &App) -> LoginItemStatus {
    #[cfg(test)]
    {
        cx.try_global::<LoginItemStub>()
            .and_then(|stub| stub.status)
            .unwrap_or(LoginItemStatus::Disabled)
    }
    #[cfg(not(test))]
    {
        let _ = cx;
        // SAFETY: `mainAppService` has no preconditions; the binding takes
        // no main-thread marker because ServiceManagement calls are
        // thread-safe.
        let service = unsafe { SMAppService::mainAppService() };
        // SAFETY: `status` reads immutable registration state.
        match unsafe { service.status() } {
            SMAppServiceStatus::Enabled => LoginItemStatus::Enabled,
            SMAppServiceStatus::RequiresApproval => LoginItemStatus::RequiresApproval,
            _ => LoginItemStatus::Disabled,
        }
    }
}

/// Register or unregister the login item, returning the resulting status.
/// Registration can land on [`LoginItemStatus::RequiresApproval`]: the
/// caller keeps the intent and points at System Settings instead of failing.
#[cfg_attr(
    not(test),
    expect(unsafe_code, reason = "register/unregister touch only this app's own login item")
)]
pub(super) fn set_enabled(enabled: bool, cx: &mut App) -> Result<LoginItemStatus, String> {
    #[cfg(test)]
    {
        let status = if enabled {
            LoginItemStatus::Enabled
        } else {
            LoginItemStatus::Disabled
        };
        let opened = cx.try_global::<LoginItemStub>().is_some_and(|stub| stub.settings_opened);
        cx.set_global(LoginItemStub { status: Some(status), settings_opened: opened });
        Ok(status)
    }
    #[cfg(not(test))]
    {
        // SAFETY: same contract as [`status`].
        let service = unsafe { SMAppService::mainAppService() };
        let outcome = if enabled {
            // SAFETY: register touches only this app's own login item.
            unsafe { service.registerAndReturnError() }
        } else {
            // SAFETY: unregister touches only this app's own login item.
            unsafe { service.unregisterAndReturnError() }
        };
        outcome.map_err(|error| error.localizedDescription().to_string())?;
        Ok(status(cx))
    }
}

/// Open System Settings at Login Items so the user can approve the item.
#[cfg_attr(
    not(test),
    expect(unsafe_code, reason = "opening the Settings pane has no preconditions")
)]
pub(super) fn open_login_settings(cx: &mut App) {
    #[cfg(test)]
    {
        let status = cx.try_global::<LoginItemStub>().and_then(|stub| stub.status);
        cx.set_global(LoginItemStub { status, settings_opened: true });
    }
    #[cfg(not(test))]
    {
        let _ = cx;
        // SAFETY: opening the Settings pane has no preconditions.
        unsafe { SMAppService::openSystemSettingsLoginItems() };
    }
}

/// Sync the saved intent with the OS at launch. Approval-pending keeps the
/// intent: the settings row shows the approval hint until macOS confirms.
pub(super) fn reconcile_launch_preference(preferences: &mut preferences::Preferences, cx: &mut App) {
    match status(cx) {
        LoginItemStatus::Enabled => {
            if !preferences.launch_at_login_enabled() {
                preferences.launch_at_login = Some(true);
                preferences.save();
            }
        }
        LoginItemStatus::Disabled => {
            if preferences.launch_at_login_enabled() {
                preferences.launch_at_login = Some(false);
                preferences.save();
            }
        }
        LoginItemStatus::RequiresApproval => {}
    }
}

/// Drive the settings toggle: apply and record the intent. The caller
/// saves, mirroring the menu-bar toggle.
pub(super) fn apply_launch_at_login(
    enabled: bool,
    preferences: &mut preferences::Preferences,
    cx: &mut App,
) -> Result<LoginItemStatus, String> {
    let status = set_enabled(enabled, cx)?;
    preferences.launch_at_login = Some(enabled);
    Ok(status)
}

/// Set the stub status (`None` restores the Disabled default and clears the
/// opener flag).
#[cfg(test)]
pub(super) fn test_override(status: Option<LoginItemStatus>, cx: &mut App) {
    cx.set_global(LoginItemStub { status, settings_opened: false });
}

/// Whether the stub opener ran since the last override.
#[cfg(test)]
pub(super) fn test_settings_opened(cx: &App) -> bool {
    cx.try_global::<LoginItemStub>().is_some_and(|stub| stub.settings_opened)
}

impl DesktopView {
    /// Refresh the cached login-item status from the OS. Call on explicit
    /// gestures only (settings open, VT Code tab select); render reads the
    /// cache so scrolling never pays for synchronous ServiceManagement IPC.
    pub(super) fn refresh_login_item_status(&mut self, cx: &mut App) {
        self.login_item_status = status(cx);
    }

    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure.
    pub(super) fn render_login_items_settings_content(
        launch_at_login_enabled: bool,
        login_item_status: LoginItemStatus,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let title = rust_i18n::t!("app.settings.login_items.title").to_string();
        let colors = &cx.theme().colors;
        let mut group = settings_group_box(title.clone(), colors).child(settings_row(
            title.clone(),
            rust_i18n::t!("app.settings.login_items.description").to_string(),
            gpui_kit::component::switch::Switch::new("launch-at-login")
                .checked(launch_at_login_enabled)
                .accessibility_label(title.clone())
                .on_click({
                    let view = view.clone();
                    move |checked: &bool, window: &mut Window, cx: &mut App| {
                        let applied = view.update(cx, |view, cx| {
                            // The launch helper takes `&mut App`; the update
                            // context derefs to it.
                            match apply_launch_at_login(*checked, &mut view.preferences, cx) {
                                Ok(status) => {
                                    view.login_item_status = status;
                                    view.save_preferences();
                                    cx.notify();
                                    Ok(status)
                                }
                                Err(error) => Err(error),
                            }
                        });
                        if let Err(error) = applied {
                            window.push_notification(
                                gpui_kit::component::notification::Notification::error(
                                    rust_i18n::t!("app.settings.login_items.unavailable", error = error).to_string(),
                                ),
                                cx,
                            );
                        }
                    }
                }),
            colors,
        ));
        // Approval-pending keeps the intent on: the hint names the fix and
        // the button opens System Settings at Login Items. The cached
        // status keeps render off the synchronous ServiceManagement call.
        if login_item_status == LoginItemStatus::RequiresApproval {
            group = group.child(settings_row(
                rust_i18n::t!("app.settings.login_items.approval_title").to_string(),
                rust_i18n::t!("app.settings.login_items.approval_hint").to_string(),
                desktop_button("open-login-items")
                    .label(rust_i18n::t!("app.settings.login_items.open_settings").to_string())
                    .on_click(move |_, _: &mut Window, cx: &mut App| open_login_settings(cx)),
                colors,
            ));
        }
        accessible_group_box("login-items-settings-group", title, group)
    }
}
