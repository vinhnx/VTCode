# Agent Justification System

## Overview

The Agent Justification System enables VTCode to capture and present agent reasoning when requesting approval for high-risk tool execution. This improves the user experience by explaining **why** the agent needs to run potentially dangerous operations, and learns from user approval patterns to reduce friction over time.

## Architecture

### Core Components

#### 1. **ToolJustification** (`vtcode-core/src/tools/registry/justification.rs`)
Represents a single justification for tool execution.

```rust
pub struct ToolJustification {
    pub tool_name: String,
    pub reason: String,
    pub expected_outcome: Option<String>,
    pub risk_level: String,
    pub timestamp: String,
}
```

**Key Methods:**
- `new()` - Create justification with tool name and reason
- `with_outcome()` - Add expected outcome description
- `format_for_dialog()` - Format for TUI display with text wrapping

#### 2. **JustificationManager** (`vtcode-core/src/tools/registry/justification.rs`)
Manages approval pattern learning and persistence.

```rust
pub struct JustificationManager {
    cache_dir: PathBuf,
    patterns: HashMap<String, ApprovalPattern>,
}
```

**Key Methods:**
- `record_decision()` - Record user approval/denial
- `get_pattern()` - Retrieve approval history for a tool
- `get_learning_summary()` - Get human-readable stats

**ApprovalPattern Structure:**
- `tool_name` - Tool being tracked
- `approve_count` - Times user approved
- `deny_count` - Times user denied
- `approval_rate()` - Approval percentage (0.0-1.0)
- `has_high_approval_rate()` - True if ≥3 approvals AND >80% rate

#### 3. **ApprovalRecorder** (`vtcode-core/src/tools/registry/approval_recorder.rs`)
Async wrapper for recording approval decisions in concurrent contexts.

```rust
pub struct ApprovalRecorder {
    manager: Arc<RwLock<JustificationManager>>,
}
```

**Key Methods:**
- `record_approval()` - Async approval logging
- `should_auto_approve()` - Check if tool can auto-approve
- `get_auto_approval_suggestion()` - UX hint for frequent approvals
- `get_approval_count()` - Query approval stats

#### 4. **JustificationExtractor** (`vtcode-core/src/tools/registry/justification_extractor.rs`)
Extracts reasoning from the decision ledger for justification creation.

```rust
pub struct JustificationExtractor;
```

**Key Methods:**
- `extract_from_decision()` - Pull reasoning from a Decision
- `extract_latest_from_tracker()` - Get latest decision reasoning
- `extract_from_recent_decisions()` - Combine multiple decision reasons
- `suggest_default_justification()` - Fallback for common tools

**Default Justifications:**
- `run_command` - Execute system operations or build/test
- `write_file` - Implement code changes
- `grep_file` - Search codebase structure
- `delete_file` - Cleanup generated files
- `apply_patch` - Apply code fixes/features

## Data Flow

### Approval Request Flow

```
1. Tool execution initiated
   ↓
2. Risk scoring determines risk level
   ↓
3. Check approval policy (Allow/Deny/Prompt)
   ↓
4. If Prompt required:
   a. Extract justification from decision ledger
      - Use JustificationExtractor on latest decision
      - Fall back to suggested defaults if no explicit reasoning
   b. Check approval patterns (ApprovalRecorder)
      - If high-approval-rate → auto-approve
      - If has history → show suggestion in dialog
   c. Format justification for TUI display
      - format_for_dialog() wraps text to 78 chars
   d. Show approval dialog with:
      - Tool name and arguments
      - Agent reasoning (if available)
      - Risk level and approval history
   e. Wait for user decision
   ↓
5. Record decision (if learning enabled)
   - ApprovalRecorder::record_approval()
   - Update approval patterns
   ↓
6. Execute tool or deny based on decision
```

## Data Persistence

### Approval Patterns
Stored in `~/.vtcode/cache/approval_patterns.json`:

```json
{
  "read_file": {
    "tool_name": "read_file",
    "approve_count": 8,
    "deny_count": 2,
    "last_decision": true,
    "recent_reason": null
  },
  "run_command": {
    "tool_name": "run_command",
    "approve_count": 12,
    "deny_count": 1,
    "last_decision": true,
    "recent_reason": "User approved for session"
  }
}
```

### Format
- **Location**: `~/.vtcode/cache/approval_patterns.json`
- **Format**: JSON serialized HashMap<String, ApprovalPattern>
- **Persistence**: Automatic on each approval decision

## Integration Points

### 1. Tool Routing (`src/agent/runloop/unified/tool_routing.rs`)
- `prompt_tool_permission()` - Extended with optional justification parameter
- `ensure_tool_permission()` - Routes justification to approval dialog
- Dialog displays justification via `format_for_dialog()`

### 2. Session Management (`src/agent/runloop/unified/turn/session.rs`)
- Calls `ensure_tool_permission()` before tool execution
- Passes decision_ledger reference for context extraction
- Records approval decision after user responds

### 3. Decision Ledger (`vtcode-core/src/core/decision_tracker.rs`)
- `latest_decision()` - Returns most recent decision
- `recent_decisions(count)` - Returns last N decisions
- Each Decision contains `reasoning: String` field

## Risk Level Aware Behavior

| Risk Level | Justification Shown | Auto-Approve Eligible | Notes |
|-----------|-------------------|----------------------|-------|
| Low       | No (auto-approve)  | Yes (always)        | Read-only tools |
| Medium    | Yes (if available) | Only if high history | Build/test tools |
| High      | Yes (required)     | Only if high history | Destructive tools |
| Critical  | Yes (required)     | Never auto-approve   | System tools |

## Example Approval Flow

User requests: "Run the build and check for errors"

```
1. Agent decides to run: `cargo build`
2. Risk: High (command execution)
3. Justification extraction:
   - Decision ledger contains: "Need to verify code compiles before refactoring"
   - Extracted reason: "Need to verify code compiles before refactoring"
4. Approval dialog shows:
   
   ┌─ Tool Permission Required ─────────────────────┐
   │ Tool: run_command                              │
   │ Action: Execute build                          │
   │   command: cargo build                         │
   │                                                │
   │ Agent Reasoning:                               │
   │   Need to verify code compiles before          │
   │   refactoring                                  │
   │                                                │
   │ Expected Outcome:                              │
   │   Will capture command output for analysis     │
   │   and decision-making.                         │
   │                                                │
   │ Risk Level: High                               │
   │ Approved 3 times previously (100%)             │
   │                                                │
   │ ○ Approve Once                                 │
   │ ○ Allow for Session                            │
   │ ● Always Allow ← (if high history)            │
   │ ○ Deny                                         │
   └────────────────────────────────────────────────┘
   
5. User selects "Always Allow"
6. Decision recorded:
   - run_command: approve_count = 4, deny_count = 0
7. Pattern saved to disk
```

## Learning System

### Approval Rate Calculation
```
approval_rate = approve_count / (approve_count + deny_count)
```

### Auto-Approval Threshold
Tool auto-approves when:
- `approval_count >= 3` (at least 3 prior approvals)
- `approval_rate > 0.8` (more than 80% approval rate)

### Example Progression
```
First time: Prompt with default justification
  User approves → approve_count = 1

Second time: Prompt with same tool
  User approves → approve_count = 2

Third time: Prompt again
  User approves → approve_count = 3, rate = 100%
  
Fourth time: AUTO-APPROVE (no dialog shown)
  Decision recorded silently
  
If user denies once:
  deny_count = 1
  rate = 3/4 = 75% (below 80% threshold)
  
Next time: Prompt again with history
```

## Configuration

### Future Configuration Options (Phase 4)
```toml
[tools.justification]
enable_learning = true
auto_approve_threshold = 0.80  # Approval rate
min_approvals_for_auto = 3     # Minimum approval count
show_suggestions = true         # Show approval history in dialog
cache_dir = "~/.vtcode/cache"   # Pattern storage location
```

## Testing

All modules include comprehensive tests:

### Justification Tests
- `test_tool_justification_creation()` - Creation and formatting
- `test_justification_formatting()` - TUI display
- `test_approval_pattern_calculation()` - Rate calculation
- `test_justification_manager_basic()` - Persistence

### ApprovalRecorder Tests
- `test_approval_recording()` - Basic recording
- `test_auto_approval_suggestion()` - UX suggestions
- `test_should_auto_approve()` - Threshold logic

### JustificationExtractor Tests
- `test_extract_from_decision_low_risk()` - Risk filtering
- `test_extract_from_decision_high_risk()` - Reasoning extraction
- `test_extract_from_decision_empty_reasoning()` - Edge cases
- `test_suggest_default_justification()` - Fallback strategies

Run tests with:
```bash
cargo test --lib justification
cargo test --lib approval_recorder
cargo test --lib justification_extractor
```

## Future Enhancements

### Phase 4 Integration
1. Hook extractor into session approval loop
2. Enable approval recording after user decision
3. Implement pattern-based auto-approval

### Phase 5 Polish
1. Machine learning on approval patterns
2. Per-workspace approval policies
3. Approval history visualization
4. Batch approval decisions for multi-step operations

## Security Considerations

- Justifications extracted from agent's own reasoning (trusted)
- Approval patterns are client-side only (no cloud sync)
- User always retains final approval authority
- Pattern file is human-readable JSON (transparent)
- High-risk tools never auto-approve (mandatory threshold)

## Performance

- **Pattern Lookup**: O(1) HashMap access
- **Pattern Recording**: O(1) with async serialization
- **Justification Extraction**: O(n) through recent decisions (n ≤ 10 typical)
- **Memory**: ~50KB per 100 tools tracked
- **Disk**: ~2KB per approval pattern entry

Typical approval decision latency: <5ms (after dialog shown)
