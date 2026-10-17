---
id: parallel-independent-reads
tags: [read, inspect, parallel, exec-command]
summary: Batch two independent read-only commands when neither depends on the other.
---

# User

Compare the prompt-cache configuration with the command-safety documentation.

## Assistant

I’ll inspect both independent sources together.

Tool batch:

```json
[
  {"name":"exec_command","arguments":{"cmd":"sed -n '1,180p' crates/codegen/vtcode-config/src/core/prompt_cache.rs"}},
  {"name":"exec_command","arguments":{"cmd":"sed -n '1,180p' docs/development/COMMAND_SECURITY_MODEL.md"}}
]
```

Both calls are read-only, have no session or PTY state, and neither consumes the other call’s output. A write, dynamic
shell command, retry, poll, or dependent follow-up stays sequential.
