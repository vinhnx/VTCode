//! Operational diagnostics shared by runtime events, agents, and human surfaces.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct DiagnosticAction {
    /// Stable input action; displaying it does not execute it.
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema-export", derive(schemars::JsonSchema))]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    /// Stable machine-readable incident classification.
    pub code: String,
    /// Concise human-facing summary, always displayed.
    pub summary: String,
    /// Operational evidence only. Never include private model reasoning.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent_details: String,
    /// Stable incident identity; updates replace the existing presentation.
    pub correlation_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub actions: Vec<DiagnosticAction>,
}

impl Diagnostic {
    /// Plain-text representation for headless consumers and transcript exports.
    pub fn text(&self) -> String {
        let mut text = self.summary.clone();
        if !self.actions.is_empty() {
            text.push_str("\nAvailable actions: ");
            text.push_str(
                &self
                    .actions
                    .iter()
                    .map(|action| action.label.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        if !self.agent_details.is_empty() {
            text.push_str("\nDetails:\n");
            text.push_str(&self.agent_details);
        }
        text
    }
}

/// Latest durable diagnostic per incident, ordered by its most recent transition.
/// Feed this once during session resume; live turns use the existing event sink.
#[derive(Default)]
pub struct DiagnosticProjection {
    latest: std::collections::BTreeMap<String, (u64, Diagnostic)>,
    sequence: u64,
}

impl DiagnosticProjection {
    pub fn observe(&mut self, event: &crate::ThreadEvent) {
        use crate::{ThreadEvent, ThreadItemDetails};
        let item = match event {
            ThreadEvent::ItemStarted(event) => &event.item,
            ThreadEvent::ItemUpdated(event) => &event.item,
            ThreadEvent::ItemCompleted(event) => &event.item,
            _ => return,
        };
        let ThreadItemDetails::Reasoning(reasoning) = &item.details else {
            return;
        };
        let Some(diagnostic) = &reasoning.diagnostic else {
            return;
        };
        let _ = self
            .latest
            .insert(diagnostic.correlation_id.clone(), (self.sequence, diagnostic.as_ref().clone()));
        self.sequence = self.sequence.saturating_add(1);
    }

    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        let mut ordered: Vec<_> = self.latest.into_values().collect();
        ordered.sort_by_key(|(sequence, _)| *sequence);
        ordered.into_iter().map(|(_, diagnostic)| diagnostic).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ReasoningItem, ThreadEvent};

    #[test]
    fn diagnostic_projection_keeps_latest_incidents_in_transition_order() {
        use crate::{ItemCompletedEvent, ThreadItem, ThreadItemDetails};
        let make = |id: &str, summary: &str| {
            ThreadEvent::ItemCompleted(ItemCompletedEvent {
                item: ThreadItem {
                    id: id.into(),
                    context: None,
                    details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
                        text: summary.into(),
                        stage: Some("diagnosis".into()),
                        diagnostic: Some(Box::new(Diagnostic {
                            severity: DiagnosticSeverity::Warning,
                            code: "provider.retrying".into(),
                            summary: summary.into(),
                            agent_details: format!("evidence for {summary}"),
                            correlation_id: id.into(),
                            actions: Vec::new(),
                        })),
                    })),
                },
            })
        };
        let mut projection = DiagnosticProjection::default();
        projection.observe(&make("request-a", "first"));
        projection.observe(&make("request-b", "second"));
        projection.observe(&make("request-a", "third"));
        let diagnostics = projection.into_diagnostics();
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].correlation_id, "request-b");
        assert_eq!(diagnostics[1].summary, "third");
        assert_eq!(diagnostics[1].agent_details, "evidence for third");
    }

    #[test]
    fn diagnostic_legacy_reasoning_remains_readable() {
        let old: ReasoningItem = serde_json::from_str(r#"{"text":"saved diagnosis","stage":"diagnosis"}"#).unwrap();
        assert!(old.diagnostic.is_none());
        let diagnostic = Diagnostic {
            severity: DiagnosticSeverity::Warning,
            code: "planning.awaiting_input".into(),
            summary: "Planning needs your input.".into(),
            agent_details: "attempt=3; remaining=0".into(),
            correlation_id: "plan-7".into(),
            actions: vec![DiagnosticAction {
                id: "retry".into(),
                label: "Retry from retained evidence".into(),
            }],
        };
        let item = ReasoningItem {
            text: diagnostic.summary.clone(),
            stage: Some("diagnosis".into()),
            diagnostic: Some(Box::new(diagnostic.clone())),
        };
        assert_eq!(serde_json::from_value::<ReasoningItem>(serde_json::to_value(&item).unwrap()).unwrap(), item);
        assert!(diagnostic.text().contains("Details:\nattempt=3; remaining=0"));
        assert!(size_of::<ThreadEvent>() <= 80);
    }
}
