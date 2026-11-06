//! Diff viewer presentation settings (Appearance tab).
use super::*;
use gpui_kit::component::diff::{DiffChangeIndicator, DiffHoverHighlight, DiffHunkSeparator, DiffInlineUnit};

impl DesktopView {
    pub(super) fn render_diff_settings(&self, cx: &Context<Self>) -> AnyElement {
        let colors = &cx.theme().colors;
        let title = rust_i18n::t!("app.settings.diff.group").to_string();
        accessible_group_box(
            "appearance-diff-group",
            title.clone(),
            settings_group_box(title, colors)
                .child(
                    gpui_kit::component::switch::Switch::new("diff-soft-wrap")
                        .label(rust_i18n::t!("app.settings.diff.wrap.label").to_string())
                        .checked(self.preferences.diff.soft_wrap())
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_diff_soft_wrap(*checked, cx);
                        })),
                )
                .child(
                    gpui_kit::component::switch::Switch::new("diff-line-numbers")
                        .label(rust_i18n::t!("app.settings.diff.numbers.label").to_string())
                        .checked(self.preferences.diff.line_numbers())
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_diff_line_numbers(*checked, cx);
                        })),
                )
                .child(
                    gpui_kit::component::switch::Switch::new("diff-syntax-highlight")
                        .label(rust_i18n::t!("app.settings.diff.syntax.label").to_string())
                        .checked(self.preferences.diff.syntax_highlight())
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_diff_syntax_highlight(*checked, cx);
                        })),
                )
                .child(
                    gpui_kit::component::switch::Switch::new("diff-file-headers")
                        .label(rust_i18n::t!("app.settings.diff.headers.label").to_string())
                        .checked(self.preferences.diff.file_headers())
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_diff_file_headers(*checked, cx);
                        })),
                )
                .child(
                    gpui_kit::component::switch::Switch::new("diff-change-background")
                        .label(rust_i18n::t!("app.settings.diff.tint.label").to_string())
                        .checked(self.preferences.diff.change_background())
                        .on_click(cx.listener(|view, checked, _, cx| {
                            view.set_diff_change_background(*checked, cx);
                        })),
                )
                .child(self.render_indicator_options(colors, cx))
                .child(self.render_separator_options(colors, cx))
                .child(self.render_hover_options(colors, cx))
                .child(self.render_context_options(colors, cx))
                .child(self.render_inline_options(colors, cx)),
        )
    }

    /// One segmented option button: same selected styling as the settings
    /// profile buttons, keyboard-focusable like every other action.
    fn diff_option_button<Select>(
        id: &'static str,
        label: String,
        selected: bool,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &Context<Self>,
        select: Select,
    ) -> impl IntoElement + use<Select>
    where
        Select: Fn(&mut Self, &mut Context<Self>) + 'static,
    {
        desktop_button(id)
            .label(label)
            .small()
            .desktop_selected(selected, colors)
            .on_click(cx.listener(move |view, _, _, cx| select(view, cx)))
    }

    /// One exclusive-choice row: title and description stacked left, the
    /// labeled option group right (dropping below the text when narrow).
    fn diff_options_row(
        group_id: &'static str,
        label_key: &'static str,
        description_key: &'static str,
        colors: &gpui_kit::component::theme::ThemeColor,
        options: Vec<impl IntoElement>,
    ) -> impl IntoElement {
        let label = rust_i18n::t!(label_key).to_string();
        let mut group = div()
            .id(group_id)
            .test_support()
            .role(gpui_kit::Role::Group)
            .aria_label(label.clone())
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2();
        for option in options {
            group = group.child(option);
        }
        settings_row(label, rust_i18n::t!(description_key).to_string(), group, colors)
    }

    fn render_indicator_options(
        &self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let current = self.preferences.diff.change_indicator();
        let mut buttons = Vec::new();
        for (id, key, value) in [
            ("diff-indicator-signs", "app.settings.diff.indicator.signs", DiffChangeIndicator::Signs),
            ("diff-indicator-bars", "app.settings.diff.indicator.bars", DiffChangeIndicator::Bars),
            ("diff-indicator-none", "app.settings.diff.indicator.none", DiffChangeIndicator::None),
        ] {
            buttons.push(Self::diff_option_button(
                id,
                rust_i18n::t!(key).to_string(),
                current == value,
                colors,
                cx,
                move |view, cx| view.set_diff_change_indicator(value, cx),
            ));
        }
        Self::diff_options_row(
            "diff-indicator-options",
            "app.settings.diff.indicator.label",
            "app.settings.diff.indicator.description",
            colors,
            buttons,
        )
    }

    fn render_separator_options(
        &self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let current = self.preferences.diff.hunk_separator();
        let mut buttons = Vec::new();
        for (id, key, value) in [
            ("diff-separator-metadata", "app.settings.diff.separator.metadata", DiffHunkSeparator::Metadata),
            ("diff-separator-lineinfo", "app.settings.diff.separator.line_info", DiffHunkSeparator::LineInfo),
            ("diff-separator-simple", "app.settings.diff.separator.simple", DiffHunkSeparator::Simple),
        ] {
            buttons.push(Self::diff_option_button(
                id,
                rust_i18n::t!(key).to_string(),
                current == value,
                colors,
                cx,
                move |view, cx| view.set_diff_hunk_separator(value, cx),
            ));
        }
        Self::diff_options_row(
            "diff-separator-options",
            "app.settings.diff.separator.label",
            "app.settings.diff.separator.description",
            colors,
            buttons,
        )
    }

    fn render_hover_options(
        &self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let current = self.preferences.diff.hover_highlight();
        let mut buttons = Vec::new();
        for (id, key, value) in [
            ("diff-hover-none", "app.settings.diff.hover.none", DiffHoverHighlight::None),
            ("diff-hover-line", "app.settings.diff.hover.line", DiffHoverHighlight::Line),
            ("diff-hover-linenumber", "app.settings.diff.hover.line_number", DiffHoverHighlight::LineNumber),
            ("diff-hover-both", "app.settings.diff.hover.both", DiffHoverHighlight::Both),
        ] {
            buttons.push(Self::diff_option_button(
                id,
                rust_i18n::t!(key).to_string(),
                current == value,
                colors,
                cx,
                move |view, cx| view.set_diff_hover_highlight(value, cx),
            ));
        }
        Self::diff_options_row(
            "diff-hover-options",
            "app.settings.diff.hover.label",
            "app.settings.diff.hover.description",
            colors,
            buttons,
        )
    }

    fn render_context_options(
        &self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let current = self.preferences.diff.context_lines();
        let mut buttons = Vec::new();
        for (id, key, value) in [
            ("diff-context-3", "app.settings.diff.context.depth_3", Some(3)),
            ("diff-context-5", "app.settings.diff.context.depth_5", Some(5)),
            ("diff-context-10", "app.settings.diff.context.depth_10", Some(10)),
            ("diff-context-unfolded", "app.settings.diff.context.unfolded", None),
        ] {
            buttons.push(Self::diff_option_button(
                id,
                rust_i18n::t!(key).to_string(),
                current == value,
                colors,
                cx,
                move |view, cx| view.set_diff_context_lines(value, cx),
            ));
        }
        Self::diff_options_row(
            "diff-context-options",
            "app.settings.diff.context.label",
            "app.settings.diff.context.description",
            colors,
            buttons,
        )
    }

    fn render_inline_options(
        &self,
        colors: &gpui_kit::component::theme::ThemeColor,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let current = self.preferences.diff.inline_unit();
        let mut buttons = Vec::new();
        for (id, key, value) in [
            ("diff-inline-words", "app.settings.diff.inline.words", Some(DiffInlineUnit::Word)),
            ("diff-inline-characters", "app.settings.diff.inline.characters", Some(DiffInlineUnit::Character)),
            ("diff-inline-lines", "app.settings.diff.inline.lines", None),
        ] {
            buttons.push(Self::diff_option_button(
                id,
                rust_i18n::t!(key).to_string(),
                current == value,
                colors,
                cx,
                move |view, cx| view.set_diff_inline_unit(value, cx),
            ));
        }
        Self::diff_options_row(
            "diff-inline-options",
            "app.settings.diff.inline.label",
            "app.settings.diff.inline.description",
            colors,
            buttons,
        )
    }

    pub(super) fn set_diff_soft_wrap(&mut self, enabled: bool, cx: &mut Context<Self>) {
        // An explicit selection always persists, even when it matches the
        // effective default, so the saved file records the user's choice.
        self.preferences.diff.set_soft_wrap(enabled);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    pub(super) fn set_diff_line_numbers(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.preferences.diff.set_line_numbers(enabled);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    pub(super) fn set_diff_syntax_highlight(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.preferences.diff.set_syntax_highlight(enabled);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    pub(super) fn set_diff_file_headers(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.preferences.diff.set_file_headers(enabled);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    pub(super) fn set_diff_change_background(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.preferences.diff.set_change_background(enabled);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    pub(super) fn set_diff_change_indicator(&mut self, indicator: DiffChangeIndicator, cx: &mut Context<Self>) {
        self.preferences.diff.set_change_indicator(indicator);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    pub(super) fn set_diff_hunk_separator(&mut self, separator: DiffHunkSeparator, cx: &mut Context<Self>) {
        self.preferences.diff.set_hunk_separator(separator);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    pub(super) fn set_diff_hover_highlight(&mut self, highlight: DiffHoverHighlight, cx: &mut Context<Self>) {
        self.preferences.diff.set_hover_highlight(highlight);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    pub(super) fn set_diff_context_lines(&mut self, lines: Option<usize>, cx: &mut Context<Self>) {
        self.preferences.diff.set_context_lines(lines);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    pub(super) fn set_diff_inline_unit(&mut self, unit: Option<DiffInlineUnit>, cx: &mut Context<Self>) {
        self.preferences.diff.set_inline_unit(unit);
        self.preferences.save();
        self.refresh_diff_viewers(cx);
        cx.notify();
    }

    /// Push the global diff state options to both retained viewers. Layout
    /// stays toolbar-owned per surface, so only context depth and inline
    /// emphasis sync here; appearance builders re-read preferences on render.
    /// Kit setters no-op on unchanged values, so this is safe to call blindly.
    fn refresh_diff_viewers(&self, cx: &mut Context<Self>) {
        let context = self.preferences.diff.context_lines();
        let inline = self.preferences.diff.inline_unit();
        let sync = |viewer: &Entity<DiffState>, cx: &mut Context<Self>| {
            viewer.update(cx, |viewer, cx| {
                viewer.set_context_lines(context, cx);
                viewer.set_inline_unit(inline, cx);
            });
        };
        if let Some(viewer) = self.workspace_review.as_ref().and_then(|review| review.viewer.as_ref()) {
            sync(viewer, cx);
        }
        if let Some(viewer) = self.change_viewer.as_ref() {
            sync(viewer, cx);
        }
    }
}
