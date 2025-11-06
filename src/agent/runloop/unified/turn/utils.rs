use std::time::Instant;

use anyhow::Result;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::hooks::lifecycle::{HookMessage, HookMessageLevel};

pub(super) fn safe_force_redraw(handle: &InlineHandle, last_forced_redraw: &mut Instant) {
    if last_forced_redraw.elapsed() > std::time::Duration::from_millis(100) {
        handle.force_redraw();
        *last_forced_redraw = Instant::now();
    }
}

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
