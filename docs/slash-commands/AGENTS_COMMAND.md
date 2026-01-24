# /agents Command

The `/agents` command provides an interface to manage VT Code subagents, which are specialized AI assistants for task-specific workflows.

## Overview

Subagents enable more efficient problem-solving by providing task-specific configurations with customized system prompts, filtered tool access, and isolated context windows.

## Usage

### List Available Subagents

```
/agents
/agents list
/agents ls
/subagents
/subagents list
```

Lists all available subagents (built-in and custom).

**Built-in Subagents:**
- `explore` - Fast, lightweight agent for searching and analyzing codebases (haiku)
- `plan` - Research specialist for planning mode (sonnet)
- `general` - Capable general-purpose agent for complex, multi-step tasks (sonnet)
- `code-reviewer` - Code quality and security review expert
- `debugger` - Error investigation and fix specialist

### Create a New Subagent

```
/agents create
/agents new
/subagents create
```

Starts the interactive creation workflow for a new subagent. You can either:
1. Use Claude to generate the configuration with a description of what the subagent should do
2. Create the configuration file manually in `.vtcode/agents/`

**Manual Creation:**
Create a Markdown file with YAML frontmatter in `.vtcode/agents/` or `~/.vtcode/agents/`:

```markdown
---
name: my-agent
description: Description of what this agent does
tools: read_file, grep_file, run_pty_cmd
model: sonnet
permissionMode: default
skills: skill1, skill2
---

Your system prompt goes here.
This can be multiple paragraphs and should clearly define
the agent's role, capabilities, and approach.
```

### Edit an Existing Subagent

```
/agents edit <agent-name>
/agents edit code-reviewer
/subagents edit <agent-name>
```

Shows instructions for editing an existing subagent configuration.

### Delete a Subagent

```
/agents delete <agent-name>
/agents remove <agent-name>
/agents rm <agent-name>
/subagents delete <agent-name>
```

Shows instructions for deleting a subagent configuration.

### Show Help

```
/agents help
/agents --help
```

Displays command usage information.

## Subagent Configuration

### Enablement

Subagents are disabled by default. Enable them in `vtcode.toml`:

```toml
[subagents]
enabled = true
```

### File Locations

| Type | Location | Scope | Priority |
|------|----------|-------|----------|
| **Project subagents** | `.vtcode/agents/` | Current project only | Highest |
| **User subagents** | `~/.vtcode/agents/` | All projects | Lower |
| **Built-in subagents** | Shipped with binary | Available everywhere | Lowest |

### Configuration Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Unique identifier (lowercase with hyphens) |
| `description` | Yes | Natural language description of the agent's purpose |
| `tools` | No | Comma-separated list of available tools (inherits all if omitted) |
| `model` | No | Model to use: `sonnet`, `opus`, `haiku`, or `inherit` |
| `permissionMode` | No | Permission handling: `default`, `acceptEdits`, `bypassPermissions`, `plan`, `ignore` |
| `skills` | No | Comma-separated list of skills to auto-load |

### Example: Test Runner Agent

```markdown
---
name: test-runner
description: Run tests and fix failures. Use proactively when code changes occur.
tools: read_file, grep_file, bash, edit_file
model: sonnet
---

You are a test automation expert specializing in running tests and fixing failures.

When invoked:
1. Identify the test framework and test files
2. Run the appropriate tests
3. If tests fail, analyze the failures
4. Implement minimal fixes while preserving test intent
5. Re-run tests to verify fixes

Focus on understanding the failure root cause and fixing the underlying issue.
```

## Using Subagents

### Automatic Delegation

VT Code delegates tasks to subagents when:
- The task matches a subagent's description
- The subagent is available and properly configured
- The current context suggests the subagent would be helpful

To encourage proactive usage, include phrases like:
- "Use PROACTIVELY"
- "MUST BE USED"

in your subagent's `description` field.

### Explicit Invocation

Request a specific subagent by mentioning it:

```
> Use the code-reviewer subagent to check my recent changes
> Have the debugger subagent investigate this error
> Ask the test-runner subagent to fix failing tests
```

## Best Practices

1. **Start with Claude-generated agents** - Generate initial configurations with Claude, then customize
2. **Design focused agents** - Single clear responsibility rather than trying to do everything
3. **Write detailed prompts** - Include specific instructions, examples, and constraints
4. **Limit tool access** - Only grant necessary tools for the agent's purpose
5. **Version control** - Check project agents into source control for team collaboration

## Related Documentation

- `docs/subagents/SUBAGENTS.md`
- `docs/SKILLS_GUIDE.md`
