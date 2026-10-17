# VT Code eval suites

Eval suites are JSON files describing autonomous tasks that an agent runs in an isolated git worktree, verified by
environment probes, and scored with pass@k / pass^k metrics. VT Code ships two kinds of suites here:

- **Regression suites** target VT Code's own behavior (e.g. `preview-budget-blocked-replan.json`) and run against the
  vtcode repository.
- **Baseline capability suites** are repository-agnostic (e.g. `smoke-workspace-basics.json`) and run against any
  workspace, which makes results comparable across projects and models.

## Running

```sh
# Prerequisites: full-auto enabled ([automation.full_auto]) and workspace trust.
vtcode eval --suite crates/codegen/vtcode-eval/evals/smoke-workspace-basics.json

# Reproducible, publishable result (JSON envelope with metrics + run metadata)
vtcode eval --suite my-suite.json --format json --output report.json
```

`vtcode exec eval` is the long form of the same command. Reports render as markdown by default.

## Suite format

```json
{
  "id": "suite-id",
  "name": "Human suite name",
  "attempts": 3,
  "tasks": [
    {
      "id": "task-id",
      "name": "Human task name",
      "category": "capability",
      "prompt": "Instruction given to the agent.",
      "timeout_secs": 300,
      "verify_commands": ["grep -q expected output.txt"]
    }
  ]
}
```

- `category` is `capability` or `regression`; metrics are aggregated per category as well as overall.
- `verify_commands` run in the worktree after the agent claims success. All commands must succeed for the attempt to
  pass — a pass is never credited from the agent's own claims alone.

## Publishing reproducible results

`--format json` emits an envelope that bundles the metrics with everything needed to re-verify the run later:

- harness name and version
- provider and model route used for the run
- suite path and the SHA-256 of the exact suite file

When publishing benchmark numbers ("verified on these models with these scores"), attach the JSON envelope and the suite
file; the digest pins the task definitions to the reported metrics. Per-attempt traces (tool/error counts, latency,
token usage — no transcript content) are written to `.vtcode/eval/traces/` for auditability.
