# Context Manager Analysis: OpenAI Codex vs VT Code

## Executive Summary

OpenAI Codex implements a **robust conversation history manager** with strict invariant enforcement, while VT Code uses a **distributed context optimization approach**. This document outlines key patterns from Codex that could enhance VT Code's context reliability and introduces an optional centralized context manager.

---

## 1. OpenAI Codex Context Manager Architecture

### 1.1 Core Components

```
context_manager/
├── mod.rs           # Public API & main ContextManager struct
├── history.rs       # Conversation transcript management (296 lines)
├── normalize.rs     # History invariant enforcement (212 lines)
└── history_tests.rs # Comprehensive test suite
```

### 1.2 Key Design Patterns

#### Pattern 1: Strict Invariant Enforcement
Codex enforces two critical invariants on every history normalization:

1. **Every call has an output**: Function calls, custom tool calls, and shell calls MUST have corresponding output entries
2. **Every output has a call**: Orphan outputs (without matching calls) are removed

```rust
// From normalize.rs
fn ensure_call_outputs_present(items: &mut Vec<ResponseItem>) {
    // For each call, check if output exists
    // If missing, create synthetic "aborted" output
}

fn remove_orphan_outputs(items: &mut Vec<ResponseItem>) {
    // Collect all call IDs
    // Remove outputs without matching calls
}
```

**Why this matters**: Prevents LLM context corruption from incomplete tool interactions. When a user cancels a tool call or an error occurs, Codex creates a synthetic "aborted" response rather than leaving a dangling call.

#### Pattern 2: Corresponding Item Removal
When removing one item from the conversation, automatically remove its pair:

```rust
fn remove_corresponding_for(items: &mut Vec<ResponseItem>, item: &ResponseItem) {
    match item {
        ResponseItem::FunctionCall { call_id, .. } => {
            // Remove matching FunctionCallOutput
        },
        ResponseItem::FunctionCallOutput { call_id, .. } => {
            // Remove matching FunctionCall or LocalShellCall
        },
        // ... similar for CustomToolCall/CustomToolCallOutput
    }
}
```

**Use case**: When trimming old context, this ensures call/output pairs stay in sync.

#### Pattern 3: Selective Item Processing
Only certain items participate in the conversation history (filtering out internal bookkeeping):

```rust
fn record_items<I>(&mut self, items: I, policy: TruncationPolicy) {
    for item in items {
        let is_ghost_snapshot = matches!(item, ResponseItem::GhostSnapshot { .. });
        if !is_api_message(item) && !is_ghost_snapshot {
            continue; // Skip internal items
        }
        // Process and truncate...
    }
}
```

#### Pattern 4: Dual History Views
Two history methods for different purposes:

- `get_history()`: Full history with all tracking data (for resuming sessions)
- `get_history_for_prompt()`: Clean history with GhostSnapshots removed (for sending to LLM)

---

## 2. Current VT Code Context System

### 2.1 Distributed Architecture

VT Code manages context through **multiple specialized systems**:

| Component | Location | Purpose |
|-----------|----------|---------|
| `ContextOptimizer` | `vtcode-core/src/core/context_optimizer.rs` | Tool output curation (grep, file listing, command output truncation) |
| `ContextManager` (Skills) | `vtcode-core/src/skills/context_manager.rs` | Progressive disclosure of tool/skill metadata |
| `ContextManager` (Agent) | `src/agent/runloop/unified/context_manager.rs` | Dynamic system prompt construction |
| `ConversationMemory` | `vtcode-core/src/context/conversation_memory.rs` | Entity tracking, pronoun resolution, recent file contexts |
| `TaskRunState` | `vtcode-core/src/core/agent/state.rs` | Active conversation history and message tracking |
| `ConversationManager` (VS Code) | `vscode-extension/src/conversation/conversationManager.ts` | Message persistence, history trimming (max 1000 messages) |

### 2.2 Strengths

✅ **Tool-specific optimization**: Each tool has custom curation logic (e.g., grep dedups by path/line)
✅ **Conversation memory**: Tracks entities and context across turns
✅ **Dynamic system prompt**: Agent context manager adjusts based on tool usage
✅ **VS Code integration**: Native persistence and UI synchronization

### 2.3 Gaps Identified

❌ **No centralized call/output pairing enforcement**: Multiple systems manage history without shared invariants
❌ **No synthetic response handling**: Canceled or errored tool calls don't get marked with explicit status
❌ **Implicit history state**: Tool results aren't tracked in a structured, queryable format
❌ **Limited resumption safety**: Session restoration relies on implicit message ordering

---

## 3. Recommended Enhancements

### 3.1 Phase 1: Adopt Codex Patterns (Low-risk, High-value)

#### 3.1.1 Add Invariant Checking to TaskRunState

Extend `vtcode-core/src/core/agent/state.rs` with normalization:

```rust
impl TaskRunState {
    /// Ensure all tool calls have corresponding outputs
    pub fn normalize_history(&mut self) {
        self.ensure_tool_call_outputs_present();
        self.remove_orphan_outputs();
    }
    
    /// Create synthetic response for canceled/failed tool calls
    fn ensure_tool_call_outputs_present(&mut self) {
        for (idx, item) in self.history.iter().enumerate() {
            if let HistoryItem::ToolCall { tool_call_id, .. } = item {
                if !self.has_output_for(&tool_call_id) {
                    self.insert_output(idx + 1, HistoryItem::ToolOutput {
                        tool_call_id,
                        status: "aborted".to_string(),
                        content: "Tool execution was canceled or failed".to_string(),
                    });
                }
            }
        }
    }
    
    /// Remove outputs without matching calls
    fn remove_orphan_outputs(&mut self) {
        let call_ids: HashSet<_> = self.history.iter()
            .filter_map(|item| {
                if let HistoryItem::ToolCall { tool_call_id, .. } = item {
                    Some(tool_call_id)
                } else {
                    None
                }
            })
            .collect();
        
        self.history.retain(|item| {
            if let HistoryItem::ToolOutput { tool_call_id, .. } = item {
                call_ids.contains(tool_call_id)
            } else {
                true
            }
        });
    }
}
```

**Benefits**:
- Prevents "dangling call" errors during session resumption
- Makes history more robust to interrupted tool executions
- Improves clarity for debugging incomplete interactions

#### 3.1.2 Explicit Tool Output Status Tracking

Enhance `vtcode-core/src/tools/result.rs` to track execution outcomes:

```rust
pub enum ToolExecutionStatus {
    Success,
    Failed { reason: String },
    Canceled,
    Timeout,
}

pub struct ToolResult {
    pub tool_id: String,
    pub status: ToolExecutionStatus,
    pub output: String,
    pub token_count: usize,
}

impl ToolResult {
    /// Create an aborted result when user cancels or timeout occurs
    pub fn aborted(tool_id: String, reason: &str) -> Self {
        Self {
            tool_id,
            status: ToolExecutionStatus::Canceled,
            output: reason.to_string(),
            token_count: 0,
        }
    }
}
```

**Benefits**:
- Explicit failure modes for context reconstruction
- Better telemetry on tool execution patterns
- Simpler resumption logic (don't guess at failed calls)

### 3.2 Phase 2: Optional Centralized Context Manager (Lower-risk variant)

Create a new lightweight context manager that **layers on top** of existing systems without replacing them:

#### 3.2.1 File: `vtcode-core/src/context/history_manager.rs`

```rust
//! Centralized conversation history with Codex-inspired invariants
//!
//! This manager sits between TaskRunState and tool execution, ensuring:
//! 1. All tool calls have corresponding outputs
//! 2. All outputs have matching calls
//! 3. History can be safely truncated while preserving call/output pairs

use std::collections::{HashMap, HashSet};
use crate::tools::ToolResult;

/// Tracked state of a tool call in the conversation
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub tool_call_id: String,
    pub tool_name: String,
    pub timestamp: u64,
    pub has_output: bool,
}

/// Conversation history with invariant enforcement
pub struct HistoryManager {
    /// Tool calls in order (oldest first)
    calls: HashMap<String, ToolCallRecord>,
    call_order: Vec<String>, // Maintain insertion order
}

impl HistoryManager {
    pub fn new() -> Self {
        Self {
            calls: HashMap::new(),
            call_order: Vec::new(),
        }
    }
    
    /// Record a tool call without output
    pub fn record_tool_call(&mut self, tool_call_id: String, tool_name: String) {
        let record = ToolCallRecord {
            tool_call_id: tool_call_id.clone(),
            tool_name,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            has_output: false,
        };
        
        self.call_order.push(tool_call_id.clone());
        self.calls.insert(tool_call_id, record);
    }
    
    /// Mark a tool call as having output
    pub fn record_tool_output(&mut self, tool_call_id: &str) -> Result<()> {
        if let Some(record) = self.calls.get_mut(tool_call_id) {
            record.has_output = true;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No tool call found for ID: {}", tool_call_id))
        }
    }
    
    /// Get all tool calls missing outputs (for crash recovery)
    pub fn missing_outputs(&self) -> Vec<ToolCallRecord> {
        self.calls.iter()
            .filter(|(_, record)| !record.has_output)
            .map(|(_, record)| record.clone())
            .collect()
    }
    
    /// Check if history is in a valid state
    pub fn validate(&self) -> Result<()> {
        for (id, record) in &self.calls {
            if !record.has_output {
                // This is a warning, not an error
                // We'll synthesize outputs on demand
                tracing::warn!("Tool call {} ({}) is missing output", id, record.tool_name);
            }
        }
        Ok(())
    }
    
    /// Compact history, removing oldest calls
    pub fn compact(&mut self, keep_count: usize) -> Vec<String> {
        let to_remove = self.call_order.len().saturating_sub(keep_count);
        let mut removed = Vec::new();
        
        for _ in 0..to_remove {
            if let Some(id) = self.call_order.first() {
                removed.push(id.clone());
                self.calls.remove(id);
                self.call_order.remove(0);
            }
        }
        
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_record_and_validate() {
        let mut manager = HistoryManager::new();
        manager.record_tool_call("call-1".to_string(), "grep_file".to_string());
        manager.record_tool_output("call-1").unwrap();
        assert!(manager.validate().is_ok());
    }
    
    #[test]
    fn test_missing_outputs() {
        let mut manager = HistoryManager::new();
        manager.record_tool_call("call-1".to_string(), "grep_file".to_string());
        manager.record_tool_call("call-2".to_string(), "read_file".to_string());
        manager.record_tool_output("call-1").unwrap();
        
        let missing = manager.missing_outputs();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].tool_call_id, "call-2");
    }
}
```

#### 3.2.2 Integration Point: Minimal Changes to Existing Code

In `vtcode-core/src/core/agent/state.rs`, add opt-in history validation:

```rust
impl TaskRunState {
    /// Optional: Validate that conversation invariants are maintained
    pub fn validate_history_invariants(&self) -> Result<()> {
        // Count tool calls vs outputs
        let calls = self.history.iter()
            .filter(|item| matches!(item, HistoryItem::ToolCall { .. }))
            .count();
        
        let outputs = self.history.iter()
            .filter(|item| matches!(item, HistoryItem::ToolOutput { .. }))
            .count();
        
        if calls != outputs {
            tracing::warn!(
                "History invariant violation: {} calls but {} outputs",
                calls, outputs
            );
        }
        
        Ok(())
    }
}
```

**Benefits**:
- Opt-in, doesn't break existing flow
- Low-risk addition for testing invariant violations
- Can be enabled per-session via config

### 3.3 Phase 3: Enhanced Error Recovery

Add resilience for crashed or interrupted sessions:

#### 3.3.1 Crash Recovery in `vtcode-core/src/core/agent/state.rs`

```rust
impl TaskRunState {
    /// Recover from crashed session by marking incomplete calls
    pub async fn recover_from_crash(&mut self) -> Result<()> {
        self.validate_history_invariants()?;
        
        let missing_outputs = self.missing_tool_outputs();
        
        for (call_id, tool_name) in missing_outputs {
            tracing::warn!(
                "Recovering from crash: marking {} ({}) as canceled",
                tool_name, call_id
            );
            
            // Add synthetic output to maintain invariant
            self.add_tool_output(
                call_id,
                "canceled".to_string(),
                "Session crashed. Tool execution was interrupted.".to_string(),
            )?;
        }
        
        Ok(())
    }
    
    fn missing_tool_outputs(&self) -> Vec<(String, String)> {
        let mut call_map = HashMap::new();
        let mut output_ids = HashSet::new();
        
        for item in &self.history {
            match item {
                HistoryItem::ToolCall { tool_call_id, tool_name, .. } => {
                    call_map.insert(tool_call_id.clone(), tool_name.clone());
                }
                HistoryItem::ToolOutput { tool_call_id, .. } => {
                    output_ids.insert(tool_call_id.clone());
                }
                _ => {}
            }
        }
        
        call_map.into_iter()
            .filter(|(id, _)| !output_ids.contains(id))
            .collect()
    }
}
```

---

## 4. Implementation Roadmap

### Short-term (1-2 weeks)
1. ✅ Add `ensure_tool_call_outputs_present()` to TaskRunState
2. ✅ Add synthetic "aborted" responses for missing outputs
3. ✅ Write tests for invariant violations

### Medium-term (2-4 weeks)
4. Add `validate_history_invariants()` as opt-in check
5. Enhance ToolResult with explicit status enum
6. Add history recovery to session restoration

### Long-term (Optional)
7. Create standalone `HistoryManager` for future refactoring
8. Integrate with conversation memory for richer context
9. Add telemetry for tool execution patterns

---

## 5. Comparison Matrix

| Feature | Codex | VT Code | Recommendation |
|---------|-------|---------|-----------------|
| **Call/output pairing** | ✅ Enforced | ❌ Implicit | Adopt in Phase 1 |
| **Synthetic responses** | ✅ "aborted" status | ❌ No | Adopt in Phase 1 |
| **History normalization** | ✅ Explicit | ❌ No | Adopt in Phase 1 |
| **Tool output curation** | ❌ Not in context manager | ✅ Robust | Keep as-is |
| **Conversation memory** | ❌ Not in context manager | ✅ Rich entity tracking | Keep as-is |
| **Dual history views** | ✅ Internal/external | ✅ Similar (taskrunstate/prompt) | Consider formalizing |
| **Session resumption** | ✅ Robust with invariants | ⚠️ Depends on implicit state | Improve with Phase 1 |

---

## 6. Risk Assessment

### Low Risk (recommend implementing)
- Adding normalization checks to TaskRunState
- Creating synthetic responses for missing outputs
- Validation tests for invariants

### Medium Risk (consider carefully)
- Changing HistoryItem structure to track call/output pairing
- Modifying TaskRunState to enforce invariants at runtime

### High Risk (future consideration)
- Complete rewrite of conversation history system
- Breaking changes to session persistence format

---

## 7. References

- **Codex context_manager/**: https://github.com/openai/codex/tree/main/codex-rs/core/src/context_manager
- **VT Code TaskRunState**: `vtcode-core/src/core/agent/state.rs`
- **VT Code ContextOptimizer**: `vtcode-core/src/core/context_optimizer.rs`
- **Codex normalize.rs**: Pattern for call/output invariants (lines 60-100+)

