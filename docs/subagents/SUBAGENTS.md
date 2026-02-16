# VT Code Subagents

Subagents are specialized AI assistants that VT Code can delegate tasks to. Each subagent operates with isolated context and can be configured with specific tools, system prompts, and model selections.

## Why Use Subagents

-   **Context isolation**: keep large exploration output out of the main conversation
-   **Parallel execution**: run multiple focused tasks at once (within concurrency limits)
-   **Specialized expertise**: tune prompts, tools, and models per task
-   **Reusability**: share project-specific agents across the team

## How Subagents Work

When VT Code spawns a subagent, it starts with a clean context. The parent agent provides the relevant background in the prompt, and the subagent returns a concise result plus an `agent_id`.

VT Code runs subagents in the foreground today; background mode is not currently supported. Spawned subagents currently execute as single-turn LLM calls and do not invoke tools yet.

### Auto-Selection Behavior

VT Code auto-selects a built-in subagent by scoring:
- explicit subagent name mentions
- keyword and phrase matches (built-in agents include curated keywords)
- overlap with the agent’s description

For ambiguous requests, explicitly set `subagent_type` or mention the agent name in the prompt.

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

## Built-in Subagents

| Name            | Purpose                                 | Model   | Tools                                                    |
| --------------- | --------------------------------------- | ------- | -------------------------------------------------------- |
| `explore`       | Fast read-only codebase search          | haiku   | list_files, grep_file, read_file, run_pty_cmd            |
| `plan`          | Research for planning mode              | sonnet  | list_files, grep_file, read_file, run_pty_cmd            |
| `general`       | Multi-step tasks with full capabilities | sonnet  | all                                                      |
| `code-reviewer` | Code quality and security review        | inherit | read_file, grep_file, list_files, run_pty_cmd            |
| `debugger`      | Error investigation and fixes           | inherit | read_file, edit_file, run_pty_cmd, grep_file, list_files |

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

1. **Focused Purpose**: Built-in subagents have single, clear responsibilities
2. **Detailed Prompts**: Use specific instructions when delegating to subagents
3. **Limited Tools**: Subagents operate with a subset of tools appropriate for their task
4. **Iterative Refinement**: Provide feedback to subagents if results are not as expected

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
│  │  │ explore  │ │  plan    │ │ general  │ │ reviewer │   ││
│  │  │ (haiku)  │ │ (sonnet) │ │ (sonnet) │ │ (sonnet) │   ││
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
