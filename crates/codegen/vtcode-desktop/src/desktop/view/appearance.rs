//! Theme, typography, reduced-motion, and status-bar presentation.

use super::*;

impl DesktopView {
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
            ("Agent: running", colors.primary)
        } else {
            ("Agent: idle", colors.muted_foreground)
        };
        let configuration = self.configuration.as_ref();
        let provider_model = configuration
            .map(|config| format!("{} · {}", config.provider, config.model))
            .unwrap_or_else(|| "No provider configured".to_string());
        let sandbox = configuration.and_then(|config| config.sandbox.clone());
        let context_percent = configuration
            .and_then(|config| config.context_window)
            .zip(self.last_usage.as_ref())
            .and_then(|(window, usage)| context_usage_percent(usage, window));
        let workspace_details = div()
            .flex()
            .items_center()
            .gap_3()
            .min_w_0()
            .child(div().truncate().child(workspace.to_string()))
            .when(self.git_branch.is_some(), |bar| {
                bar.child(
                    div()
                        .truncate()
                        .child(format!("Branch: {}", self.git_branch.clone().unwrap_or_default())),
                )
            })
            .when(sandbox.is_some(), |bar| {
                bar.child(div().truncate().child(format!("Sandbox: {}", sandbox.unwrap_or_default())))
            });
        let runtime_details = div()
            .flex()
            .items_center()
            .gap_3()
            .min_w_0()
            .when(context_percent.is_some(), |bar| {
                bar.child(
                    div()
                        .id("context-usage")
                        .test_support()
                        .child(format!("Context: {}%", context_percent.unwrap_or_default())),
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
            .aria_label("Status bar")
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
                .filter(|id| themes::mode_for_id(id) == Some(target_mode))
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
        self.theme_mode = themes::apply_by_id(&id, window, cx);
        let id = if themes::mode_for_id(&id) == Some(self.theme_mode) {
            id
        } else {
            themes::default_id(self.theme_mode).to_string()
        };
        if self.preferences.separate_theme_profiles {
            match self.theme_mode {
                ThemeMode::Light => self.preferences.light_theme = Some(id.clone()),
                ThemeMode::Dark => self.preferences.dark_theme = Some(id.clone()),
            }
        }
        self.theme_id = id;
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

    pub(super) fn render_reduced_motion(&self, cx: &Context<Self>) -> AnyElement {
        let current = self.preferences.reduced_motion;
        let colors = cx.theme().colors;
        div()
            .id("reduced-motion-setting")
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label("Reduce motion")
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(row_title("Reduce motion", &colors))
                    .child(row_description(
                        "System uses macOS accessibility settings; select it again to refresh.",
                        &colors,
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .children(preferences::ReducedMotionPreference::ALL.into_iter().map(|preference| {
                        let label = preference.label();
                        let selected = preference == current;
                        desktop_button(format!("reduce-motion-{}", preference.id()))
                            .label(label)
                            .accessibility_label(format!(
                                "Reduce motion: {label}{}",
                                if selected { " (selected)" } else { "" }
                            ))
                            .tooltip(match preference {
                                preferences::ReducedMotionPreference::System => "Re-read system accessibility settings",
                                preferences::ReducedMotionPreference::On => "Reduce interface animations",
                                preferences::ReducedMotionPreference::Off => "Allow interface animations",
                            })
                            .ghost()
                            .when(selected, |button| button.bg(colors.secondary).text_color(colors.foreground))
                            .on_click(cx.listener(move |view, _, _, cx| view.set_reduced_motion(preference, cx)))
                    })),
            )
            .into_any_element()
    }

    pub(super) fn render_font_size(&self, code: bool, cx: &Context<Self>) -> AnyElement {
        let (key, label, description, size, minimum, maximum) = if code {
            (
                "code",
                "Code font size",
                "Code, diffs, and terminal output",
                self.preferences.typography_for(self.theme_mode).code_font_size,
                preferences::MIN_CODE_FONT_SIZE,
                preferences::MAX_CODE_FONT_SIZE,
            )
        } else {
            (
                "ui",
                "UI font size",
                "Base size for controls and conversation text",
                self.preferences.typography_for(self.theme_mode).ui_font_size,
                preferences::MIN_UI_FONT_SIZE,
                preferences::MAX_UI_FONT_SIZE,
            )
        };
        div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .py_2()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(row_title(label, &cx.theme().colors))
                    .child(row_description(description, &cx.theme().colors)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children([-1i16, 1].into_iter().enumerate().map(|(index, delta)| {
                        desktop_button(format!("font-{key}-{index}"))
                            .label(if delta < 0 { "−" } else { "+" })
                            .accessibility_label(format!("{} {label}", if delta < 0 { "Decrease" } else { "Increase" }))
                            .ghost()
                            .desktop_disabled(if delta < 0 { size <= minimum } else { size >= maximum })
                            .on_click(cx.listener(move |view, _, _, cx| {
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
                            }))
                    }))
                    .child(
                        div()
                            .id(format!("font-{key}-value"))
                            .role(gpui_kit::Role::Label)
                            .aria_label(format!("{label}: {size} pixels"))
                            .test_support()
                            .text_sm()
                            .child(format!("{size} px")),
                    ),
            )
            .into_any_element()
    }

    /// One theme entry in the settings picker: a color preview chip that
    /// applies live on click.
    pub(super) fn render_theme_swatch(
        &self,
        entry: themes::SwatchEntry,
        duplicate: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (background, foreground) = entry.preview();
        let entry_id = entry.id();
        let label = if duplicate {
            format!("{entry_id} · {}", entry.label())
        } else {
            entry.label().to_owned()
        };
        let active = self.theme_id == entry_id;
        let light_profile = self.preferences.separate_theme_profiles
            && self.preferences.light_theme.as_deref() == Some(entry_id.as_str());
        let dark_profile = self.preferences.separate_theme_profiles
            && self.preferences.dark_theme.as_deref() == Some(entry_id.as_str());
        let badge = if active {
            "active"
        } else if light_profile && dark_profile {
            "light and dark profiles"
        } else if light_profile {
            "light profile"
        } else if dark_profile {
            "dark profile"
        } else {
            ""
        };
        let selected = !badge.is_empty();
        let id = format!("theme-{entry_id}");
        let swatch = div()
            .w_full()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .h(px(40.))
                    .rounded_md()
                    .bg(background)
                    .p_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .when_some(entry.accent(), |chip, accent| chip.child(div().h(px(6.)).rounded_sm().bg(accent)))
                    .child(div().h(px(4.)).w_3_4().rounded_sm().bg(foreground))
                    .child(div().h(px(4.)).w_1_2().rounded_sm().bg(foreground.opacity(0.6))),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .when(selected, |label| label.font_semibold())
                    .child(label.clone()),
            );
        desktop_button(id)
            .accessibility_label(if selected { format!("{label} ({badge})") } else { label })
            .toggled(selected)
            .ghost()
            .w(px(104.))
            .flex_shrink_0()
            .h(px(80.))
            .p_2()
            .when(selected, |button| button.bg(cx.theme().colors.popover))
            .rounded_lg()
            .on_click(cx.listener(move |this, _, window, cx| {
                let theme_id = entry.id();
                this.apply_theme(theme_id, window, cx);
            }))
            .child(swatch)
            .into_any_element()
    }
}
