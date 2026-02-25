# Desire Paths in VT Code

This document explains the design philosophy of paving "desire paths" in VT Code—optimizing the tool to work naturally with how agents think and work.

## Philosophy

A desire path is a common pattern that emerges from how users actually behave, rather than how designers intended them to behave. In software, this principle means:

**When an agent intuitively guesses wrong about a feature, we should improve the interface to make that guess right.**

Instead of:
- Documenting that agents are using the tool incorrectly
- Rejecting intuitive commands

We do:
- Add aliases that match intuitive expectations
- Implement flags agents naturally expect
- Design interfaces that align with agent mental models

Over time, this compounds. Each small UX improvement reduces friction, and agents naturally stop making "mistakes" because the tool now works the way they naturally think.

## Current Paved Paths

### Cargo Aliases

Agents intuitively try short command names. We've paved these paths:

| Intuitive Guess | Mapped Command | Status |
|---|---|---|
| `cargo t` | `cargo test` | ✅ Implemented |
| `cargo c` | `cargo check` | ✅ Implemented |
| `cargo r` | `cargo run` | ✅ Implemented |

**Location**: `.cargo/config.toml` → `[alias]` section

**Why it works**: Agents coming from other CLI tools expect abbreviated commands. By providing these aliases, we eliminate the cognitive overhead of remembering full command names.

### Test Invocation Patterns

Agents expect these patterns to work without explanation:

| Intuitive Usage | Behavior | Status |
|---|---|---|
| `cargo test function_name` | Run tests matching function name | ✅ Works natively |
| `cargo test --lib` | Run unit tests only | ✅ Works natively |
| `cargo test --integration` | Run integration tests only | ✅ Works natively |

These are native Cargo behaviors, but documented here for agent awareness.

## Desire Paths to Implement

These are patterns agents have tried or might try that aren't yet smooth:

### 1. Tool Operation Shortcuts

**Current friction**:
```
unified_search {
  "action": "grep",
  "pattern": "fn main",
  "path": "src/"
}
```

**Intuitive expectation**:
```
unified_search grep "fn main" src/
```

**Priority**: Medium (requires tool refactoring)

### 2. Subagent Naming

**Current friction**:
```
spawn_subagent {
  "prompt": "...",
  "subagent_type": "explore"
}
```

**Intuitive expectation**:
```
spawn_subagent --name explore --prompt "..."
```

**Priority**: Medium (ergonomics improvement)

### 3. Configuration Shortcuts

**Current friction**:
- Agents must know exact config file locations
- Need to remember TOML structure

**Intuitive expectation**:
- `vtcode config set llm.model gpt-4`
- `vtcode config get llm.model`
- `vtcode config edit` (opens default editor)

**Priority**: Low (workaround exists: direct file editing)

## How to Report Friction

If you notice an agent (or yourself) repeatedly guessing at:
- A flag that doesn't exist
- A command structure that's not intuitive
- A subcommand name that's confusing
- A configuration pattern that's hard to remember

1. **Document it** in this file under "Desire Paths to Implement"
2. **Note the pattern**: Why did they guess that way?
3. **Propose the improvement**: What would be more intuitive?
4. **Prioritize**: Is it a common blocker or edge case?

## Implementation Checklist

When implementing a new desire path:

- [ ] Identify the intuitive expectation
- [ ] Implement the alias/shortcut/flag
- [ ] Verify it works with `cargo test` or similar
- [ ] Document in AGENTS.md
- [ ] Document in DESIRE_PATHS.md
- [ ] Consider if the change needs release notes
- [ ] Update any relevant CLI help text

## Related Documentation

- **AGENTS.md**: Agent UX & Desire Paths section for quick reference
- **CLAUDE.md**: Development guidelines (includes Desire Paths philosophy)
- **docs/ARCHITECTURE.md**: System architecture

## Examples in Practice

### Example: Cargo Aliases Success

**Before**: Agents would type `cargo test` (correct), but try `cargo t` (wrong)
- Error: "Unknown subcommand"
- Friction: Agent has to remember the full command

**After**: Added `t`, `c`, `r` aliases in `.cargo/config.toml`
- Result: Both `cargo test` and `cargo t` work
- Friction: Eliminated
- Bonus: Tool aligns with Unix conventions (short flags)

This small change has compounding effects:
- Agents build muscle memory faster
- Tool feels more polished
- Reduces cognitive load

### Example: Help Text Improvement

When agents ask for help, the response should be clear and actionable:

```bash
$ cargo invalid-command
error: unknown subcommand `invalid-command`

Did you mean one of these?
    test     Run tests (`cargo t` for short)
    build    Build the project
    check    Check compilation (`cargo c` for short)
```

By making suggestions and noting aliases in help text, we guide agents toward good practices.

---

**Last Updated**: Dec 30, 2025  
**Philosophy Introduced By**: Amp AI Agent  
**Based On**: Wikipedia's "Desire Path" concept
