# Context Manager: OpenAI Codex Patterns Applied to VT Code

## Quick Navigation

This folder contains a complete analysis and implementation plan for adopting OpenAI Codex's proven context manager patterns in VT Code.

### 📄 Documents

| Document | Purpose | Audience | Read Time |
|----------|---------|----------|-----------|
| **[CODEX_PATTERNS_SUMMARY.md](./CODEX_PATTERNS_SUMMARY.md)** | Executive overview of patterns & recommendations | Tech Leads, Architects | 10 min |
| **[CONTEXT_MANAGER_ANALYSIS.md](./CONTEXT_MANAGER_ANALYSIS.md)** | Deep dive comparing Codex vs VT Code | Developers, Architects | 20 min |
| **[CONTEXT_MANAGER_IMPLEMENTATION.md](./CONTEXT_MANAGER_IMPLEMENTATION.md)** | Concrete implementation code patterns | Developers | 30 min |
| **[CONTEXT_MANAGER_QUICKSTART.md](./CONTEXT_MANAGER_QUICKSTART.md)** | Day-by-day implementation checklist | Developers | 15 min |
| **[README_CONTEXT_MANAGER.md](./README_CONTEXT_MANAGER.md)** | This file | Everyone | 5 min |

---

## The Problem

VT Code has a **distributed context management system** (ContextOptimizer, TaskRunState, ConversationMemory, etc.), but lacks **centralized conversation history validation**.

**Current issue**: When a user cancels a tool or VT Code crashes during execution, the conversation history can contain "dangling" tool calls without outputs. This creates:

- ❌ Incomplete context for the LLM → Confusion or errors
- ❌ Fragile session restoration → May crash on reload
- ❌ Implicit history state → Hard to debug

---

## The Solution

Adopt **OpenAI Codex's call/output pairing invariants**:

1. **Every tool call must have an output** (success, failed, canceled, or timeout)
2. **Every output must have a matching call**
3. **If a call is missing its output, create a synthetic output** (e.g., "canceled")

This is proven in production by OpenAI's Codex system.

---

## The Recommendation

### Phase 1: Core Invariants (2 weeks, Low Risk)
- Add output status tracking
- Implement `normalize()` to enforce invariants
- Add crash recovery
- Comprehensive tests

**Benefit**: Crash-safe sessions, clear LLM context

### Phase 2: Automatic Recovery (1 week, Low Risk)
- Auto-recover on session load
- Log recovered items for debugging

**Benefit**: Sessions survive interruptions automatically

### Phase 3: Optional Standalone Manager (3 weeks, Future)
- Create centralized `HistoryManager`
- Add telemetry on execution patterns

**Benefit**: Better diagnostics, foundation for deeper optimization

---

## Getting Started

### For Decision Makers
1. Read **[CODEX_PATTERNS_SUMMARY.md](./CODEX_PATTERNS_SUMMARY.md)** (10 min)
2. Review **risk & mitigation** section
3. Approve Phase 1 scope

### For Developers (Ready to Code)
1. Read **[CONTEXT_MANAGER_QUICKSTART.md](./CONTEXT_MANAGER_QUICKSTART.md)** (15 min)
2. Review **[CONTEXT_MANAGER_IMPLEMENTATION.md](./CONTEXT_MANAGER_IMPLEMENTATION.md)** (30 min)
3. Copy code patterns
4. Run tests: `cargo test history_invariant_tests`

### For Architects (Deep Understanding)
1. Read **[CONTEXT_MANAGER_ANALYSIS.md](./CONTEXT_MANAGER_ANALYSIS.md)** (20 min)
2. Review comparison matrix
3. Plan integration points
4. Design Phase 3 (optional)

---

## Key Figures

### Code Impact
- **Files to modify**: ~5 (TaskRunState + integration points)
- **Lines to add**: ~300 (core) + ~150 (tests)
- **Breaking changes**: 0 (fully additive)

### Timeline
- **Phase 1**: 2 weeks (core + tests)
- **Phase 2**: 1 week (recovery)
- **Phase 3**: 3 weeks (optional, later)

### Risk Profile
- **Phase 1**: ✅ **LOW** (additive, opt-in, comprehensive tests)
- **Phase 2**: ✅ **LOW** (auto-recovery, logging)
- **Phase 3**: ⚠️ **MEDIUM** (optional, can defer)

---

## Architecture Overview

```
Codex Context Manager (proven pattern)
        ↓
    Analysis (CONTEXT_MANAGER_ANALYSIS.md)
        ↓
    Design (CONTEXT_MANAGER_IMPLEMENTATION.md)
        ↓
    Phase 1: Core Invariants
    ├── ensure_call_outputs_present()
    ├── remove_orphan_outputs()
    ├── validate_history_invariants()
    ├── normalize()
    └── tests
        ↓
    Phase 2: Crash Recovery (optional)
    ├── recover_from_crash()
    └── auto-recovery on session load
        ↓
    Phase 3: Standalone Manager (future)
    ├── HistoryManager
    ├── Call tracking
    └── Telemetry
```

---

## Core Concepts

### Call/Output Pair Example

```rust
// History before normalization
[
    ToolCall { id: "call-1", tool: "grep_file" },
    // ← Missing output (user canceled)
    ToolCall { id: "call-2", tool: "read_file" },
    ToolOutput { id: "call-2", status: "success" },
]

// After ensure_call_outputs_present()
[
    ToolCall { id: "call-1", tool: "grep_file" },
    ToolOutput { id: "call-1", status: "canceled" }, // ← Synthetic
    ToolCall { id: "call-2", tool: "read_file" },
    ToolOutput { id: "call-2", status: "success" },
]
```

### Validation Report

```rust
let report = state.validate_history_invariants();

// Returns:
HistoryValidationReport {
    missing_outputs: vec![
        MissingOutput { 
            call_id: "call-1", 
            tool_name: "grep_file" 
        }
    ],
    orphan_outputs: vec![], // Outputs without calls
}

// Used for logging and recovery
if !report.is_valid() {
    println!("⚠️ History issues found: {}", report.summary());
}
```

---

## Implementation Checklist

### Phase 1 (Weeks 1-2)

- [ ] **Day 1-2**: Add types (OutputStatus, ToolCallId, ValidationReport)
- [ ] **Day 2-3**: Add methods (ensure_*, remove_orphan, validate, normalize)
- [ ] **Day 3**: Write comprehensive tests
- [ ] **Day 4**: Integrate into session loading/trimming
- [ ] **Day 5**: Verify (clippy, fmt, tests)

### Phase 2 (Week 3)

- [ ] Add `recover_from_crash()` method
- [ ] Call during session restoration
- [ ] Test crash scenarios
- [ ] Add configuration option

### Phase 3 (Week 4+, Optional)

- [ ] Design `HistoryManager` struct
- [ ] Implement call tracking
- [ ] Add telemetry
- [ ] Integrate with existing systems

---

## Success Criteria

✅ All tests pass (unit + integration)
✅ No performance regression
✅ Dangling calls get synthetic outputs
✅ Sessions restore without errors
✅ Orphan outputs are removed
✅ Logs show what was recovered
✅ Feature is opt-in via config

---

## Source

- **OpenAI Codex**: https://github.com/openai/codex/tree/main/codex-rs/core/src/context_manager
- **Codex normalize.rs**: 212 lines handling call/output pairing
- **Codex history.rs**: 296 lines conversation management

---

## FAQ

### Q: Will this change existing APIs?
**A**: No. Phase 1 is purely additive. Methods are added to TaskRunState without modifying existing methods.

### Q: Is this required for VT Code to work?
**A**: No. It's an opt-in enhancement for robustness. Existing code continues to work.

### Q: How much code needs to be written?
**A**: ~300 lines of new code + ~150 lines of tests. Copy-paste ready from CONTEXT_MANAGER_IMPLEMENTATION.md.

### Q: What's the risk?
**A**: Low. Comprehensive tests, non-breaking changes, opt-in feature flag.

### Q: When can we start?
**A**: Immediately. Code is ready to implement. Start with CONTEXT_MANAGER_QUICKSTART.md.

### Q: Do we need all 3 phases?
**A**: Phase 1 is recommended. Phase 2 is nice-to-have. Phase 3 is optional, can defer to future.

---

## Related Files

- **CLAUDE.md**: Development standards for VT Code
- **AGENTS.md**: Agent commands and workflows
- **ARCHITECTURE.md**: System architecture overview
- **docs/SECURITY_MODEL.md**: Security patterns

---

## Questions?

1. **For technical overview**: See CODEX_PATTERNS_SUMMARY.md
2. **For implementation details**: See CONTEXT_MANAGER_IMPLEMENTATION.md
3. **For comparison with Codex**: See CONTEXT_MANAGER_ANALYSIS.md
4. **To get started coding**: See CONTEXT_MANAGER_QUICKSTART.md

---

## Version History

| Date | Status | Version |
|------|--------|---------|
| 2025-12-31 | Analysis Complete | 1.0 |
| | Ready for Phase 1 Implementation | |

---

**Last Updated**: 2025-12-31
**Status**: Ready for Implementation
**Next Step**: Review by tech lead → Approval → Phase 1 implementation starts
