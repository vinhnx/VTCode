use std::time::Duration;

use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_tui::InlineHandle;

use vtcode_core::hooks::SessionEndReason;

use super::{SlashCommandContext, SlashCommandControl};

const EXTERNAL_APP_EVENT_LOOP_SETTLE_DELAY: Duration = Duration::from_millis(50);

pub(crate) async fn handle_new_session(
    ctx: SlashCommandContext<'_>,
) -> Result<SlashCommandControl> {
    ctx.renderer
        .line(MessageStyle::Info, "Starting new session...")?;
    Ok(SlashCommandControl::BreakWithReason(
        SessionEndReason::NewSession,
    ))
}

pub(crate) async fn handle_open_docs(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    const DOCS_URL: &str = "https://deepwiki.com/vinhnx/vtcode";
    match webbrowser::open(DOCS_URL) {
        Ok(_) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Opening documentation in browser: {}", DOCS_URL),
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to open browser: {}", err),
            )?;
            ctx.renderer
                .line(MessageStyle::Info, &format!("Please visit: {}", DOCS_URL))?;
        }
    }
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_launch_editor(
    ctx: SlashCommandContext<'_>,
    file: Option<String>,
) -> Result<SlashCommandControl> {
    use std::path::PathBuf;
    use vtcode_core::tools::terminal_app::{EditorLaunchConfig, TerminalAppLauncher};

    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());
    let editor_config = ctx
        .vt_cfg
        .as_ref()
        .map(|config| config.tools.editor.clone())
        .unwrap_or_default();
    if !editor_config.enabled {
        ctx.renderer.line(
            MessageStyle::Warning,
            "External editor is disabled (`tools.editor.enabled = false`).",
        )?;
        ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
        return Ok(SlashCommandControl::Continue);
    }

    ctx.renderer.line(
        MessageStyle::Info,
        if file.is_some() {
            "Launching editor..."
        } else {
            "Launching editor with current input..."
        },
    )?;

    let file_path = file.as_ref().map(|f| {
        let path = PathBuf::from(f);
        if path.is_absolute() {
            path
        } else {
            ctx.config.workspace.join(path)
        }
    });

    let launch_config = EditorLaunchConfig {
        preferred_editor: if editor_config.preferred_editor.trim().is_empty() {
            None
        } else {
            Some(editor_config.preferred_editor.clone())
        },
    };

    let launch_result =
        run_with_event_loop_suspended(ctx.handle, editor_config.suspend_tui, || {
            launcher.launch_editor_with_config(file_path, launch_config)
        })
        .await;

    let (message_style, message) = match launch_result {
        Ok(Some(edited_content)) => {
            ctx.handle.set_input(edited_content);
            (
                MessageStyle::Info,
                "Editor closed. Input updated with edited content.".to_owned(),
            )
        }
        Ok(None) => (MessageStyle::Info, "Editor closed.".to_owned()),
        Err(err) => (
            MessageStyle::Error,
            format!("Failed to launch editor: {}", err),
        ),
    };

    ctx.handle.force_redraw();
    ctx.renderer.line(message_style, &message)?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub(crate) async fn handle_launch_git(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    use vtcode_core::tools::terminal_app::TerminalAppLauncher;

    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());

    ctx.renderer
        .line(MessageStyle::Info, "Launching git interface (lazygit)...")?;

    let (message_style, message) =
        match run_with_event_loop_suspended(ctx.handle, true, || launcher.launch_git_interface())
            .await
        {
            Ok(()) => (MessageStyle::Info, "Git interface closed.".to_owned()),
            Err(err) => (
                MessageStyle::Error,
                format!("Failed to launch git interface: {}", err),
            ),
        };

    ctx.handle.force_redraw();
    ctx.renderer.line(message_style, &message)?;
    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

async fn run_with_event_loop_suspended<T, F>(
    handle: &InlineHandle,
    suspend_tui: bool,
    launch: F,
) -> T
where
    F: FnOnce() -> T,
{
    if suspend_tui {
        handle.suspend_event_loop();
        tokio::time::sleep(EXTERNAL_APP_EVENT_LOOP_SETTLE_DELAY).await;
        handle.clear_input_queue();
    }

    let result = launch();

    if suspend_tui {
        handle.clear_input_queue();
        handle.resume_event_loop();
    }

    result
}
