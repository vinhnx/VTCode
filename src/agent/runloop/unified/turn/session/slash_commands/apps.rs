use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;
use webbrowser;

use crate::hooks::lifecycle::SessionEndReason;

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_new_session(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    ctx.renderer
        .line(MessageStyle::Info, "Starting new session...")?;
    Ok(SlashCommandControl::BreakWithReason(
        SessionEndReason::NewSession,
    ))
}

pub async fn handle_open_docs(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
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

pub async fn handle_launch_editor(
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

    if editor_config.suspend_tui {
        // Pause event loop to prevent it from reading input while editor is running.
        // This prevents stdin conflicts between the TUI event loop and the external editor.
        ctx.handle.suspend_event_loop();
        // Wait for pause to take effect
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // Drain any queued key events before editor launch.
        ctx.handle.clear_input_queue();
    }

    match launcher.launch_editor_with_config(file_path, launch_config) {
        Ok(Some(edited_content)) => {
            // User edited temp file, replace input with edited content
            ctx.handle.set_input(edited_content);
            ctx.handle.force_redraw(); // Force redraw to clear any artifacts
            ctx.renderer.line(
                MessageStyle::Info,
                "Editor closed. Input updated with edited content.",
            )?;
        }
        Ok(None) => {
            // User edited existing file
            ctx.handle.force_redraw(); // Force redraw to clear any artifacts
            ctx.renderer.line(MessageStyle::Info, "Editor closed.")?;
        }
        Err(err) => {
            ctx.handle.force_redraw(); // Force redraw even on error
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to launch editor: {}", err),
            )?;
        }
    }

    if editor_config.suspend_tui {
        // Clear any stale terminal events that might have been buffered around editor exit.
        ctx.handle.clear_input_queue();
        // Resume event loop to process input again
        ctx.handle.resume_event_loop();
    }

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_launch_git(ctx: SlashCommandContext<'_>) -> Result<SlashCommandControl> {
    use vtcode_core::tools::terminal_app::TerminalAppLauncher;

    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());

    ctx.renderer
        .line(MessageStyle::Info, "Launching git interface (lazygit)...")?;

    // Suspend TUI event loop to prevent input stealing
    ctx.handle.suspend_event_loop();
    // Give a small moment for the suspend command to propagate to the TUI thread
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    match launcher.launch_git_interface() {
        Ok(_) => {
            ctx.handle.force_redraw(); // Force redraw to clear any artifacts
            ctx.renderer
                .line(MessageStyle::Info, "Git interface closed.")?;
        }
        Err(err) => {
            ctx.handle.force_redraw(); // Force redraw even on error
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to launch git interface: {}", err),
            )?;
        }
    }

    // Resume TUI event loop
    ctx.handle.resume_event_loop();

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}
