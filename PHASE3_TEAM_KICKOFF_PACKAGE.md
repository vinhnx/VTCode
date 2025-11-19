# Phase 3 Team Kickoff Package
## Complete Execution Guide for Engineering Team

**Prepared**: November 19, 2025  
**For**: Engineering Team (Lead + 2-3 Engineers + QA)  
**Status**: READY FOR IMMEDIATE EXECUTION  
**Start Date**: Monday, November 24, 2025  

---

## Quick Start (Read This First)

### Your Mission
Implement 5 evidence-based optimizations to VT Code's system prompt that will:
- **40% fewer tokens** (30K → 18K per task)
- **15-20% smarter** on complex tasks
- **Enterprise-scale** long-horizon task capability

### Timeline
**Week 1** (Nov 24-28): Implementation (5 days)  
**Week 2** (Dec 1-5): Validation (5 days)  
**Total**: 2 weeks, ~5 engineer-weeks effort

### Must-Have Wins (You WILL Deliver These)
1. ✅ ReAct thinking patterns (Day 1)
2. ✅ .progress.md infrastructure (Day 2)
3. ✅ Tool guidance refactor (Day 3)
4. ✅ Semantic context rules (Day 3-4)

### Nice-to-Have Wins (If Time Permits)
5. Multi-turn conversation structure
6. Semantic clustering examples
7. Iterative refinement patterns

### Success = All Must-Haves Done + 50-Task Suite Passes

---

## Team Structure & Responsibilities

### Role: Tech Lead
**Responsibility**: Coordination, code review, unblocking  
**Time**: Full-time (Week 1), Part-time (Week 2)

**Tasks**:
- [ ] Create & manage phase-3-implementation branch
- [ ] Daily standup facilitation
- [ ] Code review of all changes
- [ ] Escalate blockers immediately
- [ ] Coordinate with QA on validation

### Role: Implementation Engineer 1 (Thinking + Tools)
**Responsibility**: ReAct patterns + tool guidance  
**Time**: Full-time (Week 1), Part-time (Week 2)

**Tasks**:
- [ ] Implement ReAct thinking templates in system.rs (Day 1)
- [ ] Refactor tool guidance to outcome-focused (Day 3)
- [ ] Unit test both changes
- [ ] Participate in integration (Day 4)

### Role: Implementation Engineer 2 (Persistence)
**Responsibility**: .progress.md infrastructure  
**Time**: Full-time (Week 1), Part-time (Week 2)

**Tasks**:
- [ ] Design .progress.md schema (Day 1)
- [ ] Implement load/detect logic in context.rs (Day 2)
- [ ] Add consolidation algorithm (Day 2)
- [ ] Unit test thoroughly
- [ ] Participate in integration (Day 4)

### Role: Implementation Engineer 3 (Context + Docs)
**Responsibility**: Semantic context + documentation  
**Time**: Full-time (Week 1), Part-time (Week 2)

**Tasks**:
- [ ] Add semantic context rules to AGENTS.md (Day 3)
- [ ] Create examples & test cases (Day 3)
- [ ] Update all system prompt documentation (Day 4)
- [ ] Ensure backward compatibility

### Role: QA Engineer
**Responsibility**: Testing & validation  
**Time**: Part-time (Week 1), Full-time (Week 2)

**Tasks**:
- [ ] Set up 50-task validation suite (Day 5)
- [ ] Create metric collection tools (Day 5)
- [ ] Execute validation tests (Week 2)
- [ ] Document all results

---

## Week 1: Implementation Plan (Nov 24-28)

### Monday, Nov 24: ReAct Thinking Patterns

**Goal**: Add Thought→Action→Observation templates to system.rs

**What to Do**:
```
1. Pull main branch
2. Create phase-3-implementation branch
3. Open: vtcode-core/src/prompts/system.rs
4. Find: "# Execution algorithm" section
5. Add after: New section "# Extended Thinking & Reasoning Patterns"
6. Include:
   - ReAct template (Thought→Action→Observation)
   - Thinking budget guidance (5K, 8K, 16K tokens)
   - Multi-pass refinement pattern
   - Examples for each LLM (Claude, GPT, Gemini)
   - Optional thinking exposure
7. Test: Verify section parses correctly
8. Commit: "Phase 3a: Add extended thinking patterns to system.rs"
```

**Reference**: PHASE3_QUICK_START_GUIDE.md, Win 1

**Success Criteria**:
- ✅ New section added (~300 tokens)
- ✅ Templates provided for all LLMs
- ✅ Examples included
- ✅ Code compiles without errors
- ✅ Backward compatible

**Time**: 4 hours (with breaks)

---

### Tuesday, Nov 25: .progress.md Infrastructure

**Goal**: Implement persistent state architecture

**What to Do**:
```
1. Design (1 hour):
   - Review PHASE3_QUICK_START_GUIDE.md, Win 2 for schema
   - Sketch detection logic
   - Plan consolidation algorithm
   
2. Implement load logic in context.rs (2 hours):
   - Add function: detect_progress_file()
   - Add function: load_progress_state()
   - Add function: compress_progress_snapshot()
   - Return: structured state for context injection
   
3. Add to system.rs (1 hour):
   - Add detection at prompt start
   - Add context injection logic
   - Add state merge instructions
   
4. Test (1 hour):
   - Create .progress.md test fixture
   - Test load with full file
   - Test load with partial file
   - Test no file (graceful degradation)
   
5. Commit: "Phase 3b: Implement .progress.md infrastructure"
```

**Reference**: PHASE3_SYSTEM_PROMPT_OPTIMIZATION_PLAN.md, Section 3.2

**Success Criteria**:
- ✅ .progress.md loads correctly
- ✅ State merges with current context
- ✅ Gracefully handles missing file
- ✅ <2KB snapshot compression
- ✅ No regression if file absent

**Time**: 5 hours

---

### Wednesday, Nov 26: Tool Guidance & Semantic Context

**Goal**: Refactor tool selection + add semantic rules

**What to Do - Part A: Tool Guidance (2 hours)**:
```
1. Open: vtcode-core/src/prompts/system.rs
2. Find: "Tool Selection Decision Tree" section
3. Rewrite from prescriptive → outcome-focused:
   OLD: "Use grep for patterns"
   NEW: "To find patterns:
         - If exact strings: grep is fastest
         - If semantic: consider Grep + Read combo
         - If discovering files: start with Glob"
4. Add decision matrix by outcome
5. Provide anti-patterns (when NOT to use each tool)
6. Test: Verify changes make sense
7. Commit: "Phase 3c: Refactor tool guidance to outcome-focused"
```

**What to Do - Part B: Semantic Context (2 hours)**:
```
1. Open: AGENTS.md
2. Find: "Tool Policy Implementation" section
3. Add new section: "Semantic Context Engineering"
4. Include:
   - Bad example (file list)
   - Good example (semantic grouping)
   - Deduplication rules
   - Hierarchical context patterns
5. Add examples:
   - Authentication system (clustering)
   - Database layer (semantic grouping)
6. Test: Verify examples are clear
7. Commit: "Phase 3d: Add semantic context rules to AGENTS.md"
```

**Reference**: 
- PHASE3_QUICK_START_GUIDE.md, Wins 3-4
- CODING_AGENT_BEST_PRACTICES_SYNTHESIS.md, Sections 1, 4

**Success Criteria**:
- ✅ Tool guidance rewritten (outcome-focused)
- ✅ Semantic context rules documented
- ✅ Examples provided & clear
- ✅ Backward compatible
- ✅ AGENTS.md is updated

**Time**: 4 hours

---

### Thursday, Nov 27: Integration & Testing

**Goal**: Merge all changes, verify no regressions

**What to Do**:
```
1. Code Review (1 hour):
   - Tech lead reviews all Phase 3a-3d commits
   - Engineers address feedback
   - Approval before merge
   
2. Integration Testing (2 hours):
   - Create integration test:
     * Load system prompt with thinking patterns
     * Detect .progress.md if present
     * Route through new tool guidance
     * Verify semantic context rules apply
   - Run on 10 representative tasks
   - Verify no Phase 1-2 regressions
   
3. Documentation (1 hour):
   - Update system.rs inline comments
   - Update AGENTS.md cross-references
   - Create PHASE3_IMPLEMENTATION_NOTES.md
   
4. Commit: "Phase 3: Integration & testing complete"
```

**Success Criteria**:
- ✅ All code reviewed & approved
- ✅ Integration tests passing
- ✅ No Phase 1-2 regressions
- ✅ Documentation complete
- ✅ Ready for validation

**Time**: 4 hours

---

### Friday, Nov 28: Cleanup & Validation Prep

**Goal**: Final polish + prepare for Week 2 validation

**What to Do**:
```
1. Final Cleanup (1 hour):
   - Fix any remaining issues from review
   - Format code (cargo fmt)
   - Clippy check (cargo clippy)
   - Update changelog
   
2. Documentation (1 hour):
   - Create PHASE3A_COMPLETION_SUMMARY.md
   - Document what was built
   - Note any decisions made
   - Highlight success metrics
   
3. Validation Prep (2 hours):
   - QA creates 50-task validation suite:
     * 10 simple tasks (no thinking)
     * 15 moderate (thinking helpful)
     * 15 complex (thinking essential)
     * 10 multi-turn (persistence critical)
   - Set up metric collection
   - Define baseline measurements
   
4. Team Sync (1 hour):
   - Celebrate completion of Week 1
   - Preview Week 2 validation
   - Address any concerns
   
5. Commit: "Phase 3a: Complete - Ready for validation"
```

**Success Criteria**:
- ✅ All code polished & formatted
- ✅ Documentation complete
- ✅ Validation suite ready
- ✅ Team ready for Week 2

**Time**: 5 hours

---

## Week 2: Validation Plan (Dec 1-5)

### Monday-Wednesday (Dec 1-3): Run 50-Task Validation

**What to Do**:
```
For each of 50 tasks, run on all 3 LLMs:
1. Claude 3.5 Sonnet
2. OpenAI GPT-4o
3. Google Gemini 2.0

For each run, measure:
- Tokens used (compare to Phase 2 baseline: 30K)
- Task completion quality (1-5 scale)
- Thinking quality if applicable (1-5 scale)
- Tool selection appropriateness (yes/no/maybe)
- Multi-turn coherence % (if multi-turn task)
- Any regressions vs Phase 1-2 (yes/no)

Log all results to: PHASE3_VALIDATION_RESULTS.csv
```

**Success Criteria**:
- ✅ All 50 tasks × 3 models = 150 runs complete
- ✅ Metrics collected for all runs
- ✅ <1% regression rate
- ✅ Average tokens ≤ 20K (target: 18K)

**Time**: 12 hours (distributed across 3 days)

---

### Thursday (Dec 4): Metrics Analysis

**What to Do**:
```
1. Analyze data (2 hours):
   - Calculate average tokens/task by LLM
   - Calculate thinking quality averages
   - Check multi-turn coherence rates
   - Identify any regressions
   - Compare to Phase 2 baseline
   
2. Create visualizations (1 hour):
   - Token reduction graph
   - Quality by task complexity
   - Multi-LLM comparison
   - Regression analysis
   
3. Document findings (1 hour):
   - What worked well
   - What needs improvement
   - Unexpected discoveries
   - Recommendations for Phase 4
```

**Success Criteria**:
- ✅ All metrics calculated
- ✅ Visualizations created
- ✅ Comparison to baseline clear
- ✅ Go/no-go decision documented

**Time**: 4 hours

---

### Friday (Dec 5): Phase 3 Completion Report

**What to Do**:
```
1. Write completion report (2 hours):
   - Executive summary (1 page)
   - Metrics & results (tables)
   - Multi-LLM compatibility (per-model)
   - Impact assessment
   - Learnings & recommendations
   
2. Team presentation (1 hour):
   - Present findings to leadership
   - Celebrate success
   - Get approval for Phase 4
   
3. Archive & cleanup (1 hour):
   - Commit all Phase 3 work
   - Create Phase 3 final commit
   - Archive validation data
   - Close Phase 3 issues
   
4. Phase 4 kickoff planning (1 hour):
   - Review Phase 4 recommendations
   - Identify Phase 4 team
   - Schedule Phase 4 kickoff
```

**Deliverable**: PHASE3_COMPLETION_SUMMARY.md

**Success Criteria**:
- ✅ Completion report delivered
- ✅ Results clear & compelling
- ✅ Leadership informed
- ✅ Phase 4 planned

**Time**: 5 hours

---

## Daily Standup Template

**When**: 9:30 AM (15 minutes)  
**Who**: All team members + tech lead

**Format**:
```
EACH PERSON (2 min):
- What I completed yesterday
- What I'm working on today
- Blockers or questions

TECH LEAD (3 min):
- Highlight progress vs. plan
- Adjust priorities if needed
- Recognize wins
```

**Example**:
```
Engineer 1: "Finished ReAct templates (300 tokens added). 
Today: Tool guidance refactor. No blockers."

Engineer 2: ".progress.md load logic done + tested. 
Today: Consolidation algorithm. Question: Compression target 2KB?
Answer: Yes, aggressive consolidation."

Tech Lead: "Day 1 on track. Both wins delivered. 
Tomorrow: Both tool guidance and semantic context due. 
Staying on schedule."
```

---

## Git Workflow for Phase 3

### Branch Strategy
```
Main branch: phase-3-implementation (off main)
Commit per win: 3-4 commits total
Final merge: Back to main with detailed commit message
```

### Commit Messages
```
Format:
Phase 3[A-D]: [Category] - Brief description

Examples:
"Phase 3A: Thinking - Add ReAct patterns to system.rs"
"Phase 3B: Persistence - Implement .progress.md infrastructure"
"Phase 3C: Tools - Refactor to outcome-focused guidance"
"Phase 3D: Context - Add semantic grouping rules to AGENTS.md"
"Phase 3: Integration - Merge all changes, tests passing"
```

### Code Review Checklist
```
For each change:
- [ ] Code compiles (cargo check)
- [ ] Tests pass (cargo test)
- [ ] No clippy warnings (cargo clippy)
- [ ] Code formatted (cargo fmt)
- [ ] Comments clear & helpful
- [ ] Backward compatible
- [ ] No Phase 1-2 regressions
- [ ] Documentation updated
```

---

## Success Metrics Tracking

### Daily Tracking Sheet

| Metric | Mon | Tue | Wed | Thu | Fri | Target |
|--------|-----|-----|-----|-----|-----|--------|
| ReAct patterns added | ✅ | — | — | — | — | ✅ |
| .progress.md infrastructure | — | ✅ | — | — | — | ✅ |
| Tool guidance refactored | — | — | ✅ | — | — | ✅ |
| Semantic rules documented | — | — | ✅ | — | — | ✅ |
| Integration tests passing | — | — | — | ✅ | — | ✅ |
| Documentation complete | — | — | — | — | ✅ | ✅ |
| Validation suite ready | — | — | — | — | ✅ | ✅ |

### Week 2 Validation Metrics

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Avg tokens/task | ≤18K | ? | ⏳ |
| Token reduction | 40% | ? | ⏳ |
| Thinking quality | 4.0+/5.0 | ? | ⏳ |
| Multi-LLM compat | 98%+ | ? | ⏳ |
| Multi-turn coherence | 95%+ | ? | ⏳ |
| Regression rate | <1% | ? | ⏳ |

---

## Blockers & Escalation

### If You Hit a Blocker

**Step 1**: Check PHASE3_QUICK_START_GUIDE.md FAQ section

**Step 2**: Check CODING_AGENT_BEST_PRACTICES_SYNTHESIS.md for patterns

**Step 3**: Ask tech lead (30 minutes response)

**Step 4**: Escalate to project lead if still blocked

### Common Blockers & Solutions

**"How do I add to system.rs?"**
→ Look at existing sections, follow same pattern

**"What should consolidation algorithm look like?"**
→ See PHASE3_SYSTEM_PROMPT_OPTIMIZATION_PLAN.md, Section 3.2

**"How do I test .progress.md loading?"**
→ Create test file, verify parse + merge

**"Is backward compatibility required?"**
→ YES. .progress.md optional, thinking optional, tool guidance optional

**"How many lines should new sections be?"**
→ Try: Thinking ~300 tokens, .progress.md ~200 tokens, tools ~200 tokens

---

## Resources & References

### Your Documentation
```
PHASE3_QUICK_START_GUIDE.md          ← START HERE
PHASE3_SYSTEM_PROMPT_OPTIMIZATION_PLAN.md
CODING_AGENT_BEST_PRACTICES_SYNTHESIS.md
PHASE3_IMPLEMENTATION_INDEX.md
AGENTS.md (in vtcode repo)
```

### Key Files to Modify
```
vtcode-core/src/prompts/system.rs     (thinking patterns, .progress.md detection)
vtcode-core/src/prompts/context.rs    (load/consolidation logic)
AGENTS.md                              (semantic context, tool guidance)
```

### Test Files
```
Create: tests/phase3_integration_test.rs
Create: tests/fixtures/.progress.md (for testing)
Modify: tests/system_prompt_tests.rs
```

---

## Team Communication

### Daily Standup
**Time**: 9:30 AM  
**Duration**: 15 minutes  
**Format**: Each person 2 min, tech lead 3 min  

### Issues/Questions
**Channel**: Create GitHub issues tagged `phase-3`  
**Response**: Tech lead responds within 30 min  

### Code Review
**Reviewer**: Tech lead  
**Timeline**: Request → Review → Approval (within 4 hours)  

### Final Sync
**Time**: Friday 5 PM Week 1  
**Celebration**: Acknowledge Week 1 completion  

---

## Next Phase (Dec 8+)

### Phase 4: Error Recovery

**What's Coming**:
- Systematic error handling
- Recovery patterns per error type
- Persistent error state
- 98% success on retryable tasks

**Prep**: Read PHASE3_SYSTEM_PROMPT_OPTIMIZATION_PLAN.md, Section 10

---

## Final Thoughts

### You're Building
A coding agent that's 2.5x more efficient + 15-20% smarter + enterprise-capable.

### You're Enabling
Long-horizon tasks, complex reasoning, and persistence that competitors don't have.

### Success Looks Like
- All 4 must-haves delivered by Friday
- 50-task suite passing Week 2
- Team excited about Phase 4

### Let's Do This
Questions? Refer to docs, ask tech lead, escalate blockers.

**Timeline**: 2 weeks  
**Effort**: ~5 engineer-weeks total  
**Risk**: Low  
**Impact**: High  
**Status**: READY TO START MONDAY

---

**Package Version**: 1.0  
**Status**: READY FOR TEAM  
**Start Date**: Monday, November 24, 2025  
**Prepared By**: Amp AI Agent + VT Code Research Team
