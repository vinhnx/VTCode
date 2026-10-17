# vtcode-safety

Command safety detection, execution policies, and sandboxing for VT Code.

## Overview

Layer 1 crate that provides the safety subsystem for command execution. Detects dangerous commands, manages execution
policies, and enforces sandboxing constraints.

## Module Groups

| Area             | Modules           | Description                                               |
| ---------------- | ----------------- | --------------------------------------------------------- |
| Command Safety   | `command_safety/` | Dangerous command detection, shell parsing                |
| Execution Policy | `exec_policy/`    | Policy management, approval workflows, command validation |
| Sandboxing       | `sandboxing/`     | Sandbox policy, permissions, execution environments       |

## Command Safety

The `command_safety` module detects potentially dangerous commands before execution:

- Shell command parsing and analysis
- Dangerous pattern detection (rm -rf, sudo, etc.)
- Command risk assessment

## Execution Policy

The `exec_policy` module manages how commands are approved and executed:

- Policy-based command validation
- Approval workflows for dangerous operations
- Command validation rules
- Integration with `command_safety::command_might_be_dangerous`

## Sandboxing

The `sandboxing` module provides execution environment constraints:

- Sandbox policy definitions
- Permission management
- Execution environment configuration
- Tree-sitter based Bash AST analysis

## Architecture

The three modules form a tightly coupled safety subsystem:

```text
exec_policy::manager
  ├── imports command_safety::command_might_be_dangerous
  └── imports sandboxing::SandboxPolicy
```

## Rules

- Re-export facades in vtcode-core (`command_safety/mod.rs`, `exec_policy/mod.rs`, `sandboxing/mod.rs`) must stay in
  sync
- The `BashParser` singleton (`once_cell::Lazy`) is safe across crates — read-after-init pattern

## Dependencies

- `vtcode-commons` — filesystem, paths, JSON parsing
- Tree-sitter — Bash AST analysis (pinned versions)

## See Also

- [Security Model](../security/SECURITY_MODEL.md) — security architecture
- [Process Hardening](../development/PROCESS_HARDENING.md) — runtime hardening controls
- [Command Security Model](../development/COMMAND_SECURITY_MODEL.md) — command security details
