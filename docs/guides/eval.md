# Eval framework guide

VT Code ships a first-class evaluation framework ([`vtcode-eval`](../../crates/codegen/vtcode-eval)) that measures how
reliably an agent completes tasks: each task runs autonomously in an isolated git worktree, the claimed outcome is
verified with environment probes (the agent's own report never counts as success), and results aggregate into pass@k /
pass^k metrics split by capability and regression categories.

This is the "verifiable harness" wedge: because suites are plain JSON and reports carry run metadata, VT Code can
publish reproducible benchmark numbers for any model route — including local models — something closed competitors
structurally cannot match.

## One-command usage

```sh
# Prerequisites: [automation.full_auto] enabled and workspace trust granted.
vtcode eval --suite my-suite.json                     # markdown report to stdout
vtcode eval --suite my-suite.json --format json --output report.json
vtcode exec eval --suite my-suite.json                # long form, identical behavior
```

## What `--format json` emits

A reproducible-run envelope (`schema_version` 1):

| Field                         | Purpose                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `harness` / `harness_version` | Which VT Code produced the numbers.                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `provider` / `model`          | The model route under test.                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `suite_path` / `suite_sha256` | Pins the exact task definitions to the metrics.                                                                                                                                                                                                                                                                                                                                                                                                            |
| `report`                      | Per-task and aggregate pass@k / pass^k, cost, duration, trace summaries. Cost-efficiency fields (`cost_per_solve`, `mean_cost_per_attempt`, `mean_tokens_per_attempt`, `mean_turns_per_attempt`) appear once attempts are priced; they are `null` when any attempt has unknown pricing (unknown cost is not free) or nothing passed. The markdown report adds an `Efficiency: cost/solve $X · $Y per attempt · Z tokens · W turns` line on the same terms. |

Publishing "verified on these models with these scores" means attaching this envelope plus the suite file; the SHA-256
digest ties the numbers to the tasks. Per-attempt traces (turn/tool/error counts, latency, token usage — no transcript
content) land in `.vtcode/eval/traces/` for auditing.

## Authoring suites

Suites are JSON: tasks with a `prompt`, `category` (`capability` / `regression`), optional `timeout_secs`, and
`verify_commands`. Each verify command runs as **raw argv in the worktree** (no shell): write them as space-separated
tokens without quotes, pipes, or `&&`; every command must exit zero for the attempt to pass.

Two suite families live in `crates/codegen/vtcode-eval/evals/` (see its README):

- regression suites that pin VT Code's own documented behaviors, and
- `smoke-workspace-basics.json`, a repository-agnostic capability baseline.

For repo-specific credibility, author a suite against your own repository's hot paths and run it per model — the
harness, not the model vendor, owns the numbers.

## Related

- [docs/development/preview-budget-blocked-replan.md](../development/preview-budget-blocked-replan.md) — an example of a
  suite-derived regression doc.
- `vtcode benchmark` — the separate SWE-bench-style runner for single tasks with JSON reports.
