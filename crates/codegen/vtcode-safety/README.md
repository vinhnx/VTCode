# vtcode-safety

Command safety detection, execution policies, and sandboxing for VT Code.
Extracted from `vtcode-core` to isolate the safety subsystem into an
independently testable crate.

<!-- cargo-rdme start -->

Command safety detection, execution policies, and sandboxing for VT Code.

This crate provides the safety subsystem extracted from `vtcode-core`:

- **command_safety**: Granular command safety evaluation based on subcommands and options
- **exec_policy**: Execution authorization policies and approval requirements
- **sandboxing**: Sandbox policies and execution environment transformations

<!-- cargo-rdme end -->

## Modules

| Module | Purpose |
|---|---|
| `command_safety` | Dangerous command detection, shell parsing, safety evaluation |
| `exec_policy` | Execution policy management, approval workflows, command validation |
| `sandboxing` | Sandbox policy, permissions, execution environments |

## Public entrypoints

### command_safety

- `command_might_be_dangerous()` – check if a command is potentially harmful
- `shell_string_might_be_dangerous()` – check shell string safety
- `UnifiedCommandEvaluator` – unified safety evaluation
- `validate_command_safety()` – validate command against safety rules

### exec_policy

- `ExecPolicyManager` – policy lifecycle management
- `ExecApprovalRequirement` – approval requirement classification
- `ExecPolicyAmendment` – runtime policy amendments
- `PolicyParser` – parse policy configuration files

### sandboxing

- `SandboxManager` – sandbox lifecycle management
- `SandboxPolicy` – sandbox configuration
- `SandboxPermissions` – permission set for sandboxed execution
- `SandboxType` – sandbox technology selection

## Features

This crate has no feature flags. All functionality is always available.

## Usage

```rust
use vtcode_safety::command_safety::command_might_be_dangerous;

assert!(command_might_be_dangerous("rm -rf /"));
assert!(!command_might_be_dangerous("ls -la"));
```

## API reference

<https://docs.rs/vtcode-safety>
