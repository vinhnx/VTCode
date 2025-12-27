use std::time::Instant;

use anyhow::Result;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::hooks::lifecycle::{HookMessage, HookMessageLevel};

#[allow(dead_code)]
pub(super) fn safe_force_redraw(handle: &InlineHandle, last_forced_redraw: &mut Instant) {
    if last_forced_redraw.elapsed() > std::time::Duration::from_millis(100) {
        handle.force_redraw();
        *last_forced_redraw = Instant::now();
    }
}

pub(crate) fn render_hook_messages(
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
pub(crate) fn should_trigger_turn_balancer(
    step_count: usize,
    max_tool_loops: usize,
    repeated: usize,
    repeat_limit: usize,
) -> bool {
    step_count > max_tool_loops / 2 && repeated >= repeat_limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balancer_triggers_only_after_halfway_and_repeats() {
        assert!(should_trigger_turn_balancer(11, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(9, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(12, 20, 2, 3));
    }
}
