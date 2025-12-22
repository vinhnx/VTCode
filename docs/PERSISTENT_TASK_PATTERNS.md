# VT Code Persistent Task Patterns & Thinking Structures

**Status**: Implementation Guide  
**Scope**: Long-horizon tasks (100+ tokens), context compaction, state management  
**Goal**: Enable agents to work on tasks spanning multiple context windows

---

## Executive Summary

Long-running tasks (complex refactoring, large debugging, multi-file analysis) can exceed context limits. This guide provides patterns to maintain coherence without keeping full conversation history.

**Key Patterns**:
1. **Progress Files** (.progress.md) for state tracking
2. **Thinking Structures** (ReAct-style) for complex reasoning
3. **Compaction Strategy** for context resets
4. **Memory Files** for persistent knowledge

---

## Part 1: Progress Files (.progress.md)

### When to Create

Create `.progress.md` when:
- Task requires 5+ distinct steps
- Work spans 100+ tokens or 15+ tool calls
- Context window is 70%+ full
- Task might need to resume after context reset

### Structure

```markdown
# Task: [User's Original Request]

## Metadata
- **Started**: 2025-11-19 14:30
- **Status**: IN_PROGRESS | BLOCKED | COMPLETED
- **Step**: 2/5
- **Context Resets**: 1

## Goal
One-line summary of what user asked for

## Completed Steps
- [x] Step 1: Description of what was done (5-10 words)
- [x] Step 2: Description (can include line numbers, file paths)
- [ ] Step 3: (incomplete)

## Current Work
- **What**: Specific action being taken
- **Where**: File path + line number if applicable
- **Why**: Brief reason for this approach
- **Blocker**: (if any)

## Key Decisions & Rationale
- **Decision 1**: Used pattern X instead of Y because Z (with reasoning)
- **Implementation detail**: File locations, key line numbers
- **Architecture choice**: Any trade-offs made

## Test Status
- Tests passing: [x] or [ ]
- Failed tests: (list if any)
- Coverage: Basic | Comprehensive | Full

## Files Modified
- src/models/user.rs (lines 10-50)
- tests/models_test.rs (added 3 tests)

## Next Action
Be specific. Not: "Continue working"
Instead: "Add email validation to User::new() in src/models/user.rs, starting at line 42"

## Notes
- Anything unusual or worth remembering
- Gotchas discovered
- Alternative approaches tried
```

### Example: Real-World Use

```markdown
# Task: Refactor User struct to support email validation with persistence

## Metadata
- **Started**: 2025-11-19 14:30
- **Status**: IN_PROGRESS
- **Step**: 3/5
- **Context Resets**: 1

## Goal
Add optional email field to User struct with validation rules and database storage

## Completed Steps
- [x] Step 1: Found User struct in src/models/user.rs (lines 12-40)
- [x] Step 2: Analyzed current fields and constraints
- [ ] Step 3: Design validation rules and database schema
- [ ] Step 4: Implement email field + validation
- [ ] Step 5: Update tests + verify backward compatibility

## Current Work
- **What**: Design email validation approach
- **Where**: Still in analysis phase
- **Why**: Need to decide: regex validation vs. email crate
- **Blocker**: None yet

## Key Decisions & Rationale
- **Decision 1**: Use email-validator crate instead of regex (more maintainable, follows Rust ecosystem)
- **File structure**: User struct in src/models/user.rs, tests in tests/models_test.rs
- **Database**: Will use existing migration pattern in db/migrations/

## Test Status
- Tests passing: [x] (existing tests still pass)
- Failed tests: None
- Coverage: Basic (existing User tests)

## Files Modified So Far
- src/models/user.rs: Read only (lines 12-40)
- Cargo.toml: Not modified yet (will add email-validator)

## Next Action
1. Add email-validator to Cargo.toml
2. Implement email field in User struct with validation
3. Add tests for email validation

## Notes
- User struct has custom serialization (line 38) - must preserve
- Existing tests use builder pattern - update tests to include email
- Database migration will be needed for existing users
```

---

## Part 2: Thinking Structures (ReAct Pattern)

### When to Use

For tasks with **3+ decision points** or **unclear scope**, explicitly plan work:

```markdown
<task_analysis>
  <goal>
    Restate the user request in your own words.
    Be specific about what needs to change.
  </goal>
  
  <scope>
    What files will change?
    Which functions/modules are affected?
    How many distinct changes?
  </scope>
  
  <complexity>
    Number of steps: 3-5? More?
    Risky operations? (deleting code, breaking API)
    Testing needs? (unit tests, integration tests)
  </complexity>
  
  <strategy>
    High-level approach (3-5 sentences).
    Why this approach vs. alternatives?
    Expected risks or gotchas?
  </strategy>
</task_analysis>
```

### Execution Flow

After planning, execute step-by-step with explicit narration:

```markdown
<execution>

<step num="1">
  <action>Search for User struct definition</action>
  <command>grep -r "struct User" src/</command>
  <result>Found in src/models/user.rs:12</result>
  <observation>File has 150 lines; struct definition spans lines 12-40</observation>
  <next>Read the full struct to understand current fields</next>
</step>

<step num="2">
  <action>Analyze current User struct fields</action>
  <command>read_file(path="src/models/user.rs", range=[12, 40])</command>
  <result>
    Fields: id, name, email (existing), created_at, updated_at
    Email field already exists! But it's not validated.
  </result>
  <observation>Don't need to add email field; just add validation</observation>
  <next>Check if there are email validation functions elsewhere</next>
</step>

<step num="3">
  <action>Search for existing validation patterns</action>
  <command>grep -r "fn validate" src/models/</command>
  <result>Found User::validate() in user.rs:120</result>
  <observation>Validation is done in a separate method, not in new()</observation>
  <next>Add email validation to User::validate()</next>
</step>

<step num="4">
  <action>Add email validation logic</action>
  <command>edit_file(...)</command>
  <result>
    Added email validation: must be non-empty and contain @
    Updated User::validate() to check email
  </result>
  <observation>Tests will fail until we update test data</observation>
  <next>Update tests to use valid email addresses</next>
</step>

<step num="5">
  <action>Update tests</action>
  <command>grep -r "User {" tests/</command>
  <result>Found 5 test cases using hardcoded User data</result>
  <observation>Some tests use invalid emails (e.g., "test")</observation>
  <next>Fix test data + run tests</next>
</step>

<step num="6">
  <action>Run tests to verify</action>
  <command>cargo test</command>
  <result>All tests pass</result>
  <observation>Done!</observation>
</step>

</execution>
```

### Benefits of This Approach

  **Clear reasoning**: Each step shows why the action was taken  
  **Easy to resume**: If context resets, can see exact progress  
  **Debugging**: If something fails, can point to exact step  
  **Collaboration**: Other engineers can understand the logic  

---

## Part 3: Compaction Strategy (Context Resets)

### When Context Gets Too Full

At **80%+ of context used**, start compaction:

### Step 1: Create State File

```bash
# Create .progress.md with current state (see template above)
# Include: completed steps, current task, key decisions, next action
# Make it readable for humans AND resumable by agents
```

### Step 2: Save Key Results

```markdown
## Cached Results (for next context window)

### File Locations
- User struct: src/models/user.rs:12-40
- Validation function: src/models/user.rs:120-150
- Tests: tests/models_test.rs

### Line Numbers (for quick navigation)
- Email field declaration: src/models/user.rs:18
- Validation logic: src/models/user.rs:125-130

### Architecture Decisions
- Use email-validator crate (RFC 5321 compliant)
- Validation in User::validate(), not new()
- Tests use builder pattern (already updated)

### Current Error (if any)
- None; tests are passing
```

### Step 3: Forget Non-Essential

Remove from context:
- Verbose tool outputs (already used)
- Long grep results (file locations noted)
- Full file contents (only keep line numbers)
- Explanatory text from earlier exchanges

### Step 4: Start Fresh Context

```markdown
---
# [Starting fresh context window]

I'm continuing work on a task from the .progress.md file.

**Task Summary**: Add email validation to User struct  
**Status**: Step 4/5 complete; next is updating tests  
**Key Files**: src/models/user.rs, tests/models_test.rs  
**Progress**: Email validation implemented; tests need updates  

Reading .progress.md for current state...
[Continue from "Next Action" section]
```

---

## Part 4: Memory Files (Persistent Knowledge)

### Purpose

For tasks with learnings that apply to future work (not just this task), create memory files:

### Pattern: Common Knowledge Files

#### CLAUDE.md or VTCODE.md (Project-level)

```markdown
# Project Context & Patterns

## Build Commands
- `cargo check` - Quick validation
- `cargo test` - Run tests
- `cargo clippy` - Linting

## Code Style
- snake_case for functions/variables
- PascalCase for types
- 4-space indentation
- No unwrap() - use ? or anyhow::Result

## Architecture Patterns
- Validation in separate methods (not constructors)
- Database operations in db/ module
- Tests use builder pattern for setup

## Common Gotchas
- User::new() is lightweight (doesn't validate)
- Validation is explicit: call user.validate()?
- Tests must use valid email addresses (@ required)
```

#### NOTES.md (Task-specific)

```markdown
# Task-Specific Notes

## Email Validation Discovery
- Chose email-validator crate over regex (RFC 5321)
- Added to Cargo.toml (already done)
- Validation function: User::validate()

## Test Pattern
- Always use builder pattern: User::builder().email("test@example.com").build()?
- Invalid emails trigger validation error
- Tests cover: valid, empty, no @, no domain

## Performance Note
- Email validation is regex-based (from email-validator)
- Not a bottleneck for our use case (user creation)
```

### When to Create

Create memory files when:
- Learning applies to **multiple future tasks**
- Project has **recurring patterns** (validation, error handling)
- Team should **know about gotchas** (API quirks, configuration)

---

## Part 5: Long-Horizon Task Example (Start to Finish)

### Initial Task Request

```
User: Refactor User struct to support email validation with persistence.
Must maintain backward compatibility and not break existing tests.
```

### Context Window 1 (Tokens 1-2000)

```markdown
<task_analysis>
  <goal>Add email field validation to User struct + store in database</goal>
  <scope>
    - src/models/user.rs (User struct + validation)
    - db/migrations/ (add email column)
    - tests/models_test.rs (update tests)
  </scope>
  <complexity>
    - 4 distinct steps
    - Database migration needed (risky)
    - 5+ test files to update
  </complexity>
  <strategy>
    1. Analyze current User struct and validation
    2. Design email validation rules
    3. Update struct + add validation logic
    4. Create database migration
    5. Update tests
  </strategy>
</task_analysis>

[Agent executes steps 1-3]

<progress>
  Completed: Steps 1-3 (analysis done, validation logic added)
  Tokens used: 1850/2000
  Next: Database migration
</progress>

[Agent creates .progress.md before context reset]
```

### Context Window 2 (Starting with .progress.md)

```markdown
---
Resuming from .progress.md:
- Task: Email validation for User struct
- Status: Step 3/5 complete
- Next: Create database migration

---

[Agent reads .progress.md, understands state, continues]

[Executes steps 4-5]

Completed: Database migration created, tests updated
All tests passing

Task complete.
```

---

## Part 6: Compaction Examples

### Example 1: After Task Completes

**Before compaction** (verbose):
```
User: Add email validation
Agent: I'll analyze the User struct...
[50 lines of explanation]
Agent: Found it in src/models/user.rs...
[100 lines of grep output]
Agent: Now I'll add validation...
[Long diff]
Agent: Tests updated...
[Verbose summary]
```

**After compaction** (state file):
```
# Task: Add email validation to User struct

## Status: COMPLETED

## What Was Done
- Added email-validator crate to Cargo.toml
- Updated User::validate() to validate email format
- Updated 5 tests to use valid email addresses
- All tests passing

## Files Modified
- Cargo.toml: Added email-validator dependency
- src/models/user.rs: Updated validate() method
- tests/models_test.rs: Fixed test data

## Why This Approach
- email-validator is RFC 5321 compliant
- Validation in validate() method (existing pattern)
- Tests follow builder pattern (existing style)
```

### Example 2: Context Reset Mid-Task

**Progress file** (saved before reset):
```
# Task: Refactor error handling in api module

## Status: IN_PROGRESS
## Step: 2/4

## Completed
- [x] Analyzed error types in src/api/errors.rs
- [x] Identified 12 error variants needing refactoring

## Current Work
- Implementing new error struct
- File: src/api/errors.rs:50-100

## Next Action
Complete error struct implementation in src/api/errors.rs,
then update error handling in src/api/handlers/ (3 files)

## Key Decision
Using Error::new(type, message) pattern instead of enum variants
(matches rest of codebase)
```

**Resume** (next context):
```
Resuming refactor of error handling in api module.
Current status from .progress.md:
- Analyzed errors (done)
- Current: Complete error struct implementation
- Next: Update 3 handler files

Reading src/api/errors.rs at line 50...
```

---

## Part 7: Implementation Checklist

### For VT Code Team

- [ ] **Tier 1: Progress Files**
  - [ ] Document .progress.md template
  - [ ] Implement support for reading/writing .progress.md
  - [ ] Add to gitignore (local state file)

- [ ] **Tier 2: Thinking Structures**
  - [ ] Add optional thinking markers to system prompt
  - [ ] Document when to use (3+ step tasks)
  - [ ] Provide examples

- [ ] **Tier 3: Compaction Logic**
  - [ ] Implement token counting in context
  - [ ] Trigger .progress.md creation at 80% full
  - [ ] Detect + resume from .progress.md on startup

- [ ] **Tier 4: Memory Files**
  - [ ] Document CLAUDE.md / VTCODE.md pattern
  - [ ] Document NOTES.md pattern
  - [ ] Add to documentation

- [ ] **Testing**
  - [ ] Test: Long task spanning 2+ context resets
  - [ ] Test: Resume from .progress.md
  - [ ] Test: Compaction doesn't lose critical info
  - [ ] Benchmark: Token savings from compaction

---

## Part 8: Best Practices

### DO

  Create .progress.md for **5+ step tasks**  
  Update .progress.md **after each major step**  
  Include **file paths + line numbers** for quick navigation  
  State **next action clearly** (not vague)  
  Track **key decisions** (why, not just what)  
  Use thinking structures for **complex reasoning**  
  Compaction at **80% context used**  

### DON'T

  Over-update .progress.md (once per step is enough)  
  Include verbose outputs in state file  
  Forget to note **file paths + line numbers**  
  Skip thinking analysis for complex tasks  
  Wait until 95% full to compact (harder recovery)  
  Create progress file for simple tasks (<5 steps)  

---

## Part 9: Troubleshooting

### Issue 1: Resume Lost Context
**Problem**: Agent resumed from .progress.md but forgot key implementation detail  
**Solution**: Include decision rationale in .progress.md (not just what, but why)

### Issue 2: Compaction Too Aggressive
**Problem**: Summarized away critical information  
**Solution**: Require 3+ uses of info before discarding it

### Issue 3: Progress File Out of Sync
**Problem**: .progress.md shows wrong current step  
**Solution**: Update .progress.md at START of each step, not end

### Issue 4: Thinking Structures Too Verbose
**Problem**: Thinking adds 500+ tokens, doesn't help  
**Solution**: Use thinking only for 3+ step tasks; skip for simple work

---

## Conclusion

Persistent task patterns enable VT Code agents to:

1.   Work on **arbitrarily long tasks** (no context limit)
2.   **Resume gracefully** after context resets
3.   **Maintain coherence** across multiple windows
4.   **Scale to enterprise tasks** (large refactorings, migrations)

**Implementation Priority**:
1. Progress files (.progress.md) - high impact, low effort
2. Thinking structures - medium impact, low effort
3. Compaction logic - high impact, medium effort
4. Memory files - low priority, medium effort

---

**Document Version**: 1.0  
**Last Updated**: Nov 2025  
**Review By**: VT Code Team  
**Implementation Target**: Week 3 of optimization rollout
