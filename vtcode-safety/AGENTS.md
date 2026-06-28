# vtcode-safety

[Root AGENTS.md](../AGENTS.md) | Command safety detection, execution policies, and sandboxing. Layer 1 crate — depends on vtcode-commons.

## Module Groups

| Area | Modules |
|---|---|
| Command Safety | `command_safety/` — dangerous command detection, shell parsing |
| Execution Policy | `exec_policy/` — policy management, approval workflows, command validation |
| Sandboxing | `sandboxing/` — sandbox policy, permissions, execution environments |

## Rules

- `exec_policy::manager` imports `command_safety::command_might_be_dangerous` and `sandboxing::SandboxPolicy` — these form a tightly coupled safety subsystem.
- Re-export facades in vtcode-core (`command_safety/mod.rs`, `exec_policy/mod.rs`, `sandboxing/mod.rs`) must stay in sync.
- The `BashParser` singleton (`once_cell::Lazy`) is safe across crates — read-after-init pattern.

## Gotchas

- `exec_policy/parser.rs` imports `vtcode_commons::fs::{parse_json_with_context, read_file_with_context}`.
- `exec_policy/command_validation.rs` imports `vtcode_commons::paths::{canonicalize_workspace, normalize_path}`.
- `sandboxing/` uses tree-sitter for Bash AST analysis — pinned to specific versions.
- `command_safety::shell_parser` must extract nested simple commands from loops/conditionals so safety checks and approval caching see loop bodies, not just top-level shell syntax.
