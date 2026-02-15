use anyhow::Result;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::hooks::lifecycle::{HookMessage, HookMessageLevel};

pub(super) fn render_hook_messages(
    renderer: &mut AnsiRenderer,
    messages: &[HookMessage],
) -> Result<()> {
    for message in messages {
        let text = message.text.trim();
        if text.is_empty() {
            continue;
        }

        let style = match message.level {
            HookMessageLevel::Info => MessageStyle::Info,
            HookMessageLevel::Warning => MessageStyle::Info,
            HookMessageLevel::Error => MessageStyle::Error,
        };

        renderer.line(style, text)?;
    }

    Ok(())
}
