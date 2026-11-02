//! Sanitize the canonical payload before persistence, logging, and display.
pub use vtcode_exec_events::{Diagnostic, DiagnosticAction, DiagnosticSeverity};

/// Apply the existing secret redaction to every externally supplied field.
/// Call before emitting a `ThreadEvent`, not merely when drawing its UI.
pub fn redact_diagnostic(diagnostic: &Diagnostic) -> Diagnostic {
    use crate::sanitizer::redact_secrets;
    let mut redacted = diagnostic.clone();
    redacted.code = redact_secrets(redacted.code);
    redacted.summary = redact_secrets(redacted.summary);
    redacted.agent_details = redact_secrets(redacted.agent_details);
    redacted.correlation_id = redact_secrets(redacted.correlation_id);
    for action in &mut redacted.actions {
        action.id = redact_secrets(std::mem::take(&mut action.id));
        action.label = redact_secrets(std::mem::take(&mut action.label));
    }
    redacted
}

/// Adapt legacy styled messages without dropping evidence after the summary.
/// Correlation identity belongs to the producer; this adapter does not authorize actions.
pub fn legacy_diagnostic(severity: DiagnosticSeverity, text: &str, correlation_id: String) -> Diagnostic {
    let text = crate::sanitizer::redact_secrets(text.to_owned());
    let mut lines = text.trim().splitn(2, '\n');
    let first = lines.next().unwrap_or_default();
    let (summary, details) = if first.len() <= 180 {
        (first.to_owned(), lines.next().unwrap_or_default().to_owned())
    } else {
        let summary = match severity {
            DiagnosticSeverity::Info => "Additional information is available. Expand Details for the full message.",
            DiagnosticSeverity::Warning => "A warning needs attention. Expand Details for the full message.",
            DiagnosticSeverity::Error => "An error needs attention. Expand Details for the full message.",
        };
        (summary.to_owned(), text.trim().to_owned())
    };
    let code = match severity {
        DiagnosticSeverity::Info => "runtime.info",
        DiagnosticSeverity::Warning => "runtime.warning",
        DiagnosticSeverity::Error => "runtime.error",
    };
    Diagnostic {
        severity,
        code: code.into(),
        summary,
        agent_details: details,
        correlation_id,
        actions: Vec::new(),
    }
}

/// Early-startup/headless diagnostic boundary before a thread event sink exists.
/// Formatting and redaction finish before stderr is touched.
pub fn stderr(arguments: std::fmt::Arguments<'_>) {
    let text = arguments.to_string();
    let lower = text.to_ascii_lowercase();
    let severity = if lower.contains("warn") || lower.contains("cancel") {
        DiagnosticSeverity::Warning
    } else {
        DiagnosticSeverity::Error
    };
    let diagnostic = legacy_diagnostic(severity, &text, "startup".into());
    stderr_diagnostic(&diagnostic);
}

/// Render a canonical diagnostic before any terminal UI is available.
pub fn stderr_diagnostic(diagnostic: &Diagnostic) {
    eprintln!("{}", redact_diagnostic(diagnostic).text());
}

/// Session-local observer shared by clones of one inline handle.
pub type DiagnosticSink = std::sync::Arc<dyn Fn(&Diagnostic) -> anyhow::Result<()> + Send + Sync>;

#[derive(Clone, Default)]
pub struct DiagnosticObserver(std::sync::Arc<std::sync::RwLock<DiagnosticObserverState>>);

#[derive(Default)]
struct DiagnosticObserverState {
    sink: Option<DiagnosticSink>,
    recent: std::collections::VecDeque<Diagnostic>,
}

impl std::fmt::Debug for DiagnosticObserver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DiagnosticObserver")
    }
}

impl DiagnosticObserver {
    pub fn set_sink(&self, sink: DiagnosticSink) -> anyhow::Result<()> {
        let mut observer = self
            .0
            .write()
            .map_err(|error| anyhow::anyhow!("diagnostic observer lock poisoned: {error}"))?;
        observer.sink = Some(sink);
        observer.recent.clear();
        Ok(())
    }

    fn notify(&self, diagnostic: &Diagnostic) -> anyhow::Result<()> {
        let sink = {
            let observer = self
                .0
                .read()
                .map_err(|error| anyhow::anyhow!("diagnostic observer lock poisoned: {error}"))?;
            if observer.recent.iter().any(|previous| previous == diagnostic) {
                return Ok(());
            }
            observer.sink.clone()
        };
        let Some(sink) = sink else {
            return Ok(());
        };
        sink(diagnostic)?;
        let mut observer = self
            .0
            .write()
            .map_err(|error| anyhow::anyhow!("diagnostic observer lock poisoned: {error}"))?;
        observer
            .recent
            .retain(|previous| previous.correlation_id != diagnostic.correlation_id);
        if observer.recent.len() == 128 {
            let _ = observer.recent.pop_front();
        }
        observer.recent.push_back(diagnostic.clone());
        Ok(())
    }

    /// Void UI APIs report observer failures visibly and through sanitized telemetry.
    pub fn prepare(&self, diagnostic: &Diagnostic) -> Diagnostic {
        let mut diagnostic = redact_diagnostic(diagnostic);
        let result = self.notify(&diagnostic);
        if let Err(error) = result {
            let message = crate::sanitizer::sanitize_provider_diagnostic(error.to_string());
            tracing::warn!(error = %message, "canonical diagnostic persistence failed");
            if !diagnostic.agent_details.is_empty() {
                diagnostic.agent_details.push_str("\n\n");
            }
            diagnostic
                .agent_details
                .push_str("Diagnostic persistence failed; this message may not be saved.\n");
            diagnostic.agent_details.push_str(&message);
        }
        diagnostic
    }
}

/// Legacy calls represent separate incidents. Producers updating one incident
/// supply an explicit correlation identity through `Diagnostic` instead.
pub fn legacy_ui_diagnostic(severity: DiagnosticSeverity, text: &str) -> Diagnostic {
    legacy_diagnostic(severity, text, format!("legacy-{:032x}", rand::random::<u128>()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_legacy_failures_are_distinct_incidents_but_explicit_incidents_deduplicate() {
        let observer = DiagnosticObserver::default();
        let captured = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = captured.clone();
        observer
            .set_sink(std::sync::Arc::new(move |diagnostic| {
                output.lock().unwrap().push(diagnostic.clone());
                Ok(())
            }))
            .unwrap();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let diagnostic = legacy_ui_diagnostic(DiagnosticSeverity::Warning, "Clipboard unavailable");
            ids.push(observer.prepare(&diagnostic).correlation_id);
        }
        assert_ne!(ids[0], ids[1], "a repeated command failure is a new incident");
        let explicit = legacy_diagnostic(DiagnosticSeverity::Warning, "Retrying", "request-1".into());
        assert_eq!(observer.prepare(&explicit), explicit);
        assert_eq!(observer.prepare(&explicit), explicit);
        assert_eq!(captured.lock().unwrap().len(), 3);
    }
    #[test]
    fn diagnostic_redacts_summary_details_and_actions() {
        let diagnostic = Diagnostic {
            severity: DiagnosticSeverity::Error,
            code: "provider.failure".into(),
            summary: "Authorization: Bearer secret-summary-value".into(),
            agent_details: "password=secret-detail-value".into(),
            correlation_id: "request-2".into(),
            actions: vec![DiagnosticAction {
                id: "retry".into(),
                label: "api_key=secret-action-value".into(),
            }],
        };
        let text = serde_json::to_string(&redact_diagnostic(&diagnostic)).unwrap();
        for secret in ["secret-summary-value", "secret-detail-value", "secret-action-value"] {
            assert!(!text.contains(secret), "leaked {secret}");
        }
        assert!(text.contains("provider.failure"));
        assert!(text.contains("request-2"));
    }
    #[test]
    fn diagnostic_long_message_preserves_complete_command_and_short_message_body() {
        let command =
            format!("Run `/a/very/long/path/{} --exact` to recover.\nExpected: exit zero.", "segment/".repeat(40));
        let diagnostic = legacy_diagnostic(DiagnosticSeverity::Error, &command, "incident-7".into());
        assert_eq!(diagnostic.summary, "An error needs attention. Expand Details for the full message.");
        assert_eq!(diagnostic.agent_details, command);
        let short =
            legacy_diagnostic(DiagnosticSeverity::Warning, "Retry needs input.\nAttempts: 3", "incident-8".into());
        assert_eq!(short.summary, "Retry needs input.");
        assert_eq!(short.agent_details, "Attempts: 3");
    }
}
