# Context Manager Implementation: Phase 1 - Invariant Enforcement

This document provides concrete code patterns for implementing call/output pairing invariants in VT Code, adapted from OpenAI Codex.

## Overview

**Goal**: Add robustness to conversation history by ensuring every tool call has a corresponding output, and vice versa.

**Scope**: Minimal changes to `TaskRunState` to enforce and validate these invariants.

**Risk**: Low (validation + recovery code, no breaking changes to external APIs).

---

## 1. Define History Item Types with Pairing

First, clarify what constitutes a "call" and "output" pair. Add to `vtcode-core/src/core/agent/state.rs`:

```rust
/// Represents a tool invocation in the conversation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallId(pub String);

/// Categories of history items that participate in call/output pairing
#[derive(Debug, Clone)]
pub enum PairableHistoryItem {
    /// Tool call without output (yet)
    ToolCall {
        call_id: ToolCallId,
        tool_name: String,
    },
    /// Tool output for a previous call
    ToolOutput {
        call_id: ToolCallId,
        status: OutputStatus,
    },
}

/// Status of a tool execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputStatus {
    Success,
    Failed,
    Canceled,
    Timeout,
}

impl OutputStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
            Self::Timeout => "timeout",
        }
    }
}
```

---

## 2. Add Validation Methods to TaskRunState

Add these methods to the `TaskRunState` implementation:

```rust
impl TaskRunState {
    /// Ensure all tool calls have corresponding outputs
    /// 
    /// This enforces a critical invariant: the conversation history should not contain
    /// "dangling" tool calls without outputs. If a tool call is missing its output
    /// (due to cancellation, timeout, or crash), we synthesize an output to maintain
    /// the invariant.
    pub fn ensure_call_outputs_present(&mut self) {
        let mut missing_outputs: Vec<(usize, ToolCallId, String)> = Vec::new();
        
        // Identify calls without outputs
        for (idx, item) in self.history.iter().enumerate() {
            if let Some(HistoryItem::ToolCall { call_id, tool_name }) = self.extract_pairable(item) {
                let has_output = self.history.iter().any(|h| {
                    if let Some(PairableHistoryItem::ToolOutput { call_id: output_id, .. }) = 
                        self.extract_pairable(h) {
                        output_id == call_id
                    } else {
                        false
                    }
                });
                
                if !has_output {
                    tracing::warn!(
                        "Tool call {} ({}) missing output - will create synthetic response",
                        call_id.0, tool_name
                    );
                    missing_outputs.push((idx, call_id, tool_name));
                }
            }
        }
        
        // Insert synthetic outputs in reverse order to avoid index shifting
        for (idx, call_id, _tool_name) in missing_outputs.iter().rev() {
            let synthetic_output = self.create_synthetic_output(
                call_id.clone(),
                OutputStatus::Canceled,
                "Tool execution was interrupted. Output was not received."
            );
            self.history.insert(idx + 1, synthetic_output);
        }
    }
    
    /// Remove outputs without corresponding calls (orphaned outputs)
    pub fn remove_orphan_outputs(&mut self) {
        // Collect all valid call IDs
        let mut call_ids = std::collections::HashSet::new();
        for item in &self.history {
            if let Some(PairableHistoryItem::ToolCall { call_id, .. }) = self.extract_pairable(item) {
                call_ids.insert(call_id);
            }
        }
        
        // Remove outputs without matching calls
        let initial_len = self.history.len();
        self.history.retain(|item| {
            if let Some(PairableHistoryItem::ToolOutput { call_id, .. }) = self.extract_pairable(item) {
                if !call_ids.contains(&call_id) {
                    tracing::warn!("Removing orphan output for call {}", call_id.0);
                    return false;
                }
            }
            true
        });
        
        if self.history.len() != initial_len {
            tracing::info!(
                "Removed {} orphan outputs",
                initial_len - self.history.len()
            );
        }
    }
    
    /// Validate that conversation invariants are maintained
    /// 
    /// Returns a report of any invariant violations (non-fatal)
    pub fn validate_history_invariants(&self) -> HistoryValidationReport {
        let mut report = HistoryValidationReport::default();
        
        // Count calls and outputs
        let mut call_ids = std::collections::HashMap::new();
        let mut output_ids = std::collections::HashSet::new();
        
        for item in &self.history {
            if let Some(PairableHistoryItem::ToolCall { call_id, tool_name }) = self.extract_pairable(item) {
                call_ids.insert(call_id, tool_name);
            }
            if let Some(PairableHistoryItem::ToolOutput { call_id, .. }) = self.extract_pairable(item) {
                output_ids.insert(call_id);
            }
        }
        
        // Find missing outputs
        for (call_id, tool_name) in &call_ids {
            if !output_ids.contains(call_id) {
                report.missing_outputs.push(MissingOutput {
                    call_id: call_id.clone(),
                    tool_name: tool_name.clone(),
                });
            }
        }
        
        // Find orphaned outputs
        for output_id in &output_ids {
            if !call_ids.contains_key(output_id) {
                report.orphan_outputs.push(output_id.clone());
            }
        }
        
        report
    }
    
    /// Attempt to recover from a crashed or interrupted session
    pub async fn recover_from_crash(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let report = self.validate_history_invariants();
        
        if !report.missing_outputs.is_empty() {
            tracing::warn!("Found {} missing outputs during recovery", report.missing_outputs.len());
            self.ensure_call_outputs_present();
        }
        
        if !report.orphan_outputs.is_empty() {
            tracing::warn!("Found {} orphan outputs during recovery", report.orphan_outputs.len());
            self.remove_orphan_outputs();
        }
        
        if report.is_valid() {
            tracing::info!("History invariants are valid");
        }
        
        Ok(())
    }
    
    // --- Private helpers ---
    
    fn extract_pairable(&self, item: &HistoryItem) -> Option<PairableHistoryItem> {
        match item {
            HistoryItem::ToolCall { call_id, tool_name, .. } => {
                Some(PairableHistoryItem::ToolCall {
                    call_id: ToolCallId(call_id.clone()),
                    tool_name: tool_name.clone(),
                })
            }
            HistoryItem::ToolOutput { call_id, status, .. } => {
                Some(PairableHistoryItem::ToolOutput {
                    call_id: ToolCallId(call_id.clone()),
                    status: self.status_from_str(status),
                })
            }
            _ => None,
        }
    }
    
    fn create_synthetic_output(
        &self,
        call_id: ToolCallId,
        status: OutputStatus,
        message: &str,
    ) -> HistoryItem {
        HistoryItem::ToolOutput {
            call_id: call_id.0,
            status: status.as_str().to_string(),
            content: message.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
    
    fn status_from_str(&self, status: &str) -> OutputStatus {
        match status {
            "success" => OutputStatus::Success,
            "failed" => OutputStatus::Failed,
            "canceled" => OutputStatus::Canceled,
            "timeout" => OutputStatus::Timeout,
            _ => OutputStatus::Failed,
        }
    }
}

/// Report of history validation results
#[derive(Debug, Default, Clone)]
pub struct HistoryValidationReport {
    pub missing_outputs: Vec<MissingOutput>,
    pub orphan_outputs: Vec<ToolCallId>,
}

#[derive(Debug, Clone)]
pub struct MissingOutput {
    pub call_id: ToolCallId,
    pub tool_name: String,
}

impl HistoryValidationReport {
    pub fn is_valid(&self) -> bool {
        self.missing_outputs.is_empty() && self.orphan_outputs.is_empty()
    }
    
    pub fn summary(&self) -> String {
        if self.is_valid() {
            "History invariants are valid".to_string()
        } else {
            format!(
                "{} missing outputs, {} orphan outputs",
                self.missing_outputs.len(),
                self.orphan_outputs.len()
            )
        }
    }
}
```

---

## 3. Integration Points

### 3.1 Call normalization in TaskRunState

Add a public method that enforces invariants (called after major operations):

```rust
impl TaskRunState {
    /// Public API: Normalize history to enforce invariants
    /// 
    /// This should be called:
    /// - After loading a saved session
    /// - After removing items from history
    /// - Before sending history to the LLM
    pub fn normalize(&mut self) {
        self.ensure_call_outputs_present();
        self.remove_orphan_outputs();
        
        let report = self.validate_history_invariants();
        if !report.is_valid() {
            tracing::warn!("After normalization: {}", report.summary());
        } else {
            tracing::debug!("History normalized successfully");
        }
    }
}
```

### 3.2 Call in session loading

In the code that restores sessions from disk:

```rust
pub async fn load_session(path: &Path) -> Result<TaskRunState> {
    let mut state = TaskRunState::load_from_file(path).await?;
    
    // Recover from any interrupted operations
    state.recover_from_crash().await?;
    
    // Ensure history is in a valid state
    state.normalize();
    
    Ok(state)
}
```

### 3.3 Call when truncating history

When removing old items to stay within token budget:

```rust
pub fn trim_old_history(&mut self, keep_count: usize) {
    if self.history.len() > keep_count {
        // Remove oldest items
        let to_remove = self.history.len() - keep_count;
        self.history.drain(0..to_remove);
        
        // Ensure invariants still hold after removal
        self.normalize();
    }
}
```

---

## 4. Testing

Add comprehensive tests to `vtcode-core/src/core/agent/state.rs`:

```rust
#[cfg(test)]
mod history_invariant_tests {
    use super::*;
    
    #[test]
    fn test_ensure_call_outputs_present() {
        let mut state = TaskRunState::new();
        
        // Add a call without output
        state.history.push(HistoryItem::ToolCall {
            call_id: "call-1".to_string(),
            tool_name: "grep_file".to_string(),
            args: Default::default(),
            timestamp: 0,
        });
        
        // Validate shows missing output
        let report = state.validate_history_invariants();
        assert_eq!(report.missing_outputs.len(), 1);
        
        // Ensure outputs present fixes it
        state.ensure_call_outputs_present();
        let report = state.validate_history_invariants();
        assert!(report.is_valid());
        
        // Verify synthetic output was created
        assert_eq!(state.history.len(), 2);
    }
    
    #[test]
    fn test_remove_orphan_outputs() {
        let mut state = TaskRunState::new();
        
        // Add an output without a call
        state.history.push(HistoryItem::ToolOutput {
            call_id: "call-1".to_string(),
            status: "success".to_string(),
            content: "result".to_string(),
            timestamp: 0,
        });
        
        // Validate shows orphan
        let report = state.validate_history_invariants();
        assert_eq!(report.orphan_outputs.len(), 1);
        
        // Remove orphans
        state.remove_orphan_outputs();
        
        // Now valid
        let report = state.validate_history_invariants();
        assert!(report.is_valid());
        assert_eq!(state.history.len(), 0);
    }
    
    #[test]
    fn test_normalize() {
        let mut state = TaskRunState::new();
        
        // Mix of valid and invalid items
        state.history.push(HistoryItem::ToolCall {
            call_id: "call-1".to_string(),
            tool_name: "read_file".to_string(),
            args: Default::default(),
            timestamp: 0,
        });
        
        state.history.push(HistoryItem::ToolOutput {
            call_id: "call-2".to_string(), // Orphan!
            status: "success".to_string(),
            content: "result".to_string(),
            timestamp: 0,
        });
        
        // Normalize fixes everything
        state.normalize();
        
        // Should have: call-1, synthetic output for call-1
        // Orphan output for call-2 should be removed
        assert_eq!(state.history.len(), 2);
        
        let report = state.validate_history_invariants();
        assert!(report.is_valid());
    }
    
    #[test]
    fn test_validation_report_summary() {
        let report = HistoryValidationReport {
            missing_outputs: vec![
                MissingOutput {
                    call_id: ToolCallId("call-1".to_string()),
                    tool_name: "grep".to_string(),
                }
            ],
            orphan_outputs: vec![],
        };
        
        assert!(!report.is_valid());
        assert!(report.summary().contains("missing outputs"));
    }
}
```

---

## 5. Configuration & Opt-in Behavior

Add to `vtcode.toml`:

```toml
# Context management settings
[context]
# Enforce call/output pairing invariants (may help debug edge cases)
enforce_history_invariants = false

# Automatically recover from crashed sessions
auto_recover_from_crash = true

# Log warnings about history issues
warn_on_invariant_violations = true
```

Load in `context_manager.rs`:

```rust
pub struct ContextConfig {
    pub enforce_history_invariants: bool,
    pub auto_recover_from_crash: bool,
    pub warn_on_invariant_violations: bool,
}

impl ContextConfig {
    pub fn from_vtcode_toml() -> Result<Self> {
        let config = // load from vtcode.toml
        Ok(Self {
            enforce_history_invariants: config.context.enforce_history_invariants.unwrap_or(false),
            auto_recover_from_crash: config.context.auto_recover_from_crash.unwrap_or(true),
            warn_on_invariant_violations: config.context.warn_on_invariant_violations.unwrap_or(true),
        })
    }
}
```

---

## 6. Rollout Plan

### Week 1: Core Implementation
- [ ] Add `PairableHistoryItem` enum and `OutputStatus`
- [ ] Add validation/normalization methods to TaskRunState
- [ ] Write unit tests
- [ ] Test with `cargo test`

### Week 2: Integration
- [ ] Add calls to `normalize()` in session loading
- [ ] Add calls to `normalize()` in history trimming
- [ ] Test recovery from crashes
- [ ] Monitor for any issues in internal use

### Week 3: Hardening
- [ ] Add configuration options
- [ ] Add telemetry/metrics for invariant violations
- [ ] Document the feature in CLAUDE.md

### Week 4+: Optional Extensions
- [ ] Implement standalone `HistoryManager` if beneficial
- [ ] Add rich telemetry on tool execution patterns
- [ ] Consider deeper integration with conversation memory

---

## 7. Verification Checklist

Before deployment:

- [ ] All new code compiles without warnings
- [ ] `cargo clippy` passes with strict rules
- [ ] `cargo test` passes (new + existing tests)
- [ ] Unit tests cover normal + edge cases
- [ ] Session loading test: verify synthetic outputs created
- [ ] Session trimming test: verify invariants maintained
- [ ] Crash recovery test: verify dangling calls handled
- [ ] No performance regression in normalization

---

## 8. Example Usage

Once implemented, usage is straightforward:

```rust
// Load a session and automatically recover from crashes
let mut state = TaskRunState::load_from_disk("session.json").await?;
state.recover_from_crash().await?;

// Later, when trimming history to fit in token budget
state.trim_old_history(500);

// Before sending to LLM, ensure invariants
state.normalize();
let history = state.get_history_for_prompt();
```

---

## References

- OpenAI Codex normalize.rs: https://github.com/openai/codex/blob/main/codex-rs/core/src/context_manager/normalize.rs
- VT Code TaskRunState: `vtcode-core/src/core/agent/state.rs`
- Error handling patterns: CLAUDE.md (use `anyhow::Result` + `with_context()`)

