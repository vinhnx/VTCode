# Development Guide

Entry point for VT Code contributor workflows.

## Getting Started

- [Development Setup](./DEVELOPMENT_SETUP.md) - Canonical local setup and quality loop.
- [Rust Style Guide](./rust-style-guide.md) - Google Rust guidance adoption, enforcement, exceptions, and API migrations.
- [Architecture Guide](../ARCHITECTURE.md#request-projection-and-context-budgets) - Runtime ownership, request projections,
  context budgets, and staged headless compaction.
- [Testing Guide](./testing.md) - Test commands, structure, and benchmark coverage, including the startup launch
  benchmark.
- [TUI design system](./tui-design-system.md) - Canonical list rows, keyboard copy, and group spacing in
  `vtcode-ui::design`.
- [TUI list search](./tui-list-search.md) - Shared live filter for modal and standalone pickers (labels, descriptions,
  keywords).
- [C++ Core Guidelines Adoption](./CPP_CORE_GUIDELINES_ADOPTION.md) - Policy for any C/C++ code introduced in this
  repository.
- [Deep Rust Analysis with Charon](./deep-rust-analysis.md) - Semantic Rust inspection (drops, layout, resolution) via
  Charon, and the design lessons it mirrors.
- [CI/CD](./ci-cd.md) - Pipeline behavior and verification stages.
- [Markdown Quality](./markdown-quality.md) - Maintained-file selection, pinned linting, and formatting policy.
- [Cross Compilation](./cross-compilation.md) - Multi-target build workflows.
- [Fuzzing](./fuzzing.md) - `cargo-fuzz` usage and parser hardening.

## Security and Execution

- [Process Hardening](./PROCESS_HARDENING.md) - Runtime hardening controls.
- [Execution Policy](./EXECUTION_POLICY.md) - Command policy model.
- [Command Security Model](./COMMAND_SECURITY_MODEL.md) - Command validation and threat model.
- [WebMCP bridge](./webmcp.md) - Authenticated browser editing, pairing, runtime adapters, and security boundaries.
- [VT Code Companion](./vtcode-companion.md) - Local plugin packaging, marketplace generation, install-script and CLI
  behavior, and GitHub sharing boundaries.
- [Source inspection](./source-inspection.md) - Retained source coverage, targeted refresh, shell writes, and verification.
- [Execution explanations](./execution-explanations.md) - Canonical projection, decisions, evidence, and reports.
- [Matrix orchestration](./matrix-orchestration.md) - Coordinator tasks, resource leases, final verification, and recovery.
- [Security Guide](../guides/security.md) - Process sandbox boundaries, MCP inheritance, and provider diagnostic
  redaction.
- [Sandboxing Basics Reference](./sandboxing-basics-reference.md) - Distilled notes on the Emilua sandboxing-basics
  article with the VT Code applicability mapping; consult before touching `vtcode-safety/src/sandboxing/`.
- [Runtime Guidance](./runtime-guidance.md) - Boundary between compiled universal behavior and dynamic project
  instructions.
- [Prompt and Model Evaluation](./prompt-model-evaluation.md) - Supplied-context cases, settings comparisons, and
  cost/latency measurement.
- [vtcode Binary Gotchas](./vtcode-binary-gotchas.md) - Binary startup, runloop recovery, allocator, and
  request-assembly invariants.
- [AI Tool Surface Migration](./ai-tool-surface-migration.md) - Breaking-change notes for the Codex-style default tools.

## Performance and Reliability

- [Performance Guide](./performance.md) - Profiling and optimization workflow.
- [Response Progress and Latency](./response-progress.md) - Transient phases, preparation overlap, and monotonic timings.
- [Terminal program status](./terminal-program-status.md) - Opt-in OSC 7501 activity, child records, and lifecycle.
- [Rust Performance Principles](./rust-performance-principles.md) - Rust hot-path and I/O guidance.
- [Declarative Macro Hygiene](./rust-macro-hygiene.md) - `$crate`/absolute-path qualification and follow-set rules for
  exported `macro_rules!`.
- [Rust Error Handling](./rust-error-handling-reference.md) - Type-first recovery, context that never masks the cause,
  and the cause-chain fallback used by `classify_anyhow_error`.
- [Performance Hasher Policy](./performance-hasher-policy.md) - `rustc_hash` usage policy.
- [Async Performance Audit](./async-performance-audit.md) - Async architecture performance findings.
- [Session Event Persistence](./session-persistence.md) - Canonical session events, exporter boundaries, retention, and
  verification.
- [Configuration reset and live reload](./configuration-reload.md) - Shared reset service, watcher contract, runtime
  application, and verification.
- [Diff Preview Architecture](./diff-preview.md) - Reusable diff model, bounded layout, renderer boundaries, TUI
  wrap/expand readability, and compatibility behavior.

## Model Management

- [Fast mode](./fast-mode.md) - `/fast`, service-tier compatibility, persistence, and client replacement.

- [Responses WebSocket streaming](./responses-websocket-streaming.md) - Connection lifecycle, continuation,
  cancellation, HTTP fallback, and service-tier recovery.
- [Adding Models](./ADDING_MODELS.md) - Complete workflow for adding new LLM models.
- [Model Addition Checklist](./MODEL_ADDITION_CHECKLIST.md) - Step-by-step checklist for model additions.
- [Compaction Engine](./compaction.md) - Request-only clearing, complete tool-batch retention, provider strategies, and
  local summary budgets.

## Maintenance Workflows

- [Asset Synchronization](./asset-synchronization.md) - Embedded asset maintenance.
- [AI Tool Surface Eval Report](./ai-tool-surface-eval-report.md) - Executable tool-surface cases, suite validation, and
  the baseline comparison format.
- [Changelog Generation](./CHANGELOG_GENERATION.md) - `git-cliff`-based changelog updates.
- [Desire Paths](./DESIRE_PATHS.md) - Known architecture pressure points.
- [TUI-Only Refactoring Notes](./TUI_ONLY_REFACTORING.md) - Historical refactor details.
- [Tool Summary Display](./tool-summary-display.md) - Compact and expanded tool transition summaries, configuration, and
  testing boundaries.
- [Preview Budget, Blocked Turns, and Replans](./preview-budget-blocked-replan.md) - Per-turn model-visible budget (64
  KiB execution, 96 KiB planning), `turn.blocked` handoff and resume, and mid-execution replan continuation.
- [First-party debt scan](../../scripts/first-party-debt-scan.sh) - Detect actionable debt markers while excluding
  generated and fixture content.

## Navigation

- [Documentation Hub](../README.md)
- [Docs Index](../INDEX.md)
- [Contributing](../CONTRIBUTING.md)

- [Checkpoint file storage](filesnap-checkpoints.md): filesnap storage, legacy compatibility, and restore boundaries.

- [Octen web search integration](web-search.md): credentials, bounded requests, and fetch approval separation.
