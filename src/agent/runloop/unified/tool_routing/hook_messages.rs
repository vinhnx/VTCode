use anyhow::Result;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use vtcode_core::hooks::{HookMessage, HookMessageLevel};

pub(super) fn render_hook_messages(renderer: &mut AnsiRenderer, messages: &[HookMessage]) -> Result<()> {
    for message in messages {
        let text = message.text.trim();
        if text.is_empty() {
            continue;
        }

        let style = match message.level {
            HookMessageLevel::Info => MessageStyle::Info,
            HookMessageLevel::Warning => MessageStyle::Warning,
            HookMessageLevel::Error => MessageStyle::Error,
        };

        // Rendering is cosmetic; a single failed render must not abort the
        // hook phase (which would discard every hook's messages and block the
        // tool call). Log and continue.
        if let Err(err) = renderer.line(style, text) {
            tracing::warn!(error = %err, "Failed to render lifecycle hook message");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hook_warning_keeps_warning_severity_and_redacts_operational_details() {
        use vtcode_ui::tui::app::{InlineCommand, InlineHandle};
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let mut renderer = AnsiRenderer::with_inline_ui(InlineHandle::new_for_tests(sender), Default::default());
        render_hook_messages(
            &mut renderer,
            &[HookMessage::warning(
                "Hook needs attention\npassword=hook-private-secret",
            )],
        )
        .unwrap();
        let InlineCommand::Diagnostic(diagnostic) = receiver.try_recv().unwrap() else {
            panic!("canonical hook warning");
        };
        assert_eq!(diagnostic.severity, vtcode_commons::diagnostics::DiagnosticSeverity::Warning);
        assert_eq!(diagnostic.summary, "Hook needs attention");
        assert!(!diagnostic.agent_details.contains("hook-private-secret"));
    }
}
