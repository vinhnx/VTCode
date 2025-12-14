# VT Code Multi-LLM Compatibility Guide

**Status**: Implementation Ready  
**Scope**: Claude, GPT-4/4o, Google Gemini 2.0+  
**Goal**: 95%+ compatibility across all models

---

## Executive Summary

VT Code system prompt can work across multiple LLM providers. This guide documents model-specific behaviors and adjustments needed for uniform performance.

**Key Finding**: Universal instruction patterns work better than model-specific prompts. Differences are handled through parameter tuning + optional sections rather than separate prompts.

---

## Part 1: Model Capabilities Matrix

### Instruction Comprehension

| Feature | Claude 3.5 | GPT-4o | Gemini 2.0 | Notes |
|---------|-----------|--------|-----------|-------|
| **XML Tags** |   Excellent |   Good |   Good | Structured thinking |
| **Markdown Headers** |   Excellent |   Excellent |   Excellent | Preferred |
| **Nested Conditionals** |   Works (5 levels) |  Works (3 levels) |  Works (2 levels) | Keep flat when possible |
| **Long Instructions** |   200K tokens |  128K tokens |   1M tokens | Compress for GPT |
| **Few-Shot Examples** |   5+ examples |   3-5 examples |   3-5 examples | Quality > quantity |
| **Structured Output** |   JSON/XML |   JSON/XML |   JSON/XML | All support |
| **Tool Use** |   Native |   Native |   Native | Different formats |

### Context Window & Token Management

| Model | Context | Typical Input | Safe Zone | Recommen​ded Max |
|-------|---------|---------------|-----------|------------------|
| Claude 3.5 Sonnet | 200K | 4K | 150K | 180K |
| GPT-4o | 128K | 4K | 100K | 120K |
| Gemini 2.0 | 1M | 4K | 800K | 900K |

---

## Part 2: Instruction Language Differences

### Universal Patterns (Works Everywhere)

  **Use These**:
```
- Direct commands: "Find the fetch_user function"
- Active voice: "Add validation to the email field"
- Specific outcomes: "Return the file path and line number"
- Examples with input/output pairs
- Markdown headers for structure
```

### Model-Specific Adjustments

#### Claude 3.5 Sonnet
**Strengths**: Excellent reasoning, loves detailed instructions, strong XML parsing

**Optimal patterns**:
- Detailed system prompts (2K+ tokens acceptable)
- XML tags for structure: `<task>, <analysis>, <result>`
- "IMPORTANT" and "CRITICAL" keywords work well
- Long chains of thought (thinking markers)

**Example**:
```
<task>
  <goal>Refactor User struct to support email validation</goal>
  <scope>Change User struct in src/models/user.rs</scope>
  <constraints>Must maintain backward compatibility</constraints>
</task>
```

#### GPT-4/4o
**Strengths**: Fast, good at coding, prefers conciseness

**Optimal patterns**:
- Compact instructions (compress unused sections)
- Numbered lists instead of nested structures
- Examples are powerful (3-4 good examples > long explanation)
- Instruction clarity > creative phrasing

**Example**:
```
1. Find User struct in src/models/user.rs
2. Add email field with validation
3. Update tests
4. Run: cargo test
```

#### Google Gemini 2.0
**Strengths**: Large context, multimodal, fast reasoning

**Optimal patterns**:
- Straightforward, direct language
- Flat instruction lists (avoid deep nesting)
- Explicit parameter definitions
- Works well with images/diagrams (not used in CLI context)

**Example**:
```
Task: Add email validation to User struct
File: src/models/user.rs
Required: Update tests, maintain backward compatibility
```

---

## Part 3: Tool Format Differences

### Function Calling Format

| Model | Format | Example |
|-------|--------|---------|
| Claude | Tool calls in text + XML | `<tool>read_file</tool><path>/path</path>` |
| GPT | Function calling API | `{"type": "function", "function": {...}}` |
| Gemini | Function calling native | Google-style function calls |

**Impact**: VT Code abstracts this; agents don't see the difference.

### Error Message Handling

```
Claude: Verbose error messages, explains context
→ Use full error output for debugging

GPT: Concise error messages
→ Extract key info, ignore padding

Gemini: Standard error messages
→ Similar to GPT, extract key parts
```

---

## Part 4: Prompt Tuning per Model

### Temperature & Sampling

| Model | Recommend​ed | Range | Use Case |
|-------|-------------|-------|----------|
| Claude | 1.0 (default) | 0-1 | Coding tasks prefer 1.0 |
| GPT-4o | 1.0 | 0-2 | 1.0 for reliability, 1.5+ for creativity |
| Gemini | 1.0 | 0-2 | 1.0 for consistency |

**For coding tasks**: Use 1.0 across all models (consistency > creativity).

### Top-P (Nucleus Sampling)

| Model | Recommended | Notes |
|-------|-------------|-------|
| Claude | 1.0 (default) | Works well |
| GPT | 1.0 (default) | Can go 0.9 for more focused |
| Gemini | 1.0 (default) | Works well |

### Max Tokens

| Model | Safe Default | Max | Notes |
|-------|-------------|-----|-------|
| Claude | 2048 | 4096 | Usually sufficient |
| GPT | 1024 | 2048 | More conservative |
| Gemini | 2048 | 4096 | Can use higher |

---

## Part 5: Behavioral Differences

### Tool Call Patterns

**Claude**:
- Calls multiple tools in sequence
- Reads tool outputs before next action
- Good at chaining complex operations

**GPT**:
- Prefers direct, simple tool sequences
- Less good at "think then call" patterns
- Works best with explicit examples

**Gemini**:
- Flexible with tool usage
- Handles complex chains well
- Prefer explicit instruction ordering

### Error Recovery

**Claude**: 
- Retries intelligently
- Good at understanding error context
- May retry too many times (set hard limits)

**GPT**:
- Gives up faster on retry
- Prefers clear failure paths
- Need explicit recovery instructions

**Gemini**:
- Middle ground
- Good error messages
- Follows explicit retry instructions

---

## Part 6: Practical Testing Checklist

### For Each New Feature

Test these scenarios on **all three models**:

1. **Simple Task** (file read + edit)
   ```
   Model: Claude | GPT | Gemini
    Completed task
    Token usage reasonable
    Error handling worked
   ```

2. **Tool Chain Task** (search → read → edit → verify)
   ```
   Model: Claude | GPT | Gemini
    Followed tool order
    Understood context between steps
    Verified results
   ```

3. **Error Recovery Task** (command fails, tries alternative)
   ```
   Model: Claude | GPT | Gemini
    Detected error
    Tried recovery strategy
    Gave up appropriately
   ```

4. **Long Task** (100+ token conversation)
   ```
   Model: Claude | GPT | Gemini
    Maintained context
    Didn't loop indefinitely
    Created .progress.md if needed
   ```

5. **Complex Coding Task** (multi-file refactoring)
   ```
   Model: Claude | GPT | Gemini
    Understood scope
    Checked for side effects
    Ran tests
    Completed successfully
   ```

---

## Part 7: Model-Specific Prompt Sections

### How to Implement Model-Specific Variants

Option A: **Single Prompt with Conditional Sections** (Recommended)

```markdown
# VT Code System Prompt

[TIER 0: CORE - Same for all models]

[TIER 1: ESSENTIAL - Same for all models]

### Tool Selection (Model-Specific)
[Claude]
When searching, use grep_file with detailed patterns
[/Claude]

[GPT]
Keep grep patterns simple and direct
[/GPT]

[Gemini]
Flatten nested conditions in patterns
[/Gemini]

[TIER 2: ADVANCED - Same for all models]

[TIER 3: REFERENCE - Same for all models]
```

Option B: **Separate Prompt Files** (Not Recommended)
- More complex to maintain
- Harder to spot differences
- Increases validation burden

**Recommendation**: Use Option A (conditional sections within single file).

---

## Part 8: Validation & Benchmarking

### Benchmark Suite (50-Task Test)

**Categories**:
- 10 simple file operations (read, search, edit)
- 10 tool chains (search → analyze → edit)
- 10 error recovery scenarios
- 10 long tasks (100+ tokens)
- 10 complex multi-file refactorings

**Metrics to Measure**:
1. **Context Efficiency**: Tokens used / task complexity
2. **Accuracy**: Task completed correctly without errors
3. **Speed**: Tool calls required / typical count
4. **Error Handling**: Recovery rate from errors
5. **Multi-LLM Consistency**: Score variance across models

**Pass Criteria**:
- All 50 tasks complete successfully
- Context usage: ±10% across models
- Error recovery: 95%+ success
- Token efficiency: Baseline ±15%

### Benchmark Execution

```bash
# Pseudocode structure
for each_model in [claude, gpt4o, gemini]:
    for each_task in benchmark_suite:
        prompt = load_prompt(model_specific=True)
        result = run_task(prompt, task)
        measure(tokens, speed, accuracy, errors)
    
    report_metrics(model, results)
    compare_to_baseline()
```

---

## Part 9: Known Issues & Workarounds

### Issue 1: GPT Context Window Too Small
**Problem**: GPT-4o has 128K context; large prompts reduce available space  
**Workaround**:
- Load only TIER 0 + TIER 1 by default
- Load TIER 2 only when `complex_task=true`
- Compress examples in TIER 1

### Issue 2: Gemini Struggles with Deep Nesting
**Problem**: Nested conditions confuse Gemini  
**Workaround**:
- Flatten tool selection decision tree for Gemini
- Use numbered lists instead of conditionals
- Keep instruction depth ≤ 2 levels

### Issue 3: Claude Over-Explanatory
**Problem**: Claude produces verbose outputs even when asked for conciseness  
**Workaround**:
- Add explicit: "Keep response to 1-2 sentences"
- Use "ULTRA IMPORTANT" for critical brevity requirements
- Provide examples of desired conciseness

### Issue 4: Error Recovery Differences
**Problem**: Different retry patterns per model  
**Workaround**:
- Hardcode retry limits: max 2 retries for all models
- Explicit error handlers per exit code
- Clear "STOP" conditions to prevent infinite loops

---

## Part 10: Migration Path

### Phase 1: Validation (Week 1)
- [ ] Create 50-task benchmark suite
- [ ] Run benchmark on current prompt (baseline)
- [ ] Test across Claude, GPT, Gemini

### Phase 2: Rollout (Week 2-3)
- [ ] Update system prompt with conditional sections
- [ ] Test on 20% of tasks across all models
- [ ] Validate metrics against baseline

### Phase 3: Deployment (Week 4)
- [ ] Full rollout to all models
- [ ] Monitor real-world performance
- [ ] Iterate on issues

### Phase 4: Optimization (Ongoing)
- [ ] Quarterly benchmark runs
- [ ] Model-specific tuning as needed
- [ ] Update compatibility matrix

---

## Part 11: Quick Reference: Per-Model Adjustments

### Prompt Section Adjustments

| Section | Claude | GPT | Gemini |
|---------|--------|-----|--------|
| Tool selection tree | Full detail | Simplified + examples | Flat list |
| Examples | 5 detailed | 3-4 concise | 4 clear |
| Thinking patterns | Full XML | Optional XML | Markdown only |
| Context limits | 150K+ | 100K max | 400K+ |
| Retry instructions | "IMPORTANT" | Numbered steps | Direct language |
| Error codes | Full list | Top 5 | Top 5 |

### Temperature & Sampling

```
For all models, coding tasks:
temperature = 1.0
top_p = 1.0
max_tokens = 2048 (Claude), 1024 (GPT), 2048 (Gemini)
```

---

## Conclusion

VT Code can achieve **95%+ compatibility** across Claude, GPT, Gemini through:

1.   Universal base prompt (Tiers 0-3)
2.   Model-specific sections (marked with [Model] tags)
3.   Consistent testing & benchmarking
4.   Clear error handling + retry patterns
5.   Token budget awareness per model

**Next Steps**:
1. Implement conditional sections in system prompt
2. Run 50-task benchmark on current baseline
3. Validate new prompt on all 3 models
4. Deploy with monitoring
5. Iterate quarterly

---

**Document Version**: 1.0  
**Last Updated**: Nov 2025  
**Review By**: VT Code Team  
**Testing**: Planned for Week 1 of implementation
