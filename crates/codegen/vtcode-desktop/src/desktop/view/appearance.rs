//! Theme, typography, reduced-motion, and status-bar presentation.

use super::settings::switch_action;
use super::*;

/// Cloned render inputs for the appearance tab (see
/// `NotificationSettingsProps` for the Kit-hosting rationale).
#[derive(Clone)]
pub(super) struct AppearanceSettingsProps {
    pub(super) separate_theme_profiles: bool,
    pub(super) theme_mode: ThemeMode,
    pub(super) theme_id: String,
    pub(super) light_theme: Option<String>,
    pub(super) dark_theme: Option<String>,
    pub(super) theme_scroll_light: ScrollHandle,
    pub(super) theme_scroll_dark: ScrollHandle,
    pub(super) ui_font_size: u8,
    pub(super) code_font_size: u8,
    pub(super) ui_language_select: Entity<UiLanguageSelectState>,
    pub(super) navigation_visible: bool,
    pub(super) navigation_on_right: bool,
    pub(super) translucent_sidebar: bool,
    pub(super) file_destination_select: Entity<preferences::FileDestinationSelectState>,
    pub(super) diff: settings_diff::DiffSettingsProps,
    pub(super) app_icon: app_icon_settings::AppIconSettingsProps,
    pub(super) custom: appearance_themes::CustomThemesProps,
}

impl AppearanceSettingsProps {
    /// Snapshot the view fields the appearance builders read. Both the
    /// legacy shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        let typography = view.preferences.typography_for(view.theme_mode);
        Self {
            separate_theme_profiles: view.preferences.separate_theme_profiles,
            theme_mode: view.theme_mode,
            theme_id: view.theme_id.clone(),
            light_theme: view.preferences.light_theme.clone(),
            dark_theme: view.preferences.dark_theme.clone(),
            theme_scroll_light: view.theme_scroll_light.clone(),
            theme_scroll_dark: view.theme_scroll_dark.clone(),
            ui_font_size: typography.ui_font_size,
            code_font_size: typography.code_font_size,
            ui_language_select: view.ui_language_select.clone(),
            navigation_visible: view.preferences.navigation_visible(),
            navigation_on_right: view.preferences.navigation_on_right(),
            translucent_sidebar: view.preferences.translucent_sidebar,
            file_destination_select: view.file_destination_select.clone(),
            diff: settings_diff::DiffSettingsProps::capture(view),
            app_icon: app_icon_settings::AppIconSettingsProps::capture(view),
            custom: appearance_themes::CustomThemesProps::capture(view),
        }
    }
}

impl DesktopView {
    pub(super) fn set_contrast(&mut self, value: f32, window: &mut Window, cx: &mut Context<Self>) {
        if !value.is_finite() {
            return;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "finite value is rounded and clamped to 0..=100"
        )]
        let contrast = value.round().clamp(0.0, 100.0) as u8;
        if self.preferences.contrast() == contrast {
            return;
        }
        self.preferences.contrast = contrast;
        self.theme_mode = themes::apply_selection_with_contrast(
            &self.theme_id,
            contrast,
            &self.preferences.custom_themes,
            Some(window),
            cx,
        );
        self.apply_typography(cx);
        self.transcript.remeasure();
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn reset_contrast(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_contrast(0.0, window, cx);
        self.contrast_slider.update(cx, |slider, cx| slider.set_value(0.0, window, cx));
    }

    pub(super) fn render_translucent_sidebar_switch_content(
        props: &AppearanceSettingsProps,
        view: Entity<Self>,
        _cx: &mut App,
    ) -> AnyElement {
        gpui_kit::component::switch::Switch::new("translucent-sidebar")
            .label(rust_i18n::t!("app.settings.appearance.translucent_sidebar").to_string())
            .checked(props.translucent_sidebar)
            .on_click({
                let view = view.clone();
                move |checked: &bool, window: &mut Window, cx: &mut App| {
                    view.update(cx, |view, cx| {
                        view.set_translucent_sidebar(*checked, window, cx);
                    });
                }
            })
            .into_any_element()
    }

    pub(super) fn set_translucent_sidebar(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.preferences.translucent_sidebar == enabled {
            return;
        }
        self.preferences.translucent_sidebar = enabled;
        Self::apply_sidebar_background(enabled, window, cx);
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn apply_sidebar_background(enabled: bool, window: &mut Window, cx: &mut App) {
        use gpui_kit::Refineable as _;
        window.set_background_appearance(if enabled {
            gpui_kit::WindowBackgroundAppearance::Blurred
        } else {
            gpui_kit::WindowBackgroundAppearance::Opaque
        });
        if let Some(root) = window.root::<gpui_kit::base::Root>().flatten() {
            root.update(cx, |root, cx| {
                if enabled {
                    root.style()
                        .refine(&gpui_kit::StyleRefinement::default().bg(gpui_kit::transparent_black()));
                } else {
                    root.style().background = None;
                }
                cx.notify();
            });
        }
    }

    /// Full-width bottom bar: workspace, git branch, provider · model, and
    /// the live turn status.
    pub(super) fn toggle_status_bar(&mut self, cx: &mut Context<Self>) {
        self.preferences.show_status_bar = !self.preferences.show_status_bar;
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn set_reduced_motion(
        &mut self,
        preference: preferences::ReducedMotionPreference,
        cx: &mut Context<Self>,
    ) {
        if self.preferences.reduced_motion == preference && preference != preferences::ReducedMotionPreference::System {
            return;
        }
        self.preferences.reduced_motion = preference;
        preferences::apply_reduced_motion(preference, self.system_motion_baseline, cx);
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let workspace = self
            .workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("No workspace");
        let agent = if self.running {
            ("Agent: Running", colors.primary)
        } else {
            ("Agent: Idle", colors.muted_foreground)
        };
        let configuration = self.configuration.as_ref();
        let provider_model = configuration
            .map(|config| format!("{} · {}", config.provider, config.model))
            .unwrap_or_else(|| "No provider configured".to_string());
        let sandbox = configuration.and_then(|config| config.sandbox.clone());
        let context_percent = configuration
            .and_then(|config| config.context_window)
            .zip(self.last_usage.as_ref())
            .and_then(|(window, usage)| context_format::context_usage_percent(usage, window));
        let workspace_details = div()
            .flex()
            .items_center()
            .gap_3()
            .min_w_0()
            .child(div().truncate().child(workspace.to_string()))
            .when(self.git_branch.is_some(), |bar| {
                bar.child(
                    div().truncate().child(
                        rust_i18n::t!(
                            "app.appearance.branch_label",
                            value = self.git_branch.clone().unwrap_or_default()
                        )
                        .to_string(),
                    ),
                )
            })
            .when(sandbox.is_some(), |bar| {
                bar.child(div().truncate().child(
                    rust_i18n::t!("app.appearance.sandbox_label", value = sandbox.unwrap_or_default()).to_string(),
                ))
            });
        let runtime_details = div()
            .flex()
            .items_center()
            .gap_3()
            .min_w_0()
            .when(context_percent.is_some(), |bar| {
                bar.child(
                    div().id("context-usage").test_support().child(
                        rust_i18n::t!("app.appearance.context_label", percent = context_percent.unwrap_or_default())
                            .to_string(),
                    ),
                )
            })
            .child(div().truncate().child(provider_model))
            .child(
                div()
                    .id("agent-status")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .aria_label(agent.0)
                    .child(
                        Marker::new()
                            .loading(self.running)
                            .text_xs()
                            .text_color(agent.1)
                            .w_auto()
                            .content(MarkerContent::new().text(agent.0)),
                    ),
            );

        div()
            .id("status-bar")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.settings.appearance.status_bar").to_string())
            .w_full()
            .flex_shrink_0()
            .child(
                StatusBar::new()
                    .left(workspace_details)
                    .right(runtime_details)
                    .w_full()
                    .h(px(26.))
                    .gap_3()
                    .px_3()
                    .py_0()
                    .bg(colors.background)
                    .text_xs()
                    .text_color(colors.muted_foreground),
            )
            .with_animation("status-bar-fade", zen_fade_animation(cx.theme()), |el, delta| el.opacity(delta))
    }

    pub(super) fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let target_mode = if self.theme_mode == ThemeMode::Light {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        self.apply_theme_mode(target_mode, window, cx);
    }

    pub(super) fn apply_theme_mode(&mut self, target_mode: ThemeMode, window: &mut Window, cx: &mut Context<Self>) {
        let id = if self.preferences.separate_theme_profiles {
            let candidate = match target_mode {
                ThemeMode::Light => self.preferences.light_theme.as_deref(),
                ThemeMode::Dark => self.preferences.dark_theme.as_deref(),
            };
            candidate
                .filter(|id| themes::mode_for_selection(id, &self.preferences.custom_themes) == Some(target_mode))
                .unwrap_or_else(|| themes::default_id(target_mode))
        } else {
            themes::system_id(target_mode)
        };
        self.apply_theme(id.to_owned(), window, cx);
    }

    pub(super) fn toggle_separate_theme_profiles(&mut self, cx: &mut Context<Self>) {
        let enabled = !self.preferences.separate_theme_profiles;
        if enabled {
            let current_theme = self.theme_id.clone();
            self.preferences.light_theme = Some(if self.theme_mode == ThemeMode::Light {
                current_theme.clone()
            } else {
                themes::default_id(ThemeMode::Light).to_string()
            });
            self.preferences.dark_theme = Some(if self.theme_mode == ThemeMode::Dark {
                current_theme
            } else {
                themes::default_id(ThemeMode::Dark).to_string()
            });
            self.preferences.follow_cli_theme = Some(false);
        }
        self.preferences.set_separate_theme_profiles(enabled, self.theme_mode);
        self.save_preferences();
        cx.notify();
    }

    /// Apply a theme from the picker and remember the choice.
    pub(super) fn apply_theme(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.preferences.follow_cli_theme = Some(false);
        self.apply_theme_state(id, Some(window), cx);
    }

    /// Restore the saved selection at startup. The current id is the fallback
    /// when only the contrast setting changed; custom ids resolve through the
    /// same path as palette rows. Loading never re-saves, so this stops short
    /// of [`Self::apply_theme_state`].
    pub(super) fn restore_saved_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferences = self.preferences.clone();
        if preferences.theme.is_none() && preferences.contrast() == 0 {
            return;
        }
        let id = preferences.theme.clone().unwrap_or_else(|| self.theme_id.clone());
        let mode = themes::apply_selection_with_contrast(
            &id,
            preferences.contrast(),
            &preferences.custom_themes,
            Some(window),
            cx,
        );
        self.theme_mode = mode;
        // A deleted custom theme (or a stale preference) must not survive as a
        // dangling id: resolve to the applied mode's default exactly as a live
        // selection does.
        self.theme_id = themes::canonical_selection(&id, mode, &preferences.custom_themes);
    }

    pub(super) fn apply_theme_state(&mut self, id: String, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let mode = themes::apply_selection_with_contrast(
            &id,
            self.preferences.contrast(),
            &self.preferences.custom_themes,
            window,
            cx,
        );
        let mode_changed = mode != self.theme_mode;
        self.theme_mode = mode;
        let id = themes::canonical_selection(&id, mode, &self.preferences.custom_themes);
        if self.preferences.separate_theme_profiles {
            match self.theme_mode {
                ThemeMode::Light => self.preferences.light_theme = Some(id.clone()),
                ThemeMode::Dark => self.preferences.dark_theme = Some(id.clone()),
            }
        }
        self.theme_id = id;
        if mode_changed
            && matches!(self.preferences.app_icon(), AppIconPreference::Dynamic | AppIconPreference::Unknown)
        {
            self.refresh_dock_icon();
        }
        self.apply_typography(cx);
        self.transcript.remeasure();
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn apply_typography(&mut self, cx: &mut Context<Self>) {
        let typography = self.preferences.typography_for(self.theme_mode);
        let ui = typography.ui_font_size;
        let code = typography.code_font_size;
        let installed = typography::installed_fonts(cx);
        let ui_family = typography::resolve_font(typography.ui_font_family, ".SystemUIFont", &installed);
        let code_family =
            typography::resolve_font(typography.code_font_family, self.default_code_font.as_str(), &installed);
        Theme::update(cx, |theme| {
            theme.font_family = ui_family.into();
            theme.mono_font_family = code_family.into();
            theme.font_size = px(f32::from(ui));
            theme.mono_font_size = px(f32::from(code));
        });
        self.transcript.remeasure();
        cx.notify();
    }

    pub(super) fn render_reduced_motion_content(
        current: preferences::ReducedMotionPreference,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        div()
            .id("reduced-motion-setting")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.settings.appearance.reduce_motion").to_string())
            .w_full()
            .child(settings_row(
                rust_i18n::t!("app.settings.appearance.reduce_motion").to_string(),
                rust_i18n::t!("app.settings.appearance.reduce_motion_description").to_string(),
                div()
                    .flex()
                    .gap_1()
                    .children(preferences::ReducedMotionPreference::ALL.into_iter().map(|preference| {
                        let label = preference.label();
                        let selected = preference == current;
                        desktop_button(format!("reduce-motion-{}", preference.id()))
                            .label(label.clone())
                            .accessibility_label(
                                rust_i18n::t!(
                                    "app.appearance.motion_aria",
                                    label = label.as_str(),
                                    suffix = if selected {
                                        rust_i18n::t!("app.appearance.selected_suffix").to_string()
                                    } else {
                                        String::new()
                                    }
                                )
                                .to_string(),
                            )
                            .tooltip(match preference {
                                preferences::ReducedMotionPreference::System => {
                                    rust_i18n::t!("app.appearance.reread_system").to_string()
                                }
                                preferences::ReducedMotionPreference::On => {
                                    rust_i18n::t!("app.appearance.reduce_animations").to_string()
                                }
                                preferences::ReducedMotionPreference::Off => {
                                    rust_i18n::t!("app.appearance.allow_animations").to_string()
                                }
                            })
                            .ghost()
                            .toggled(selected)
                            .when(selected, |button| button.bg(colors.popover).text_color(colors.foreground))
                            .on_click({
                                let view = view.clone();
                                move |_, _: &mut Window, cx: &mut App| {
                                    view.update(cx, |view, cx| view.set_reduced_motion(preference, cx));
                                }
                            })
                    })),
                &colors,
            ))
            .into_any_element()
    }

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
                    desktop_button(format!("font-{key}-{index}"))
                        .label(if delta < 0 { "−" } else { "+" })
                        .accessibility_label(format!(
                            "{} {label}",
                            if delta < 0 {
                                rust_i18n::t!("app.appearance.decrease")
                            } else {
                                rust_i18n::t!("app.appearance.increase")
                            }
                        ))
                        .ghost()
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

    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure.
    pub(super) fn render_appearance_settings_content(
        props: AppearanceSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        // Count every swatch in both modes so labels never rename when the
        // visible grid changes.
        let mut counts = HashMap::new();
        for entry in themes::picker_swatches() {
            *counts.entry(entry.label()).or_insert(0_usize) += 1;
        }
        // With separate profiles only the editing profile's grid renders, so
        // the picker never shows the mode the user is not editing. Without
        // separate profiles both grids stay visible: there is no mode
        // selector, so an opposite-mode click is the mode switch.
        // Build the swatch groups before borrowing the theme colors: the
        // groups need `cx` mutably while the colors borrow it immutably.
        let separate = props.separate_theme_profiles;
        let editing = props.theme_mode;
        let show_light = !separate || editing == ThemeMode::Light;
        let show_dark = !separate || editing == ThemeMode::Dark;
        let light_group =
            show_light.then(|| Self::render_theme_group_content(ThemeMode::Light, &counts, &props, view.clone(), cx));
        let dark_group =
            show_dark.then(|| Self::render_theme_group_content(ThemeMode::Dark, &counts, &props, view.clone(), cx));
        // Own the theme: `cx.theme()` borrows `cx`, but the content
        // builders below take `cx` mutably in later chain links.
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let active_theme_profile = if props.theme_mode == ThemeMode::Light {
            rust_i18n::t!("app.settings.appearance.light").to_string()
        } else {
            rust_i18n::t!("app.settings.appearance.dark").to_string()
        };
        let visual_style = rust_i18n::t!("app.settings.visual_style").to_string();
        let typography = rust_i18n::t!("app.settings.typography").to_string();
        let layout = rust_i18n::t!("app.settings.layout").to_string();
        let mut content = div()
            .id("appearance-settings")
            .test_support()
            .flex()
            .flex_col()
            .gap_6()
            .child(accessible_group_box(
                "appearance-visual-style-group",
                visual_style.clone(),
                settings_group_box(visual_style, colors)
                    .child(
                        gpui_kit::component::switch::Switch::new("separate-theme-profiles")
                            .label(rust_i18n::t!("app.settings.appearance.separate_modes").to_string())
                            .checked(props.separate_theme_profiles)
                            .on_click({
                                let view = view.clone();
                                move |_, _: &mut Window, cx: &mut App| {
                                    view.update(cx, |view, cx| view.toggle_separate_theme_profiles(cx));
                                }
                            }),
                    )
                    .when(props.separate_theme_profiles, |group| {
                        group.child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .id("theme-profile-hint")
                                        .role(gpui_kit::Role::Label)
                                        .aria_label(
                                            rust_i18n::t!(
                                                "app.settings.appearance.editing_profile",
                                                profile = active_theme_profile
                                            )
                                            .to_string(),
                                        )
                                        .text_sm()
                                        .child(
                                            rust_i18n::t!("app.settings.appearance.editing_profile_short").to_string(),
                                        ),
                                )
                                .children([ThemeMode::Light, ThemeMode::Dark].into_iter().map(|mode| {
                                    let light = mode == ThemeMode::Light;
                                    let id = if light { "light" } else { "dark" };
                                    let label = if light {
                                        rust_i18n::t!("app.settings.appearance.light").to_string()
                                    } else {
                                        rust_i18n::t!("app.settings.appearance.dark").to_string()
                                    };
                                    let selected = props.theme_mode == mode;
                                    let profile_action_key = if selected {
                                        "app.settings.appearance.edit_profile_selected"
                                    } else {
                                        "app.settings.appearance.edit_profile"
                                    };
                                    desktop_button(format!("edit-{id}-profile"))
                                        .label(label.clone())
                                        .accessibility_label(
                                            rust_i18n::t!(profile_action_key, profile = label).to_string(),
                                        )
                                        .desktop_selected(selected, colors)
                                        .on_click({
                                            let view = view.clone();
                                            move |_, window: &mut Window, cx: &mut App| {
                                                view.update(cx, |view, cx| {
                                                    if view.theme_mode != mode {
                                                        view.apply_theme_mode(mode, window, cx);
                                                    }
                                                });
                                            }
                                        })
                                })),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(section_label(&rust_i18n::t!("app.settings.appearance.themes").to_string(), colors))
                            .child(div().flex().gap_1().children([false, true].into_iter().map(|forward| {
                                let light = props.theme_scroll_light.clone();
                                let dark = props.theme_scroll_dark.clone();
                                // Owned copies: the click listener below is
                                // 'static and cannot borrow this render frame.
                                // Both arrays stay `Copy` so the `Fn` listener
                                // can reuse them on every click.
                                let visible = [show_light, show_dark];
                                desktop_button(if forward { "next-themes" } else { "previous-themes" })
                                    .icon(if forward {
                                        IconName::ChevronRight
                                    } else {
                                        IconName::ChevronLeft
                                    })
                                    .accessibility_label(
                                        rust_i18n::t!(if forward {
                                            "app.settings.appearance.next_themes"
                                        } else {
                                            "app.settings.appearance.previous_themes"
                                        })
                                        .to_string(),
                                    )
                                    .ghost()
                                    .small()
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _: &mut Window, cx: &mut App| {
                                            for (shown, scroll) in visible.into_iter().zip([&light, &dark]) {
                                                if !shown {
                                                    continue;
                                                }
                                                let mut offset = scroll.offset();
                                                offset.x += px(if forward { -336. } else { 336. });
                                                scroll.set_offset(offset);
                                            }
                                            cx.notify(view.entity_id());
                                        }
                                    })
                            }))),
                    )
                    .child(
                        div()
                            .id("theme-list")
                            .test_support()
                            .role(gpui_kit::Role::Group)
                            .aria_label(rust_i18n::t!("app.settings.appearance.visual_themes").to_string())
                            .w_full()
                            .min_w_0()
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .when_some(light_group, |list, group| list.child(group))
                            .when_some(dark_group, |list, group| list.child(group)),
                    )
                    .child(Self::render_custom_themes_group_content(&props.custom, view.clone(), cx)),
            ))
            .child(accessible_group_box(
                "appearance-typography-group",
                typography.clone(),
                settings_group_box(typography, colors)
                    .child(Self::render_font_family_content(FontTarget::Ui, view.clone(), cx))
                    .child(Self::render_font_family_content(FontTarget::Code, view.clone(), cx))
                    .child(Self::render_font_size_content(false, &props, view.clone(), cx))
                    .child(Self::render_font_size_content(true, &props, view.clone(), cx))
                    .child(
                        div()
                            .id("appearance-code-preview")
                            .test_support()
                            .w_full()
                            .child(render_markdown(
                                "appearance-code-preview-text".into(),
                                "```rust\nfn main() { /* VT Code */ }\n```".into(),
                                false,
                                cx,
                            )),
                    ),
            ));

        let language_group_title = rust_i18n::t!("app.settings.language.group").to_string();
        content = content.child(accessible_group_box(
            "appearance-language-group",
            language_group_title.clone(),
            settings_group_box(language_group_title, colors).child(settings_row(
                rust_i18n::t!("app.settings.language.label").to_string(),
                rust_i18n::t!("app.settings.language.description").to_string(),
                Select::new(&props.ui_language_select)
                    .id("ui-language-select")
                    .accessibility_label(rust_i18n::t!("app.settings.language.label").to_string())
                    .search_placeholder(rust_i18n::t!("app.settings.language.search").to_string())
                    .menu_max_h(px(320.))
                    .w(px(240.)),
                colors,
            )),
        ));

        content = content.child(accessible_group_box(
            "appearance-layout-group",
            layout.clone(),
            settings_group_box(layout, colors)
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            gpui_kit::component::switch::Switch::new("navigation-visible")
                                .label(rust_i18n::t!("app.settings.appearance.show_navigation").to_string())
                                .checked(props.navigation_visible)
                                .on_click(switch_action(&view, DesktopView::set_navigation_visible)),
                        )
                        .child(row_description(
                            rust_i18n::t!("app.settings.appearance.navigation_hint").to_string(),
                            colors,
                        )),
                )
                .child(Self::render_translucent_sidebar_switch_content(&props, view.clone(), cx))
                .child(
                    gpui_kit::component::switch::Switch::new("navigation-on-right")
                        .label(rust_i18n::t!("app.settings.appearance.navigation_right").to_string())
                        .checked(props.navigation_on_right)
                        .on_click(switch_action(&view, DesktopView::set_navigation_on_right)),
                ),
        ));

        content = content.child(
            desktop_default_button("reset-appearance")
                .label(rust_i18n::t!("app.settings.appearance.reset").to_string())
                .on_click({
                    let view = view.clone();
                    move |_, window: &mut Window, cx: &mut App| {
                        view.update(cx, |view, cx| {
                            view.preferences.separate_theme_profiles = false;
                            view.preferences.light_theme = None;
                            view.preferences.dark_theme = None;
                            view.preferences.light_typography = None;
                            view.preferences.dark_typography = None;
                            view.preferences.ui_font_family = None;
                            view.preferences.code_font_family = None;
                            view.preferences.ui_font_size = None;
                            view.preferences.code_font_size = None;
                            view.preferences.app_icon = None;
                            view.preferences.reduced_motion = preferences::ReducedMotionPreference::System;
                            view.set_translucent_sidebar(false, window, cx);
                            view.reset_contrast(window, cx);
                            preferences::apply_reduced_motion(
                                view.preferences.reduced_motion,
                                view.system_motion_baseline,
                                cx,
                            );
                            view.apply_theme(themes::DEFAULT_THEME_ID.to_string(), window, cx);
                            view.refresh_dock_icon();
                        });
                    }
                }),
        );
        let files_group_title = rust_i18n::t!("app.settings.files.group").to_string();
        content = content.child(accessible_group_box(
            "appearance-files-group",
            files_group_title.clone(),
            settings_group_box(files_group_title, colors).child(settings_row(
                rust_i18n::t!("app.settings.files.destination.label").to_string(),
                rust_i18n::t!("app.settings.files.destination.description").to_string(),
                Select::new(&props.file_destination_select)
                    .id("file-open-destination-select")
                    .accessibility_label(rust_i18n::t!("app.settings.files.destination.label").to_string())
                    .menu_max_h(px(320.))
                    .w(px(240.)),
                colors,
            )),
        ));
        content = content.child(Self::render_diff_settings_content(props.diff.clone(), view.clone(), cx));
        content = content.child(Self::render_app_icon_settings_content(props.app_icon.clone(), view.clone(), cx));
        content.into_any_element()
    }

    /// Legacy Context shell: captures props and delegates to the
    /// App-context content builder.
    pub(super) fn render_appearance_settings(&self, cx: &mut Context<Self>) -> AnyElement {
        let props = AppearanceSettingsProps::capture(self);
        let view = cx.entity();
        Self::render_appearance_settings_content(props, view, cx)
    }
}
