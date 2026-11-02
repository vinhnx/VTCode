//! CLI handoff card: continue the selected thread via the terminal resume command.
//!
//! One shared action behind the thread-header button, `/cli`, and the ⌘K
//! palette entry. The command comes from the settings terminal row builder so
//! both surfaces can never disagree; opening a terminal copies it first so
//! the user only pastes.
use super::*;

/// Hint under the resume command. English literal: the repo does not add
/// locale keys without full native-quality translations, and the command
/// itself is locale-independent.
const HANDOFF_HINT: &str =
    "Paste in any terminal to continue this thread in the CLI. Opening a terminal copies it automatically.";
const NO_SESSION_ERROR: &str = "Select a thread first to continue it in the CLI.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CliHandoff {
    pub(super) session_id: String,
    pub(super) command: String,
}

impl DesktopView {
    /// Open the handoff card for the selected thread. Idempotent: reopening
    /// refreshes the command when the selection changed.
    pub(super) fn open_cli_handoff(&mut self, cx: &mut Context<Self>) {
        let Some(session_id) = self.selected_session_id.clone() else {
            self.cli_handoff = None;
            self.error = Some(NO_SESSION_ERROR.to_string());
            cx.notify();
            return;
        };
        let command = Self::terminal_command_for_session(Some(&session_id));
        self.cli_handoff = Some(CliHandoff { session_id, command });
        cx.notify();
    }

    pub(super) fn dismiss_cli_handoff(&mut self, cx: &mut Context<Self>) {
        self.cli_handoff = None;
        cx.notify();
    }

    /// Adjust the service slash catalog for this client after every refresh:
    /// append the desktop-only `/cli` (the CLI has no such command, so the
    /// service never sends it) and drop `/desktop` (natively intercepted as
    /// an already-here notice; suggesting it would promise a second window).
    /// Descriptions stay English like the service set.
    pub(super) fn localize_slash_catalog(&mut self) {
        self.slash_commands.retain(|command| command.name != "desktop");
        if !self.slash_commands.iter().any(|command| command.name == "cli") {
            self.slash_commands.push(protocol::SlashCommandInfo {
                name: "cli".to_string(),
                description: "Continue the selected thread in the CLI".to_string(),
            });
        }
    }

    /// Copy the resume command and open Terminal at the workspace, mirroring
    /// the external-editor launch (intercepted in tests, shared completion).
    /// A missing workspace is a silent no-op (unreachable: a selection
    /// implies a workspace); a missing selection reports through the refresh.
    pub(super) fn open_cli_handoff_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The selection may have changed since the card opened: refresh so the
        // copied command always names the current thread.
        self.open_cli_handoff(cx);
        let (Some(handoff), Some(workspace)) = (self.cli_handoff.clone(), self.workspace.clone()) else {
            return;
        };
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(handoff.command));
        super::super::notifications::copied_to_clipboard(window, cx);
        #[cfg(test)]
        if let Some(result) = self.intercept_test_launch(".", files::ExternalEditor::Terminal) {
            self.apply_external_launch_result(&workspace, result, cx);
            return;
        }
        self.external_launch_task = Some(cx.spawn_in(window, async move |view, cx| {
            let root = workspace.clone();
            let result = cx.background_spawn(async move { files::open_terminal_at(&root) }).await;
            let _updated = view.update_in(cx, |view, _, cx| {
                view.apply_external_launch_result(&workspace, result, cx);
            });
        }));
    }

    /// Composer card mirroring the retained-submission panel: title, command,
    /// hint, and copy / open-terminal / dismiss actions.
    pub(super) fn render_cli_handoff_card(&self, handoff: &CliHandoff, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.theme().colors;
        let copy_command = handoff.command.clone();
        div()
            .w_full()
            .rounded_md()
            .border_1()
            .border_color(themes::hairline(colors.popover, colors.foreground))
            .bg(colors.popover)
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(row_title(rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string(), &colors))
            .child(div().id("cli-handoff-command").test_support().child(handoff.command.clone()))
            .child(HANDOFF_HINT.to_string())
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        desktop_button("cli-handoff-terminal")
                            .label(rust_i18n::t!(i18n::SETTINGS_CONNECTIONS_TERMINAL).to_string())
                            .on_click(cx.listener(|view, _, window, cx| view.open_cli_handoff_terminal(window, cx))),
                    )
                    .child(
                        desktop_button("cli-handoff-copy")
                            .label(rust_i18n::t!(i18n::FILE_PREVIEW_COPY).to_string())
                            .ghost()
                            .on_click(move |_, window, cx| {
                                cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(copy_command.clone()));
                                super::super::notifications::copied_to_clipboard(window, cx);
                            }),
                    )
                    .child(
                        desktop_button("cli-handoff-dismiss")
                            .label(rust_i18n::t!("app.composer.dismiss").to_string())
                            .ghost()
                            .on_click(cx.listener(|view, _, _, cx| view.dismiss_cli_handoff(cx))),
                    ),
            )
    }
}
