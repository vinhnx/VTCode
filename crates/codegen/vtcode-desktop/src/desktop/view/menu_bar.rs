//! macOS AppKit interop: status item and close-to-menu-bar lifecycle.
use super::*;
use gpui_kit::Global;
#[cfg(not(test))]
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject, sel};
#[cfg(not(test))]
use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
#[cfg(not(test))]
use objc2_foundation::NSString;

struct MenuBarState {
    enabled: bool,
    #[cfg(not(test))]
    item: Option<Retained<NSStatusItem>>,
}

impl Global for MenuBarState {}

pub(super) fn enabled(cx: &App) -> bool {
    cx.try_global::<MenuBarState>().is_some_and(|state| state.enabled)
}

#[cfg(test)]
pub(super) fn set_enabled(enabled: bool, cx: &mut App) -> bool {
    cx.set_global(MenuBarState { enabled });
    true
}

#[cfg(not(test))]
#[expect(unsafe_code, reason = "AppKit menu actions require Objective-C selectors")]
pub(super) fn set_enabled(enabled: bool, cx: &mut App) -> bool {
    if enabled == self::enabled(cx) {
        return true;
    }
    if enabled {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let status_bar = NSStatusBar::systemStatusBar();
        let item = status_bar.statusItemWithLength(NSVariableStatusItemLength);
        let Some(button) = item.button(mtm) else {
            status_bar.removeStatusItem(&item);
            return false;
        };
        button.setTitle(&NSString::from_str("VT"));
        let app = NSApplication::sharedApplication(mtm);
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        for (title, action) in [
            (rust_i18n::t!("app.settings.menu_bar.show").to_string(), sel!(unhide:)),
            (rust_i18n::t!("app.settings.menu_bar.quit").to_string(), sel!(terminate:)),
        ] {
            let menu_item = NSMenuItem::new(mtm);
            menu_item.setTitle(&NSString::from_str(&title));
            // SAFETY: NSApplication is a process-lifetime target used on the main thread.
            unsafe { menu_item.setTarget(Some(&*app as &AnyObject)) };
            // SAFETY: both selectors are NSApplication actions with one sender argument.
            unsafe { menu_item.setAction(Some(action)) };
            menu.addItem(&menu_item);
        }
        item.setMenu(Some(&menu));
        cx.set_global(MenuBarState { enabled: true, item: Some(item) });
    } else if let Some(state) = cx.try_global::<MenuBarState>() {
        if let Some(item) = state.item.as_ref() {
            NSStatusBar::systemStatusBar().removeStatusItem(item);
        }
        cx.set_global(MenuBarState { enabled: false, item: None });
    }
    true
}

pub(super) fn refresh_labels(cx: &App) {
    #[cfg(not(test))]
    if let Some((mtm, item)) =
        MainThreadMarker::new().zip(cx.try_global::<MenuBarState>().and_then(|state| state.item.as_ref()))
    {
        if let Some(menu) = item.menu(mtm) {
            for (index, title) in [
                (0, rust_i18n::t!("app.settings.menu_bar.show").to_string()),
                (1, rust_i18n::t!("app.settings.menu_bar.quit").to_string()),
            ] {
                if let Some(item) = menu.itemAtIndex(index) {
                    item.setTitle(&NSString::from_str(&title));
                }
            }
        }
    }
    #[cfg(test)]
    let _ = cx;
}

/// Hide the app instead of closing when menu-bar mode owns the last window.
pub(super) fn should_hide(cx: &App) -> bool {
    enabled(cx) && cx.windows().len() == 1
}

pub(super) fn close_or_hide(window: &mut Window, cx: &mut App) {
    if should_hide(cx) {
        hide(cx);
    } else {
        window.remove_window();
    }
}

pub(super) fn hide(cx: &mut App) {
    #[cfg(not(test))]
    cx.hide();
    #[cfg(test)]
    let _ = cx;
}

impl DesktopView {
    pub(super) fn render_menu_bar_settings(&self, cx: &Context<Self>) -> AnyElement {
        let title = rust_i18n::t!("app.settings.menu_bar.title").to_string();
        accessible_group_box(
            "menu-bar-settings-group",
            title.clone(),
            settings_group_box(title.clone(), &cx.theme().colors).child(settings_row(
                title.clone(),
                rust_i18n::t!("app.settings.menu_bar.description").to_string(),
                gpui_kit::component::switch::Switch::new("show-in-menu-bar")
                    .checked(self.preferences.show_in_menu_bar && enabled(cx))
                    .accessibility_label(title)
                    .on_click(cx.listener(|view, checked, window, cx| {
                        if set_enabled(*checked, cx) {
                            view.preferences.show_in_menu_bar = *checked;
                            view.save_preferences();
                            cx.notify();
                        } else {
                            window.push_notification(
                                gpui_kit::component::notification::Notification::error(
                                    rust_i18n::t!("app.settings.menu_bar.unavailable").to_string(),
                                ),
                                cx,
                            );
                        }
                    })),
                &cx.theme().colors,
            )),
        )
    }
}
