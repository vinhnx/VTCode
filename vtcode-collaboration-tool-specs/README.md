# vtcode-collaboration-tool-specs

Passive JSON schemas for VT Code collaboration and human-in-the-loop (HITL) tools.

Each public function returns a `serde_json::Value` describing the parameter schema
for a single tool, ready to embed in tool definitions consumed by `vtcode-tools`.

## Usage

```rust
use vtcode_collaboration_tool_specs::spawn_agent_parameters;

let schema = spawn_agent_parameters();
assert_eq!(schema["type"], "object");
```

## API Reference

| Function | Description |
|---|---|
| `spawn_agent_parameters()` | Schema for spawning a subagent with an optional model override and background flag. |
| `send_input_parameters()` | Schema for sending follow-up input to a delegated child agent. |
| `wait_agent_parameters()` | Schema for blocking until one or more child agents complete. |
| `resume_agent_parameters()` | Schema for resuming a paused child agent. |
| `close_agent_parameters()` | Schema for closing a child agent. |
| `request_user_input_parameters()` | Schema for the HITL tool that prompts the user with 1–3 questions. |
| `request_user_input_description()` | Static description string for the HITL tool. |

## Dependencies

- `serde_json`
