use anyhow::Result;
use crossterm::style::Stylize;
use std::env;
use vtcode_core::utils::ansi::MessageStyle;

use super::{SlashCommandContext, SlashCommandControl};
use crate::updater::{
    InlineUpdateOutcome, Updater, execute_inline_update, run_inline_update_prompt,
};

fn control_for_update_outcome(outcome: InlineUpdateOutcome) -> SlashCommandControl {
    match outcome {
        InlineUpdateOutcome::Continue => SlashCommandControl::Continue,
        InlineUpdateOutcome::RestartRequested => {
            SlashCommandControl::BreakWithReason(vtcode_core::hooks::SessionEndReason::Completed)
        }
    }
}

pub(crate) async fn handle_update(
    ctx: SlashCommandContext<'_>,
    check_only: bool,
    install: bool,
    force: bool,
) -> Result<SlashCommandControl> {
    let current_version = env!("CARGO_PKG_VERSION");
    let updater = Updater::new(current_version)?;

    ctx.renderer.line(
        MessageStyle::Info,
        &format!("Checking for updates (current: v{})...", current_version),
    )?;

    match updater.check_for_updates().await {
        Ok(Some(update)) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("New version available: v{}", update.version),
            )?;

            if !update.release_notes.trim().is_empty() {
                ctx.renderer.line(MessageStyle::Info, "Release notes:")?;
                for line in update.release_notes.lines().take(8) {
                    ctx.renderer.line(MessageStyle::Output, line)?;
                }
            }

            if check_only || !install {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Run /update install to apply this release.",
                )?;
                let guidance = updater.update_guidance();
                ctx.renderer.line(
                    MessageStyle::Info,
                    &format!("Recommended command: {}", guidance.command().green()),
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            let notice = updater.notice_for_version(update.version.clone());
            run_inline_update_prompt(
                ctx.renderer,
                ctx.handle,
                ctx.session,
                ctx.ctrl_c_state,
                ctx.ctrl_c_notify,
                ctx.config.workspace.as_path(),
                &notice,
            )
            .await
            .map(control_for_update_outcome)
        }
        Ok(None) => {
            ctx.renderer
                .line(MessageStyle::Info, "Already on the latest version.")?;
            if install && force {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "Force reinstall requested; running the current install command.",
                )?;
                let notice = updater.notice_for_version(updater.current_version().clone());
                execute_inline_update(
                    ctx.renderer,
                    ctx.handle,
                    ctx.config.workspace.as_path(),
                    &notice,
                )
                .map(control_for_update_outcome)
            } else {
                Ok(SlashCommandControl::Continue)
            }
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to check updates: {}", err),
            )?;
            Ok(SlashCommandControl::Continue)
        }
    }
}
