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
            // Check if code intelligence tool is available
            let tools = ctx.tools.read().await;
            let has_code_intelligence = tools
                .iter()
                .any(|t| t.function.as_ref().map(|f| f.name.as_str()) == Some("code_intelligence"));

            if has_code_intelligence {
                ctx.renderer.line(
                    MessageStyle::Status,
                    "  LSP Status: Active (code intelligence tool available)",
                )?;
            } else {
                ctx.renderer.line(
                    MessageStyle::Info,
                    "  LSP Status: Using tree-sitter-based code analysis",
                )?;
            }
            Ok(SlashCommandControl::Continue)
        }
        LspCommandAction::Detect => {
            ctx.renderer
                .line(MessageStyle::Info, "Detecting LSP servers...")?;
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
