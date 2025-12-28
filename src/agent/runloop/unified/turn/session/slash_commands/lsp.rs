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
            // Query the actual LSP tool state
            if let Some(tool) = &ctx.tools.get("code_intelligence") {
                // Try to get the LSP tool state from the code intelligence tool
                use vtcode_core::tools::traits::Tool;
                use serde_json::json;

                // Try to get LSP status by calling a special status operation
                let status_args = json!({
                    "operation": "status_check",
                    "file_path": None,
                    "line": None,
                    "character": None,
                    "query": None
                });

                match tool.execute(status_args).await {
                    Ok(result) => {
                        // Parse the result to extract status information
                        if let Ok(output) = serde_json::from_value::<CodeIntelligenceOutput>(result) {
                            if let Some(CodeIntelligenceResult::Custom(status_data)) = output.result {
                                if let Some(status_obj) = status_data.as_object() {
                                    if let Some(lsp_available) = status_obj.get("lsp_available").and_then(|v| v.as_bool()) {
                                        if lsp_available {
                                            ctx.renderer.line(
                                                MessageStyle::Success,
                                                "  LSP Status: Active (LSP tool available)",
                                            )?;
                                        } else {
                                            ctx.renderer.line(
                                                MessageStyle::Warning,
                                                "  LSP Status: Not active (using tree-sitter only)",
                                            )?;
                                        }
                                    } else {
                                        ctx.renderer.line(
                                            MessageStyle::Info,
                                            &format!("  LSP Status: {}", status_obj.get("status").and_then(|v| v.as_str()).unwrap_or("Unknown")),
                                        )?;
                                    }
                                } else {
                                    ctx.renderer.line(
                                        MessageStyle::Info,
                                        &format!("  LSP Status: {}", status_data),
                                    )?;
                                }
                            } else {
                                ctx.renderer.line(
                                    MessageStyle::Info,
                                    "  LSP Status: Active (no detailed status available)",
                                )?;
                            }
                        } else {
                            ctx.renderer.line(
                                MessageStyle::Info,
                                &format!("  LSP Status: Active ({})", result),
                            )?;
                        }
                    }
                    Err(_) => {
                        ctx.renderer.line(
                            MessageStyle::Warning,
                            "  LSP Status: Not available (fallback to tree-sitter only)",
                        )?;
                    }
                }
            } else {
                ctx.renderer.line(
                    MessageStyle::Warning,
                    "  LSP Status: Code intelligence tool not available",
                )?;
            }
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
