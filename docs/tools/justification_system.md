# Agent Justification System

## Overview

The Agent Justification System enables VT Code to capture and present agent reasoning when requesting approval for
high-risk tool execution. This improves the user experience by explaining **why** the agent needs to run potentially
dangerous operations, and learns from user approval patterns to reduce friction over time.

## Architecture

### Core Components

#### 1. **ToolJustification** (`crates/codegen/vtcode-core/src/tools/registry/justification.rs`)

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
- `format_for_dialog()` - Verbose multi-line format for logs/tests (78-char wrap with `Agent Reasoning:` /
  `Expected Outcome:` / `Risk Level:` headers)

#### 2. **JustificationManager** (`crates/codegen/vtcode-core/src/tools/registry/justification.rs`)

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

#### 3. **ApprovalRecorder** (`crates/codegen/vtcode-core/src/tools/registry/approval_recorder.rs`)

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

#### 4. **JustificationExtractor** (`crates/codegen/vtcode-core/src/tools/registry/justification_extractor.rs`)

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

- `exec_command` - Execute shell operations or build/test
- `write_stdin` - Continue a live command session
- `apply_patch` - Implement code changes
- `code_search` - Search recognised definitions, syntactic usages, literal text, and matching paths in the advanced
  profile

## Data Flow

### Approval Request Flow

```text
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
       - If high-approval-rate → auto-approve (no dialog shown)
    c. Format justification for TUI display
       - Approval dialog uses minimal single-row fields
         (`What the agent is trying to do:` / `Risk:` only), first logical
         line only + middle-truncation (160 chars, 32 for risk), via
         `compact_justification_lines()` in `permission_prompt.rs` so the HITL popup stays scannable.
         Expected outcome and auto-approval suggestions remain in logs only.
         `format_for_dialog()` remains the verbose log/test format.
    d. Show approval dialog with:
       - A question-style overlay title for shell commands (`Would you
         like to run the following command?`); other tools keep the
         generic title. No `Tool:` jargon, no `COMMAND` / `WHY`
         headers, no `│` gutter. Command tools show an indented,
         `$ `-prefixed, syntax-highlighted command block; file tools
         show the diff preview directly.
       - An `Environment:` row for shell commands naming the requested
         sandbox posture (`default policy`, `default policy + extra
         grants`, `escalated privileges`, `no sandbox`).
       - The full shell command for command tools, without per-line
         truncation. Modal wrapping owns viewport width; scripts beyond
         8 lines collapse middle lines behind an explicit omission
         count so head and tail stay reviewable.
       - A human-friendly action sentence for non-command tools
         (for example, `The agent wants to edit file src/main.rs and needs
         your approval.`)
       - Agent goal + risk level as subordinate context rows, with a fallback
         explanation when the agent provided no details
       - Concise options; the permanent option middle-truncates long
         command labels to 60 chars so the executable prefix and the
         trailing flags stay visible.
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

Stored in the user cache directory's approval-pattern file:

```json
{
    "apply_patch": {
        "tool_name": "apply_patch",
        "approve_count": 8,
        "deny_count": 2,
        "last_decision": true,
        "recent_reason": null
    },
    "exec_command": {
        "tool_name": "exec_command",
        "approve_count": 12,
        "deny_count": 1,
        "last_decision": true,
        "recent_reason": "User approved for session"
    }
}
```

### Format

- **Location**: `<cache>/approval_patterns.json`
- **Format**: JSON serialized HashMap<String, ApprovalPattern>
- **Persistence**: Automatic on each approval decision

## Integration Points

### 1. Tool Routing (`src/agent/runloop/unified/tool_routing.rs`)

- `prompt_tool_permission()` - Extended with optional justification parameter
- `ensure_tool_permission()` - Routes justification to approval dialog
- Dialog displays the minimal justification (`What the agent is trying to do:` / `Risk:` single rows); the verbose
  `format_for_dialog()` output remains for logs/tests

### 2. Session Management (`src/agent/runloop/unified/turn/session.rs`)

- Calls `ensure_tool_permission()` before tool execution
- Passes decision_ledger reference for context extraction
- Records approval decision after user responds

### 3. Decision Ledger (`crates/codegen/vtcode-core/src/core/decision_tracker.rs`)

- `latest_decision()` - Returns most recent decision
- `recent_decisions(count)` - Returns last N decisions
- Each Decision contains `reasoning: String` field

## Risk Level Aware Behavior

| Risk Level | Justification Shown | Auto-Approve Eligible | Notes             |
| ---------- | ------------------- | --------------------- | ----------------- |
| Low        | No (auto-approve)   | Yes (always)          | Read-only tools   |
| Medium     | Yes (if available)  | Only if high history  | Build/test tools  |
| High       | Yes (required)      | Only if high history  | Destructive tools |
| Critical   | Yes (required)      | Never auto-approve    | System tools      |

## Example Approval Flow

User requests: "Run the build and check for errors"

```text
1. Agent decides to run: `cargo build`
2. Risk: High (command execution)
3. Justification extraction:
   - Decision ledger contains: "Need to verify code compiles before refactoring"
   - Extracted reason: "Need to verify code compiles before refactoring"
4. Approval dialog shows (plain permission language, no COMMAND / WHY headers):

    Would you like to run the following command?
    Environment: default policy
        $ cargo build
      What the agent is trying to do: Need to verify code compiles before refactoring
      Risk: High

     Approve Once               Allow this time only
     Allow for Session          For the current session
     Always approve…            Remember `cargo build` in this workspace
     ──────────────────────────
     Deny Once                  Ask again next time


5. User selects "Always approve"
6. Decision recorded:
   - exec_command: approve_count = 4, deny_count = 0
7. Pattern saved to disk
```

For a demonstrably safe read-only shell command that belongs to a learned family (for example a write-free
`awk 'NR>=a && NR<=b' README.md`, `sed -n <range> <path>`, or `find <dir>` read), the "Always approve" row is scoped to
the family instead of the exact invocation — the subtitle names the family (for example, "Remember safe awk reads under
README.md in this workspace"), and an equivalent read with different arguments reuses the approval instead of
re-prompting. Mutating shapes such as `awk 'print > file'`, `find -delete`, or `sed -i` are never part of a learned
family.

## Learning System

### Approval Rate Calculation

```text
approval_rate = approve_count / (approve_count + deny_count)
```

### Auto-Approval Threshold

Tool auto-approves when:

- `approval_count >= 3` (at least 3 prior approvals)
- `approval_rate > 0.8` (more than 80% approval rate)

### Example Progression

```text
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
cache_dir = "<cache>"   # Pattern storage location
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
