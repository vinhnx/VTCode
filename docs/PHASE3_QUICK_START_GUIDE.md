# Phase 3: Quick Start Implementation Guide
## VT Code System Prompt Optimization (Context + Thinking + Persistence)

**For**: Engineering team ready to implement Phase 3  
**Time**: 1-2 weeks, 4 engineers  
**Complexity**: Medium-High  
**Risk**: Low (backward compatible)  

---

## TL;DR: What's Changing

| What | Current | Phase 3 | Why |
|------|---------|---------|-----|
| Thinking | None | ReAct patterns + budget | 15-20% smarter |
| Memory | Each turn | .progress.md across resets | Persistent state |
| Context | Volume-based | Semantic grouping | 40% fewer tokens |
| Tools | Prescriptive | Outcome-focused | Better choices |
| Conversation | Linear | Iterative with state merge | Natural loops |

---

## High-Priority Wins (Do These First)

### Win 1: Add ReAct Thinking Patterns (1 day)
**File**: `vtcode-core/src/prompts/system.rs`  
**Location**: New section after "Execution Algorithm"

**Add**:
```rust
r#"# Extended Thinking & Reasoning Patterns

For complex tasks with uncertainty, structure thinking as:

## ReAct Pattern (Thought → Action → Observation)

Thought N: [Internal reasoning - what do I understand? what's next?]
Action N: [Call a tool to gather information]
Observation N: [Analyze result, update understanding]

Repeat until task complete. This pattern helps with:
- Complex debugging (multiple hypotheses)
- Unfamiliar codebases (gradual understanding)
- Refactoring (impact analysis needed)
- Architecture decisions (tradeoffs to consider)

## Thinking Budget (Claude/Gemini with extended thinking)
Use `thinking_budget: 8000` for moderate complexity tasks
Use `thinking_budget: 16000` for high complexity (architecture, security)
Use `thinking_budget: 0` for straightforward tasks

## Multi-Pass Refinement
Even without extended thinking, support self-improvement:
1. First attempt: Get basic solution
2. Critique: Check for edge cases, errors
3. Refine: Address gaps identified in critique
4. Verify: Final validation

## High-Level Guidance vs. Steps
Instead of: "Do X, then Y, then Z"
Prefer: "Goal is X. Approach it as you see fit."
Exception: Security-sensitive code always needs explicit steps."#
```

**Effort**: Low (250 tokens, mostly examples)  
**Impact**: High (enables thinking on all 3 LLMs)  
**Test**: 5 complex tasks, measure thinking quality

---

### Win 2: .progress.md Infrastructure (2 days)
**Files**:
- `vtcode-core/src/prompts/context.rs` (add load logic)
- `vtcode-core/src/prompts/system.rs` (add detection)

**Pattern**:
```rust
// In system.rs, add detection:
if let Ok(progress) = read_progress_file() {
    // Extract key state
    let snapshot = compress_progress(&progress);
    // Inject as context
    context.push(snapshot);
}

// Support thresholds:
// At 70% tokens: Warn about upcoming reset
// At 85% tokens: Suggest creating .progress.md
// At 90% tokens: Auto-consolidate to .progress.md
```

**Schema** (`.progress.md`):
```markdown
# Progress: [Task Name]

## Metadata
- Started: 2025-11-19T10:00:00Z
- Model: claude-3-5-sonnet
- Resets: 1

## State
- Goal: [1 sentence]
- Completed: [checkpoint 1], [checkpoint 2]
- Pending: [next step 1], [next step 2]
- Files: src/auth/mod.rs, src/lib.rs

## Key Findings
- Pattern 1: [description]
- Pattern 2: [description]

## Blockers
- [If any]

## Next Actions
1. [Action]
2. [Action]
```

**Effort**: Medium (300 lines Rust + schema)  
**Impact**: Very High (enables long-horizon tasks)  
**Test**: 3+ turn conversation, verify state preservation

---

### Win 3: Outcome-Focused Tool Guidance (1 day)
**File**: `vtcode-core/src/prompts/system.rs`  
**Section**: "Tool Selection Decision Tree"

**Current** (prescriptive):
```
grep_file: Use for exact text matches
read_file: Use for understanding file contents
list_files: Use for directory exploration
```

**New** (outcome-focused):
```
## Tool Selection by Outcome

### "I need to find specific patterns"
Primary: Grep (exact strings, regex)
  → Use when: Looking for specific keywords or patterns
  → Example: Find all error handling paths
  → Speed: <1s for most codebases

Semantic alternative: Finder
  → Use when: Patterns are semantic (concepts, not strings)
  → Example: Find authentication entry points
  → Speed: 1-3s depending on complexity

### "I need to understand file structure"
Primary: Glob (file listing)
  → Use when: Discovering directory structure
  → Example: List all test files
  → Speed: <100ms

Secondary: Read (file contents)
  → Use when: Need actual code
  → Example: Understand module organization
  → Speed: Depends on file size

### "I need to refactor multiple files"
Approach: Edit loop (file by file)
  1. Use Grep to find all locations
  2. Use Read to understand context
  3. Use Edit for each change
  4. Use Bash to verify (tests, builds)

Why: Surgical edits preserve untouched code
```

**Effort**: Low (200 tokens)  
**Impact**: Medium (better tool choices)  
**Test**: 10 tasks, compare tool selection patterns

---

### Win 4: Semantic Context Guidelines (1 day)
**File**: `AGENTS.md`  
**Add Section**: "Semantic Context Engineering"

**Content**:
```markdown
## Semantic Context Engineering

### Grouping by Semantics, Not Structure

BAD:
```
Here's all the files in src/:
- main.rs (500 lines)
- lib.rs (200 lines)
- utils.rs (300 lines)
...
```

GOOD:
```
Core entry points:
- main.rs: CLI parsing + argument handling
- lib.rs: Public API exports

Authentication system:
- auth/mod.rs: Token validation logic
- auth/providers.rs: OAuth2 implementations
- auth/errors.rs: Auth-specific errors

Database:
- db/connection.rs: Connection pooling
- db/queries.rs: Query builders
```

### Deduplication Rules

When extracting context, merge related findings:

BEFORE (repeated):
- File A: Uses Pattern X
- File B: Uses Pattern X
- File C: Uses Pattern X

AFTER (consolidated):
- Pattern X used in: A, B, C
- Common theme: Error handling via match
```

**Effort**: Low (150 tokens)  
**Impact**: Medium-High (35% context reduction)  
**Test**: Large codebases (50+ files), measure redundancy

---

## Medium-Priority Improvements (Do These Second)

### Improvement 1: Multi-Turn Conversation Structure

**Add to system.rs**:
```
## Multi-Turn Conversation Protocol

### Turn Boundaries
Each turn, maintain explicit state:
1. What task are we solving?
2. What did we accomplish this turn?
3. What's pending?
4. Load prior .progress.md if available

### State Merge Algorithm
When resuming conversation:
1. Load .progress.md (or prior summary)
2. Identify unchanged work (cache it mentally)
3. Preserve completed findings
4. Build on prior decisions
5. Avoid re-analyzing what's done

### Context Budget Awareness
- <70%: Normal operation
- 70-85%: Consider summarizing findings
- 85%+: Create .progress.md snapshot
- 90%+: Prepare for context reset
```

**Effort**: Low (200 tokens)  
**Impact**: High (better conversation UX)

---

### Improvement 2: Semantic Clustering Examples

**Add to AGENTS.md**:
```markdown
## Semantic Clustering Examples

### Example 1: Authentication System

Instead of listing 12 files:
```
src/auth/
  ├── mod.rs
  ├── jwt.rs
  ├── oauth.rs
  ├── errors.rs
  ├── middleware.rs
  └── tests/
```

Cluster semantically:
```
Authentication Layer:
  Core: auth/mod.rs (entry points)
  Implementations:
    - JWT: auth/jwt.rs
    - OAuth: auth/oauth.rs
  Error handling: auth/errors.rs
  Integration: auth/middleware.rs
  Testing: auth/tests/
  
Dependency chain: middleware → jwt/oauth → core → errors
```

### Example 2: Database

Instead of flat listing:
```
Connection management: db/connection.rs, db/pool.rs
Query building: db/queries.rs, db/builders.rs
Migrations: db/migrations.rs
Testing: db/test_utils.rs
```

Cluster by capability:
```
Database Subsystem:
  Runtime:
    - Connection pooling: db/pool.rs, connection.rs
  Query layer:
    - Query builders: db/builders.rs
    - SQL generation: db/queries.rs
  Development:
    - Migrations: db/migrations.rs
    - Test utilities: db/test_utils.rs
```
```

**Effort**: Low (examples only)  
**Impact**: Medium (clarity, better context understanding)

---

### Improvement 3: Iterative Refinement Examples

**Add to system.rs**:
```
## Iterative Refinement Loop

For complex tasks, expect multiple turns:

TURN 1: Generate solution
  "Implement authentication using JWT"
  → Creates basic JWT implementation

TURN 2: Add missing features
  User: "Also handle token refresh"
  Model: Adds refresh logic to Turn 1 work
  
TURN 3: Edge cases
  User: "Handle concurrent requests"
  Model: Adds mutex/atomic patterns
  
TURN 4: Verification
  "Run tests and verify edge cases"
  → All tests pass

Each turn builds on prior work.
Use .progress.md to preserve context.
Success = converged solution after N turns (usually 2-3).
```

**Effort**: Low  
**Impact**: Medium (manages expectations for conversations)

---

## Implementation Roadmap

### Week 1: High-Priority Wins

**Day 1 (Mon)**:
- [ ] Add ReAct thinking patterns to system.rs
- [ ] Create Phase 3a documentation
- [ ] Test on 5 complex tasks

**Day 2 (Tue)**:
- [ ] Design .progress.md schema
- [ ] Add load/detect logic to context.rs
- [ ] Test on 3-turn conversation

**Day 3 (Wed)**:
- [ ] Refactor tool guidance (outcome-focused)
- [ ] Add semantic context rules to AGENTS.md
- [ ] Test on 10 tasks

**Day 4 (Thu)**:
- [ ] Integration testing (all 3 together)
- [ ] Update AGENTS.md comprehensively
- [ ] Test on 30-task suite

**Day 5 (Fri)**:
- [ ] Final refinements based on testing
- [ ] Create Phase 3 documentation bundle
- [ ] Prepare for Phase 3 validation

### Week 2: Medium-Priority + Validation

**Days 6-7**: Medium-priority improvements

**Days 8-10**: Full 50-task validation suite
- Measure: context efficiency, quality, compatibility
- Document: Phase 3 completion report

---

## Testing Checklist

### Quick Test Suite (Day 1-3)

```rust
// Test 1: ReAct thinking enabled
[
  (task: "Refactor auth module", expect_thinking: true),
  (task: "Fix bug in parser", expect_thinking: true),
  (task: "Extract variable name", expect_thinking: false),
]

// Test 2: .progress.md load/detect
[
  (with_progress: true, expect: "state preserved"),
  (with_progress: false, expect: "normal mode"),
  (partial_progress: true, expect: "merge state"),
]

// Test 3: Tool selection
[
  (goal: "find pattern", expect: "grep first"),
  (goal: "understand structure", expect: "glob first"),
  (goal: "refactor", expect: "edit loop"),
]

// Test 4: Semantic grouping
[
  (input: "list of 20 files", expect: "clustered by function"),
  (input: "scattered findings", expect: "deduplicated"),
  (input: "repeated facts", expect: "merged"),
]
```

### Validation Suite (Week 2)

**50 representative tasks**:
- 10 simple (classify, extract) - no thinking needed
- 15 moderate (refactor, debug) - thinking helpful
- 15 complex (architecture, design) - thinking essential
- 10 multi-turn (conversation, iteration) - persistence critical

**Measurements**:
- Tokens/task (target: 18K avg, 40% reduction from Phase 2)
- Thinking quality (1-5 scale, target: 4.0+)
- .progress.md compression (target: <2KB)
- Multi-LLM compatibility (target: 98%+)
- Tool selection quality (subjective: better/same/worse)

---

## Risk Mitigation

### Risk: Extended Thinking Adds Latency
**Mitigation**: Make thinking optional; default only on complexity score < 0.7

### Risk: .progress.md Overhead
**Mitigation**: Keep <2KB; consolidate aggressively; auto-cleanup

### Risk: Semantic Clustering Breaks Existing Logic
**Mitigation**: Start with simple rules; evolve incrementally; test thoroughly

### Risk: Multi-Turn Regression
**Mitigation**: Test on 20+ multi-turn tasks; verify state preservation

---

## Success Metrics

### Must-Have
- ✅ ReAct thinking patterns in system.rs
- ✅ .progress.md infrastructure working
- ✅ Tool guidance rewritten (outcome-focused)
- ✅ No Phase 1-2 regressions
- ✅ Backward compatible

### Should-Have
- ✅ 40% context reduction (18K avg tokens)
- ✅ 95%+ multi-turn coherence
- ✅ 98%+ multi-LLM compatibility
- ✅ Comprehensive documentation

### Nice-to-Have
- Semantic clustering automation
- Parallel thinking (multishot)
- Advanced error recovery

---

## Handoff Checklist

### For Code Review
- [ ] System prompt changes documented
- [ ] AGENTS.md updated
- [ ] .progress.md schema approved
- [ ] Tests pass (50-task suite)
- [ ] No regressions (Phase 1-2 still work)

### For Deployment
- [ ] Phase 3 documentation complete
- [ ] Backward compatibility verified
- [ ] Team trained on new patterns
- [ ] Validation metrics collected
- [ ] Ready for Phase 4 planning

### For Next Phase (Phase 4)
- [ ] Error recovery patterns documented
- [ ] Persistent error state design
- [ ] Recovery strategy catalog
- [ ] Testing framework prepared

---

## Key Files to Modify

```
Core Changes:
  vtcode-core/src/prompts/system.rs     (extended thinking + .progress)
  vtcode-core/src/prompts/context.rs    (semantic grouping, load logic)
  AGENTS.md                              (all sections)

Documentation:
  docs/PHASE3_SYSTEM_PROMPT_OPTIMIZATION_PLAN.md
  docs/CODING_AGENT_BEST_PRACTICES_SYNTHESIS.md
  docs/PHASE3_QUICK_START_GUIDE.md (this file)
  docs/PHASE3A_THINKING_PATTERNS.md (new)
  docs/PHASE3B_PERSISTENT_MEMORY.md (new)
  docs/PHASE3C_SEMANTIC_CONTEXT.md (new)
  docs/PHASE3D_TOOL_GUIDANCE.md (new)
  docs/PHASE3_COMPLETION_SUMMARY.md (end of phase)
```

---

## Questions & Escalations

**Q: Should thinking be enabled by default?**  
A: No. Use heuristic: confidence score < 0.7 → enable thinking.

**Q: What if model doesn't support extended thinking (GPT)?**  
A: ReAct pattern works on all models (uses output text, not thinking tokens).

**Q: How big can .progress.md be?**  
A: Target <2KB. Consolidation should keep it compact. If >4KB, need better consolidation logic.

**Q: Do we need to support backward compat with Phase 1?**  
A: Yes. Phase 3 must work on prompts without .progress.md.

**Q: When do we move to Phase 4?**  
A: After Phase 3 validation passes metrics. Probably mid-December.

---

## References

- `docs/PHASE3_SYSTEM_PROMPT_OPTIMIZATION_PLAN.md` (detailed plan)
- `docs/CODING_AGENT_BEST_PRACTICES_SYNTHESIS.md` (research findings)
- `AGENTS.md` (current reference)
- `.github/copilot-instructions.md` (code style guide)

---

**Document Version**: 1.0  
**Status**: READY FOR IMPLEMENTATION  
**Created**: November 19, 2025  
**Target**: Week of November 24, 2025
