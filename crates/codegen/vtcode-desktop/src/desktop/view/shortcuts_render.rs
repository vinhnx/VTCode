//! Searchable shortcut editor: per-row capture and unbind over a staged draft.
use super::*;
use gpui_kit::component::kbd::Kbd;

impl DesktopView {
    pub(super) fn render_shortcuts(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let colors = &cx.theme().colors;
        // Capture is live only while the pill holds focus. Tab and click-away move focus
        // elsewhere without touching state, so the pill renders exactly when armed and focused;
        // every exit path (commit, cancel, save, reset, import, page close) clears the arm.
        let pill_focused = self.shortcut_capture_focus.is_focused(window);
        let description = if self.shortcuts_editing {
            rust_i18n::t!("app.shortcuts.unsaved_description").to_string()
        } else {
            rust_i18n::t!("app.shortcuts.browse_description").to_string()
        };
        let query = self.shortcut_search_input.read(cx).value().trim().to_string();
        let terms = config_search::config_search_terms(&query);
        let visible: Vec<&shortcuts::ShortcutCommand> = shortcuts::COMMANDS
            .iter()
            .filter(|command| {
                shortcuts::shortcut_matches_query(
                    command,
                    shortcuts::staged_value(&self.shortcut_draft, command),
                    &terms,
                )
            })
            .collect();

        let cli_shortcuts_available = self
            .configuration
            .as_ref()
            .and_then(|configuration| configuration.shared_ui.as_ref())
            .is_some();
        let mut actions = div().flex().flex_wrap().gap_2();
        if self.shortcuts_editing {
            actions = actions
                .child(
                    desktop_button("save-shortcuts")
                        .label(rust_i18n::t!("app.shortcuts.save").to_string())
                        .primary()
                        .small()
                        .on_click(cx.listener(|view, _, window, cx| view.save_shortcut_draft(window, cx))),
                )
                .child(
                    desktop_button("cancel-shortcuts")
                        .label(rust_i18n::t!(i18n::CONFIGURATION_CANCEL).to_string())
                        .ghost()
                        .small()
                        .on_click(cx.listener(|view, _, window, cx| view.cancel_shortcut_edit(window, cx))),
                );
        }
        actions = actions
            .child(
                desktop_button("reset-shortcuts")
                    .label(rust_i18n::t!("app.shortcuts.reset_all").to_string())
                    .ghost()
                    .small()
                    .on_click(cx.listener(|view, _, window, cx| view.reset_shortcuts(window, cx))),
            )
            .child(
                desktop_button("import-cli-shortcuts")
                    .label(rust_i18n::t!("app.shortcuts.import_cli").to_string())
                    .ghost()
                    .small()
                    .desktop_disabled(!cli_shortcuts_available)
                    .on_click(cx.listener(|view, _, window, cx| view.import_cli_shortcuts(window, cx))),
            );

        let mut children = Vec::with_capacity(visible.len() * 2);
        for (index, command) in visible.iter().enumerate() {
            if index > 0 {
                children.push(row_divider(colors).into_any_element());
            }
            children.push(shortcut_row(
                command,
                shortcuts::staged_value(&self.shortcut_draft, command),
                pill_focused && self.shortcut_capture == Some(command.id),
                &self.shortcut_capture_focus,
                colors,
                cx,
            ));
        }
        if visible.is_empty() {
            children.push(
                div()
                    .id("shortcut-empty")
                    .test_support()
                    .role(gpui_kit::Role::Status)
                    .text_sm()
                    .text_color(themes::muted_text(colors))
                    .child(rust_i18n::t!("app.shortcuts.no_match", query = query.as_str()).to_string())
                    .into_any_element(),
            );
        }

        let group = settings_group_box(rust_i18n::t!("app.shortcuts.group_label").to_string(), colors)
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(description),
            )
            .child(
                div().flex().gap_2().child(
                    Input::new(&self.shortcut_search_input)
                        .id("shortcut-search")
                        .flex_1()
                        .min_w_0()
                        .small()
                        .aria_label(rust_i18n::t!(i18n::SHORTCUTS_SEARCH_ARIA).to_string())
                        .prefix(Icon::new(IconName::Search).size_4())
                        .cleanable(true),
                ),
            )
            .child(actions)
            .children(children)
            .children(self.shortcut_message.as_ref().map(|message| {
                div()
                    .id("shortcut-status")
                    .role(gpui_kit::Role::Status)
                    .aria_label(message.clone())
                    .text_sm()
                    .child(message.clone())
            }))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(rust_i18n::t!("app.shortcuts.footer_hint").to_string()),
            );
        accessible_group_box("keyboard-shortcuts-group", rust_i18n::t!("app.shortcuts.group_label").to_string(), group)
    }

    fn begin_shortcut_capture(&mut self, id: shortcuts::CommandId, window: &mut Window, cx: &mut Context<Self>) {
        self.shortcut_capture = Some(id);
        self.shortcut_message = None;
        self.shortcut_capture_focus.focus(window, cx);
        cx.notify();
    }

    /// True while the capture pill holds focus: bound chords must reach the
    /// pill's validator raw instead of firing desktop actions. Renders omit
    /// their key contexts in this state, which restores the inertness the
    /// old settings sheet layer provided (global bindings like Cancel still
    /// match, exactly as they did inside the sheet).
    pub(super) fn capture_suppresses_bindings(&self, window: &Window) -> bool {
        self.shortcut_capture.is_some() && self.shortcut_capture_focus.is_focused(window)
    }

    pub(super) fn cancel_shortcut_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Entered from dismiss_overlay ordering, the pill's Cancel action,
        // and its Escape key branch. Keymap actions dispatch before element
        // key handlers, so stopping here is what keeps dismissal from
        // closing Settings underneath the capture.
        cx.stop_propagation();
        self.shortcut_capture = None;
        self.shortcut_message = None;
        self.focus_shortcut_search(window, cx);
        cx.notify();
    }

    /// Editing continues after capture ends (commit, cancel, staged reset), so focus the
    /// search field: the next row is one filter away. Session-ending actions (save, cancel,
    /// import) return to settings navigation instead.
    fn focus_shortcut_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shortcut_search_input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn capture_shortcut_key(
        &mut self,
        id: shortcuts::CommandId,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Desktop-context chords reach in-tree settings, and dismiss_overlay
        // ordering cancels an armed capture before closing the page; the
        // pill's own Cancel action stays as a fallback. Stopping here keeps
        // the keystroke from reaching ancestor key handlers and text input.
        cx.stop_propagation();
        if event.keystroke.key.as_str() == "escape" {
            self.cancel_shortcut_capture(window, cx);
            return;
        }
        if shortcuts::is_modifier_key(event.keystroke.key.as_str()) {
            return;
        }
        // Tab and Shift-Tab move focus natively before this stale path runs; they are never
        // committable chords, so skip validation instead of flashing a misleading error.
        if event.keystroke.key.as_str() == "tab" {
            return;
        }
        let mut candidate = self.shortcut_draft.clone();
        let _previous = candidate.insert(id.as_str().to_string(), event.keystroke.unparse());
        match shortcuts::bindings(&candidate) {
            Ok(_) => {
                if candidate.get(id.as_str()) != self.shortcut_draft.get(id.as_str()) {
                    self.shortcut_draft = candidate;
                    self.shortcuts_editing = true;
                }
                self.shortcut_capture = None;
                self.shortcut_message = None;
                self.focus_shortcut_search(window, cx);
            }
            Err(error) => {
                self.shortcut_message = Some(error);
            }
        }
        cx.notify();
    }

    fn unbind_shortcut(&mut self, id: shortcuts::CommandId, cx: &mut Context<Self>) {
        debug_assert!(self.shortcut_draft.len() == shortcuts::COMMANDS.len(), "shortcut draft stays a full table");
        self.shortcut_capture = None;
        if self.shortcut_draft.get(id.as_str()).is_some_and(|staged| staged.is_empty()) {
            return;
        }
        let _previous = self.shortcut_draft.insert(id.as_str().to_string(), String::new());
        self.shortcuts_editing = true;
        self.shortcut_message = None;
        cx.notify();
    }

    fn save_shortcut_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        debug_assert!(self.shortcut_draft.len() == shortcuts::COMMANDS.len(), "shortcut draft stays a full table");
        match shortcuts::bindings(&self.shortcut_draft) {
            Ok(_) => {
                let mut preferences = self.preferences.clone();
                preferences.shortcuts = self.shortcut_draft.clone();
                match preferences.try_save() {
                    Ok(()) => {
                        self.preferences = preferences;
                        self.shortcut_capture = None;
                        self.shortcuts_editing = false;
                        self.settings_navigation_focus.focus(window, cx);
                        self.shortcut_message = Some(rust_i18n::t!("app.shortcuts.saved").to_string());
                    }
                    Err(error) => self.shortcut_message = Some(error),
                }
            }
            Err(error) => self.shortcut_message = Some(error),
        }
        cx.notify();
    }

    fn cancel_shortcut_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shortcut_draft = shortcuts::draft_from(&self.preferences.shortcuts);
        self.shortcut_capture = None;
        self.shortcut_message = None;
        self.shortcuts_editing = false;
        self.settings_navigation_focus.focus(window, cx);
        cx.notify();
    }

    fn reset_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Reset stages rather than saving: a misclick recovers through Cancel, and Save keeps
        // its single role as the persistence point.
        let defaults = shortcuts::draft_from(&std::collections::BTreeMap::new());
        self.shortcut_capture = None;
        if defaults == self.shortcut_draft {
            self.shortcut_message = Some(rust_i18n::t!("app.shortcuts.already_default").to_string());
            cx.notify();
            return;
        }
        self.shortcut_draft = defaults;
        self.shortcuts_editing = true;
        self.shortcut_message = Some(rust_i18n::t!("app.shortcuts.staged_defaults").to_string());
        self.focus_shortcut_search(window, cx);
        cx.notify();
    }
}

fn shortcut_row(
    command: &shortcuts::ShortcutCommand,
    staged: &str,
    capturing: bool,
    capture_focus: &FocusHandle,
    colors: &gpui_kit::component::theme::ThemeColor,
    cx: &Context<DesktopView>,
) -> AnyElement {
    let id = command.id;
    let faded = themes::faded_chrome(colors.background, colors.foreground);
    let value_element = if capturing {
        div()
            .id(format!("shortcut-capture-{}", id.as_str()))
            .test_support()
            .track_focus(capture_focus)
            .role(gpui_kit::Role::Group)
            .aria_label(rust_i18n::t!("app.shortcuts.press_keys", label = command.label()).to_string())
            .px_2()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(colors.border)
            .bg(colors.popover)
            .text_xs()
            .text_color(colors.foreground)
            .child(rust_i18n::t!("app.shortcuts.press_keys_hint").to_string())
            .on_action(cx.listener(|view, _: &gpui_kit::base::actions::Cancel, window, cx| {
                view.cancel_shortcut_capture(window, cx);
            }))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                view.capture_shortcut_key(id, event, window, cx);
            }))
            .into_any_element()
    } else {
        shortcut_staged_badge(command, staged, colors)
    };
    let control = div()
        .flex()
        .items_center()
        .gap_2()
        .child(value_element)
        .child(
            desktop_button(format!("shortcut-edit-{}", id.as_str()))
                .icon(Icon::new(IconName::Pencil).size_4().text_color(faded))
                .ghost()
                .small()
                .accessibility_label(rust_i18n::t!("app.shortcuts.edit_for", label = command.label()).to_string())
                .on_click(cx.listener(move |view, _, window, cx| view.begin_shortcut_capture(id, window, cx))),
        )
        .child(
            desktop_button(format!("shortcut-remove-{}", id.as_str()))
                .icon(Icon::new(IconName::Trash).size_4().text_color(faded))
                .ghost()
                .small()
                .accessibility_label(rust_i18n::t!("app.shortcuts.remove_for", label = command.label()).to_string())
                .desktop_disabled(staged.is_empty())
                .on_click(cx.listener(move |view, _, _, cx| view.unbind_shortcut(id, cx))),
        );
    div()
        .id(format!("shortcut-row-{}", id.as_str()))
        .test_support()
        .role(gpui_kit::Role::Group)
        .aria_label(
            rust_i18n::t!(
                "app.shortcuts.row_aria",
                label = command.label(),
                value = shortcuts::shortcut_accessibility_value(staged)
            )
            .to_string(),
        )
        .w_full()
        .child(settings_row(command.label(), command.description(), control, colors))
        .into_any_element()
}

fn shortcut_staged_badge(
    command: &shortcuts::ShortcutCommand,
    staged: &str,
    colors: &gpui_kit::component::theme::ThemeColor,
) -> AnyElement {
    let badge = if staged.is_empty() {
        div()
            .text_xs()
            .text_color(themes::muted_text(colors))
            .child(rust_i18n::t!("app.shortcuts.badge_unassigned").to_string())
            .into_any_element()
    } else {
        match gpui_kit::Keystroke::parse(staged) {
            Ok(keystroke) => div().child(Kbd::new(keystroke).outline()).into_any_element(),
            Err(_) => div()
                .text_xs()
                .text_color(colors.danger)
                .child(rust_i18n::t!("app.shortcuts.badge_invalid").to_string())
                .into_any_element(),
        }
    };
    div()
        .id(format!("shortcut-badge-{}", command.id.as_str()))
        .test_support()
        .role(gpui_kit::Role::Label)
        .aria_label(
            rust_i18n::t!(
                "app.shortcuts.badge_aria",
                label = command.label(),
                value = shortcuts::shortcut_accessibility_value(staged)
            )
            .to_string(),
        )
        .child(badge)
        .into_any_element()
}
