//! Application-icon picker: Appearance group, tiles, and Dock application.

use super::*;

/// Cloned render inputs for the app-icon picker (see
/// `NotificationSettingsProps` for the Kit-hosting rationale).
#[derive(Clone)]
pub(super) struct AppIconSettingsProps {
    pub(super) current: AppIconPreference,
}

impl AppIconSettingsProps {
    /// Snapshot the view fields the app-icon builder reads. Both the
    /// legacy shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self { current: view.preferences.app_icon() }
    }
}

impl DesktopView {
    pub(super) fn set_app_icon(&mut self, preference: AppIconPreference, cx: &mut Context<Self>) {
        if self.preferences.app_icon() == preference {
            return;
        }
        self.preferences.set_app_icon(preference);
        self.refresh_dock_icon();
        self.save_preferences();
        cx.notify();
    }

    /// Re-apply the saved icon selection to the Dock. AppKit failures stay
    /// silent: the bundle icon remains and the preference still applies on
    /// future launches and theme-mode changes. On Tahoe+ `Dynamic` clears
    /// any prior override so the system-composed bundle icon returns.
    pub(super) fn refresh_dock_icon(&self) {
        let _applied = app_icon::apply_dock_preference(self.preferences.app_icon(), self.theme_mode);
    }

    fn render_app_icon_tile_content(
        preference: AppIconPreference,
        active: bool,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let label = match preference {
            AppIconPreference::Dynamic | AppIconPreference::Unknown => {
                rust_i18n::t!("app.settings.appearance.app_icon_dynamic").to_string()
            }
            AppIconPreference::Light => rust_i18n::t!("app.settings.appearance.light").to_string(),
            AppIconPreference::Dark => rust_i18n::t!("app.settings.appearance.dark").to_string(),
            AppIconPreference::Clear => rust_i18n::t!("app.settings.appearance.app_icon_clear").to_string(),
            AppIconPreference::Tinted => rust_i18n::t!("app.settings.appearance.app_icon_tinted").to_string(),
        };
        let accessibility_label = if active {
            format!("{label}{}", rust_i18n::t!("app.appearance.selected_suffix"))
        } else {
            label.clone()
        };
        desktop_button(format!("app-icon-{}", preference.id()))
            .accessibility_label(accessibility_label)
            .toggled(active)
            .ghost()
            .w(px(104.))
            .flex_shrink_0()
            .h(px(112.))
            .p_2()
            .when(active, |button| button.bg(cx.theme().colors.popover).text_color(cx.theme().colors.foreground))
            .rounded_lg()
            .on_click({
                let view = view.clone();
                move |_, _: &mut Window, cx: &mut App| {
                    view.update(cx, |view, cx| {
                        view.set_app_icon(preference, cx);
                    });
                }
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .child(app_icon_preview(preference, 64.))
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .when(active, |label| label.font_semibold())
                            .child(label),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_app_icon_settings_content(
        props: AppIconSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let title = rust_i18n::t!("app.settings.appearance.app_icon").to_string();
        // Build the cx-consuming tiles before borrowing the theme colors:
        // `cx.theme()` borrows `cx`, so the borrow must start after the
        // last mutable `cx` use.
        let tiles = AppIconPreference::ALL
            .into_iter()
            .map(|preference| {
                Self::render_app_icon_tile_content(preference, props.current == preference, view.clone(), cx)
            })
            .collect::<Vec<_>>();
        let theme = cx.theme();
        let colors = &theme.colors;
        accessible_group_box(
            "appearance-app-icon-group",
            title.clone(),
            settings_group_box(title, colors)
                .child(row_description(
                    rust_i18n::t!("app.settings.appearance.app_icon_description").to_string(),
                    colors,
                ))
                .child(div().flex().flex_wrap().items_start().gap_2().children(tiles)),
        )
    }
}
