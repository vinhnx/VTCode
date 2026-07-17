# vtcode-eval

Agent evaluation framework for VT Code — defines eval tasks, runs them through
an agent executor, grades outcomes, and reports pass@k / pass^k metrics split by
capability and regression categories.

The crate is deliberately small and I/O-free at its core: `run_suite` orchestrates
the loop of tasks × attempts, computes per-task metrics, and assembles a report.
Everything that touches the filesystem, config, or the concrete agent runner is
pushed behind the `EvalExecutor` trait, so the harness is fully unit-testable with
an in-memory fake executor.

## Layout

| Module | Responsibility |
|---|---|
| `task` | Data model: `EvalTask`, `EvalCategory`, `RunOutcome`, `EvalRunResult` |
| `suite` | `EvalSuite` — a named set of tasks with an `attempts` count |
| `metric` | `EvalMetric` and `compute_metric` / `aggregate_metrics` / `pass_at_k` / `pass_all_k` |
| `executor` | `EvalExecutor` trait + `run_suite` pure orchestration |
| `environment` | `EnvironmentProbe` checks: `CommandProbe`, `FileExistsProbe`, `GitCleanProbe` |
| `report` | `EvalReport` / `SuiteReport` / `TaskReport` + `to_markdown` renderer |

## Concepts

- **`EvalTask`** — a prompt plus `verify_commands` and an optional `timeout_secs`.
  `category` is `Capability` or `Regression`.
- **`RunOutcome`** — `Pass`, `Fail`, or `Error` for a single task attempt.
- **`EvalMetric`** — `pass_at_k` (fraction of runs that passed) and `pass_all_k`
  (1.0 only if every run passed), plus raw `passed_runs` / `total_runs`.
- **`EvalExecutor`** — the trait boundary. Implementors own "run this task" semantics
  (drive the agent, apply environment probes, grade the result). `run_suite` only
  calls `execute_task`.

## Usage

```rust
use vtcode_eval::{EvalExecutor, run_suite, EvalSuite};

// Implement EvalExecutor to drive your agent + grade outcomes, then:
let report = run_suite(&my_executor, &suite).await?;
println!("{}", report.to_markdown());
```

## Notes

- `run_suite` performs no file I/O or trust checks; the caller owns configuration
  and the `attempts >= 1` guardrail.
- Environment verification (`EnvironmentProbe`) is a separate concern from outcome
  grading — executor implementations decide whether and how to apply probes before
  returning a `RunOutcome`.
