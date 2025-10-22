# `vtcode-bash-runner` Extraction Strategy

## Overview
`vtcode-bash-runner` currently wraps a curated set of shell commands (`cd`, `ls`, `mkdir`, `rm`, `cp`, `mv`, `grep`, `find`, `cat`, `head`, `tail`, `stat`, and arbitrary `run`) by invoking the system binaries through `std::process::Command`.【F:vtcode-core/src/bash_runner.rs†L11-L200】【F:vtcode-core/src/bash_runner.rs†L200-L312】 The goal of this extraction phase is to publish the runner as a standalone crate that offers the same ergonomic API while accounting for portability, safety, and customization needs that downstream consumers may have.

## Extraction Goals
- Preserve the simple method-oriented ergonomics (`runner.cd(..)`, `runner.ls(..)`) so adopters can integrate without rewriting call sites.
- Eliminate assumptions about POSIX availability by injecting platform-aware execution strategies and guard rails.
- Offer policy hooks for workspaces, command allowlists, and dry-run logging so the crate fits sandboxed or audited environments.
- Provide feature-gated command subsets and optional dependencies to keep the crate lean for constrained targets.

## Command Execution Abstraction
- Introduce a `CommandExecutor` trait responsible for resolving binaries, spawning processes, and reporting structured results. The default implementation can remain process-based, but downstream crates can substitute pure-Rust shims or remote execution proxies.
- Split helper logic (path resolution, argument construction, error mapping) from the execution backend so cross-platform variants can share code.
- Move workspace path canonicalization behind the shared `WorkspacePaths` contract from `vtcode-commons` once the crate depends on it.

## Cross-Platform Strategy
- Provide two built-in executors:
  - `posix` (default): uses POSIX flags currently wired in the module.
  - `powershell`: emits equivalent PowerShell commands (`Get-ChildItem`, `New-Item`, etc.) and handles Windows path normalization.
- Detect platform at runtime for the default executor, but allow explicit selection via constructor or feature flag.
- Offer a `dry-run` adapter that logs resolved commands without executing them, enabling safe previews and deterministic tests.

## Feature Flag Plan
- `posix-process` (default): pulls in `std::process` and exposes the current behavior.
- `powershell-process`: enables the Windows executor plus optional dependencies (e.g., `powershell_script`).
- `pure-rust`: swaps selected commands (`ls`, `cat`, `stat`) for standard library implementations to avoid shelling out.
- `dry-run`: re-exports the logging executor and ensures examples/tests can compile without process access.
- `serde-errors` (optional): enables serialization of error payloads for telemetry/reporting stacks.

## Safety and Policy Hooks
- Accept an optional policy object (trait) that validates incoming command requests (paths, glob patterns, recursion flags) before execution.
- Provide helpers to clamp operations to a configured workspace root and surface descriptive errors using `anyhow::Context`.
- Integrate structured telemetry events through `vtcode-commons` once the crate depends on shared error/telemetry traits.

## Testing and Examples
- Ship cross-platform snapshot tests that validate the command builders without executing processes (using the dry-run executor).
- Gate integration tests that spawn real commands behind the respective feature flags and skip when binaries are unavailable.
- Publish examples:
  - `examples/posix_basic.rs`: demonstrates directory traversal and file management on Unix.
  - `examples/windows_basic.rs`: mirrors the workflow using the PowerShell backend.
  - `examples/dry_run.rs`: shows logging output for CI environments without shell access.

## Migration Checklist
- [x] Extract the current module into a new crate with the executor trait and default implementations.
  - The crate exports `BashRunner`, policy adapters, and the process-backed executor while re-exporting them through the crate root for downstream use.【F:vtcode-bash-runner/src/lib.rs†L1-L17】【F:vtcode-bash-runner/src/executor.rs†L1-L190】
- [x] Wire policy hooks and workspace guards into the new API surface.
  - `WorkspaceGuardPolicy` enforces workspace roots and optional allowlists, while invocations carry touched paths for custom policy implementations.【F:vtcode-bash-runner/src/runner.rs†L1-L372】【F:vtcode-bash-runner/src/policy.rs†L1-L73】
- [x] Port existing CLI integrations to depend on the crate via the default feature set.
  - `vtcode-core` consumes the runner through the workspace dependencies, and the crate defaults align with the original POSIX behaviour while gating PowerShell support separately.【F:vtcode-core/Cargo.toml†L1-L120】【F:vtcode-bash-runner/Cargo.toml†L1-L40】
- [x] Author crate documentation covering configuration, feature flags, and examples.
  - The refreshed guide documents feature flags, built-in executors, and telemetry hooks alongside the dry-run example for CI validation.【F:docs/vtcode_bash_runner.md†L1-L120】
- [ ] Publish the crate with semantic versioning and cross-platform CI coverage.
