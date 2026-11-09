# vtcode-eval

[Root AGENTS.md](../../../AGENTS.md) | Agent evaluation framework: pass@k / pass^k metrics, capability/regression
evals, environment-based outcome verification.

## Module Groups

| Area | Modules |
| --- | --- |
| Data model | `task` — `EvalTask`, `EvalCategory`, `RunOutcome`, `EvalRunResult` |
| Suite config | `suite` — `EvalSuite` (tasks + attempts + id) |
| Metrics | `metric` — `pass@k`/`pass^k`; `test_quality` — asymmetric triage |
| Orchestration | `executor` — `EvalExecutor` + bounded `run_suite_with_options` (I/O-free) |
| Environment | `environment` — `EnvironmentProbe` and its probes |
| Reporting | `report` — `EvalReport`, `SuiteReport`, `TaskReport`, `to_markdown` |
| Trace | `trace_analyzer` — privacy-preserving JSONL aggregate summaries |

## Rules

- The manifest lists Clippy checks explicitly (API-style and panic-safety); it does not inherit workspace lints.
- `run_suite_with_options` depends only on `EvalExecutor`: no file I/O, config, or trust checks.
- `EvalSuite` lives once in `suite.rs`; do not duplicate it in `task.rs`.
- `Capability` and `Regression` are the only valid category split keys.

## Gotchas

- Serde accepts `attempts: 0`; the scheduler and CLI reject it before running.
- Probes are separate from grading: executors decide whether to apply them.
- `trace_analyzer` keeps aggregates only: never command arguments, file contents, or tool output.

Details: [maintainer notes](../../../docs/development/maintainer-notes/vtcode-eval.md) — read before changing a
subsystem.
