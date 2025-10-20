use std::borrow::Cow;

use anyhow::{Error, Result};

/// Formats an error into a user-facing description. This allows extracted
/// components to present consistent error messaging without depending on the
/// CLI presentation layer.
pub trait ErrorFormatter: Send + Sync {
    /// Render the error into a user-facing string.
    fn format_error(&self, error: &Error) -> Cow<'_, str>;
}

/// Reports non-fatal errors to an observability backend.
pub trait ErrorReporter: Send + Sync {
    /// Capture the provided error for later inspection.
    fn capture(&self, error: &Error) -> Result<()>;

    /// Convenience helper to capture a simple message.
    fn capture_message(&self, message: impl Into<Cow<'static, str>>) -> Result<()> {
        let message: Cow<'static, str> = message.into();
        self.capture(&Error::msg(message))
    }
}

/// Error reporting implementation that drops every event. Useful for tests or
/// when a consumer does not yet integrate with error monitoring.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopErrorReporter;

impl ErrorReporter for NoopErrorReporter {
    fn capture(&self, _error: &Error) -> Result<()> {
        Ok(())
    }
}

/// Default formatter that surfaces the error's display output.
#[derive(Debug, Default, Clone, Copy)]
pub struct DisplayErrorFormatter;

impl ErrorFormatter for DisplayErrorFormatter {
    fn format_error(&self, error: &Error) -> Cow<'_, str> {
        Cow::Owned(format!("{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatter_uses_display() {
        let formatter = DisplayErrorFormatter;
        let error = Error::msg("test error");
        assert_eq!(formatter.format_error(&error), "test error");
    }

    #[test]
    fn noop_reporter_drops_errors() {
        let reporter = NoopErrorReporter;
        let error = Error::msg("test");
        assert!(reporter.capture(&error).is_ok());
        assert!(reporter.capture_message("message").is_ok());
    }
}
