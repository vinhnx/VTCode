//! macOS menus: the native application menu bar, plus AppKit interop for the monochrome status item, grouped status menu, and close-to-menu-bar lifecycle.
use super::*;
use gpui_kit::Global;
#[cfg(not(test))]
use objc2::runtime::{Bool, NSObject};
#[cfg(not(test))]
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::AnyObject, sel,
};
#[cfg(not(test))]
use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSVariableStatusItemLength};
#[cfg(not(test))]
use objc2_foundation::{NSSize, NSString};

/// Status-item icon edge: the 18pt square macOS centers in the menu bar.
#[cfg(not(test))]
const STATUS_ICON_EDGE: f64 = 18.;

/// Monochrome V mark for the status item. Installed as a template image, so
/// AppKit recolors it for light/dark appearances automatically; the app-icon
/// preference and theme mode never touch this art.
const STATUS_ICON_PNG: &[u8] = include_bytes!("../../../../../../resources/icons/vtcode-mono-120.png");

/// Decode the embedded monochrome art into a menu-bar-sized template image.
#[cfg(not(test))]
fn status_item_image(mtm: MainThreadMarker) -> Option<Retained<NSImage>> {
    use objc2_foundation::NSData;

    let data = NSData::with_bytes(STATUS_ICON_PNG);
    let image = NSImage::initWithData(mtm.alloc::<NSImage>(), &data)?;
    image.setSize(NSSize { width: STATUS_ICON_EDGE, height: STATUS_ICON_EDGE });
    image.setTemplate(true);
    Some(image)
}

/// Actions reachable from the macOS status-item menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuBarAction {
    Show,
    NewSession,
    OpenWorkspace,
    OpenSettings,
    OpenHelp,
    About,
    Quit,
}

impl MenuBarAction {
    /// Every action in status-menu order.
    #[cfg(test)]
    const ALL: [Self; 7] = [
        Self::Show,
        Self::NewSession,
        Self::OpenWorkspace,
        Self::OpenSettings,
        Self::OpenHelp,
        Self::About,
        Self::Quit,
    ];

    /// Stable tag stored on the `NSMenuItem`; the flat position in [`menu_groups`] order.
    fn tag(self) -> isize {
        match self {
            Self::Show => 0,
            Self::NewSession => 1,
            Self::OpenWorkspace => 2,
            Self::OpenSettings => 3,
            Self::OpenHelp => 4,
            Self::About => 5,
            Self::Quit => 6,
        }
    }

    fn from_tag(tag: isize) -> Option<Self> {
        match tag {
            0 => Some(Self::Show),
            1 => Some(Self::NewSession),
            2 => Some(Self::OpenWorkspace),
            3 => Some(Self::OpenSettings),
            4 => Some(Self::OpenHelp),
            5 => Some(Self::About),
            6 => Some(Self::Quit),
            _ => None,
        }
    }

    /// Localized item title. Window and workspace commands reuse the main-menu
    /// copy so both surfaces stay in sync across all locales.
    #[cfg(not(test))]
    fn title(self) -> String {
        self.title_in_locale(&rust_i18n::locale())
    }

    /// Whether the status-menu item is enabled. Window-free actions always
    /// are; the rest read the main window's rendered tree through the same
    /// check main-menu validation uses, rooted at the active-or-first window
    /// so hidden apps still validate.
    pub(super) fn is_allowed(self, cx: &mut App) -> bool {
        let dispatched: &dyn gpui_kit::Action = match self {
            Self::Show | Self::Quit | Self::OpenHelp => return true,
            Self::NewSession => &NewSession,
            Self::OpenWorkspace => &ChooseWorkspace,
            Self::OpenSettings => &OpenSettings,
            Self::About => &AboutApplication,
        };
        main_window(cx).is_some_and(|window| {
            window
                .update(cx, |_, window, cx| window.is_action_available(dispatched, cx))
                .unwrap_or(false)
        })
    }

    /// Title for an explicit locale, so tests pin every language without
    /// mutating the process-wide locale other tests read. Production resolves
    /// through the same match via the ambient locale.
    fn title_in_locale(self, locale: &str) -> String {
        match self {
            Self::Show => rust_i18n::t!("app.settings.menu_bar.show", locale = locale).to_string(),
            Self::NewSession => rust_i18n::t!("app.menus.new_session", locale = locale).to_string(),
            Self::OpenWorkspace => rust_i18n::t!("app.menus.open_workspace", locale = locale).to_string(),
            Self::OpenSettings => rust_i18n::t!("app.menus.settings_menu", locale = locale).to_string(),
            Self::OpenHelp => rust_i18n::t!("app.menus.help_website", locale = locale).to_string(),
            Self::About => rust_i18n::t!("app.menus.about", locale = locale).to_string(),
            Self::Quit => rust_i18n::t!("app.settings.menu_bar.quit", locale = locale).to_string(),
        }
    }
}

/// Status-menu groups in display order: window, workspace, app, lifecycle.
/// AppKit inserts one separator between groups.
pub(super) fn menu_groups() -> &'static [&'static [MenuBarAction]] {
    use MenuBarAction::{About, NewSession, OpenHelp, OpenSettings, OpenWorkspace, Quit, Show};
    &[
        &[Show],
        &[NewSession, OpenWorkspace],
        &[OpenSettings, OpenHelp, About],
        &[Quit],
    ]
}

/// Flattened status-menu actions in display order.
#[cfg(test)]
fn menu_items() -> Vec<MenuBarAction> {
    menu_groups().iter().flat_map(|group| group.iter().copied()).collect()
}

/// Run a status-menu action. Window-bound commands show and activate the main
/// window first so they always have a view to land on.
pub(super) fn dispatch_status_action(action: MenuBarAction, cx: &mut App) {
    match action {
        MenuBarAction::Show => bootstrap::show_main_window(cx),
        MenuBarAction::Quit => cx.quit(),
        MenuBarAction::OpenHelp => cx.open_url(bootstrap::HELP_URL),
        MenuBarAction::NewSession => show_and_dispatch(Box::new(NewSession), cx),
        MenuBarAction::OpenWorkspace => show_and_dispatch(Box::new(ChooseWorkspace), cx),
        MenuBarAction::OpenSettings => show_and_dispatch(Box::new(OpenSettings), cx),
        MenuBarAction::About => show_and_dispatch(Box::new(AboutApplication), cx),
    }
}

fn show_and_dispatch(action: Box<dyn gpui_kit::Action>, cx: &mut App) {
    bootstrap::show_main_window(cx);
    if let Some(window) = main_window(cx) {
        // The window can close between lookup and dispatch; a failed update has no landing spot left.
        if window.update(cx, |_, window, cx| window.dispatch_action(action, cx)).is_err() {}
    }
}

/// Preferred window for status-menu commands: the active window, else the first.
fn main_window(cx: &App) -> Option<gpui_kit::AnyWindowHandle> {
    cx.active_window().or_else(|| cx.windows().first().copied())
}

/// `AsyncApp` bridge for status-menu actions, mirroring GPUI's own menu
/// dispatch (`handleGPUIMenuItem:` reads the item tag and updates the app).
#[cfg(not(test))]
struct StatusTargetIvars {
    async_app: gpui_kit::AsyncApp,
}

#[cfg(not(test))]
define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - `MenuBarTarget` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "VTCodeMenuBarTarget"]
    #[ivars = StatusTargetIvars]
    struct MenuBarTarget;

    impl MenuBarTarget {
        #[unsafe(method(validateMenuItem:))]
        fn validate_item(&self, item: Option<&NSMenuItem>) -> Bool {
            let Some(item) = item else {
                return false.into();
            };
            let Some(action) = MenuBarAction::from_tag(item.tag()) else {
                return false.into();
            };
            self.ivars().async_app.update(|cx| action.is_allowed(cx)).into()
        }

        #[unsafe(method(handleMenuBarItem:))]
        fn handle_item(&self, sender: Option<&NSMenuItem>) {
            let Some(item) = sender else {
                return;
            };
            let Some(action) = MenuBarAction::from_tag(item.tag()) else {
                return;
            };
            // The target is owned by App globals and actions fire on the main
            // runloop while the app lives, so the weak upgrade inside `update`
            // cannot fail here; a failure would be an invariant violation.
            self.ivars().async_app.update(|cx| dispatch_status_action(action, cx));
        }
    }
);

#[cfg(not(test))]
impl MenuBarTarget {
    #[expect(unsafe_code, reason = "AppKit object construction requires an Objective-C init call")]
    fn new(async_app: gpui_kit::AsyncApp, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(StatusTargetIvars { async_app });
        // SAFETY: `init` on a freshly allocated NSObject subclass instance.
        unsafe { msg_send![super(this), init] }
    }
}

/// Append the grouped status-menu items, with separators between groups.
#[cfg(not(test))]
#[expect(unsafe_code, reason = "AppKit menu actions require Objective-C selectors")]
fn add_menu_groups(menu: &NSMenu, target: &MenuBarTarget, mtm: MainThreadMarker) {
    for (index, group) in menu_groups().iter().enumerate() {
        if index > 0 {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
        }
        for action in group.iter() {
            let item = NSMenuItem::new(mtm);
            item.setTitle(&NSString::from_str(&action.title()));
            item.setTag(action.tag());
            // SAFETY: the target outlives the menu (retained in `MenuBarState`) and both run on the main thread.
            unsafe { item.setTarget(Some(target as &AnyObject)) };
            // SAFETY: `handleMenuBarItem:` takes one sender argument.
            unsafe { item.setAction(Some(sel!(handleMenuBarItem:))) };
            menu.addItem(&item);
        }
    }
}

struct MenuBarState {
    enabled: bool,
    #[cfg(not(test))]
    item: Option<Retained<NSStatusItem>>,
    #[cfg(not(test))]
    target: Option<Retained<MenuBarTarget>>,
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
pub(super) fn set_enabled(enabled: bool, cx: &mut App) -> bool {
    if enabled == self::enabled(cx) {
        return true;
    }
    if enabled {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(image) = status_item_image(mtm) else {
            return false;
        };
        let status_bar = NSStatusBar::systemStatusBar();
        let item = status_bar.statusItemWithLength(NSVariableStatusItemLength);
        let Some(button) = item.button(mtm) else {
            status_bar.removeStatusItem(&item);
            return false;
        };
        button.setImage(Some(&image));
        button.setToolTip(Some(&NSString::from_str(app::NAME)));
        let target = MenuBarTarget::new(cx.to_async(), mtm);
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(true);
        add_menu_groups(&menu, &target, mtm);
        item.setMenu(Some(&menu));
        cx.set_global(MenuBarState {
            enabled: true,
            item: Some(item),
            target: Some(target),
        });
    } else if let Some(state) = cx.try_global::<MenuBarState>() {
        if let Some(item) = state.item.as_ref() {
            NSStatusBar::systemStatusBar().removeStatusItem(item);
        }
        cx.set_global(MenuBarState { enabled: false, item: None, target: None });
    }
    true
}

/// Rebuild the status-menu items so titles follow the active locale.
pub(super) fn refresh_labels(cx: &App) {
    #[cfg(not(test))]
    if let Some(mtm) = MainThreadMarker::new()
        && let Some(state) = cx.try_global::<MenuBarState>()
        && let (Some(item), Some(target)) = (state.item.as_ref(), state.target.as_ref())
        && let Some(menu) = item.menu(mtm)
    {
        menu.removeAllItems();
        add_menu_groups(&menu, target, mtm);
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
    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure.
    pub(super) fn render_menu_bar_settings_content(
        show_in_menu_bar: bool,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let title = rust_i18n::t!("app.settings.menu_bar.title").to_string();
        accessible_group_box(
            "menu-bar-settings-group",
            title.clone(),
            settings_group_box(title.clone(), &cx.theme().colors).child(settings_row(
                title.clone(),
                rust_i18n::t!("app.settings.menu_bar.description").to_string(),
                gpui_kit::component::switch::Switch::new("show-in-menu-bar")
                    .checked(show_in_menu_bar && enabled(cx))
                    .accessibility_label(title)
                    .on_click({
                        let view = view.clone();
                        move |checked: &bool, window: &mut Window, cx: &mut App| {
                            if set_enabled(*checked, cx) {
                                view.update(cx, |view, cx| {
                                    view.preferences.show_in_menu_bar = *checked;
                                    view.save_preferences();
                                    cx.notify();
                                });
                            } else {
                                window.push_notification(
                                    gpui_kit::component::notification::Notification::error(
                                        rust_i18n::t!("app.settings.menu_bar.unavailable").to_string(),
                                    ),
                                    cx,
                                );
                            }
                        }
                    }),
                &cx.theme().colors,
            )),
        )
    }
}

pub(super) fn init_native_menus(cx: &mut App) {
    register_global_actions(cx);
    set_native_menus(cx, false);
}

/// Register the global action handlers backing native menu items. Called once
/// at startup: `on_action` handlers accumulate, so re-registering on every
/// menu rebuild would run actions (OpenHelp, ShowMainWindow) N+1 times.
fn register_global_actions(cx: &mut App) {
    let _app = cx.on_action(|_: &QuitApplication, cx| cx.quit());
    let _app = cx.on_action(|_: &HideApplication, cx| cx.hide());
    let _app = cx.on_action(|_: &HideOtherApplications, cx| cx.hide_other_apps());
    let _app = cx.on_action(|_: &ShowAllApplications, cx| cx.unhide_other_apps());
    let _app = cx.on_action(|_: &ShowMainWindow, cx| bootstrap::show_main_window(cx));
    let _app = cx.on_action(|_: &OpenHelp, cx| cx.open_url(bootstrap::HELP_URL));
}

/// (Re)install the native menus. Re-invoked when full-screen state changes so
/// the View item reads Enter or Exit Full Screen as HIG requires.
pub(super) fn set_native_menus(cx: &mut App, full_screen: bool) {
    cx.set_menus([
        Menu::new(app::NAME).items([
            MenuItem::action(rust_i18n::t!("app.menus.about").to_string(), AboutApplication),
            MenuItem::separator(),
            MenuItem::action(rust_i18n::t!("app.menus.settings_menu").to_string(), OpenSettings),
            MenuItem::separator(),
            MenuItem::os_submenu(rust_i18n::t!("app.menus.services").to_string(), SystemMenuType::Services),
            MenuItem::separator(),
            MenuItem::action(rust_i18n::t!("app.menus.hide_app").to_string(), HideApplication),
            MenuItem::action(rust_i18n::t!("app.menus.hide_others").to_string(), HideOtherApplications),
            MenuItem::action(rust_i18n::t!("app.menus.show_all").to_string(), ShowAllApplications),
            MenuItem::separator(),
            MenuItem::action(rust_i18n::t!("app.menus.quit_app").to_string(), QuitApplication),
        ]),
        Menu::new(rust_i18n::t!("app.menus.file_menu").to_string()).items([
            MenuItem::action(rust_i18n::t!("app.menus.new_session").to_string(), NewSession),
            MenuItem::action(rust_i18n::t!("app.menus.open_workspace").to_string(), ChooseWorkspace),
            MenuItem::action(rust_i18n::t!("app.menus.close_session").to_string(), CloseSession),
            MenuItem::action(rust_i18n::t!("app.menus.choose_model").to_string(), OpenModelPicker),
        ]),
        Menu::new(rust_i18n::t!("app.menus.edit_menu").to_string()).items([
            MenuItem::os_action(rust_i18n::t!("app.menus.undo").to_string(), Undo, OsAction::Undo),
            MenuItem::os_action(rust_i18n::t!("app.menus.redo").to_string(), Redo, OsAction::Redo),
            MenuItem::separator(),
            MenuItem::os_action(rust_i18n::t!("app.menus.cut").to_string(), Cut, OsAction::Cut),
            MenuItem::os_action(rust_i18n::t!("app.menus.copy").to_string(), Copy, OsAction::Copy),
            MenuItem::os_action(rust_i18n::t!("app.menus.paste").to_string(), Paste, OsAction::Paste),
            MenuItem::os_action(rust_i18n::t!("app.menus.select_all").to_string(), SelectAll, OsAction::SelectAll),
        ]),
        Menu::new(rust_i18n::t!("app.menus.view_menu").to_string()).items([
            MenuItem::action(rust_i18n::t!("app.menus.command_palette").to_string(), OpenCommandPalette),
            MenuItem::action(rust_i18n::t!("app.menus.find_conversation").to_string(), OpenConversationSearch),
            MenuItem::action(rust_i18n::t!("app.menus.focus_composer").to_string(), FocusComposer),
            MenuItem::action(rust_i18n::t!("app.menus.toggle_inspector").to_string(), ToggleInspector),
            MenuItem::action(rust_i18n::t!("app.menus.toggle_sidebar").to_string(), ToggleSidebar),
            MenuItem::action(rust_i18n::t!("app.menus.toggle_zen").to_string(), ToggleZenMode),
            MenuItem::separator(),
            MenuItem::action(rust_i18n::t!("app.menus.cancel_run").to_string(), CancelRun),
            MenuItem::separator(),
            // macOS convention: the full-screen toggle closes the View menu;
            // ⌃⌘F is the system-standard shortcut (HIG: full screen offers a
            // distraction-free context). The label tracks the current state.
            MenuItem::action(
                if full_screen {
                    rust_i18n::t!("app.menus.exit_full_screen").to_string()
                } else {
                    rust_i18n::t!("app.menus.enter_full_screen").to_string()
                },
                ToggleFullScreen,
            ),
        ]),
        Menu::new(rust_i18n::t!("app.menus.window_menu").to_string()).items([
            MenuItem::action(rust_i18n::t!("app.menus.show_app").to_string(), ShowMainWindow),
            MenuItem::action(rust_i18n::t!("app.menus.minimize").to_string(), MinimizeWindow),
            MenuItem::action(rust_i18n::t!("app.menus.zoom").to_string(), ZoomWindow),
            MenuItem::action(rust_i18n::t!("app.menus.close_window").to_string(), CloseWindow),
        ]),
        Menu::new(rust_i18n::t!("app.menus.help_menu").to_string()).items([MenuItem::action(
            rust_i18n::t!("app.menus.help_website").to_string(),
            OpenHelp,
        )]),
    ]);
}

#[cfg(test)]
mod tests {
    use super::{MenuBarAction, STATUS_ICON_PNG, menu_groups, menu_items};
    use crate::desktop::localization::UiLanguage;

    #[test]
    fn status_menu_groups_window_workspace_app_and_lifecycle() {
        use MenuBarAction::{About, NewSession, OpenHelp, OpenSettings, OpenWorkspace, Quit, Show};
        assert_eq!(
            menu_groups(),
            &[
                &[Show][..],
                &[NewSession, OpenWorkspace][..],
                &[OpenSettings, OpenHelp, About][..],
                &[Quit][..]
            ]
        );
        assert_eq!(menu_items(), MenuBarAction::ALL);
    }

    #[test]
    fn status_menu_tags_are_stable_flat_positions() {
        let flat = menu_items();
        assert_eq!(flat.len(), MenuBarAction::ALL.len());
        for (action, tag) in flat.iter().zip(0..) {
            assert_eq!(action.tag(), tag, "{action:?} must keep its flat position as tag");
            assert_eq!(MenuBarAction::from_tag(tag), Some(*action));
        }
        assert_eq!(MenuBarAction::from_tag(-1), None);
        assert_eq!(MenuBarAction::from_tag(flat.len().cast_signed()), None);
    }

    #[test]
    fn status_menu_titles_are_localized_for_every_language() {
        // Legitimate cognate: Dutch keeps "Help" like the system Help menu (mirrors the app.menus whitelist).
        const COGNATES: [(&str, MenuBarAction); 1] = [("nl", MenuBarAction::OpenHelp)];
        for language in UiLanguage::ALL {
            let locale = language.locale();
            for action in MenuBarAction::ALL {
                let text = action.title_in_locale(locale);
                assert!(!text.is_empty(), "{locale}/{action:?}");
                if language != UiLanguage::English && !COGNATES.contains(&(locale, action)) {
                    assert_ne!(text, action.title_in_locale("en"), "{locale}/{action:?}");
                }
            }
        }
    }

    #[test]
    fn mono_status_icon_is_a_centered_white_glyph() {
        assert_eq!(&STATUS_ICON_PNG[..8], b"\x89PNG\r\n\x1a\n", "status icon must be a PNG");
        let art = image::load_from_memory(STATUS_ICON_PNG)
            .expect("status icon must decode")
            .to_rgba8();
        assert_eq!((art.width(), art.height()), (120, 120));
        // Template images ignore color; only alpha carries the glyph, and the
        // source art keeps every visible pixel pure white.
        for pixel in art.pixels() {
            if pixel[3] > 128 {
                assert_eq!(pixel.0[..3], [255, 255, 255], "visible pixels must stay pure white");
            }
        }
        let opaque = art.pixels().filter(|pixel| pixel[3] >= 248).count();
        let fraction = opaque as f64 / f64::from(art.width() * art.height());
        assert!(
            (0.02..=0.10).contains(&fraction),
            "status opaque fraction {fraction:.3} outside the 0.02-0.10 glyph band"
        );
        let (left, top, right, bottom) = art
            .enumerate_pixels()
            .filter(|(_, _, pixel)| pixel[3] > 128)
            .fold((120, 120, 0, 0), |(left, top, right, bottom), (x, y, _)| {
                (left.min(x), top.min(y), right.max(x), bottom.max(y))
            });
        let center = (f64::from(left + right) / 2., f64::from(top + bottom) / 2.);
        assert!(
            (center.0 - 60.).abs() <= 8. && (center.1 - 60.).abs() <= 8.,
            "glyph center {center:?} must sit near the tile center"
        );
        for (x, y) in [(0, 0), (119, 0), (0, 119), (119, 119), (60, 0), (0, 60)] {
            assert!(art.get_pixel(x, y)[3] < 16, "margin ({x}, {y}) must be transparent");
        }
    }
}
