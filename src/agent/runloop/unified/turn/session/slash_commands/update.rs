use anyhow::Result;
use crossterm::style::Stylize;
use std::env;
use vtcode_core::utils::ansi::MessageStyle;

use super::{SlashCommandContext, SlashCommandControl};
use vtcode::updater::{InstallOutcome, Updater};

pub async fn handle_update(
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
                    &format!("Recommended command: {}", guidance.command.as_str().green()),
                )?;
                return Ok(SlashCommandControl::Continue);
            }

            install_update(ctx, &updater, force).await
        }
        Ok(None) => {
            ctx.renderer
                .line(MessageStyle::Info, "Already on the latest version.")?;
            if install && force {
                install_update(ctx, &updater, true).await
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

async fn install_update(
    ctx: SlashCommandContext<'_>,
    updater: &Updater,
    force: bool,
) -> Result<SlashCommandControl> {
    let guidance = updater.update_guidance();
    if guidance.source.is_managed() {
        ctx.renderer.line(
            MessageStyle::Warning,
            &format!("Managed install detected ({}).", guidance.source.label()),
        )?;
        ctx.renderer.line(
            MessageStyle::Info,
            &format!("Update with: {}", guidance.command.as_str().green()),
        )?;
        return Ok(SlashCommandControl::Continue);
    }

    ctx.renderer
        .line(MessageStyle::Info, "Installing update now...")?;

    match updater.install_update(force).await {
        Ok(InstallOutcome::Updated(version)) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!(
                    "Updated successfully to {}. Restart VT Code.",
                    version.bold()
                ),
            )?;
        }
        Ok(InstallOutcome::UpToDate(version)) => {
            ctx.renderer.line(
                MessageStyle::Info,
                &format!("Already up to date ({})", version.bold()),
            )?;
        }
        Err(err) => {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Failed to install update: {}", err),
            )?;
        }
    }

    Ok(SlashCommandControl::Continue)
}
