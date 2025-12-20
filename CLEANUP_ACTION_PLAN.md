# VTCode Configuration Cleanup - Action Plan

## Overview
Reduce configuration complexity from vtcode.toml by removing dead code (semantic compression config), disabling experimental features by default, and moving documentation to experimental guides.

**Scope:** Configuration and documentation only. No core code changes required.

---

## Phase 1: Remove Dead Configuration (15 minutes)

### Task 1a: Remove commented hooks section from vtcode.toml
**File:** `vtcode.toml` (lines 746-774)

**Changes:**
```diff
- [hooks.lifecycle]
- session_start = []
- session_end = []
- user_prompt_submit = []
- pre_tool_use = []
- post_tool_use = []
- 
- [model]
- skip_loop_detection = false
- loop_detection_threshold = 1
- loop_detection_interactive = true
- 
- # [hooks.lifecycle]
- # # Session start hook that provides project context
- # session_start = [{ hooks = [{ command = "$VT_PROJECT_DIR/.vtcode/hooks/setup-env.sh", timeout_seconds = 30 }] }]
- # 
- # # Session end hook for cleanup/logging
- # session_end = [{ hooks = [{ command = "$VT_PROJECT_DIR/.vtcode/hooks/log-session-end.sh" }] }]
- # 
- # # Security validation for bash commands
- # pre_tool_use = [
- #   { matcher = "Bash", hooks = [{ command = "$VT_PROJECT_DIR/.vtcode/hooks/security-check.sh", timeout_seconds = 10 }] }
- # ]
- # 
- # # Post-tool hook to run linters and log commands
- # post_tool_use = [
- #   { matcher = "Write|Edit", hooks = [{ command = "$VT_PROJECT_DIR/.vtcode/hooks/run-linter.sh" }] },
- #   { matcher = "Bash", hooks = [{ command = "$VT_PROJECT_DIR/.vtcode/hooks/log-command.sh" }] },
- #   { matcher = ".*", hooks = [{ command = "$VT_PROJECT_DIR/.vtcode/hooks/log-tool-usage.sh" }] }
- # ]
```

**Keep only:**
```toml
[model]
skip_loop_detection = false
loop_detection_threshold = 1
loop_detection_interactive = true
```

**Result:** Remove 29 commented lines + empty section

---

### Task 1b: Remove dead semantic compression configuration
**File:** `vtcode.toml` (lines 481-488)

**Remove entirely:**
```toml
[context]
# ...existing...
semantic_compression = false
tool_aware_retention = false
max_structural_depth = 3
preserve_recent_tools = 5
```

**Why:** Configuration is never read. Tree-Sitter analysis is disabled by default and no code path uses these settings.

**Result:** Remove 8 lines of dead configuration

**Verification:** Check `vtcode-config/src/constants.rs`
```bash
grep -A2 "DEFAULT_SEMANTIC_COMPRESSION_ENABLED\|DEFAULT_TOOL_AWARE_RETENTION" \
  vtcode-config/src/constants.rs
```
Should show both are `false` and overrides don't work (config not read).

---

## Phase 2: Disable Experimental Features (10 minutes)

### Task 2a: Disable Vibe Coding by default
**File:** `vtcode.toml` (line 172)

**Change:**
```diff
- [agent.vibe_coding]
- enabled = true
+ [agent.vibe_coding]
+ enabled = false
```

**Why:** Feature is for research/analysis, not core agent functionality. Users enabling it explicitly is better than opt-out.

**Files affected:**
- `src/agent/runloop/prompt.rs:670` - Tests set this to true, leave as-is

---

### Task 2b: Ensure telemetry defaults are correct
**File:** `vtcode.toml` (lines 525-532)

**Verify/Set:**
```toml
[telemetry]
trajectory_enabled = true      # KEEP - required for runloop
dashboards_enabled = false     # Already false - good
sample_interval_ms = 1000
retention_days = 14
bottleneck_tracing = false     # Experimental - keep false
```

**Why:** `trajectory_enabled = true` is required (core runloop depends on it). Experimental features (dashboards, bottleneck) should stay disabled.

---

## Phase 3: Create Experimental Documentation (20 minutes)

### Task 3a: Create `docs/experimental/HOOKS.md`

**Content:**
```markdown
# Lifecycle Hooks (Experimental)

Feature status: **Documented but not enabled by default**

## Purpose
Execute custom scripts at key points in the VT Code lifecycle:
- Session start/end
- User prompt submission
- Pre/post tool execution

## Configuration

Enable hooks in `vtcode.toml`:

```toml
[hooks.lifecycle]
session_start = [{ hooks = [{ command = "path/to/script.sh", timeout_seconds = 30 }] }]
```

## Examples

### Run linter after file writes
```toml
[hooks.lifecycle]
post_tool_use = [
  { matcher = "Write|Edit", hooks = [{ command = ".vtcode/hooks/run-linter.sh" }] }
]
```

### Security validation for bash commands
```toml
[hooks.lifecycle]
pre_tool_use = [
  { matcher = "Bash", hooks = [{ command = ".vtcode/hooks/security-check.sh", timeout_seconds = 10 }] }
]
```

## Notes
- Hooks are matched using regex patterns
- Timeouts prevent hung execution
- Not enabled by default
```

---

### Task 3b: Create `docs/experimental/VIBE_CODING.md`

**Content:**
```markdown
# Vibe Coding (Experimental)

Feature status: **Experimental, disabled by default**

## Purpose
Entity-aware context enrichment that tracks:
- Variable and function references
- Workspace state changes
- Conversation memory across turns
- Relative value inference (importance of context)

## Enabling

Set in `vtcode.toml`:

```toml
[agent.vibe_coding]
enabled = true
min_prompt_length = 5
min_prompt_words = 2
enable_entity_resolution = true
entity_index_cache = ".vtcode/entity_index.json"
max_entity_matches = 5
track_workspace_state = true
max_recent_files = 20
track_value_history = true
enable_conversation_memory = true
max_memory_turns = 50
enable_pronoun_resolution = true
enable_proactive_context = true
max_context_files = 3
max_context_snippets_per_file = 20
max_search_results = 5
enable_relative_value_inference = true
```

## Performance Impact
- Higher memory usage (entity index cache)
- Increased context processing time
- Beneficial for long-running sessions with complex state

## Known Limitations
- Not suitable for resource-constrained environments
- May impact response latency
- Entity resolution can produce false positives
```

---

### Task 3c: Create `docs/experimental/CONTEXT_OPTIMIZATION.md`

**Content:**
```markdown
# Context Optimization (Experimental)

Feature status: **Not implemented, planned for future**

## Overview
Two advanced context management techniques planned for future releases:

### Semantic Compression
- AST-based structural analysis of context
- Prunes low-relevance subtrees
- Preserves semantic meaning while reducing tokens
- Status: Design phase only

### Tool-Aware Retention
- Keeps recent tool outputs longer when related actions are in progress
- Maintains awareness of active operations
- Improves tool orchestration accuracy
- Status: Prototype phase

## Configuration (Reserved for future use)

These settings are reserved but not currently used:

```toml
[context]
semantic_compression = false              # Not implemented
tool_aware_retention = false              # Not implemented
max_structural_depth = 3                  # Reserved
preserve_recent_tools = 5                 # Reserved
```

## Timeline
- Planned for evaluation in Q2 2025
- Requires significant testing with large context windows
- May introduce breaking changes to context format
```

---

## Phase 4: Update Documentation (10 minutes)

### Task 4a: Update `docs/config.md` if exists
Search for references to removed sections:
```bash
grep -n "semantic_compression\|tool_aware_retention\|hooks.lifecycle" docs/config.md
```

**Remove:** Lines referencing deleted config sections

---

### Task 4b: Update `.env.example` if needed
Ensure no experimental features are documented as default.

---

## Verification Steps

### 1. Check config parses correctly
```bash
cargo build --release 2>&1 | grep -i "config\|parse" | head -20
```

### 2. Run tests
```bash
cargo nextest run config 2>&1 | tail -20
```

### 3. Verify core functionality
```bash
cargo run -- ask "Hello world" 2>&1 | head -10
```

### 4. Check no compilation warnings
```bash
cargo clippy 2>&1 | grep -i "unused\|dead" | wc -l
# Should be minimal or same as baseline
```

---

## Files to Modify

| File | Changes | Lines |
|------|---------|-------|
| `vtcode.toml` | Remove hooks section, semantic compression config | -37 |
| `vtcode.toml` | Disable vibe_coding | 1 |
| `docs/experimental/HOOKS.md` | Create new | ~40 |
| `docs/experimental/VIBE_CODING.md` | Create new | ~35 |
| `docs/experimental/CONTEXT_OPTIMIZATION.md` | Create new | ~35 |
| `docs/config.md` | Remove dead section references | -5 to -10 |

**Total work:** 55 minutes  
**Complexity reduction:** ~5% (38 lines removed, no code changes)  
**Risk level:** Very low (only config/docs changes)

---

## Rollback Plan

All changes are in configuration and documentation only:
1. Revert `vtcode.toml` to previous version
2. Delete experimental docs (or restore from git)
3. No code rebuild needed

---

## Success Criteria

- ✅ Config file parses without errors
- ✅ All tests pass (`cargo nextest run`)
- ✅ Agent starts normally (`cargo run`)
- ✅ No new clippy warnings
- ✅ `vtcode.toml` is shorter and clearer
- ✅ Experimental features documented and disabled
- ✅ Core agent functionality unchanged
