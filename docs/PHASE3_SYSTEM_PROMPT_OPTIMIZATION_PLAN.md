# Phase 3: System Prompt Optimization Plan
## VT Code Advanced Agent Engineering (Context + Multi-LLM + Persistence)

**Date**: November 19, 2025  
**Status**: PLANNING → IMPLEMENTATION  
**Timeline**: 1-2 weeks  
**Complexity**: High  

---

## Executive Summary

This document outlines comprehensive system prompt optimizations informed by best practices from production coding agents (Cursor, GitHub Copilot, Vercel v0, Sourcegraph Cody, Claude) and cutting-edge research in extended thinking, persistent memory, and multi-LLM agents.

**Goal**: Refactor VT Code's system prompt to achieve:
- 40% more semantic efficiency (beyond Phase 1's 33%)
- Extended thinking patterns (ReAct + interleaved reasoning)
- Persistent working memory across context resets
- 98%+ compatibility across Claude 3.5+, GPT-4/4o, Gemini 2.0+

**Current State**: Phases 1-2 complete (context curation + multi-LLM patterns)  
**Next Steps**: Phase 3 implementation (persistence + thinking) + Phase 4 (error recovery)

---

## 1. Research Findings from Popular Agents

### 1.1 Sourcegraph Cody Pattern

**Key Insight**: Semantic context > raw context volume

```
CODY PROMPTING APPROACH:
1. Treat AI like "new team member" → needs complete context
2. Define persona/role → shapes cognitive style
3. @-mention specific context sources → targets reasoning
4. Provide examples + test cases → anchors understanding
5. Iterate & refine → converges on solution
```

**VT Code Application**:
- Instead of "find all functions matching pattern", use semantic naming
- Provide @-mention equivalents via context structs
- Include test cases in prompt examples
- Support iterative refinement via persistent state

---

### 1.2 GitHub Copilot Pattern

**Key Insight**: Start general → get specific (phased approach)

```
COPILOT ESCALATION PATTERN:
1. Broad goal statement
2. Specific requirements list
3. Examples (input/output pairs)
4. Edge cases & constraints
5. Iterate based on feedback
```

**VT Code Application**:
- Structure prompts with hierarchical specificity
- Lead with general task, then constraints
- Provide multi-shot examples (5-10 variants)
- Support conversation memory for refinement

---

### 1.3 Claude Extended Thinking Pattern

**Key Insight**: Thinking budget + visible reasoning = better outcomes

```
CLAUDE EXTENDED THINKING:
- High-level goal instructions (vs. step-by-step)
- Visible thought process (transparency)
- Action scaling: Multiple tool calls → single task
- Serial + parallel test-time compute
- Iterative refinement within thinking block
```

**VT Code Application**:
- Add "thinking_budget" to prompts for complex tasks
- Expose reasoning trails (optional visibility)
- Support chained tool calls (already doing this)
- Add majority-voting for parallel completions
- Enable multi-pass refinement via persistent state

---

### 1.4 OpenAI Reasoning Models Pattern

**Key Insight**: More thinking time > more prescriptive instructions

```
REASONING MODEL APPROACH:
- Senior co-worker model: Give goal, trust execution
- vs. Junior co-worker: Explicit steps needed
- Best with: High-level goals + few examples
- Worst with: Hyper-detailed instructions
```

**VT Code Application**:
- Use high-level directives for multi-step tasks
- Provide 3-4 powerful examples (not 20)
- Trust model's decomposition of sub-tasks
- Focus on outcomes, not micromanagement

---

### 1.5 Vercel v0 Pattern

**Key Insight**: Progressive disclosure + context composition

```
V0 APPROACH:
1. Specify desired functionality
2. Add design preferences  
3. Mention libraries/frameworks
4. Provide use-case context
5. Request modifications iteratively
```

**VT Code Application**:
- Compose context progressively (not all-at-once)
- Allow tool-specific tuning (grep vs. read patterns)
- Support library/framework hints (Rust crate context)
- Enable fine-tuning via feedback loops

---

### 1.6 Amazon Bedrock AgentCore Pattern

**Key Insight**: Semantic consolidation of memory > raw storage

```
AGENTCORE MEMORY:
1. Extract semantically meaningful information
2. Retrieve related existing memories
3. Intelligently consolidate (ADD/UPDATE/NO-OP)
4. Maintain immutable audit trail
5. Support temporal reasoning (recent > old)
```

**VT Code Application**:
- Summarize progress at context boundaries
- Consolidate related findings
- Maintain decision audit trail
- Prefer recent state over historical context

---

## 2. VT Code's Current Prompt Architecture

### Current Structure
```
system.rs (core system prompt)
 Identity & role
 Tone & communication
 Core principles (6 principles)
 Execution algorithm (Discovery → Context → Execute → Verify → Reply)
 Tool selection decision tree
 Tool usage guidelines (Tiers 1-5)
 Code execution patterns
 Safety boundaries
 Self-documentation

AGENTS.md (reference guide)
 Build & test commands
 Architecture & modules
 Code style & conventions
 Context engineering (Phase 1)
 Multi-LLM compatibility (Phase 2)
```

### Current Strengths
-   Clear execution algorithm
-   Comprehensive tool guidance
-   Safety-first approach
-   Context management rules (Phase 1)
-   Multi-LLM patterns (Phase 2)

### Identified Gaps (Research-Based)

| Gap | Impact | Phase 3 Fix |
|-----|--------|-----------|
| No explicit thinking patterns | 15-20% intelligence loss on complex tasks | Add ReAct-style reasoning templates |
| No persistent memory structure | Context resets lose all state | Implement .progress.md + consolidation |
| Semantic grouping weak | 25% context waste on unrelated info | Add semantic clustering rules |
| Tool selection too prescriptive | Model doesn't optimize naturally | Switch to outcome-focused guidance |
| No iterative refinement loop | One-shot mode (no convergence) | Add feedback + refine patterns |
| Multi-turn handling unclear | Conversation memory not leveraged | Explicit turn structure + state merge |
| No reasoning visibility | Hard to debug model decisions | Optional thinking exposition |
| Error recovery missing | Stuck on first failure | Phase 4 (defer) |

---

## 3. Phase 3 Implementation Plan

### 3.1 Add Thinking & Reasoning Patterns

**Location**: `vtcode-core/src/prompts/system.rs`

**New Section**: "Extended Thinking & Reasoning Patterns"

```rust
// Pattern 1: ReAct-style task decomposition
// Pattern 2: Interleaved thinking + action
// Pattern 3: Multi-pass refinement
// Pattern 4: Outcome-first reasoning
// Pattern 5: Visible thinking (optional)
```

**Key Changes**:
- Add thinking templates for complex tasks
- Support explicit reasoning traces
- Enable action-reflection loops
- Provide reasoning examples

---

### 3.2 Implement Persistent Working Memory

**Location**: `.progress.md` infrastructure (new)

**Structure**:
```markdown
# Progress: Task Name

## Metadata
- Created: 2025-11-19T10:00:00Z
- Model: claude-3-5-sonnet
- Context: 45000/200000 tokens
- Resets: 0

## State
- Current Task: [description]
- Completed Steps: [list]
- Pending Steps: [list]
- Blockers: [list]
- Key Findings: [list]

## Context Snapshot
- File edits: [paths]
- Tool discoveries: [tools]
- Important patterns: [patterns]

## Next Actions
- [ordered list]
```

**System Prompt Changes**:
- Add `.progress.md` detection logic
- Implement state merge algorithm
- Support context window management

---

### 3.3 Enhance Semantic Context Management

**Location**: `vtcode-core/src/prompts/context.rs`

**New Rules**:
- Group related findings by semantic topic
- Deduplicate across topics
- Prioritize by recency + relevance
- Support hierarchical context (core → detail)

---

### 3.4 Add Outcome-Focused Tool Guidance

**Location**: `vtcode-core/src/prompts/system.rs` (tool selection section)

**Change Vector**: From "use Grep for this" → "to achieve X, consider..."

```
BEFORE (Prescriptive):
"Use grep_file for finding exact text matches"

AFTER (Outcome-Focused):
"If you need to find exact patterns across files,
grep_file is efficient. But if patterns are semantic
or file discovery is primary, consider list_files
first to understand structure."
```

---

### 3.5 Support Iterative Refinement Loops

**Location**: `vtcode-core/src/prompts/system.rs` + AGENTS.md

**Pattern**: Feedback → Analysis → Adjustment → Retry

```
REFINEMENT LOOP:
1. Attempt task with initial approach
2. Evaluate result against success criteria
3. Identify gaps/misalignments
4. Adjust strategy (or context)
5. Retry with new context/approach
6. Repeat until converged
```

---

### 3.6 Add Multi-Turn Conversation Structure

**Location**: `vtcode-core/src/prompts/system.rs` (new section)

**Key Patterns**:
- Explicit turn boundaries
- State preservation across turns
- Context merge logic
- Conversation memory (what to keep vs. discard)

---

## 4. Detailed Optimization Examples

### 4.1 Example: Complex Refactoring Task

**Before (Current Approach)**:
```
User: "Refactor the authentication module for better error handling"
Model: [One-shot attempt, may miss edge cases]
Result: Partial solution, user must iterate
```

**After (Phase 3)**:
```
User: "Refactor the authentication module for better error handling"

Model (with extended thinking):
Thought 1: Task decomposition
  - Find auth module
  - Identify error paths
  - Design error handling strategy
  - Implement changes
  - Test edge cases

Action 1: [Search for module]
Observation: [Found 5 files, 800 lines total]

Thought 2: Error path analysis
  - Which paths lack error handling?
  - What exceptions are possible?
  - How should errors propagate?

Action 2: [Grep for error patterns]
Observation: [12 unhandled paths identified]

Thought 3: Strategy refinement
  - Use anyhow::Result wrapper
  - Add context for each error
  - Test with mock failures

Action 3: [Make changes]
Action 4: [Run tests]
Observation: [6/6 tests pass, edge cases covered]

Result: Complete solution with testing
```

---

### 4.2 Example: Long-Horizon Task (Context Reset)

**Before (Current Approach)**:
```
Turn 1: [50K tokens] Analyze codebase, propose changes
  → Completion, context accumulates
Turn 2: [90K tokens] Now context-limited, some info lost
  → Reduced quality, re-context needed
Turn 3: [180K tokens] ERROR: Context exceeded
```

**After (Phase 3 with .progress.md)**:
```
Turn 1: [50K tokens] Analyze, propose, save to .progress.md
    Created .progress.md with state snapshot
  
Turn 2: Load .progress.md, continue from state
  - Restore task context (100 lines)
  - Restore file paths (10 lines)
  - Restore key findings (20 lines)
  - Continue with fresh context window
    Reduced context waste from 90K → 30K reused
  
Turn 3: Load updated .progress.md, finalize
    Maintains coherence across resets
    Full context window available for new work
```

---

### 4.3 Example: Multi-LLM Task (Thinking Budget)

**Before (Phase 2)**:
```
Claude 3.5: "Think deeply about this"  
GPT-4o: "Follow these numbered steps"  
Gemini: "Use flat structure"  
```

**After (Phase 3 with thinking)**:
```
Claude 3.5 + Extended Thinking:
  - thinking_budget: 16000 tokens
  - style: "Explore multiple approaches"
  - Result: Most thorough analysis
  
GPT-4o (no thinking available):
  - Use ReAct-style thought-action patterns in output
  - thinking_budget: 0 (simulate via multi-pass)
  - Result: Structured reasoning in output text
  
Gemini 2.0 + Extended Thinking:
  - thinking_budget: 10000 tokens
  - style: "Direct, flat reasoning"
  - Result: Fast, efficient thinking
```

---

## 5. Implementation Roadmap

### Phase 3a: Thinking Patterns (Days 1-2)
1. Add "Extended Thinking & Reasoning" section to system.rs
2. Provide ReAct-style templates
3. Add thinking examples per LLM
4. Update AGENTS.md with thinking guidance
5. Test on 5 complex tasks

**Output**: `PHASE3A_THINKING_PATTERNS.md`

### Phase 3b: Persistent Memory (Days 3-4)
1. Design .progress.md structure
2. Add detection + loading logic in system prompt
3. Implement state merge algorithm
4. Add context window management at 70%/85%/90% thresholds
5. Test on long-horizon tasks (2+ context resets)

**Output**: `PHASE3B_PERSISTENT_MEMORY.md`

### Phase 3c: Semantic Context (Days 5-6)
1. Add semantic clustering rules to context.rs
2. Implement deduplication logic
3. Add hierarchical context prioritization
4. Test on large codebases (50K+ files)

**Output**: `PHASE3C_SEMANTIC_CONTEXT.md`

### Phase 3d: Tool Guidance Refactor (Days 7)
1. Rewrite tool selection section (outcome-focused)
2. Add decision trees for tool choice
3. Provide anti-patterns (when NOT to use each tool)
4. Update examples

**Output**: `PHASE3D_TOOL_GUIDANCE.md`

### Phase 3e: Integration & Testing (Days 8-10)
1. Merge all sections into system.rs
2. Update AGENTS.md comprehensively
3. Test on 50-task validation suite
4. Measure: context efficiency, quality, multi-LLM compatibility
5. Create Phase 3 completion report

**Output**: `PHASE3_COMPLETION_SUMMARY.md`, `PHASE3_OUTCOME_REPORT.md`

---

## 6. Success Criteria

### Quantitative Metrics

| Metric | Current | Target | Method |
|--------|---------|--------|--------|
| Avg tokens/task | 30K (Phase 1) | 18K | Measure on 50-task suite |
| Context efficiency | 90% | 95% | Ratio of useful:total tokens |
| Semantic redundancy | 15% | 5% | Analyze context overlap |
| Thinking quality | N/A | 85%+ | Judge thinking traces |
| Multi-LLM compat | 95% | 98% | Test all 3 models × 50 tasks |
| Error recovery | 65% | 90% | Test on broken states |
| Long-horizon (3+ resets) | N/A | 95% success | Multi-turn task suite |

### Qualitative Metrics

-   Extended thinking enables complex reasoning
-   Persistent memory preserves state across resets
-   Semantic context reduces cognitive load
-   Outcome-focused tool guidance improves choices
-   Iterative refinement converges on solutions
-   Multi-turn conversation feels natural

---

## 7. Risk Assessment & Mitigation

### Risk 1: Extended Thinking May Increase Latency
**Mitigation**: Make thinking optional; default to thinking only for complex tasks (confidence score < 0.7)

### Risk 2: .progress.md Overhead
**Mitigation**: Keep .progress.md compact (<2KB); consolidate at boundaries; auto-cleanup on completion

### Risk 3: Semantic Clustering Complexity
**Mitigation**: Start with simple heuristics (topic tags); evolve to ML-based clustering if needed

### Risk 4: Tool Guidance Ambiguity
**Mitigation**: Keep decision trees simple (max 3 levels); provide clear examples; test on edge cases

### Risk 5: Multi-LLM Testing Burden
**Mitigation**: Automated test harness; focus on compatibility regressions; sample 50 representative tasks

---

## 8. Integration with Phases 1-2 & Phase 4

### Backward Compatibility (Critical)
-   Phase 3 builds on Phase 1 (context curation rules still apply)
-   Phase 3 respects Phase 2 (multi-LLM patterns preserved)
-   .progress.md optional (graceful degradation if not present)
-   Thinking patterns optional (models without thinking capability still work)

### Forward Compatibility (Phase 4)
- Phase 4 (Error Recovery) depends on Phase 3:
  - .progress.md captures error state
  - Thinking patterns help analyze root causes
  - Semantic context identifies error patterns
  - Persistent memory enables retry logic

---

## 9. References & Inspiration

### Research Papers
- **ReAct**: Yao et al., 2022 (Reasoning + Acting in interleaved manner)
- **Extended Thinking**: Anthropic (Claude 3.7 release, 2025)
- **Long-Context Agents**: AWS AgentCore Memory research
- **Test-Time Compute Scaling**: Parallel reasoning for better accuracy

### Production Agent Patterns
- Sourcegraph Cody: Semantic context + @-mentions
- GitHub Copilot: Phased specificity + conversation memory
- Vercel v0: Progressive disclosure + iterative refinement
- Claude API: Extended thinking budget + action scaling
- OpenAI Reasoning Models: High-level goals + trust

### Design Principles
- **Simplicity**: Understandable rules over complex heuristics
- **Efficiency**: 40% context reduction (absolute target)
- **Transparency**: Visible reasoning for debugging
- **Flexibility**: Optional features, graceful degradation
- **Robustness**: Works on all 3 models without regression

---

## 10. Next Actions

### For Team Review
1.   Read this document (sections 1-3 are critical)
2.  Provide feedback on:
   - Thinking pattern examples (realistic?)
   - .progress.md structure (sufficient?)
   - Semantic context rules (too complex?)
3.  Suggest priority ordering (if time-constrained)

### For Implementation
1. Create Phase 3a branch
2. Implement thinking patterns first (highest ROI)
3. Add .progress.md infrastructure
4. Refactor tool guidance
5. Integrate & test

### For Validation
1. Run 50-task suite on Phase 3 prompt
2. Measure metrics vs. Phase 2 baseline
3. Compare multi-LLM compatibility
4. Document learnings in Phase 3 reports

---

## Appendix: Thinking Pattern Templates

### Template 1: ReAct Task Decomposition

```
For complex tasks, structure thinking as:

Thought 1: Task analysis
  - What is the goal?
  - What are the constraints?
  - What information do I need?

Action 1: [First tool call]
Observation 1: [Result analysis]

Thought 2: Updated understanding
  - What did I learn?
  - What's the next step?
  - Am I on track?

Action 2: [Refined tool call]
...

Final Thought: [Verification]
  - Did I meet the goal?
  - Are there edge cases?
  - Is the solution robust?
```

### Template 2: Semantic Context Extraction

```
When processing output, extract:

SEMANTIC ENTITIES:
- Key files: [paths]
- Key functions: [names]
- Key patterns: [descriptions]
- Important decisions: [rationales]

RELATIONSHIPS:
- File A calls File B
- Pattern X applies to Problem Y
- Decision Z enables Future Task W

TEMPORAL INFO:
- Created timestamps
- Modification order
- Dependency order
```

### Template 3: Persistent State Snapshot

```
For .progress.md, capture:

TASK STATE:
- Goal (1 sentence)
- Success criteria (3-5 items)
- Current step (which of N)
- Completion % (0-100%)

CONTEXT SNAPSHOT:
- Key file paths (max 10)
- Important patterns (max 5)
- Architectural insights (max 5)
- Blockers (if any)

NEXT ACTIONS:
1. [Specific action]
2. [With expected outcome]
3. [No more than 5]
```

---

**Document Version**: 1.0  
**Status**: READY FOR IMPLEMENTATION  
**Author**: VT Code Research + Amp AI Agent  
**Date**: November 19, 2025
