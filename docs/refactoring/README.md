# TUI Session Management Refactoring - Complete Guide

## Quick Start

**New to this refactoring?** Start here:
1. Read this README for overview
2. Review `MASTER_REFACTORING_ROADMAP.md` for timeline
3. Check relevant phase document for details
4. Follow implementation steps in the phase guide

**Want the latest status?** See `SESSION_SUMMARY_2025_11_09.md`

---

## Program Overview

The TUI session management module (`vtcode-core/src/ui/tui/session.rs`) is undergoing a comprehensive refactoring to improve code quality, maintainability, and performance. The program is organized into 6 phases over ~18-22 hours.

### Current Status: Phase 2 Complete ✓

- ✓ Phase 1: Manager extraction (InputManager, ScrollManager)
- ✓ Phase 2: Manager integration and event handler refactoring
- ⚠ Phase 2.5: Final verification in progress
- ⏳ Phase 3: Code deduplication (ready to start)
- ⏳ Phase 4: Complexity reduction (ready to start)
- ⏳ Phase 5: Performance optimization (planned)
- ⏳ Phase 6: Module reorganization (planned)

---

## Document Index

### Planning & Overview
- **MASTER_REFACTORING_ROADMAP.md** (500+ lines)
  - Executive summary and program overview
  - Dependency graph and timeline
  - Risk assessment and success criteria
  - Metrics tracking before/after
  - **Read this first for overall context**

- **SESSION_SUMMARY_2025_11_09.md** (400+ lines)
  - Latest session accomplishments
  - Current baseline metrics
  - Next steps and immediate actions
  - **Read this for recent progress**

### Phase Documentation

#### Phase 1-2: Manager Extraction & Integration ✓ COMPLETE
- **PHASE_2_PROGRESS.md** (361 lines)
  - Step-by-step progress tracking
  - InputManager and ScrollManager integration status
  - Test results and metrics
  - Migration patterns established
  - **Reference for how managers are integrated**

- **MIGRATION_STRATEGY.md**
  - Method-by-method migration guide
  - Detailed for Phase 2 execution

#### Phase 3: Code Deduplication (READY)
- **PHASE_3_CODE_DEDUPLICATION.md** (450+ lines)
  - Problem analysis for palette duplication (~130 lines)
  - Solution design with code examples
  - Generic PaletteRenderer implementation
  - ToolStyler consolidation strategy
  - StyleHelpers extraction plan
  - Step-by-step implementation guide (5 sub-steps)
  - Testing strategy
  - Expected outcomes: -250 lines, 6%→2% duplication
  - **Start here for Phase 3 implementation**

#### Phase 4: Complexity Reduction (READY)
- **PHASE_4_COMPLEXITY_REDUCTION.md** (600+ lines)
  - High cyclomatic complexity analysis
  - Detailed breakdown of process_key(), render_message_spans(), render_tool_header_line()
  - Handler extraction strategy (13+ focused handlers)
  - Code examples for each extracted handler
  - Step-by-step implementation for each function
  - Testing approach for handlers
  - Expected outcomes: CC ~35→<10
  - **Start here for Phase 4 implementation**

#### Phase 5-6: Optimization & Reorganization (PLANNED)
- Not yet documented (will be created as needed)
- Planned for after Phase 4 completion

### Analysis & Reference
- **SESSION_REFACTORING_ANALYSIS.md** (685 lines)
  - Comprehensive code analysis
  - Identified issues and code smells
  - God object anti-pattern details
  - Current inefficiencies
  - **Reference for understanding problems**

- **SESSION_REFACTORING_IMPLEMENTATION.md** (722 lines)
  - High-level implementation guide
  - Detailed phase descriptions
  - Manager API documentation
  - Code examples
  - **Reference for architecture decisions**

- **REFACTORING_SUMMARY.md**
  - Quick summary of overall plan

---

## Key Metrics

### Current State (Baseline)
```
Session struct:       44 fields, 158 methods
File size:           4,855 lines
Max complexity:      CC ~35 (process_key)
Code duplication:    6% (~300 lines)
Test coverage:       200+ tests (112+ passing in Phase 2)
```

### After Phase 3 (Projected)
```
Code duplication:    2% (save ~250 lines)
Functions:           150 (-8 function count)
File size:           4,600 lines
```

### After Phase 4 (Projected)
```
Max complexity:      <10 (was ~35, -25 CC!)
Functions:           180 (+30 small handlers)
Maintainability:     Significantly improved
```

### Final State (After All Phases)
```
Code duplication:    <1% (save ~300 lines)
Max complexity:      <10 (all functions)
Functions:           180+ (well-organized)
Test coverage:       250+ tests
Module clarity:      High
Performance:         No regressions
```

---

## Implementation Timeline

### Session 1 (Completed) ✓
- Phase 1: Extract InputManager & ScrollManager (~3 hours)
- Phase 2: Integrate managers, refactor event handlers (~4.5 hours)
- **Total: 7.5 hours**

### Session 2 (Recommended: This or Next)
- Phase 2.5: Final verification & cleanup (~1 hour)
- Phase 3.1: PaletteRenderer generic (~2.5 hours)
- Phase 3.2-3.3: Tool styling & style helpers (~3.5 hours)
- **Total: ~6-7 hours**

### Session 3
- Phase 3 completion & testing (~1-2 hours)
- Phase 4.1: process_key() breakdown (~4 hours)
- Phase 4.2: render_message_spans() breakdown (~2.5 hours)
- **Total: ~7-8 hours**

### Session 4 (Optional)
- Phase 4.3: render_tool_header_line() (~1.5 hours)
- Phase 5: Performance optimization (~2-3 hours)
- Phase 6: Module reorganization (~2 hours)
- **Total: ~5-6 hours**

**Total Program:** ~18-22 hours over 3-4 work sessions

---

## How to Use These Guides

### For Planning & Understanding
1. Start with **MASTER_REFACTORING_ROADMAP.md**
2. Review phase-specific documents for details
3. Check **SESSION_SUMMARY_2025_11_09.md** for latest status

### For Implementation
1. Read the phase document completely
2. Review step-by-step instructions
3. Study code examples provided
4. Follow testing strategy section
5. Verify against acceptance criteria
6. Document any changes made

### For Code Review
1. Check which phase is being implemented
2. Review the detailed plan in that phase document
3. Verify changes match the planned approach
4. Run test suite (expected: all tests pass)
5. Check metrics align with projections

### For Debugging/Questions
1. Check decision log in MASTER_REFACTORING_ROADMAP.md
2. Review rationale in relevant phase document
3. Look at code examples for implementation patterns
4. Check the Q&A section in phase documents

---

## Key Concepts

### Manager Pattern
Extracts related state and behavior into focused manager classes:
- **InputManager:** User input, cursor, history
- **ScrollManager:** Viewport scrolling, metrics

Benefits: Better encapsulation, reusability, testability

### Generic Abstractions
Template code into reusable components using Rust traits:
- **PaletteItem:** Trait for palette rendering
- **PaletteRenderer<T>:** Generic renderer for any palette type

Benefits: Less duplication, zero-cost abstractions

### Handler Extraction
Break down large complex functions into smaller, focused handlers:
- **process_key():** Split into key-specific handlers
- **render_message_spans():** Split by message type

Benefits: Lower complexity, easier testing, clearer logic

---

## Testing Strategy

### Unit Tests
- Existing tests maintained throughout
- New components get comprehensive test coverage
- Tests live in module or in separate test file

### Integration Tests
- Verify managers work within Session
- Test event handling end-to-end
- Validate rendering output

### Regression Tests
- Run full test suite after each phase
- Benchmark performance before/after
- Manual testing of visual rendering

### Test Results
- Phase 1-2: 112+ tests passing ✓
- Phase 3: +10 new tests (expected)
- Phase 4: +40 new tests (expected)
- Final: 250+ tests, 100% passing

---

## Risk Management

### Overall Risk: LOW
- Incremental changes with testing after each phase
- No breaking API changes
- Gradual migration patterns
- Comprehensive documentation

### Phase-Specific Risks

| Phase | Risk | Mitigation |
|-------|------|-----------|
| 1-2 | Low | Separate modules, tested independently |
| 3 | Low | New abstractions tested in isolation |
| 4 | Medium | Function extraction, extensive testing |
| 5 | Low | Profiling-driven, rollback if needed |
| 6 | Low | Internal reorganization only |

### Mitigation Strategies
1. Keep all existing tests throughout
2. Small, reviewable commits
3. Run tests after each phase
4. Performance validation
5. Careful code review
6. Clear documentation

---

## Success Criteria

### Must Have ✓
- [ ] All 200+ existing tests pass
- [ ] No clippy warnings
- [ ] Cyclomatic complexity <10 for all functions
- [ ] Code duplication <2%
- [ ] No performance regressions
- [ ] Public API unchanged

### Should Have
- [ ] Improved code organization
- [ ] Better error handling
- [ ] Comprehensive documentation
- [ ] Easy to extend with new message types
- [ ] Easy to customize styling

### Nice to Have
- [ ] 10%+ memory reduction
- [ ] Reusable components for other TUI work
- [ ] Performance improvement in hot paths

---

## Getting Help

### Understanding a Phase
1. Read the phase document completely
2. Review code examples
3. Check "Why?" sections for rationale
4. Look at "Expected Outcomes"

### Implementing a Phase
1. Follow step-by-step instructions
2. Study provided code examples
3. Use testing strategy section
4. Verify acceptance criteria

### Debugging Issues
1. Check decision log
2. Review problem analysis section
3. Look for similar code patterns
4. Consult test examples

### Contributing
1. Follow AGENTS.md style guide
2. Maintain test coverage
3. Document significant changes
4. Update relevant phase guides

---

## Status Tracking

### Completed ✓
- Phase 1: Extract managers
- Phase 2: Integrate managers
- Documentation: Planning for Phases 3 & 4

### In Progress ⚠
- Phase 2.5: Final verification

### Ready to Start
- Phase 3: Code deduplication (all resources ready)
- Phase 4: Complexity reduction (all resources ready)

### Planned
- Phase 5: Performance optimization
- Phase 6: Module reorganization

---

## Quick Links

### Essential Documents
- 🎯 **START HERE:** MASTER_REFACTORING_ROADMAP.md
- 📊 **LATEST:** SESSION_SUMMARY_2025_11_09.md
- 📋 **PHASE 3:** PHASE_3_CODE_DEDUPLICATION.md
- 📋 **PHASE 4:** PHASE_4_COMPLEXITY_REDUCTION.md

### Reference Documents
- 📚 **ANALYSIS:** SESSION_REFACTORING_ANALYSIS.md
- 📚 **OVERVIEW:** SESSION_REFACTORING_IMPLEMENTATION.md
- 📚 **PROGRESS:** PHASE_2_PROGRESS.md

### Code References
- 📝 **Source:** `vtcode-core/src/ui/tui/session.rs`
- 📝 **InputManager:** `vtcode-core/src/ui/tui/session/input_manager.rs`
- 📝 **ScrollManager:** `vtcode-core/src/ui/tui/session/scroll.rs`

---

## Document Structure

Each phase document follows this structure:
1. **Overview** - What and why
2. **Problem Analysis** - Current issues
3. **Solution Design** - Proposed approach with examples
4. **Implementation Steps** - Step-by-step instructions
5. **Expected Outcomes** - Metrics and improvements
6. **Testing Strategy** - How to verify correctness
7. **Timeline & Effort** - Hours and risk assessment

---

## Convention Notes

### Naming
- Phases: 1-6 (logical units)
- Sessions: Work periods (multiple phases per session)
- Commits: Should reference phase (e.g., "Phase 3.1: PaletteRenderer")

### Metrics
- **CC:** Cyclomatic complexity (goal: <10)
- **Lines:** Code lines (goal: reduced by removing duplication)
- **Fields:** Struct fields (not reducing, improving organization)
- **Tests:** Test count (goal: maintain/increase coverage)

### Status Indicators
- ✓ Complete
- ⏳ Planned/Ready
- ⚠ In Progress
- ❌ Blocked/Deferred

---

## Final Notes

This refactoring program is designed to improve code quality incrementally while maintaining stability. With comprehensive planning and documentation in place, execution should be straightforward.

**Key Principles:**
1. **Incremental:** Small changes with testing
2. **Documented:** Clear rationale and approach
3. **Tested:** Comprehensive test coverage maintained
4. **Compatible:** No breaking API changes
5. **Clear:** Code organization improved

**Questions?** Check the relevant phase document or master roadmap for answers.

---

**Program Status:** 🟡 Phase 2 Complete, Phase 3 Ready to Start
**Last Updated:** 2025-11-09
**Target Completion:** 2025-11-23
