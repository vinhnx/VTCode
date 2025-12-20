# VTCode Configuration Complexity Audit

## Summary
The `vtcode.toml` contains **7+ complex, partially-unused, or experimental features** that add configuration overhead without significant runtime benefit in current usage patterns. The core agent/LLM functionality is **NOT dependent** on these features.

---

## Complex/Redundant/Unused Features

### 1. **Vibe Coding** (Lines 171-188)
**Status:** Experimental, conditionally used  
**Code Location:** `src/agent/runloop/prompt.rs:364-504`  
**Impact:** ~18 config lines

**What it does:**
- Entity resolution (tracks variable references)
- Workspace state tracking
- Conversation memory
- Pronoun resolution
- Relative value inference

**Complexity:**
- Requires external entity index cache (`.vtcode/entity_index.json`)
- Multiple interdependent boolean flags
- Max limits on memory turns and context files
- Only enabled in test mode (`if !vtc.agent.vibe_coding.enabled`)

**Recommendation:** 
- **KEEP but DISABLE by default** - Remove from user-facing docs
- Set `enabled = false` in example config
- Keep code but mark as experimental

### 2. **Semantic Compression & Tool-Aware Retention** (Lines 481-488)
**Status:** Experimental, NEVER ENABLED (defaults to `false`)  
**Code Location:** 
- `src/agent/runloop/context.rs:34-37`
- `vtcode-core/src/llm/` context management

**What it does:**
- Structural analysis of context (AST-based pruning)
- Retains recent tool outputs during active operations
- Max depth limiting for semantic trees

**Complexity:**
- 8 configuration parameters
- Requires Tree-Sitter integration
- Semantic analyzer setup overhead
- **NEVER ACTUALLY USED** (both disabled in default config)

**Recommendation:**
- **REMOVE from vtcode.toml entirely** - Config is dead
- Keep code but mark `const DEFAULT_*_ENABLED = false`
- Move to `docs/experimental/` if planning to revisit

### 3. **Prompt Caching by Provider** (Lines 612-660)
**Status:** Active but provider-specific  
**Code Location:** `vtcode-core/src/llm/providers/`

**What it does:**
- Cache config for OpenAI, Anthropic, Gemini, OpenRouter, Moonshot, xAI, DeepSeek, Z.AI
- Per-provider TTL, breakpoints, retention policies

**Complexity:**
- 8 separate `[prompt_cache.providers.*]` sections
- Each has different semantics (implicit vs. explicit, TTL vs. retention)
- Z.AI disabled, others enabled by default

**Recommendation:**
- **KEEP** - This is necessary for multi-provider support
- Consolidate under a single schema if possible (some fields duplicate)
- Move provider-specific settings to `docs/providers/CACHING.md`

### 4. **MCP (Model Context Protocol)** (Lines 662-702)
**Status:** Active, extensible  
**Code Location:** `vtcode-core/src/mcp/`

**What it does:**
- Connects external tools (MCP servers)
- Currently: `mcp-server-time` and `mcp-server-fetch`
- Per-provider concurrency, timeout, env overrides

**Complexity:**
- 9 top-level config keys + nested provider arrays
- Renderer profiles, allowlists, security settings
- Connection pooling, tool caching

**Recommendation:**
- **KEEP but SIMPLIFY:**
  - Default to `enabled = true` (good for extensibility)
  - Remove unused renderers (`context7`, `sequential-thinking`)
  - Document which MCP servers are stable vs. experimental
  - Move MCP server config to `docs/mcp/SERVER_REGISTRY.md`

### 5. **Hooks & Lifecycle** (Lines 746-774)
**Status:** Documented but not used (all commented)  
**Code Location:** `vtcode-config/src/hooks.rs`

**What it does:**
- Pre/post tool execution hooks
- Session start/end hooks
- Hook matchers (regex patterns)

**Complexity:**
- 5 hook types with command execution
- Timeout management per hook
- Matcher patterns for conditional execution

**Recommendation:**
- **REMOVE from default config** - All examples are commented
- Move to `docs/experimental/HOOKS.md`
- If needed later, re-add with simpler default

### 6. **Decision Ledger** (Lines 505-515)
**Status:** Active, embedded in execution context  
**Code Location:** `vtcode-core/src/core/decision_tracker.rs`, `src/agent/runloop/unified/*`

**What it does:**
- Tracks important decisions in context (ACTIVE - not optional)
- Embedded in RunLoopContext and Session
- Used in tool output handling and tool pipeline

**Complexity:**
- Integrated tightly into the unified runloop
- Max entries limit with eviction
- Ledger summary in prompts (configurable via `context.ledger.include_in_prompt`)

**Recommendation:**
- **KEEP - This is core to execution tracking**
- Keep enabled by default (already enabled)
- Only disable if explicitly reducing feature set

### 7. **Telemetry & Trajectory** (Lines 525-532)
**Status:** Active, embedded in execution pipeline  
**Code Location:** 
- `src/agent/runloop/telemetry.rs`
- `vtcode-core/src/core/trajectory.rs`
- `src/agent/runloop/unified/*` (many files reference `traj`)

**What it does:**
- Logs execution trajectory (ACTIVE - tightly coupled)
- Dashboard generation (experimental)
- Bottleneck tracing (experimental)
- Route recording for execution auditing

**Complexity:**
- Sample interval configuration
- Retention policy (days)
- Dashboard/bottleneck features are experimental

**Recommendation:**
- **KEEP trajectory (core), DISABLE experimental features:**
  ```toml
  [telemetry]
  trajectory_enabled = true   # REQUIRED - don't change
  dashboards_enabled = false  # Experimental, disable
  bottleneck_tracing = false  # Experimental, disable
  ```
- Trajectory is embedded in unified runloop, cannot be disabled
- Only experimental features (dashboards, bottleneck_tracing) can be safely disabled

### 8. **Checkpointing** (Lines 160-169)
**Status:** Experimental, disabled by default ✓  
**Code Location:** `vtcode-core/src/cache/`

**Assessment:** Already disabled, good default. Keep as-is.

---

## Core Agent/LLM Functionality (PRESERVE)

These sections are **essential** and should NOT be removed:

### Required Configuration
```toml
[agent]
provider = "ollama"                    # LLM provider selection
default_model = "gpt-oss:20b-cloud"   # Model selection
temperature = 0.7                      # LLM generation settings
max_tokens = 2000                      # Context window budgeting
max_conversation_turns = 50            # Memory management
reasoning_effort = "low"               # Model-specific optimization

[context]
max_context_tokens = 128000            # Token budget for LLM
trim_to_percent = 90                   # Context pruning threshold
preserve_recent_turns = 12             # Memory preservation
```

### Required Tool Configuration
```toml
[tools]
default_policy = "prompt"              # Security policy
max_tool_loops = 50                    # Safety limits
max_repeated_tool_calls = 2

[tools.policies]
# Individual tool permissions
```

### Required PTY Configuration
```toml
[pty]
enabled = true
default_rows = 24
default_cols = 120
max_sessions = 10
command_timeout_seconds = 300
```

---

## Recommendations for Cleanup

### Phase 1: Immediate Removal (No Code Changes)
Remove these config sections from `vtcode.toml`:
1. `[hooks.lifecycle]` - Remove entire commented section (all 29 lines are comments)
2. `[context.ledger]` - Move to experimental (but keep config, it's still used)
3. `[telemetry]` experimental features - Keep `trajectory_enabled = true`, disable dashboards

**Files to Update:**
- `vtcode.toml` (remove ~30 commented lines from hooks section)
- Create `docs/experimental/HOOKS.md`

### Phase 2: Disable by Default (Config Only)
Modify these to be experimental-only:
1. **Vibe Coding:**
   - Set `enabled = false` in `vtcode.toml` (currently `true`)
   - Document in `docs/experimental/VIBE_CODING.md`

2. **Semantic Compression & Tool-Aware Retention:**
   - Remove from `vtcode.toml` (config is completely unused)
   - Move to `docs/experimental/CONTEXT_OPTIMIZATION.md`
   - Code remains available but disabled

3. **Telemetry Experimental Features:**
   - Keep `trajectory_enabled = true` (required for core runloop)
   - Set `dashboards_enabled = false` (default)
   - Set `bottleneck_tracing = false` (default)

**Files to Update:**
- `vtcode.toml` (~30 lines removed/modified)
- Documentation only (no code changes)

### Phase 3: Simplify Active Features (Optional)
Consider consolidating documentation for:
1. **Prompt Caching** - Create `docs/providers/CACHING.md` with simplified examples
2. **MCP** - Reduce default config, document in `docs/mcp/`

---

## Impact on Core Agent

**None.** The core agent loop (in `vtcode-core/src/orchestrator/` and `src/agent/runloop/unified/`) does not depend on:
- Vibe coding
- Semantic compression
- Decision ledger
- Telemetry/trajectory
- Hooks

These are **optional enhancements** that wrap around the core LLM inference loop.

---

## Configuration Size Summary

| Feature | Lines | Complexity | Essential |
|---------|-------|-----------|-----------|
| Vibe Coding | 18 | High (interdependent) | No |
| Semantic Compression | 8 | Medium | No |
| Telemetry | 7 | Low | No |
| Decision Ledger | 9 | Medium | No |
| Hooks | 30 | Medium (commented) | No |
| MCP | 40 | High | No* |
| Prompt Caching | 49 | Medium | Yes** |
| Core Agent | 65 | Low | **Yes** |
| Core Tools | 100+ | Low | **Yes** |

*MCP is optional but valuable for extensibility  
**Prompt caching is recommended for cost/latency but can be simplified

---

## Recommended Default vtcode.toml Structure

```toml
# CORE AGENT CONFIGURATION
[agent]
provider = "ollama"
default_model = "gpt-oss:20b-cloud"
temperature = 0.7
max_tokens = 2000
max_conversation_turns = 50
reasoning_effort = "low"

# UI & INTERACTION
[agent.onboarding]
enabled = true
[agent.custom_prompts]
enabled = true
[ui]
tool_output_mode = "compact"
[pty]
enabled = true

# CORE CONTEXT MANAGEMENT
[context]
max_context_tokens = 128000
trim_to_percent = 90
preserve_recent_turns = 12
# Remove: semantic_compression = false  (dead config)
# Remove: tool_aware_retention = false  (dead config)

# TRACKING (REQUIRED FOR CORE EXECUTION)
[context.ledger]
enabled = true  # Keep - core execution tracking
[telemetry]
trajectory_enabled = true  # REQUIRED - don't disable
dashboards_enabled = false # Experimental
bottleneck_tracing = false # Experimental

# TOOL SECURITY
[tools]
default_policy = "prompt"
max_tool_loops = 50
[tools.policies]
# ...

# PROMPT CACHING (RECOMMENDED)
[prompt_cache]
enabled = true
# Provider-specific caches...

# MCP (OPTIONAL)
[mcp]
enabled = true
# ...

# EXPERIMENTAL FEATURES IN DOCS/EXPERIMENTAL/
# - Vibe Coding (docs/experimental/VIBE_CODING.md)
# - Hooks (docs/experimental/HOOKS.md)
# - Semantic Context Compression (docs/experimental/CONTEXT_OPTIMIZATION.md)
```

**Changes:**
- Remove ~30 lines of commented hooks
- Remove ~8 lines of dead semantic_compression config
- Keep all core tracking (trajectory, ledger) - required for execution
- Mark experimental features as disabled but documented

**Impact:**
- Lines saved: ~38 (from 775 to ~737)
- Complexity reduced: ~5% (most complexity is legitimate)
- Core functionality preserved: 100%
- Breaking changes: None (disabled features are already unused)
