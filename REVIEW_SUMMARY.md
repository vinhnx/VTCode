# VTCode Configuration Review - Executive Summary

## Objective
Review `vtcode.toml` and core agent logic to identify and remove unnecessary complexity while preserving core functionality.

## Key Findings

### ✅ Core Agent Logic is Solid
The core LLM inference loop and tool execution pipeline are clean and well-architected:
- Clear separation: `vtcode-core/` (library) vs `src/` (CLI/TUI)
- Unified runloop in `src/agent/runloop/unified/` is the canonical implementation
- Provider abstraction is well-designed (OpenAI, Anthropic, Gemini, OpenRouter, etc.)
- Tool security model is comprehensive

**Conclusion:** Core functionality is NOT at risk. Configuration is the issue, not code.

---

## Complexity Breakdown

### Configuration Size: 775 lines
```
├── Essential (100+ lines)
│   ├── [agent] - LLM provider, model, core generation settings
│   ├── [context] - Token budgeting, trimming, memory management
│   ├── [tools] - Security policies, execution limits
│   ├── [pty] - Terminal configuration
│   └── [timeouts] - Execution ceilings
│
├── Valuable (150+ lines)
│   ├── [prompt_cache] - Provider-specific caching (8 providers)
│   ├── [mcp] - Model Context Protocol (extensible)
│   ├── [acp] - IDE integration
│   └── [commands/permissions] - Security rules
│
└── Experimental/Dead (100+ lines)
    ├── [agent.vibe_coding] - Entity resolution (enabled but experimental)
    ├── [context.semantic_compression] - DEAD CONFIG (never read)
    ├── [context.tool_aware_retention] - DEAD CONFIG (never read)
    ├── [hooks.lifecycle] - All commented out (29 lines)
    ├── [context.ledger] - Active but marked experimental
    ├── [telemetry] - Mix of active (trajectory) + experimental (dashboards)
    └── Various UI/onboarding options
```

---

## Specific Issues Found

| Issue | Severity | Lines | Fix |
|-------|----------|-------|-----|
| Dead semantic_compression config | Medium | 8 | Remove from config (code stays) |
| Dead tool_aware_retention config | Medium | 2 | Remove from config (code stays) |
| Commented hooks section | Low | 29 | Remove entirely |
| Vibe_coding enabled by default | Low | 18 | Disable by default |
| Experimental features mixed with core | Medium | 40+ | Move to docs/experimental/ |

**Total removable:** ~37 lines of config (~5% reduction)  
**Total dead code:** 0 lines (all code paths are used or properly disabled)

---

## Core vs. Experimental Breakdown

### ✅ MUST KEEP (Core Execution)
```toml
[agent]              # LLM provider & generation
[context]            # Token budgeting & memory
[context.ledger]     # Decision tracking (embedded in runloop)
[telemetry]
  trajectory_enabled = true    # REQUIRED (embedded in runloop)
[tools]              # Security policies
[pty]                # Terminal config
```

### ⚠️  KEEP BUT MARK EXPERIMENTAL
```toml
[agent.vibe_coding]  # Entity resolution (disabled by default)
[telemetry]
  dashboards_enabled = false       # Experimental
  bottleneck_tracing = false       # Experimental
```

### ❌ REMOVE FROM CONFIG
```toml
[context.semantic_compression]     # Config is dead, code disabled
[context.tool_aware_retention]     # Config is dead, code disabled
[hooks.lifecycle]                  # All commented, experimental
```

---

## Impact Assessment

### Zero Breaking Changes ✅
- Removing config only affects unused features
- Trajectory and decision ledger are required (keep enabled)
- Vibe_coding is experimental (disabling by default is expected)
- No code changes needed

### Core Functionality: Preserved ✅
- LLM inference: No impact
- Tool execution: No impact  
- Context management: No impact
- Security policies: No impact

### Complexity Reduction: ~5%
- Remove ~37 lines of config
- Add 100 lines of experimental documentation
- Net: Clearer, more maintainable, with experimental features documented

---

## Action Items (In Priority Order)

### 🔴 Required (Do First)
1. Remove dead semantic_compression config (2 lines) - 2 min
2. Remove dead tool_aware_retention config (2 lines) - 1 min
3. Remove commented hooks section (29 lines) - 3 min

### 🟡 Recommended (Do Next)
4. Disable vibe_coding by default (1 line change) - 1 min
5. Create `docs/experimental/HOOKS.md` - 10 min
6. Create `docs/experimental/VIBE_CODING.md` - 8 min
7. Create `docs/experimental/CONTEXT_OPTIMIZATION.md` - 8 min

### 🟢 Nice to Have (Optional)
8. Consolidate prompt caching docs
9. Simplify MCP default config

---

## Three-Phase Implementation Plan

### Phase 1: Configuration (10 minutes)
- Remove dead config sections
- Change vibe_coding to `enabled = false`
- Verify telemetry settings correct

### Phase 2: Documentation (26 minutes)
- Create `docs/experimental/` directory
- Document hooks, vibe_coding, context optimization
- Update existing docs if needed

### Phase 3: Verification (2 minutes + build)
- Parse and build (`cargo check`)
- Run tests (`cargo nextest run`)
- Verify agent starts

**Total time:** ~50 minutes + build time
**Risk level:** Very low (config and docs only)
**Reversibility:** 100% (git revert if needed)

---

## Configuration Size Comparison

### Before Cleanup
```
Total lines: 775
Experimental/dead: 37 lines (4.8%)
Actual complexity: High (many interdependent features)
User confusion: "Which features are stable?"
```

### After Cleanup
```
Total lines: 738
Experimental/dead: 0 lines in main config
Actual complexity: Moderate (core features only)
User confusion: Low (experimental features documented separately)
```

---

## Questions & Answers

**Q: Will removing config break anything?**
A: No. These configs are either:
- Never read by code (semantic_compression)
- Disabled by default (hooks)
- Commented out (all hook examples)

**Q: Should we remove the code too?**
A: No. Keep the code but disable in config. This allows future re-enablement.

**Q: Is trajectory logging required?**
A: Yes. It's embedded in `src/agent/runloop/unified/` and required for execution context.

**Q: Can users still enable experimental features?**
A: Yes. After moving to docs/experimental/, users can copy config into their vtcode.toml.

**Q: What about decision ledger?**
A: Keep enabled - it's core execution tracking, embedded in the runloop.

---

## Files to Create/Modify

### Modify
- `vtcode.toml` - Remove 37 lines, change 1 line

### Create
- `docs/experimental/HOOKS.md` - ~40 lines
- `docs/experimental/VIBE_CODING.md` - ~35 lines
- `docs/experimental/CONTEXT_OPTIMIZATION.md` - ~35 lines

### Check & Update
- `docs/config.md` - Remove dead section references (if any)
- `README.md` - Verify no references to experimental features

---

## Next Steps

1. **Review this summary** - Confirm alignment with goals
2. **Run Phase 1 cleanup** - 10 minute configuration changes
3. **Create experimental docs** - 26 minutes documentation
4. **Verify build** - Ensure no regressions
5. **Commit & close** - Clean git history

---

## Appendix: Code Locations

### Core Agent Loop
- Entry: `src/main.rs` → `src/agent/`
- Unified runloop: `src/agent/runloop/unified/run_loop.rs`
- Config loading: `vtcode-config/src/loader/`

### Dead Configuration
- Semantic compression: Never read from config
  - Code disabled by default: `vtcode-core/src/config/constants.rs:1335`
  - Config example: `vtcode.toml.example:685-688`
- Hooks: All examples commented
  - Empty placeholders: `vtcode.toml:746-756`
  - Examples: `vtcode.toml:758-774`

### Required Features
- Trajectory: Used in 20+ files in `src/agent/runloop/unified/`
- Decision Ledger: Used in `tool_ledger.rs`, `tool_pipeline.rs`, `turn_loop.rs`
- Context Management: Used throughout agent loop

---

**Status:** ✅ Review Complete - Ready for Implementation

**Recommendation:** Proceed with Phase 1 configuration cleanup, then Phase 2 documentation. This will reduce configuration noise while preserving all core functionality.
