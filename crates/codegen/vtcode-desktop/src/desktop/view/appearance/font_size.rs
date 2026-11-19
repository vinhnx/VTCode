//! Font-size settings row extracted from the appearance tab.

use super::*;

impl DesktopView {
    pub(super) fn render_font_size_content(
        code: bool,
        props: &AppearanceSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let (key, label, description, size, minimum, maximum) = if code {
            (
                "code",
                rust_i18n::t!("app.appearance.code_font_size").to_string(),
                rust_i18n::t!("app.appearance.code_font_desc").to_string(),
                props.code_font_size,
                preferences::MIN_CODE_FONT_SIZE,
                preferences::MAX_CODE_FONT_SIZE,
            )
        } else {
            (
                "ui",
                rust_i18n::t!("app.appearance.ui_font_size").to_string(),
                rust_i18n::t!("app.appearance.ui_font_desc").to_string(),
                props.ui_font_size,
                preferences::MIN_UI_FONT_SIZE,
                preferences::MAX_UI_FONT_SIZE,
            )
        };
        settings_row(
            label.clone(),
            description,
            div()
                .flex()
                .items_center()
                .gap_2()
                .children([-1i16, 1].into_iter().enumerate().map(|(index, delta)| {
                    desktop_ghost_button(format!("font-{key}-{index}"))
                        .label(if delta < 0 { "−" } else { "+" })
                        .accessibility_label(format!(
                            "{} {label}",
                            if delta < 0 {
                                rust_i18n::t!("app.appearance.decrease")
                            } else {
                                rust_i18n::t!("app.appearance.increase")
                            }
                        ))
                        .desktop_disabled(if delta < 0 { size <= minimum } else { size >= maximum })
                        .on_click({
                            let view = view.clone();
                            move |_, _: &mut Window, cx: &mut App| {
                                view.update(cx, |view, cx| {
                                    let current = if code {
                                        view.preferences.typography_for(view.theme_mode).code_font_size
                                    } else {
                                        view.preferences.typography_for(view.theme_mode).ui_font_size
                                    };
                                    let next = if delta < 0 {
                                        current.saturating_sub(1)
                                    } else {
                                        current.saturating_add(1)
                                    }
                                    .clamp(minimum, maximum);
                                    if code {
                                        view.preferences.set_code_font_size(view.theme_mode, next);
                                    } else {
                                        view.preferences.set_ui_font_size(view.theme_mode, next);
                                    }
                                    view.apply_typography(cx);
                                    view.save_preferences();
                                });
                            }
                        })
                }))
                .child(
                    div()
                        .id(format!("font-{key}-value"))
                        .role(gpui_kit::Role::Label)
                        .aria_label(
                            rust_i18n::t!("app.appearance.size_pixels", label = label.as_str(), size = size)
                                .to_string(),
                        )
                        .test_support()
                        .text_sm()
                        .child(format!("{size} px")),
                ),
            &cx.theme().colors,
        )
        .into_any_element()
    }
}
