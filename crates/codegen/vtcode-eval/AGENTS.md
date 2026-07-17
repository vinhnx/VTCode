# vtcode-eval

[Root AGENTS.md](../AGENTS.md) | Agent evaluation framework: pass@k / pass^k metrics, capability/regression evals, environment-based outcome verification.

## Module Groups

| Area | Modules |
|---|---|
| Data model | `task` — `EvalTask`, `EvalCategory`, `RunOutcome`, `EvalRunResult` |
| Suite config | `suite` — `EvalSuite` (tasks + attempts + id) |
| Metrics | `metric` — `EvalMetric`, `compute_metric`, `aggregate_metrics`, `pass_at_k`, `pass_all_k` |
| Orchestration | `executor` — `EvalExecutor` trait + `run_suite` (pure, I/O-free) |
| Environment | `environment` — `EnvironmentProbe` + `CommandProbe`, `FileExistsProbe`, `GitCleanProbe` |
| Reporting | `report` — `EvalReport`, `SuiteReport`, `TaskReport`, `to_markdown`, `build_task_report` |

## Rules

- `lib.rs` re-exports the public facade: types from `task`/`suite`/`metric`/`report` and `executor::{EvalExecutor, run_suite}`.
- `run_suite` depends only on the `EvalExecutor` trait — no file I/O, config, or trust checks. Keep it that way; the caller owns I/O.
- The four `EvalCategory` strings (`Capability`, `Regression`) are the only valid split keys; `report` filters on `category.label()` serialization.
- `EvalSuite` is defined once in `suite.rs` and re-exported from `lib.rs`. Do not duplicate it in `task.rs`.

## Gotchas

- `attempts >= 1` is NOT enforced by serde (suite.rs test confirms `attempts: 0` deserializes). The guardrail lives in the CLI entrypoint; `run_suite` will happily loop zero times if handed a zero-attempts suite.
- `run_suite` is concurrency-free and sequential; parallelism is the caller's responsibility (the executor owns task execution semantics).
- Environment verification (`EnvironmentProbe`) is a separate concern from outcome grading — `EvalExecutor` implementations decide how/whether to apply probes before returning `RunOutcome`.
