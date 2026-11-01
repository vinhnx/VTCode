//! Welcome empty-state actions shared by the default and Zen views.

use super::*;

impl DesktopView {
    /// Welcome prelude both empty states render from; single owner so their copy cannot drift.
    pub(super) fn welcome_header(&self) -> WelcomeHeader {
        let copy = welcome_copy(self.welcome_copy_index);
        let greeting = welcome_greeting(self.welcome_period);
        let workspace_name = self.workspace_file_name();
        let prompt = welcome_prompt(&greeting, &copy.action, &copy.workspace_action, workspace_name.as_deref());
        WelcomeHeader { copy, greeting, workspace_name, prompt }
    }

    /// One-click resume rows for recent workspaces, shared by both welcome
    /// empty states. Empty when there are no recents; the folder picker
    /// action stays the single path then. The list scrolls past its max
    /// height (the picker-popover precedent) so a full eight never overflows
    /// short windows; the eyebrow stays fixed above it.
    pub(super) fn recent_workspace_buttons(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let recent = self.recent_workspaces();
        if recent.is_empty() {
            return Vec::new();
        }
        let colors = cx.theme().colors;
        let disabled =
            self.workspace_picker_open || !self.workspace_switch_idle() || self.retained_submission.is_some();
        let mut list = div()
            .id("welcome-recent-list")
            .lock_scroll_axis()
            .test_support()
            .w_full()
            .max_h(px(240.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        for path in &recent {
            let label = workspace_short_name(path).to_string();
            let full_path = path.clone();
            let entry_path = path.clone();
            list = list.child(
                desktop_button(opaque_path_element_id("recent-workspace-welcome", path))
                    .accessibility_label(full_path.clone())
                    .tooltip(full_path)
                    .child(navigation_label(label, IconName::Folder, false, colors.primary))
                    .ghost()
                    .w_full()
                    .desktop_disabled(disabled)
                    .on_click(cx.listener(move |view, _, window, cx| {
                        view.select_workspace_from_ui(PathBuf::from(entry_path.clone()), window, cx);
                    })),
            );
        }
        vec![
            div()
                .w_full()
                .px_2()
                .child(eyebrow_label(&rust_i18n::t!("app.workspace.projects"), &colors))
                .into_any_element(),
            list.into_any_element(),
        ]
    }

    /// "Open a workspace" action shared by both welcome empty states.
    pub(super) fn open_first_workspace_button(&self, call_to_action: String, cx: &mut Context<Self>) -> Button {
        desktop_button("open-first-workspace")
            .label(call_to_action.clone())
            .accessibility_label(call_to_action)
            .primary()
            .desktop_disabled(
                self.workspace_picker_open || !self.workspace_switch_idle() || self.retained_submission.is_some(),
            )
            .on_click(cx.listener(|view, _, window, cx| view.choose_workspace(window, cx)))
    }
}
