# Development Guide

Entry point for VT Code contributor workflows.

## Getting Started

- [Development Setup](./DEVELOPMENT_SETUP.md) - Canonical local setup and quality loop.
- [Testing Guide](./testing.md) - Test commands, structure, and benchmark coverage.
- [CI/CD](./ci-cd.md) - Pipeline behavior and verification stages.
- [Cross Compilation](./cross-compilation.md) - Multi-target build workflows.
- [Fuzzing](./fuzzing.md) - `cargo-fuzz` usage and parser hardening.

## Security and Execution

- [Process Hardening](./PROCESS_HARDENING.md) - Runtime hardening controls.
- [Execution Policy](./EXECUTION_POLICY.md) - Command policy model.
- [Command Security Model](./COMMAND_SECURITY_MODEL.md) - Command validation and threat model.

## Performance and Reliability

- [Performance Guide](./performance.md) - Profiling and optimization workflow.
- [Performance Hasher Policy](./performance-hasher-policy.md) - `rustc_hash` usage policy.
- [Async Performance Audit](./async-performance-audit.md) - Async architecture performance findings.

## Maintenance Workflows

- [Asset Synchronization](./asset-synchronization.md) - Embedded asset maintenance.
- [Changelog Generation](./CHANGELOG_GENERATION.md) - `git-cliff`-based changelog updates.
- [Desire Paths](./DESIRE_PATHS.md) - Known architecture pressure points.
- [TUI-Only Refactoring Notes](./TUI_ONLY_REFACTORING.md) - Historical refactor details.

## Navigation

- [Documentation Hub](../README.md)
- [Docs Index](../INDEX.md)
- [Contributing](../CONTRIBUTING.md)
