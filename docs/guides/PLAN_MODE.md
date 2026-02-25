# Plan Mode

Plan Mode is a read-only exploration mode that allows you to iterate with the agent on what you want to build before any code gets written.

## Overview

In Plan Mode, the agent can:
- **Read files** - explore the codebase structure
- **Search code** - use grep, code intelligence, and other search tools
- **Analyze patterns** - understand architecture and design decisions

In Plan Mode, the agent **cannot**:
- Edit files or apply patches
- Run shell commands or tests
- Execute any mutating operations

`task_tracker` is unavailable in Plan Mode. Use `plan_task_tracker` for plan-scoped checklist updates under `.vtcode/plans/`. Plan output should use `<proposed_plan>...</proposed_plan>`.

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
4. **Review the plan**: The agent emits a structured `<proposed_plan>` block and a Plan item
5. **Choose next action**: Use the implementation prompt to switch to Edit mode or continue planning
6. **Execute the plan**: If approved, coding proceeds in Edit mode

## Plan Output Format

When in Plan Mode, the agent produces structured implementation plans inside a dedicated block:

```markdown
<proposed_plan>
## Summary
Brief description of the goal.

## Steps
1. Step with concrete files/symbols
2. Step with verification detail

## Risks
- Key tradeoff or dependency
</proposed_plan>
```

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
- [Subagents](subagents/SUBAGENTS.md) - Creating planning-focused subagents
