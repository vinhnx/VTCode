//! Bridge between messages and tool executions
//!
//! Links LLM messages to their tool executions and tracks intent fulfillment.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::result_metadata::EnhancedToolResult;

/// Tracks intent fulfillment
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum IntentFulfillment {
    /// Message goal completely achieved
    Fulfilled,

    /// Message goal partially achieved
    PartiallyFulfilled,

    /// Tools executed but results inconclusive
    Attempted,

    /// Tools failed or results contradicted intent
    Failed,
}

impl IntentFulfillment {
    pub fn to_string(&self) -> String {
        match self {
            Self::Fulfilled => "fulfilled".to_string(),
            Self::PartiallyFulfilled => "partially_fulfilled".to_string(),
            Self::Attempted => "attempted".to_string(),
            Self::Failed => "failed".to_string(),
        }
    }
}

/// Tool execution record tied to message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecution {
    pub tool_name: String,
    pub args: Value,
    pub result: EnhancedToolResult,
    pub duration_ms: u64,

    /// Did this tool help fulfill the intent?
    pub contributed_to_intent: bool,
}

/// Stated intent extracted from message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolIntent {
    Search(String),
    Execute(String),
    Analyze(String),
    Modify(String),
}

impl ToolIntent {
    pub fn to_string(&self) -> String {
        match self {
            Self::Search(s) => format!("search: {}", s),
            Self::Execute(s) => format!("execute: {}", s),
            Self::Analyze(s) => format!("analyze: {}", s),
            Self::Modify(s) => format!("modify: {}", s),
        }
    }
}

/// Correlation between message intent and tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageToolCorrelation {
    /// Unique message identifier
    pub message_id: String,

    /// Extracted intent from message
    pub stated_intent: ToolIntent,

    /// Original message text
    pub message_text: String,

    /// Tools executed to fulfill this message
    pub tool_executions: Vec<ToolExecution>,

    /// Overall success of fulfilling stated intent
    pub intent_fulfillment: IntentFulfillment,

    /// Confidence in fulfillment assessment (0.0-1.0)
    pub confidence: f32,

    /// Any issues encountered
    pub issues: Vec<String>,
}

impl MessageToolCorrelation {
    pub fn new(message_id: String, message_text: String, intent: ToolIntent) -> Self {
        Self {
            message_id,
            stated_intent: intent,
            message_text,
            tool_executions: vec![],
            intent_fulfillment: IntentFulfillment::Attempted,
            confidence: 0.0,
            issues: vec![],
        }
    }

    /// Add a tool execution
    pub fn add_execution(&mut self, execution: ToolExecution) {
        self.tool_executions.push(execution);
        self.reassess_fulfillment();
    }

    /// Add an issue
    pub fn add_issue(&mut self, issue: String) {
        self.issues.push(issue);
        self.reassess_fulfillment();
    }

    /// Reassess whether intent was fulfilled
    fn reassess_fulfillment(&mut self) {
        if self.tool_executions.is_empty() {
            self.intent_fulfillment = IntentFulfillment::Failed;
            self.confidence = 0.0;
            return;
        }

        // Count contributing executions
        let contributing = self
            .tool_executions
            .iter()
            .filter(|e| e.contributed_to_intent)
            .count();

        let avg_quality = self
            .tool_executions
            .iter()
            .map(|e| e.result.metadata.quality_score())
            .sum::<f32>()
            / self.tool_executions.len() as f32;

        self.intent_fulfillment = match (contributing, avg_quality) {
            (n, q) if n == self.tool_executions.len() && q > 0.75 => IntentFulfillment::Fulfilled,
            (n, q) if n > self.tool_executions.len() / 2 && q > 0.6 => {
                IntentFulfillment::PartiallyFulfilled
            }
            (0, _) => IntentFulfillment::Failed,
            _ => IntentFulfillment::Attempted,
        };

        self.confidence = (contributing as f32 / self.tool_executions.len() as f32) * avg_quality;
    }

    /// Get summary of tool execution
    pub fn summary(&self) -> String {
        format!(
            "Intent: {} | Tools: {} | Fulfillment: {} (confidence: {:.0}%)",
            self.stated_intent.to_string(),
            self.tool_executions
                .iter()
                .map(|e| e.tool_name.clone())
                .collect::<Vec<_>>()
                .join(", "),
            self.intent_fulfillment.to_string(),
            self.confidence * 100.0
        )
    }
}

/// Extractor for tool intents from messages
pub struct ToolIntentExtractor;

impl ToolIntentExtractor {
    /// Extract intent from message text
    pub fn extract(text: &str) -> Option<ToolIntent> {
        let text_lower = text.to_lowercase();

        // Search patterns
        if let Some(intent) = extract_search_intent(&text_lower) {
            return Some(intent);
        }

        // Execute patterns
        if let Some(intent) = extract_execute_intent(&text_lower) {
            return Some(intent);
        }

        // Analyze patterns
        if let Some(intent) = extract_analyze_intent(&text_lower) {
            return Some(intent);
        }

        // Modify patterns
        if let Some(intent) = extract_modify_intent(&text_lower) {
            return Some(intent);
        }

        None
    }
}

/// Extract search intent
fn extract_search_intent(text: &str) -> Option<ToolIntent> {
    let search_keywords = [
        "grep", "search", "find", "look for", "locate", "check if", "does", "exist",
    ];

    for keyword in &search_keywords {
        if text.contains(keyword) {
            // Try to extract what we're searching for
            if let Some(pattern) = extract_quoted_string(text) {
                return Some(ToolIntent::Search(pattern));
            }

            // Fallback: use keyword
            return Some(ToolIntent::Search(keyword.to_string()));
        }
    }

    None
}

/// Extract execute intent
fn extract_execute_intent(text: &str) -> Option<ToolIntent> {
    let execute_keywords = [
        "run", "execute", "command", "cargo", "npm", "python", "bash", "sh",
    ];

    for keyword in &execute_keywords {
        if text.contains(keyword) {
            // Try to extract command
            if let Some(cmd) = extract_quoted_string(text) {
                return Some(ToolIntent::Execute(cmd));
            }

            return Some(ToolIntent::Execute(keyword.to_string()));
        }
    }

    None
}

/// Extract analyze intent
fn extract_analyze_intent(text: &str) -> Option<ToolIntent> {
    let analyze_keywords = ["analyze", "check", "review", "examine", "inspect", "parse"];

    for keyword in &analyze_keywords {
        if text.contains(keyword) {
            if let Some(target) = extract_quoted_string(text) {
                return Some(ToolIntent::Analyze(target));
            }

            return Some(ToolIntent::Analyze(keyword.to_string()));
        }
    }

    None
}

/// Extract modify intent
fn extract_modify_intent(text: &str) -> Option<ToolIntent> {
    let modify_keywords = ["edit", "modify", "change", "fix", "apply", "patch"];

    for keyword in &modify_keywords {
        if text.contains(keyword) {
            if let Some(target) = extract_quoted_string(text) {
                return Some(ToolIntent::Modify(target));
            }

            return Some(ToolIntent::Modify(keyword.to_string()));
        }
    }

    None
}

/// Extract quoted string from text
fn extract_quoted_string(text: &str) -> Option<String> {
    // Look for "quoted" or 'quoted' strings
    let mut in_quote = false;
    let mut quote_char = ' ';
    let mut current = String::new();

    for c in text.chars() {
        match c {
            '"' | '\'' if !in_quote => {
                in_quote = true;
                quote_char = c;
            }
            c if in_quote && c == quote_char => {
                in_quote = false;
                if !current.is_empty() {
                    return Some(current);
                }
            }
            c if in_quote => {
                current.push(c);
            }
            _ => {}
        }
    }

    None
}

/// Track correlations across a session
pub struct MessageCorrelationTracker {
    correlations: Vec<MessageToolCorrelation>,
}

impl MessageCorrelationTracker {
    pub fn new() -> Self {
        Self {
            correlations: vec![],
        }
    }

    /// Add a correlation
    pub fn add(&mut self, correlation: MessageToolCorrelation) {
        self.correlations.push(correlation);
    }

    /// Get all correlations
    pub fn all(&self) -> &[MessageToolCorrelation] {
        &self.correlations
    }

    /// Get unfulfilled intents
    pub fn unfulfilled(&self) -> Vec<&MessageToolCorrelation> {
        self.correlations
            .iter()
            .filter(|c| c.intent_fulfillment == IntentFulfillment::Failed)
            .collect()
    }

    /// Get fulfillment statistics
    pub fn stats(&self) -> CorrelationStats {
        let total = self.correlations.len();
        let fulfilled = self
            .correlations
            .iter()
            .filter(|c| c.intent_fulfillment == IntentFulfillment::Fulfilled)
            .count();
        let partially_fulfilled = self
            .correlations
            .iter()
            .filter(|c| c.intent_fulfillment == IntentFulfillment::PartiallyFulfilled)
            .count();
        let failed = self
            .correlations
            .iter()
            .filter(|c| c.intent_fulfillment == IntentFulfillment::Failed)
            .count();

        let avg_confidence = if total > 0 {
            self.correlations.iter().map(|c| c.confidence).sum::<f32>() / total as f32
        } else {
            0.0
        };

        CorrelationStats {
            total,
            fulfilled,
            partially_fulfilled,
            attempted: total - fulfilled - partially_fulfilled - failed,
            failed,
            avg_confidence,
        }
    }
}

impl Default for MessageCorrelationTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationStats {
    pub total: usize,
    pub fulfilled: usize,
    pub partially_fulfilled: usize,
    pub attempted: usize,
    pub failed: usize,
    pub avg_confidence: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::result_metadata::ResultMetadata;

    #[test]
    fn test_intent_extraction_search() {
        let text = "Let me grep for 'error' in the logs";
        let intent = ToolIntentExtractor::extract(text);

        assert!(matches!(intent, Some(ToolIntent::Search(_))));
    }

    #[test]
    fn test_intent_extraction_execute() {
        let text = "Run 'cargo test' to check";
        let intent = ToolIntentExtractor::extract(text);

        assert!(matches!(intent, Some(ToolIntent::Execute(_))));
    }

    #[test]
    fn test_intent_extraction_analyze() {
        let text = "Analyze the config file please";
        let intent = ToolIntentExtractor::extract(text);

        assert!(matches!(intent, Some(ToolIntent::Analyze(_))));
    }

    #[test]
    fn test_message_correlation() {
        let mut corr = MessageToolCorrelation::new(
            "msg-1".to_string(),
            "Let me grep for errors".to_string(),
            ToolIntent::Search("errors".to_string()),
        );

        let exec = ToolExecution {
            tool_name: "grep_file".to_string(),
            args: Value::Null,
            result: EnhancedToolResult::new(
                Value::Null,
                ResultMetadata::success(0.9, 0.9),
                "grep_file".to_string(),
            ),
            duration_ms: 100,
            contributed_to_intent: true,
        };

        corr.add_execution(exec);

        assert!(matches!(
            corr.intent_fulfillment,
            IntentFulfillment::Fulfilled
        ));
    }

    #[test]
    fn test_correlation_tracker() {
        let mut tracker = MessageCorrelationTracker::new();

        let corr = MessageToolCorrelation::new(
            "msg-1".to_string(),
            "test".to_string(),
            ToolIntent::Search("test".to_string()),
        );

        tracker.add(corr);

        let stats = tracker.stats();
        assert_eq!(stats.total, 1);
    }

    #[test]
    fn test_extract_quoted_string() {
        assert_eq!(
            extract_quoted_string("grep for \"error pattern\""),
            Some("error pattern".to_string())
        );
        assert_eq!(
            extract_quoted_string("find 'test.rs'"),
            Some("test.rs".to_string())
        );
    }
}
