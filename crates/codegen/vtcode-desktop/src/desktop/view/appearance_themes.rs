//! Custom theme management: folded section, staged editor, saved-theme list,
//! and persistence.
//!
//! A custom theme is the same seven-seed palette a built-in row carries, so it
//! resolves through the shared derivation in [`themes::apply_palette`] and
//! inherits every WCAG floor. The section is a disclosure that starts folded,
//! keeping the Appearance page focused on the built-in palettes; expanding it
//! mounts one card holding the description, the saved rows, and the inline
//! editor. The editor is inline (like the shortcuts editor) rather than an
//! overlay: no scrim, focus trap, or overlay ordering is needed, and Escape
//! returns through `go_route_back` like any other staged edit.

use super::*;
use gpui_kit::Hsla;
use gpui_kit::component::color_picker::{ColorPickerEvent, ColorPickerState, ColorSelect};

/// One seed field in the editor, in tab and array order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CustomColorSlot {
    Background,
    Foreground,
    Accent,
    Secondary,
    Success,
    Danger,
    Warning,
}

impl CustomColorSlot {
    pub(super) const ALL: [Self; 7] = [
        Self::Background,
        Self::Foreground,
        Self::Accent,
        Self::Secondary,
        Self::Success,
        Self::Danger,
        Self::Warning,
    ];

    /// Read this slot out of the draft palette.
    fn read(self, seeds: themes::PaletteSeeds) -> Hsla {
        match self {
            Self::Background => seeds.background,
            Self::Foreground => seeds.foreground,
            Self::Accent => seeds.accent,
            Self::Secondary => seeds.secondary,
            Self::Success => seeds.success,
            Self::Danger => seeds.danger,
            Self::Warning => seeds.warning,
        }
    }

    /// Write this slot into the draft palette.
    fn write(self, seeds: &mut themes::PaletteSeeds, color: Hsla) {
        match self {
            Self::Background => seeds.background = color,
            Self::Foreground => seeds.foreground = color,
            Self::Accent => seeds.accent = color,
            Self::Secondary => seeds.secondary = color,
            Self::Success => seeds.success = color,
            Self::Danger => seeds.danger = color,
            Self::Warning => seeds.warning = color,
        }
    }

    /// Locale key per slot, so the error-field list and the seed pickers share
    /// one source of truth.
    fn label_key(self) -> &'static str {
        match self {
            Self::Background => "app.appearance.custom_color_background",
            Self::Foreground => "app.appearance.custom_color_foreground",
            Self::Accent => "app.appearance.custom_color_accent",
            Self::Secondary => "app.appearance.custom_color_secondary",
            Self::Success => "app.appearance.custom_color_success",
            Self::Danger => "app.appearance.custom_color_danger",
            Self::Warning => "app.appearance.custom_color_warning",
        }
    }

    fn label(self) -> String {
        rust_i18n::t!(self.label_key()).to_string()
    }
}

/// The staged edit. `editing_id` is `None` for a new theme and keeps the stable
/// id for a rename.
#[derive(Clone, Debug)]
pub(super) struct CustomThemeDraft {
    pub editing_id: Option<String>,
    /// The staged seed palette (including its mode); the same shape the theme
    /// layer derives from, so a save is a field-for-field copy. The name is not
    /// copied here: `name_input` owns that text, so there is one source of truth.
    pub seeds: themes::PaletteSeeds,
    pub error: Option<String>,
}

/// Retained editor state. The name input and the seven pickers live for the
/// window's lifetime; opening an editor only rewrites their values.
pub(super) struct CustomThemeState {
    pub name_input: Entity<InputState>,
    pub pickers: Vec<Entity<ColorPickerState>>,
    /// Kept alive for the window's lifetime; dropping them detaches the name and
    /// picker change streams, which would freeze the live preview.
    pub _subscriptions: Vec<Subscription>,
    pub draft: Option<CustomThemeDraft>,
    /// Disclosure state for the section; starts folded so the Appearance page
    /// leads with the built-in palettes and the saved-theme list stays opt-in.
    pub expanded: bool,
}

impl CustomThemeState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<DesktopView>) -> Self {
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(rust_i18n::t!("app.appearance.custom_theme_name_placeholder").to_string())
        });
        let seeds = current_seed_colors(cx);
        let mut pickers = Vec::with_capacity(CustomColorSlot::ALL.len());
        let mut subscriptions = Vec::with_capacity(CustomColorSlot::ALL.len() + 1);
        // The header and preview label read the name field, so a typed edit must
        // repaint. Seeding on open uses `set_value`, which emits nothing, so
        // opening never clears or rewrites staged state.
        subscriptions.push(cx.subscribe_in(&name_input, window, |view, _, event: &InputEvent, _, cx| {
            if matches!(event, InputEvent::Change) {
                if let Some(draft) = view.theme_editor.draft.as_mut() {
                    draft.error = None;
                }
                cx.notify();
            }
        }));
        for slot in CustomColorSlot::ALL {
            let picker = cx.new(|cx| ColorPickerState::new(window, cx).default_value(slot.read(seeds)));
            subscriptions.push(cx.subscribe_in(&picker, window, move |view, _, event: &ColorPickerEvent, _, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event
                    && let Some(draft) = view.theme_editor.draft.as_mut()
                {
                    slot.write(&mut draft.seeds, *color);
                    draft.error = None;
                    cx.notify();
                }
            }));
            pickers.push(picker);
        }
        Self {
            name_input,
            pickers,
            _subscriptions: subscriptions,
            draft: None,
            expanded: false,
        }
    }
}

/// Seed the editor from the active selection; system bases have no palette row,
/// so they fall back to the live kit tokens.
fn current_seed_colors(cx: &Context<DesktopView>) -> themes::PaletteSeeds {
    let colors = cx.theme().colors;
    themes::PaletteSeeds {
        mode: cx.theme().mode,
        background: colors.background,
        foreground: colors.foreground,
        accent: colors.primary,
        secondary: colors.chart_2,
        success: colors.success,
        danger: colors.danger,
        warning: colors.warning,
    }
}

impl DesktopView {
    pub(super) fn custom_theme_editor_open(&self) -> bool {
        self.theme_editor.draft.is_some()
    }

    /// Fold or unfold the saved-theme section. The editor lives inside the body,
    /// so folding closes it exactly like the Cancel button: an interior layer
    /// that is no longer visible must not keep consuming Back presses or make a
    /// later dismiss prompt name an editor nobody can see.
    pub(super) fn toggle_custom_themes_section(&mut self, cx: &mut Context<Self>) {
        self.theme_editor.expanded = !self.theme_editor.expanded;
        if !self.theme_editor.expanded {
            self.cancel_custom_theme_editor(cx);
        }
        cx.notify();
    }

    /// Route-level settings tab selection shared by the rail, the command
    /// palette, and keyboard navigation. A staged custom theme is page-local,
    /// so leaving Appearance discards it and folds the section back to its
    /// default.
    pub(super) fn select_settings_tab(&mut self, tab: settings::SettingsTab) {
        self.settings_tab = tab;
        self.config_page_open = false;
        if tab != settings::SettingsTab::Appearance {
            self.theme_editor.draft = None;
            self.theme_editor.expanded = false;
        }
    }

    pub(super) fn open_custom_theme_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let seeds = themes::selection_seeds(&self.theme_id, &self.preferences.custom_themes)
            .unwrap_or_else(|| current_seed_colors(cx));
        self.open_custom_theme_editor_with(None, String::new(), seeds, window, cx);
    }

    pub(super) fn edit_custom_theme(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(theme) = self.preferences.custom_themes.iter().find(|theme| theme.id == id).cloned() else {
            return;
        };
        let seeds = theme.seeds();
        self.open_custom_theme_editor_with(Some(theme.id), theme.name, seeds, window, cx);
    }

    fn open_custom_theme_editor_with(
        &mut self,
        editing_id: Option<String>,
        name: String,
        seeds: themes::PaletteSeeds,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_editor.draft = Some(CustomThemeDraft { editing_id, seeds, error: None });
        // Opening an editor from any entry point (Edit row, keyboard action,
        // tests) needs its host section unfolded, or the draft would be staged
        // behind a folded header.
        self.theme_editor.expanded = true;
        self.theme_editor
            .name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        // Pickers are created in `ALL` order, so zip rather than index.
        let pickers: Vec<_> = self.theme_editor.pickers.clone();
        for (slot, picker) in CustomColorSlot::ALL.into_iter().zip(pickers) {
            picker.update(cx, |state, cx| state.set_value(slot.read(seeds), window, cx));
        }
        self.theme_editor.name_input.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub(super) fn cancel_custom_theme_editor(&mut self, cx: &mut Context<Self>) {
        if self.theme_editor.draft.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn select_custom_theme_mode(&mut self, mode: ThemeMode, cx: &mut Context<Self>) {
        if let Some(draft) = self.theme_editor.draft.as_mut() {
            draft.seeds.mode = mode;
            draft.error = None;
            cx.notify();
        }
    }

    pub(super) fn save_custom_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.theme_editor.draft.clone() else {
            return;
        };
        // `name_input` owns the text; trim so the saved name and the
        // uniqueness check read the same value.
        let name = self.theme_editor.name_input.read(cx).value().trim().to_string();
        let taken = themes::name_taken(&self.preferences.custom_themes, &name, draft.editing_id.as_deref());
        let validation = themes::validate_custom_theme(&name, draft.seeds.foreground, draft.seeds.background, taken);
        if let Err(error) = validation {
            if let Some(draft) = self.theme_editor.draft.as_mut() {
                draft.error = Some(custom_theme_error_message(error));
            }
            cx.notify();
            return;
        }
        let id = draft
            .editing_id
            .clone()
            .unwrap_or_else(|| themes::custom_id_for(&name, &self.preferences.custom_themes));
        let theme = themes::CustomTheme::from_seeds(id.clone(), name, draft.seeds);
        match self.preferences.custom_themes.iter_mut().find(|saved| saved.id == id) {
            Some(saved) => *saved = theme,
            None => self.preferences.custom_themes.push(theme),
        }
        self.theme_editor.draft = None;
        self.apply_theme(id, window, cx);
        self.focus_settings_selection(window, cx);
    }

    /// Delete through the native warning prompt, matching every other
    /// destructive desktop action.
    pub(super) fn request_delete_custom_theme(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self
            .preferences
            .custom_themes
            .iter()
            .find(|theme| theme.id == id)
            .map(|theme| theme.name.clone())
        else {
            return;
        };
        let title = rust_i18n::t!("app.settings.storage.delete_title", name = name).to_string();
        let detail = rust_i18n::t!("app.appearance.custom_theme_delete_detail").to_string();
        let delete = rust_i18n::t!("app.settings.storage.delete").to_string();
        let cancel = rust_i18n::t!("app.settings.storage.cancel").to_string();
        let answer = window.prompt(
            gpui_kit::PromptLevel::Warning,
            title.as_str(),
            Some(detail.as_str()),
            &[delete.as_str(), cancel.as_str()],
            cx,
        );
        cx.spawn(async move |view, cx| {
            if matches!(answer.await, Ok(0)) {
                let _updated = view.update(cx, |view, cx| view.remove_custom_theme(&id, None, cx));
            }
        })
        .detach();
    }

    /// Removing the active theme falls back to the mode default and clears any
    /// profile reference, so a deleted id can never strand a light/dark lane.
    pub(super) fn remove_custom_theme(&mut self, id: &str, window: Option<&mut Window>, cx: &mut Context<Self>) {
        let Some(index) = self.preferences.custom_themes.iter().position(|theme| theme.id == id) else {
            return;
        };
        let was_active = self.theme_id == id;
        let _removed = self.preferences.custom_themes.remove(index);
        if self.theme_editor.draft.as_ref().and_then(|draft| draft.editing_id.as_deref()) == Some(id) {
            self.theme_editor.draft = None;
        }
        if self.preferences.light_theme.as_deref() == Some(id) {
            self.preferences.light_theme = None;
        }
        if self.preferences.dark_theme.as_deref() == Some(id) {
            self.preferences.dark_theme = None;
        }
        if was_active {
            let fallback = themes::default_id(self.theme_mode).to_string();
            self.apply_theme_state(fallback, window, cx);
        } else {
            self.save_preferences();
            cx.notify();
        }
    }

    /// The Appearance section is a disclosure: a header row that stays visible,
    /// with the saved-theme list, editor, and New theme action inside the card
    /// that only mounts when expanded. Folding closes the editor like Cancel, so
    /// no draft is ever left staged behind a hidden body (Back and route
    /// dismisses must never consume a press for an editor nobody can see).
    pub(super) fn render_custom_themes_group_content(
        props: &CustomThemesProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        // Own the theme: `cx.theme()` borrows `cx`, but the editor and row
        // builders below take `colors` and `cx` in the same call.
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let title = rust_i18n::t!("app.appearance.custom_themes").to_string();
        let expanded = props.expanded;
        let toggle_label = format!(
            "{} {title}",
            rust_i18n::t!(if expanded {
                "app.transcript.collapse"
            } else {
                "app.transcript.expand"
            })
        );
        let header = desktop_button("custom-themes-toggle")
            .accessibility_label(toggle_label)
            .toggled(expanded)
            .ghost()
            .w_full()
            .px_0()
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(group_box_title(title.clone(), colors))
                    .child(transcript_chevron_lane(expanded).text_color(colors.muted_foreground)),
            )
            .on_click({
                let view = view.clone();
                move |_, _: &mut Window, cx: &mut App| {
                    view.update(cx, |view, cx| view.toggle_custom_themes_section(cx));
                }
            });
        // A default (Normal) group is a transparent column with no fill, border,
        // or padding, and flex `gap` only spaces children *between* each other.
        // So the folded section (one child) costs exactly the header row, and
        // expanding adds `gap_3` of air above the card. Routing the header
        // through Kit's `title` slot instead would always mount the empty child
        // column and pay one gap for it (44px folded instead of 32px).
        let section = GroupBox::new()
            .gap_3()
            .child(header)
            .when(expanded, |section| section.child(Self::render_custom_themes_body(props, view, cx)));
        accessible_group_box("appearance-custom-themes-group", title, section)
    }

    /// Expanded content: description, staged editor, saved rows, and the New
    /// theme action share one card surface, so the editor and the list read as
    /// parts of the same section rather than nested boxes.
    fn render_custom_themes_body(props: &CustomThemesProps, view: Entity<Self>, cx: &mut App) -> AnyElement {
        let theme = cx.theme().clone();
        let colors = &theme.colors;
        let mut body = settings_group_box_untitled(colors)
            .child(row_description(rust_i18n::t!("app.appearance.custom_themes_description").to_string(), colors));
        if let Some(draft) = props.draft.as_ref() {
            body = body.child(Self::render_custom_theme_editor_content(draft, colors, props, view.clone(), cx));
        }
        if props.custom_themes.is_empty() {
            if props.draft.is_none() {
                body = body
                    .child(row_description(rust_i18n::t!("app.appearance.custom_themes_empty").to_string(), colors));
            }
        } else {
            body = body.children(props.custom_themes.iter().map(|theme| {
                Self::render_custom_theme_row_content(theme, colors, props.theme_id == theme.id, view.clone(), cx)
            }));
        }
        body.child(
            desktop_button("custom-theme-new")
                .icon(IconName::Plus)
                .label(rust_i18n::t!("app.appearance.custom_theme_new").to_string())
                .on_click({
                    let view = view.clone();
                    move |_, window: &mut Window, cx: &mut App| {
                        view.update(cx, |view, cx| view.open_custom_theme_editor(window, cx));
                    }
                }),
        )
        .into_any_element()
    }

    fn render_custom_theme_row_content(
        theme: &themes::CustomTheme,
        colors: &gpui_kit::component::theme::ThemeColor,
        active: bool,
        view: Entity<Self>,
        _cx: &mut App,
    ) -> AnyElement {
        let mode_label = rust_i18n::t!(match theme.mode {
            ThemeMode::Light => "app.settings.appearance.light",
            ThemeMode::Dark => "app.settings.appearance.dark",
        })
        .to_string();
        let apply_id = format!("custom-theme-apply-{}", theme.id);
        let edit_id = format!("custom-theme-edit-{}", theme.id);
        let delete_id = format!("custom-theme-delete-{}", theme.id);
        let apply_label = format!("{} · {mode_label}", theme.name);
        let edit_label = format!("{} {apply_label}", rust_i18n::t!("app.appearance.custom_theme_edit"));
        let delete_label = format!("{} {apply_label}", rust_i18n::t!("app.appearance.custom_theme_delete"));
        let apply_target = theme.id.clone();
        let edit_target = theme.id.clone();
        let delete_target = theme.id.clone();
        div()
            .id(format!("custom-theme-row-{}", apply_target))
            .test_support()
            .flex()
            .items_center()
            .gap_1()
            .w_full()
            .child(
                desktop_button(apply_id)
                    .accessibility_label(apply_label)
                    .desktop_selected(active, colors)
                    .flex_1()
                    .min_w_0()
                    .on_click({
                        let view = view.clone();
                        move |_, window: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| {
                                view.apply_theme(apply_target.clone(), window, cx);
                            });
                        }
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .size_6()
                                    .rounded_md()
                                    .bg(theme.background)
                                    .border_1()
                                    .border_color(themes::hairline(theme.background, theme.foreground))
                                    .child(div().m_1().h(px(4.)).w_1_2().rounded_sm().bg(theme.accent)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .items_start()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_sm()
                                            .text_color(colors.foreground)
                                            .child(theme.name.clone()),
                                    )
                                    .child(div().text_xs().text_color(colors.muted_foreground).child(mode_label)),
                            ),
                    ),
            )
            .child(
                desktop_button(edit_id)
                    .icon(IconName::Pencil)
                    .ghost()
                    .small()
                    .accessibility_label(edit_label)
                    .on_click({
                        let view = view.clone();
                        move |_, window: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| {
                                view.edit_custom_theme(&edit_target, window, cx);
                            });
                        }
                    }),
            )
            .child(
                desktop_button(delete_id)
                    .icon(IconName::Trash)
                    .ghost()
                    .small()
                    .accessibility_label(delete_label)
                    .on_click({
                        let view = view.clone();
                        move |_, window: &mut Window, cx: &mut App| {
                            view.update(cx, |view, cx| {
                                view.request_delete_custom_theme(delete_target.clone(), window, cx);
                            });
                        }
                    }),
            )
            .into_any_element()
    }

    fn render_custom_theme_editor_content(
        draft: &CustomThemeDraft,
        colors: &gpui_kit::component::theme::ThemeColor,
        props: &CustomThemesProps,
        view: Entity<Self>,
        cx: &mut App,
    ) -> AnyElement {
        let background = draft.seeds.background;
        let foreground = draft.seeds.foreground;
        let accent = draft.seeds.accent;
        let ratio = themes::contrast_ratio(foreground, background);
        let readable = ratio >= 4.5;
        let typed_name = props.name_input.read(cx).value().trim().to_string();
        let preview_label = if typed_name.is_empty() {
            rust_i18n::t!("app.appearance.custom_theme_preview").to_string()
        } else {
            typed_name
        };
        let contrast = if readable {
            rust_i18n::t!("app.appearance.custom_theme_contrast", ratio = format!("{ratio:.1}")).to_string()
        } else {
            rust_i18n::t!("app.appearance.custom_theme_contrast_low", ratio = format!("{ratio:.1}")).to_string()
        };
        let header = if draft.editing_id.is_some() {
            rust_i18n::t!("app.appearance.custom_theme_edit").to_string()
        } else {
            rust_i18n::t!("app.appearance.custom_theme_new").to_string()
        };
        let name_label = rust_i18n::t!("app.appearance.custom_theme_name").to_string();
        // The editor lives inside the section card, so it owns no chrome of its
        // own: a plain column separated by the card's gap.
        div()
            .id("custom-theme-editor")
            .test_support()
            .flex()
            .flex_col()
            .gap_3()
            .w_full()
            .child(row_title(header, colors))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xs().text_color(colors.muted_foreground).child(name_label.clone()))
                    .child(
                        Input::new(&props.name_input)
                            .id("custom-theme-name")
                            .aria_label(name_label)
                            .small()
                            .w_full(),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_1()
                    .children([ThemeMode::Light, ThemeMode::Dark].into_iter().map(|mode| {
                        let label = rust_i18n::t!(match mode {
                            ThemeMode::Light => "app.settings.appearance.light",
                            ThemeMode::Dark => "app.settings.appearance.dark",
                        })
                        .to_string();
                        desktop_button(format!(
                            "custom-theme-mode-{}",
                            if mode == ThemeMode::Light { "light" } else { "dark" }
                        ))
                        .label(label.clone())
                        .accessibility_label(label)
                        .desktop_selected(draft.seeds.mode == mode, colors)
                        .on_click({
                            let view = view.clone();
                            move |_, _: &mut Window, cx: &mut App| {
                                view.update(cx, |view, cx| view.select_custom_theme_mode(mode, cx));
                            }
                        })
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .children(CustomColorSlot::ALL.into_iter().zip(&props.pickers).map(|(slot, picker)| {
                        let label = slot.label();
                        div()
                            .w(px(200.))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_xs().text_color(colors.muted_foreground).child(label.clone()))
                            .child(ColorSelect::new(picker).small().accessibility_label(label))
                    })),
            )
            .child(
                div()
                    .id("custom-theme-preview")
                    .test_support()
                    .w_full()
                    .rounded_md()
                    .border_1()
                    .border_color(themes::hairline(background, foreground))
                    .bg(background)
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_sm().text_color(foreground).child(preview_label))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(div().text_xs().text_color(foreground).child(contrast))
                            .child(div().h(px(12.)).w(px(28.)).rounded_sm().bg(accent)),
                    ),
            )
            .when_some(draft.error.as_ref(), |editor, error| {
                editor.child(
                    div()
                        .id("custom-theme-error")
                        .test_support()
                        .role(gpui_kit::Role::Alert)
                        .aria_label(error.clone())
                        .text_sm()
                        .text_color(colors.danger)
                        .child(error.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        desktop_button("custom-theme-cancel")
                            .label(rust_i18n::t!("app.appearance.custom_theme_cancel").to_string())
                            .on_click({
                                let view = view.clone();
                                move |_, _: &mut Window, cx: &mut App| {
                                    view.update(cx, |view, cx| view.cancel_custom_theme_editor(cx));
                                }
                            }),
                    )
                    .child(
                        desktop_default_button("custom-theme-save")
                            .label(rust_i18n::t!("app.appearance.custom_theme_save").to_string())
                            .on_click({
                                let view = view.clone();
                                move |_, window: &mut Window, cx: &mut App| {
                                    view.update(cx, |view, cx| view.save_custom_theme(window, cx));
                                }
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// Cloned render inputs for the custom-themes group (see
/// `NotificationSettingsProps` for the Kit-hosting rationale).
#[derive(Clone)]
pub(super) struct CustomThemesProps {
    pub(super) draft: Option<CustomThemeDraft>,
    pub(super) custom_themes: Vec<themes::CustomTheme>,
    pub(super) theme_id: String,
    pub(super) name_input: Entity<InputState>,
    pub(super) pickers: Vec<Entity<ColorPickerState>>,
    pub(super) expanded: bool,
}

impl CustomThemesProps {
    /// Snapshot the view fields the custom-theme builders read. Both the
    /// legacy shell wrapper and the Kit adapter build props here.
    pub(super) fn capture(view: &DesktopView) -> Self {
        Self {
            draft: view.theme_editor.draft.clone(),
            custom_themes: view.preferences.custom_themes.clone(),
            theme_id: view.theme_id.clone(),
            name_input: view.theme_editor.name_input.clone(),
            pickers: view.theme_editor.pickers.clone(),
            expanded: view.theme_editor.expanded,
        }
    }
}

fn custom_theme_error_message(error: themes::CustomThemeError) -> String {
    rust_i18n::t!(match error {
        themes::CustomThemeError::EmptyName => "app.appearance.custom_theme_name_required",
        themes::CustomThemeError::NameTooLong => "app.appearance.custom_theme_name_too_long",
        themes::CustomThemeError::DuplicateName => "app.appearance.custom_theme_name_taken",
        themes::CustomThemeError::LowContrast => "app.appearance.custom_theme_contrast_low_error",
    })
    .to_string()
}
