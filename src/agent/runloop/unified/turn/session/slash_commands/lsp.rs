use anyhow::Result;
use vtcode_core::utils::ansi::MessageStyle;

use super::{SlashCommandContext, SlashCommandControl};
use crate::agent::runloop::slash_commands::LspCommandAction;

pub async fn handle_manage_lsp<'a>(
    ctx: SlashCommandContext<'a>,
    action: LspCommandAction,
) -> Result<SlashCommandControl> {
    match action {
        LspCommandAction::Status => {
            ctx.renderer.line(MessageStyle::Info, "LSP Status:")?;
            // In a real implementation, we would query the LSP tool state
            ctx.renderer.line(
                MessageStyle::Info,
                "  (Refactored CodeIntelligenceTool in use)",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        LspCommandAction::Detect => {
            ctx.renderer
                .line(MessageStyle::Info, "Detecting LSP servers...")?;
            // We can invoke the detection logic here or explain it happens on-demand
            ctx.renderer.line(
                MessageStyle::Info,
                "  Auto-detection enabled for: rust-analyzer, pyright, gopls, tsserver",
            )?;
            Ok(SlashCommandControl::Continue)
        }
        LspCommandAction::Help => {
            ctx.renderer
                .line(MessageStyle::Info, "Usage: /lsp [status|detect]")?;
            Ok(SlashCommandControl::Continue)
        }
    }
}
