# Plan Mode

Plan Mode is a read-only exploration mode that allows you to iterate with the agent on what you want to build before any code gets written.

## Overview

In Plan Mode, the agent can:
- **Read files** - explore the codebase structure
- **Search code** - use grep, structural search, and other read-only search tools
- **Analyze patterns** - understand architecture and design decisions

In Plan Mode, the agent **cannot**:
- Edit files or apply patches
- Run shell commands or tests
- Execute any mutating operations

`task_tracker` is available in Plan Mode and mirrors checklist state between `.vtcode/tasks/current_task.md` and active plan sidecar files under `.vtcode/plans/`. `plan_task_tracker` remains available as a compatibility alias. Plan output should use `<proposed_plan>...</proposed_plan>`.

## Benefits

1. **Better code quality**: By the time you start coding, the agent knows exactly what to do and has all the context it needs
2. **Validate assumptions**: Catch ambiguities and edge cases before implementation
3. **Reduce iteration cycles**: Discuss trade-offs and refine your approach upfront
4. **Build context**: The agent explores your codebase and loads relevant files before making changes

## Usage

### Starting a session in Plan Mode

```bash
vtcode --permission-mode plan
```

### Toggling Plan Mode in a session

```
/plan        # Toggle Plan Mode on/off
/plan on     # Enable Plan Mode
/plan off    # Disable Plan Mode
```

### Typical workflow

1. **Start in Plan Mode**: `vtcode --permission-mode plan`
2. **Describe your goal**: Explain what you want to build or change
3. **Iterate on the plan**: Ask clarifying questions, explore files, refine the approach
4. **Review the plan**: The agent captures repository facts, closes any open decisions, then emits one `<proposed_plan>` block
5. **Choose next action**: Use the implementation prompt to switch to Edit mode or continue planning (fallback: manually switch with `/plan off` or `/mode`, or `Shift+Tab`/`Alt+M`)
6. **Execute the plan**: If approved, coding proceeds in Edit mode

## Plan Output Format

When in Plan Mode, the agent should keep the output decision-complete but sparse:

```markdown
Repository facts checked:
- [file, symbol, or behavior confirmed from the repo]
- [existing pattern or constraint verified before planning]

Next open decision: [if any], otherwise: No remaining scope decisions.

<proposed_plan>
# [Task Title]

## Summary
[2-4 lines: goal, user impact, what will change, what will not]

## Implementation Steps
1. [Step] -> files: [paths] -> verify: [check]
2. [Step] -> files: [paths] -> verify: [check]
3. [Step] -> files: [paths] -> verify: [check]

## Test Cases and Validation
1. Build and lint: [project build and lint command(s) based on detected toolchain]
2. Tests: [project test command(s) based on detected toolchain]
3. Targeted behavior checks: [explicit commands/manual checks]

## Assumptions and Defaults
1. [Explicit assumption]
2. [Default chosen when user did not specify]
3. [Out-of-scope items intentionally not changed]
</proposed_plan>

Only `Next open decision` is used as the explicit reopen marker for follow-up planning.
```

## Plan Review Gate

After a plan is ready, the execution confirmation should use this 4-way gate:

1. Yes, clear context and auto-accept edits (Recommended)
2. Yes, auto-accept edits
3. Yes, manually approve edits
4. Type feedback to revise the plan

## Best Practices

1. **Use dictation**: Speak your ideas naturally; AI doesn't need perfect grammar
2. **Be specific**: Mention concrete files, functions, or patterns you want to work with
3. **Ask for clarification**: Request the agent to explain trade-offs or alternatives
4. **Review the plan**: Before exiting Plan Mode, ensure you're happy with the approach

## Configuration

Plan Mode can be combined with other configuration options in `vtcode.toml`:

```toml
[agent]
# Plan Mode respects HITL settings
human_in_the_loop = true
```

## See Also

- [CLI Reference](CLI_REFERENCE.md) - Full CLI documentation
- [Configuration](config/CONFIGURATION_PRECEDENCE.md) - Configuration options
