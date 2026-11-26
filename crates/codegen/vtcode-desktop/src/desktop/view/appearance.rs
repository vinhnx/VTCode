//! Theme, typography, reduced-motion, and status-bar presentation.

use super::settings::switch_action;
use super::*;
mod font_size;

/// Cloned render inputs for the appearance tab (see
/// `NotificationSettingsProps` for the Kit-hosting rationale).
#[derive(Clone)]
pub(super) struct AppearanceSettingsProps {
    pub(super) separate_theme_profiles: bool,
    pub(super) theme_mode: ThemeMode,
    pub(super) theme_scroll_light: VirtualListScrollHandle,
    pub(super) theme_scroll_dark: VirtualListScrollHandle,
    /// Child views hosting each virtualized swatch row (see `ThemeGrid`).
    pub(super) theme_grid_light: Entity<appearance_theme_picker::ThemeGrid>,
    pub(super) theme_grid_dark: Entity<appearance_theme_picker::ThemeGrid>,
    pub(super) ui_font_size: u8,
    pub(super) code_font_size: u8,
    pub(super) ui_language_select: Entity<UiLanguageSelectState>,
    pub(super) terminal_ansi_theme: preferences::TerminalAnsiTheme,
    pub(super) terminal_position_select: Entity<preferences::TerminalPositionSelectState>,
    pub(super) chat_width_select: Entity<preferences::ChatWidthSelectState>,
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
            theme_scroll_light: view.theme_scroll_light.clone(),
            theme_scroll_dark: view.theme_scroll_dark.clone(),
            theme_grid_light: view.theme_grid_light.clone(),
            theme_grid_dark: view.theme_grid_dark.clone(),
            ui_font_size: typography.ui_font_size,
            code_font_size: typography.code_font_size,
            ui_language_select: view.ui_language_select.clone(),
            terminal_ansi_theme: view.preferences.terminal_ansi_theme(),
            terminal_position_select: view.terminal_position_select.clone(),
            chat_width_select: view.chat_width_select.clone(),
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
        // Taking manual control of the slider ends the one-click overlay.
        // Its stored value is stale now; clearing it prevents a later toggle
        // from restoring a contrast the user has already replaced.
        if self.preferences.high_contrast {
            self.preferences.high_contrast = false;
            self.preferences.contrast_before_high_contrast = None;
        }
        self.preferences.contrast = contrast;
        self.theme_mode = themes::apply_selection_with_contrast(
            &self.theme_id,
            contrast,
            &self.preferences.custom_themes,
            Some(window),
            cx,
        );
        self.sync_terminal_palette(cx);
        // No `apply_typography`: fonts are contrast-independent and the palette
        // above already refreshed windows. Contrast rewrites colors, so the
        // composer tokens must be re-derived just like on a theme switch.
        self.refresh_composer_highlight(cx);
        self.transcript.remeasure();
        cx.notify();
        // No save here: this runs on every drag Change tick, and saving ends in
        // an fsync. Callers persist once on Release (bootstrap) or on discrete
        // keyboard/reset changes.
    }

    pub(super) fn reset_contrast(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.set_contrast(0.0, window, cx);
        self.contrast_slider.update(cx, |slider, cx| slider.set_value(0.0, window, cx));
        // Discrete action: persist once (the drag path above never saves).
        self.save_preferences();
    }

    /// One-click high-contrast overlay. The slider observer short-circuits on
    /// the value we just wrote, so the palette is applied exactly once here.
    pub(super) fn set_high_contrast(&mut self, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(contrast) = self.preferences.set_high_contrast(enabled) else {
            return;
        };
        self.theme_mode = themes::apply_selection_with_contrast(
            &self.theme_id,
            contrast,
            &self.preferences.custom_themes,
            Some(window),
            cx,
        );
        self.sync_terminal_palette(cx);
        self.refresh_composer_highlight(cx);
        self.transcript.remeasure();
        self.contrast_slider
            .update(cx, |slider, cx| slider.set_value(f32::from(contrast), window, cx));
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn set_terminal_position(&mut self, position: preferences::TerminalPosition, cx: &mut Context<Self>) {
        if self.preferences.terminal_position() == position {
            return;
        }
        self.preferences.set_terminal_position(position);
        self.save_preferences();
        cx.notify();
    }

    pub(super) fn set_terminal_ansi_theme(&mut self, theme: preferences::TerminalAnsiTheme, cx: &mut Context<Self>) {
        if self.preferences.terminal_ansi_theme() == theme {
            return;
        }
        self.preferences.set_terminal_ansi_theme(theme);
        self.sync_terminal_palette(cx);
        self.save_preferences();
        cx.notify();
    }

    /// One terminal scheme in the settings picker: a small preview chip
    /// (tuned ground plus the scheme's red/green/blue dots; Auto shows the
    /// dark/light split) that applies live on click. Simpler than the theme
    /// grid: no profiles, no scroller.
    pub(super) fn render_terminal_scheme_content(
        scheme: preferences::TerminalAnsiTheme,
        active: bool,
        mode: ThemeMode,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let preview = terminal_panel::terminal_scheme_preview(scheme, mode, &colors);
        let label = scheme.label();
        desktop_ghost_button(format!("terminal-scheme-{}", scheme.id()))
            .accessibility_label(label.clone())
            .toggled(active)
            .w(px(88.))
            .flex_shrink_0()
            // Fixed height like the theme swatches: the button's content row
            // clips overflow, so auto height (32px) cut the label off. Content
            // is p_2 (16) + preview (32) + gap (4) + label (~16) = ~68px; the
            // 12px slack absorbs UI font scaling.
            .h(px(80.))
            .p_2()
            .when(active, |button| button.bg(colors.popover))
            .rounded_lg()
            .on_click({
                let view = view.clone();
                move |_, _: &mut Window, cx: &mut App| {
                    view.update(cx, |view, cx| {
                        view.set_terminal_ansi_theme(scheme, cx);
                    });
                }
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(terminal_panel::terminal_preview_chip(&preview, colors.foreground, colors.border))
                    .child(
                        div()
                            .text_xs()
                            .truncate()
                            .when(active, |text| text.font_semibold())
                            .child(label),
                    ),
            )
            .into_any_element()
    }

    /// Build the dock-position dropdown state outside `bootstrap.rs`, which
    /// sits at the file ceiling; the setting, handler, and wiring stay together.
    pub(super) fn build_terminal_position_select(
        preferences: &preferences::Preferences,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Entity<preferences::TerminalPositionSelectState>, Subscription) {
        let select =
            cx.new(|cx| preferences::TerminalPositionOption::select_state(preferences.terminal_position(), window, cx));
        let subscription = cx.subscribe_in(&select, window, |view, _, event, _, cx| {
            if let SelectEvent::Confirm(Some(position)) = event {
                view.set_terminal_position(*position, cx);
            }
        });
        (select, subscription)
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
        Self::apply_sidebar_background(self.effective_translucent_sidebar(), window, cx);
        self.save_preferences();
        cx.notify();
    }

    /// Effective translucency: the stored preference wins unless macOS Reduce
    /// Transparency asks apps to keep surfaces opaque (HIG accessibility).
    /// The switch keeps reflecting the stored preference so the user's intent
    /// survives; rendering and window appearance follow this gate instead.
    pub(super) fn effective_translucent_sidebar(&self) -> bool {
        self.preferences.translucent_sidebar && !reduced_transparency_requested()
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
        self.preferences.show_status_bar.toggle();
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
            .child(
                div()
                    .id("status-bar-workspace")
                    .test_support()
                    .truncate()
                    .child(workspace.to_string()),
            )
            .when(self.git_branch.is_some(), |bar| {
                bar.child(
                    div().id("status-bar-branch").test_support().truncate().child(
                        rust_i18n::t!(
                            "app.appearance.branch_label",
                            value = self.git_branch.clone().unwrap_or_default()
                        )
                        .to_string(),
                    ),
                )
            })
            .when(sandbox.is_some(), |bar| {
                bar.child(div().id("status-bar-sandbox").test_support().truncate().child(
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
            .child(
                div()
                    .id("status-bar-provider-model")
                    .test_support()
                    .truncate()
                    .child(provider_model),
            )
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
            .fade_in("status-bar-fade", cx.theme())
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
        // Mode first: Auto palette resolution reads the new mode, not the old.
        self.sync_terminal_palette(cx);
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
        self.refresh_composer_highlight(cx);
        self.transcript.remeasure();
        self.save_preferences();
        cx.notify();
    }

    /// Repaint composer tokens with the newly applied palette. The highlighter
    /// reads its colors through the shared cell, so resetting the language is
    /// enough to force a retokenize with the new theme.
    pub(super) fn refresh_composer_highlight(&mut self, cx: &mut Context<Self>) {
        *self.composer_highlight_styles.borrow_mut() =
            composer_highlight::ComposerHighlightStyles::from_colors(&cx.theme().colors);
        self.composer.update(cx, |composer, cx| {
            composer.set_highlighter(composer_highlight::PROMPT_LANGUAGE, cx);
        });
    }

    pub(super) fn apply_typography(&mut self, cx: &mut Context<Self>) {
        let typography = self.preferences.typography_for(self.theme_mode);
        let ui = typography.ui_font_size;
        let code = typography.code_font_size;
        let installed = if typography.ui_font_family.is_some() || typography.code_font_family.is_some() {
            typography::installed_fonts(cx)
        } else {
            Vec::new()
        };
        let ui_family = typography::resolve_font(typography.ui_font_family, ".SystemUIFont", &installed);
        let code_family =
            typography::resolve_font(typography.code_font_family, self.default_code_font.as_str(), &installed);
        Theme::update(cx, |theme| {
            theme.font_family = ui_family.into();
            theme.mono_font_family = code_family.into();
            theme.font_size = px(f32::from(ui));
            theme.mono_font_size = px(f32::from(code));
            // Kit derives the window rem from `font_size`; the shared radius
            // tokens must be rebuilt here too or Kit surfaces keep the old
            // pixel radii while every named `.rounded_*()` helper zooms.
            themes::apply_desktop_radii(theme);
        });
        self.transcript.remeasure();
        cx.notify();
    }

    pub(super) fn render_reduced_motion_content(
        current: preferences::ReducedMotionPreference,
        focus: FocusHandle,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let colors = cx.theme().colors;
        let selected = preferences::ReducedMotionPreference::ALL
            .iter()
            .position(|preference| *preference == current)
            .unwrap_or_default();
        let ring = colors.ring;
        let current_label = current.label();
        let choice_label = rust_i18n::t!(
            "app.appearance.motion_aria",
            label = current_label.as_str(),
            suffix = rust_i18n::t!("app.appearance.selected_suffix").to_string()
        )
        .to_string();
        let click_view = view.clone();
        let key_view = view.downgrade();
        div()
            .id("reduced-motion-setting")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.settings.appearance.reduce_motion").to_string())
            .w_full()
            .child(settings_row(
                rust_i18n::t!("app.settings.appearance.reduce_motion").to_string(),
                rust_i18n::t!("app.settings.appearance.reduce_motion_description").to_string(),
                segmented_choice_wrapper(
                    "reduced-motion-tabs",
                    &focus,
                    selected,
                    preferences::ReducedMotionPreference::ALL.len(),
                    move |next, _, cx| {
                        if let Some(preference) = preferences::ReducedMotionPreference::ALL.get(next) {
                            let _updated = key_view.update(cx, |view, cx| view.set_reduced_motion(*preference, cx));
                        }
                    },
                    ring,
                    choice_label.into(),
                    desktop_segmented_tab_bar("reduced-motion-segmented")
                        .selected_index(selected)
                        .on_click(move |index: &usize, _, cx| {
                            if let Some(preference) = preferences::ReducedMotionPreference::ALL.get(*index) {
                                click_view.update(cx, |view, cx| view.set_reduced_motion(*preference, cx));
                            }
                        })
                        .children(preferences::ReducedMotionPreference::ALL.map(|preference| {
                            let label = preference.label();
                            let selected = preference == current;
                            let tooltip = match preference {
                                preferences::ReducedMotionPreference::System => {
                                    rust_i18n::t!("app.appearance.reread_system").to_string()
                                }
                                preferences::ReducedMotionPreference::On => {
                                    rust_i18n::t!("app.appearance.reduce_animations").to_string()
                                }
                                preferences::ReducedMotionPreference::Off => {
                                    rust_i18n::t!("app.appearance.allow_animations").to_string()
                                }
                            };
                            Tab::new()
                                .label(label.clone())
                                .aria_label(
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
                                .tooltip(move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                                })
                        })),
                ),
                &colors,
            ))
            .into_any_element()
    }

    /// App-context content builder, hostable inside a Kit `SettingItem`
    /// closure.
    pub(super) fn render_appearance_settings_content(
        props: AppearanceSettingsProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
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
        let light_group = show_light.then(|| Self::render_theme_group_content(&props.theme_grid_light));
        let dark_group = show_dark.then(|| Self::render_theme_group_content(&props.theme_grid_dark));
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
                                    desktop_ghost_button(format!("edit-{id}-profile"))
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
                                }))
                                .fade_in("theme-profile-row-fade", &theme),
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
                                desktop_ghost_button(if forward { "next-themes" } else { "previous-themes" })
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
                                    .small()
                                    .on_click({
                                        let view = view.clone();
                                        move |_, _: &mut Window, cx: &mut App| {
                                            // Three swatches per page, in row-stride pixels shared
                                            // with the virtual size table.
                                            let step = 3.0 * appearance_theme_picker::THEME_SWATCH_STRIDE;
                                            for (shown, scroll) in visible.into_iter().zip([&light, &dark]) {
                                                if !shown {
                                                    continue;
                                                }
                                                let mut offset = scroll.offset();
                                                offset.x += px(if forward { -step } else { step });
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

        let terminal_group_title = rust_i18n::t!("app.settings.appearance.terminal_colors").to_string();
        let selected_scheme = props.terminal_ansi_theme;
        let scheme_mode = props.theme_mode;
        content = content.child(accessible_group_box(
            "appearance-terminal-group",
            terminal_group_title.clone(),
            settings_group_box(terminal_group_title, colors)
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(row_description(
                            rust_i18n::t!("app.settings.appearance.terminal_colors_description").to_string(),
                            colors,
                        ))
                        .child(div().flex().flex_row().flex_wrap().gap_2().children(
                            preferences::TerminalAnsiTheme::ALL.map(|scheme| {
                                Self::render_terminal_scheme_content(
                                    scheme,
                                    scheme == selected_scheme,
                                    scheme_mode,
                                    view.clone(),
                                    cx,
                                )
                            }),
                        )),
                )
                .child(settings_row(
                    rust_i18n::t!("app.settings.appearance.terminal_position").to_string(),
                    rust_i18n::t!("app.settings.appearance.terminal_position_description").to_string(),
                    Select::new(&props.terminal_position_select)
                        .id("terminal-position-select")
                        .accessibility_label(rust_i18n::t!("app.settings.appearance.terminal_position").to_string())
                        .menu_max_h(px(320.))
                        .w(px(240.)),
                    colors,
                )),
        ));

        content = content.child(accessible_group_box(
            "appearance-layout-group",
            layout.clone(),
            settings_group_box(layout, colors)
                .child(appearance_chat_width::render_chat_width_row(&props.chat_width_select, colors))
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
            desktop_danger_button("reset-appearance")
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
