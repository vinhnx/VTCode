use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Represents a single decision made by the agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    pub timestamp: u64,
    pub context: DecisionContext,
    pub reasoning: String,
    pub action: Action,
    pub outcome: Option<DecisionOutcome>,
    pub confidence_score: Option<f64>,
}

/// Context information that led to a decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionContext {
    pub conversation_turn: usize,
    pub user_input: Option<String>,
    pub previous_actions: Vec<String>,
    pub available_tools: Vec<String>,
    pub current_state: HashMap<String, Value>,
}

/// Action taken as a result of the decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Action {
    ToolCall {
        name: String,
        args: Value,
        expected_outcome: String,
    },
    Response {
        content: String,
        response_type: ResponseType,
    },

    ErrorRecovery {
        error_type: String,
        recovery_strategy: String,
    },
}

/// Type of response given to user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseType {
    Text,
    ToolExecution,
    ErrorHandling,
    ContextSummary,
}

/// Outcome of a decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DecisionOutcome {
    Success {
        result: String,
        metrics: HashMap<String, Value>,
    },
    Failure {
        error: String,
        recovery_attempts: usize,
        context_preserved: bool,
    },
    Partial {
        result: String,
        issues: Vec<String>,
    },
}

/// Decision tracker for maintaining transparency
pub struct DecisionTracker {
    decisions: Vec<Decision>,
    current_context: DecisionContext,
    session_start: u64,
}

impl DecisionTracker {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            decisions: Vec::new(),
            current_context: DecisionContext {
                conversation_turn: 0,
                user_input: None,
                previous_actions: Vec::new(),
                available_tools: Vec::new(),
                current_state: HashMap::new(),
            },
            session_start: now,
        }
    }

    /// Start tracking a new conversation turn
    pub fn start_turn(&mut self, turn_number: usize, user_input: Option<String>) {
        self.current_context.conversation_turn = turn_number;
        self.current_context.user_input = user_input;
    }

    /// Update the current context with available tools
    pub fn update_available_tools(&mut self, tools: Vec<String>) {
        self.current_context.available_tools = tools;
    }

    /// Update the current state
    pub fn update_state(&mut self, key: &str, value: Value) {
        self.current_context
            .current_state
            .insert(key.to_string(), value);
    }

    /// Record a decision
    pub fn record_decision(
        &mut self,
        reasoning: String,
        action: Action,
        confidence_score: Option<f64>,
    ) -> String {
        let decision_id = format!("decision_{}_{}", self.session_start, self.decisions.len());

        let decision = Decision {
            id: decision_id.clone(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            context: self.current_context.clone(),
            reasoning,
            action: action.clone(),
            outcome: None,
            confidence_score,
        };

        self.decisions.push(decision);

        // Update previous actions for next decision
        let action_summary = match &action {
            Action::ToolCall { name, .. } => format!("tool_call:{}", name),
            Action::Response { response_type, .. } => format!("response:{:?}", response_type),
            Action::ErrorRecovery { .. } => "error_recovery".to_string(),
        };
        self.current_context.previous_actions.push(action_summary);

        decision_id
    }

    /// Record the outcome of a decision
    pub fn record_outcome(&mut self, decision_id: &str, outcome: DecisionOutcome) {
        if let Some(decision) = self.decisions.iter_mut().find(|d| d.id == decision_id) {
            decision.outcome = Some(outcome);
        }
    }

    /// Get all decisions for transparency reporting
    pub fn get_decisions(&self) -> &[Decision] {
        &self.decisions
    }

    /// Generate a transparency report
    pub fn generate_transparency_report(&self) -> TransparencyReport {
        let total_decisions = self.decisions.len();
        let successful_decisions = self
            .decisions
            .iter()
            .filter(|d| matches!(d.outcome, Some(DecisionOutcome::Success { .. })))
            .count();
        let failed_decisions = self
            .decisions
            .iter()
            .filter(|d| matches!(d.outcome, Some(DecisionOutcome::Failure { .. })))
            .count();

        let tool_calls = self
            .decisions
            .iter()
            .filter(|d| matches!(d.action, Action::ToolCall { .. }))
            .count();

        let avg_confidence = self
            .decisions
            .iter()
            .filter_map(|d| d.confidence_score)
            .collect::<Vec<f64>>();

        let avg_confidence = if avg_confidence.is_empty() {
            None
        } else {
            Some(avg_confidence.iter().sum::<f64>() / avg_confidence.len() as f64)
        };

        TransparencyReport {
            session_duration: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - self.session_start,
            total_decisions,
            successful_decisions,
            failed_decisions,
            tool_calls,
            avg_confidence,
            recent_decisions: self.decisions.iter().rev().take(5).cloned().collect(),
        }
    }

    /// Get decision context for error recovery
    pub fn get_decision_context(&self, decision_id: &str) -> Option<&DecisionContext> {
        self.decisions
            .iter()
            .find(|d| d.id == decision_id)
            .map(|d| &d.context)
    }

    pub fn get_current_context(&self) -> &DecisionContext {
        &self.current_context
    }

    /// Get the latest decision made
    pub fn latest_decision(&self) -> Option<&Decision> {
        self.decisions.last()
    }

    /// Get the N most recent decisions
    pub fn recent_decisions(&self, count: usize) -> Vec<&Decision> {
        self.decisions.iter().rev().take(count).collect()
    }

    /// Convenience: record a user goal/intention for this turn
    pub fn record_goal(&mut self, content: String) -> String {
        self.record_decision(
            "User goal provided".to_string(),
            Action::Response {
                content,
                response_type: ResponseType::ContextSummary,
            },
            None,
        )
    }

    /// Render a compact Decision Ledger for injection into the system prompt
    pub fn render_ledger_brief(&self, max_entries: usize) -> String {
        let mut out = String::new();
        out.push_str("Decision Ledger (most recent first)\n");
        let take_n = max_entries.max(1);
        for d in self.decisions.iter().rev().take(take_n) {
            let ts = d.timestamp;
            let turn = d.context.conversation_turn;
            let line = match &d.action {
                Action::ToolCall { name, args, .. } => {
                    let arg_preview = match args {
                        serde_json::Value::String(s) => s.clone(),
                        _ => {
                            let s = args.to_string();
                            if s.len() > 120 {
                                format!("{}…", &s[..120])
                            } else {
                                s
                            }
                        }
                    };
                    format!(
                        "- [turn {}] tool:{} args={} (t={})",
                        turn, name, arg_preview, ts
                    )
                }
                Action::Response {
                    response_type,
                    content,
                } => {
                    let preview = if content.len() > 120 {
                        format!("{}…", &content[..120])
                    } else {
                        content.clone()
                    };
                    format!(
                        "- [turn {}] response:{:?} {} (t={})",
                        turn, response_type, preview, ts
                    )
                }
                Action::ErrorRecovery {
                    error_type,
                    recovery_strategy,
                } => {
                    format!(
                        "- [turn {}] recovery {} via {} (t={})",
                        turn, error_type, recovery_strategy, ts
                    )
                }
            };
            out.push_str(&line);
            out.push('\n');
        }
        if out.is_empty() {
            "(no decisions yet)".to_string()
        } else {
            out
        }
    }
}

/// Transparency report for the current session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransparencyReport {
    pub session_duration: u64,
    pub total_decisions: usize,
    pub successful_decisions: usize,
    pub failed_decisions: usize,
    pub tool_calls: usize,
    pub avg_confidence: Option<f64>,
    pub recent_decisions: Vec<Decision>,
}

impl Default for DecisionTracker {
    fn default() -> Self {
        Self::new()
    }
}
