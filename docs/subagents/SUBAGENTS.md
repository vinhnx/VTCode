# VT Code Subagents

Subagents are specialized AI assistants that VT Code can delegate tasks to. Each subagent operates with isolated context and can be configured with specific tools, system prompts, and model selections.

## Why Use Subagents

-   **Context isolation**: keep large exploration output out of the main conversation
-   **Parallel execution**: run multiple focused tasks at once (within concurrency limits)
-   **Specialized expertise**: tune prompts, tools, and models per task
-   **Reusability**: share project-specific agents across the team

## How Subagents Work

When VT Code spawns a subagent, it starts with a clean context. The parent agent provides the relevant background in the prompt, and the subagent returns a concise result plus an `agent_id`.

VT Code runs subagents in the foreground today; background mode is not currently supported.

## When to Use Subagents

Use subagents when you need context isolation, parallel workstreams, or a specialized workflow. Use skills for single-purpose, repeatable actions that do not need a separate context window.

## Agent Teams (MVP)

VT Code also supports experimental **agent teams** built on subagents. Teams are managed through `/team` slash commands and provide a lightweight coordination layer.

Current MVP limitations:

- In-process only (no split panes)
- Sequential execution (one teammate task at a time)
- Session-only state (no persistence/resume)
- Lead-only messaging (no direct teammate chats)

Use subagents directly when you need full control over prompts, tools, or concurrency.

### Subagent Default Model

Use `/subagent model` to open the interactive model picker and save a default
model for subagents in `vtcode.toml` (`[subagents] default_model`).

## Quick Start

### 1. Create a Subagent

Create a markdown file in `.vtcode/agents/` (project-level) or `~/.vtcode/agents/` (user-level):

```markdown
---
name: my-agent
description: Description of when to use this agent
tools: read_file, grep_file, list_files
model: inherit
---

Your system prompt here...
```

### 2. Use the Subagent

VT Code automatically delegates tasks to appropriate subagents, or you can invoke explicitly:

```
> Use the code-reviewer subagent to check my recent changes
> Have the debugger subagent investigate this error
```

### 3. Enable Subagents

Subagents are disabled by default. Enable them in `vtcode.toml`:

```toml
[subagents]
enabled = true
```

## Built-in Subagents

| Name            | Purpose                                 | Model   | Tools                                                    |
| --------------- | --------------------------------------- | ------- | -------------------------------------------------------- |
| `explore`       | Fast read-only codebase search          | haiku   | list_files, grep_file, read_file, run_pty_cmd            |
| `plan`          | Research for planning mode              | sonnet  | list_files, grep_file, read_file, run_pty_cmd            |
| `general`       | Multi-step tasks with full capabilities | sonnet  | all                                                      |
| `code-reviewer` | Code quality and security review        | inherit | read_file, grep_file, list_files, run_pty_cmd            |
| `debugger`      | Error investigation and fixes           | inherit | read_file, edit_file, run_pty_cmd, grep_file, list_files |

## Configuration

### Enablement

```toml
[subagents]
enabled = true
# max_concurrent = 3
# default_timeout_seconds = 300
# default_model = ""
# additional_agent_dirs = []
```

### File Format

```markdown
---
name: agent-name # Required: unique identifier (lowercase, hyphens)
description: When to use # Required: natural language description
tools: tool1, tool2 # Optional: comma-separated tools (inherits all if omitted)
model: sonnet # Optional: sonnet, opus, haiku, inherit, or model ID
permissionMode: default # Optional: default, acceptEdits, bypassPermissions, plan, ignore
skills: skill1, skill2 # Optional: skills to auto-load
---

System prompt goes here (markdown body)
```

### Model Selection

| Value      | Behavior                                      |
| ---------- | --------------------------------------------- |
| `inherit`  | Use the same model as the main conversation   |
| `sonnet`   | Use Sonnet-equivalent (default for subagents) |
| `opus`     | Use Opus-equivalent                           |
| `haiku`    | Use Haiku-equivalent (fast, for exploration)  |
| `model-id` | Use a specific model ID                       |

### Permission Modes

| Mode                | Behavior                       |
| ------------------- | ------------------------------ |
| `default`           | Normal permission prompts      |
| `acceptEdits`       | Auto-accept file edits         |
| `bypassPermissions` | Bypass all prompts (dangerous) |
| `plan`              | Read-only, research mode       |
| `ignore`            | Continue on permission errors  |

## File Locations

| Type     | Location             | Priority |
| -------- | -------------------- | -------- |
| Project  | `.vtcode/agents/`    | Highest  |
| CLI      | `--agents` JSON flag | High     |
| User     | `~/.vtcode/agents/`  | Low      |
| Built-in | Compiled into binary | Lowest   |

Project-level subagents take precedence over user-level when names conflict.

`additional_agent_dirs` are loaded after user-level agents and before project-level agents.

## CLI Configuration

Define subagents dynamically with `--agents`:

```bash
vtcode --agents '{
  "my-agent": {
    "description": "Custom agent",
    "prompt": "You are a specialized agent.",
    "tools": ["read_file", "write_file"],
    "model": "sonnet"
  }
}'
```

## Resumable Subagents

Subagents can be resumed to continue previous conversations:

```
> Use the code-analyzer agent to start reviewing the auth module
[Agent completes with agentId: "abc123"]

> Resume agent abc123 and now analyze authorization as well
[Agent continues with full context]
```

## Orchestrator Flow

-   When a task fits a subagent's specialty or needs parallel focus, call `spawn_subagent` with a concise task prompt plus any relevant context (files, constraints, prior attempts).
-   If a specific agent is obvious, set `subagent_type`; otherwise omit to let the registry pick the best candidate.
-   For follow-ups, include the `resume` agent_id so the same subagent continues with its preserved context.
-   After the subagent returns, relay a brief summary and the `agent_id` back to the user, and continue main-agent reasoning with the subagent's findings.

## Best Practices

1. **Focused Purpose**: Create subagents with single, clear responsibilities
2. **Detailed Prompts**: Include specific instructions, examples, and constraints
3. **Limited Tools**: Only grant tools necessary for the subagent's purpose
4. **Version Control**: Check project subagents into source control for team sharing
5. **Start with VT Code**: Generate initial subagent with `/agents create`, then customize

## API Usage

```rust
use vtcode_core::subagents::{SubagentRegistry, SubagentRunner, SpawnParams, Thoroughness};

// Load registry
let registry = SubagentRegistry::new(workspace, config).await?;

// Create runner
let runner = SubagentRunner::new(
    Arc::new(registry),
    agent_config,
    tool_registry,
    workspace,
);

// Spawn subagent
let result = runner.spawn(
    SpawnParams::new("Find all authentication code")
        .with_subagent("explore")
        .with_thoroughness(Thoroughness::VeryThorough)
).await?;

println!("Agent {} completed: {}", result.agent_id, result.output);
```

## Example Subagents

See `docs/examples/agents/` for complete examples:

-   `code-reviewer.md` - Code review specialist
-   `verifier.md` - Verification specialist for completed work
-   `test-runner.md` - Test automation expert
-   `data-scientist.md` - Data analysis expert

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Main Agent                               │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                 SubagentRegistry                        ││
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐   ││
│  │  │ explore  │ │  plan    │ │ general  │ │ custom   │   ││
│  │  │ (haiku)  │ │ (sonnet) │ │ (sonnet) │ │ (config) │   ││
│  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘   ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                 SubagentRunner                          ││
│  │  • Spawns subagent with filtered tools                  ││
│  │  • Manages isolated context                             ││
│  │  • Tracks execution in transcript                       ││
│  └─────────────────────────────────────────────────────────┘│
│                           │                                  │
│                           ▼                                  │
│  ┌─────────────────────────────────────────────────────────┐│
│  │                 spawn_subagent Tool                     ││
│  │  Parameters:                                            ││
│  │  • prompt: Task description                             ││
│  │  • subagent_type: Optional specific agent               ││
│  │  • resume: Optional agent_id for continuation           ││
│  │  Returns: SubagentResult with output + agent_id         ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘
```
