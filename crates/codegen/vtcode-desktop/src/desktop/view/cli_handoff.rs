//! CLI handoff card: continue the selected thread in a terminal as an
//! independent fork.
//!
//! One shared action behind the thread-header button, `/cli`, and the ⌘K
//! palette entry. The command comes from the settings terminal row builder so
//! both surfaces can never disagree. Terminal.app runs it automatically via a
//! `.command` script; anything else opens at the workspace with the command
//! copied for pasting.
use super::*;

/// Hints under the fork command. English literals: the repo does not add
/// locale keys without full native-quality translations, and the command
/// itself is locale-independent.
const HANDOFF_AUTO_HINT: &str =
    "Continues this thread in Terminal as an independent copy; the command is also copied for pasting elsewhere.";
const HANDOFF_PASTE_HINT: &str =
    "Paste in any terminal to continue this thread as an independent copy. Opening a terminal copies it automatically.";
const HANDOFF_RUNNING_SUFFIX: &str = " A turn is running; the copy includes only completed turns.";

/// Hint for the chosen terminal: auto-run terminals promise execution,
/// the rest promise copy+open. A running turn discloses that the fork is
/// cut from the last archive, not the in-flight turn.
fn handoff_hint(terminal: preferences::CliTerminal, running: bool) -> String {
    let base = if files::terminal_runs_handoff_script(terminal.external_editor()) {
        HANDOFF_AUTO_HINT
    } else {
        HANDOFF_PASTE_HINT
    };
    if running {
        format!("{base}{HANDOFF_RUNNING_SUFFIX}")
    } else {
        base.to_string()
    }
}
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

    /// Run the fork command in Terminal via an auto-run `.command` script,
    /// falling back to copy+open when the script cannot be built (unresolved
    /// binary, non-UTF-8 paths) or the terminal cannot run scripts. The
    /// clipboard always receives the command first so a failed auto-run still
    /// leaves a working paste. A missing workspace is a silent no-op
    /// (unreachable: a selection implies a workspace); a missing selection
    /// reports through the refresh.
    pub(super) fn open_cli_handoff_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The selection may have changed since the card opened: refresh so the
        // copied command always names the current thread.
        self.open_cli_handoff(cx);
        let (Some(handoff), Some(workspace)) = (self.cli_handoff.clone(), self.workspace.clone()) else {
            return;
        };
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(handoff.command));
        super::super::notifications::copied_to_clipboard(window, cx);
        let terminal = self.preferences.cli_terminal().external_editor();
        // Auto-run needs a resolvable binary, encodable paths, and a terminal
        // verified to execute `.command` scripts; anything else degrades to
        // opening the workspace with the command already copied.
        let script = handoff_vtcode_binary()
            .ok()
            .and_then(|binary| files::build_handoff_script(&binary, &workspace, &handoff.session_id))
            .filter(|_| files::terminal_runs_handoff_script(terminal));
        #[cfg(test)]
        if self.test_launch_intercept.is_some() {
            let result = match script {
                Some(content) => self.intercept_test_handoff_script(content, terminal),
                None => self.intercept_test_launch(".", terminal),
            };
            // The hook is set, so the matching arm recorded and answered.
            if let Some(result) = result {
                self.apply_external_launch_result(&workspace, result, cx);
            }
            return;
        }
        self.external_launch_task = Some(cx.spawn_in(window, async move |view, cx| {
            let root = workspace.clone();
            let result = cx
                .background_spawn(async move {
                    match script {
                        Some(content) => files::write_handoff_script(&content)
                            .and_then(|path| files::open_script_in_terminal(&path, terminal)),
                        None => files::open_terminal_at(&root),
                    }
                })
                .await;
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
        let hint = handoff_hint(self.preferences.cli_terminal(), self.running);
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
            .child(
                div()
                    .id("cli-handoff-hint")
                    .test_support()
                    .role(gpui_kit::Role::Label)
                    .aria_label(hint.clone())
                    .child(hint),
            )
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
