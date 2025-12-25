use anyhow::{Result, Context};
use std::path::PathBuf;
use vtcode_core::tools::terminal_app::TerminalAppLauncher;
use vtcode_core::utils::ansi::MessageStyle;

use super::{SlashCommandContext, SlashCommandControl};

pub async fn handle_launch_editor(
    ctx: &SlashCommandContext<'_>,
    path: Option<String>,
) -> Result<SlashCommandControl> {
    let launcher = TerminalAppLauncher::new(ctx.config.workspace.clone());

    let file_path = path.map(|path| {
        if PathBuf::from(&path).is_absolute() {
            PathBuf::from(path)
        } else {
            ctx.config.workspace.join(path)
        }
    });

    // Pause event loop to prevent it from reading input while editor is running.
    // This prevents stdin conflicts between the TUI event loop and the external editor.
    ctx.handle.suspend_event_loop();
    // Wait for pause to take effect. The event loop polls every 16ms, and might be
    // in the middle of a poll when we send the suspend message. Wait a bit longer
    // to ensure the pause flag is checked before the editor launches.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    match launcher.launch_editor(file_path) {
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

    // Resume event loop to process input again
    ctx.handle.resume_event_loop();

    ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
    Ok(SlashCommandControl::Continue)
}

pub async fn handle_launch_git(ctx: &SlashCommandContext<'_>) -> Result<SlashCommandControl> {
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
