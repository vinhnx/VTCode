# Plan: Evaluate VT Code on SWE-bench Lite

## Goal

Measure VT Code as an autonomous coding agent on real repository-level issue-fixing tasks. A task
counts as resolved only when the official verifier confirms that its patch passes the tests. Produce
a reproducible result suitable for the SWE-bench leaderboard, if maintainers accept the setup.

## Benchmark choice

Use **SWE-bench Lite** (300 instances), the lower-cost real-issue subset. HumanEval/MBPP and a
generated-diff rate are not substitutes: they do not show that VT Code can understand, edit, and
verify changes in an existing repository.

- Suite and current leaderboards: <https://www.swebench.com/>
- Evaluation/submission instructions: <https://www.swebench.com/submit.html>
- Official experiments/submissions repository: <https://github.com/swe-bench/experiments>
- Evaluation tooling: follow the current official instructions (the SWE-bench site recommends `sb-cli`; re-check its supported track and local/cloud requirements before running).
- Evaluation tooling: follow current official instructions. The site recommends `sb-cli`; re-check
  its supported track and local/cloud requirements before running.

## Phases

### 1. Confirm the evaluation contract

- Read the current Lite task, prediction, and verifier instructions. Pin the dataset/version and
  evaluation-tool version/commit.
- Check the official experiments repository for the result format, metadata, and review process for a
  new **agent/system** entry.
- Ask maintainers whether a VT Code-native harness is eligible on the desired leaderboard track and
  what runtime/network constraints apply. The Verified leaderboard defaults to bash-only
  `mini-SWE-agent`, which is not automatically comparable to VT Code. Choose a track that permits and
  identifies VT Code as the agent.
- Decide which OpenAI model to evaluate. Record its exact ID and supported settings; keep model and
  agent identity distinct in the result metadata.

### 2. Build a safe, reproducible runner

- Run the actual VT Code agent loop inside each checked-out task workspace. Do not substitute
  `mini-SWE-agent` when the goal is to measure VT Code.
- Integrate with the supported SWE-bench runner/evaluator. Record task setup, agent execution, patch
  capture, and verification as separate stages.
- Pass the OpenAI key through a secret environment variable. Never put it in task images, results,
  transcripts, or committed config. Restrict the agent to the task workspace and follow evaluator
  isolation/sandbox guidance.
- Use one attempt per task for pass@1. Set per-task time, turn, and output limits. Start with low
  concurrency until resource and cost behavior are understood.
- Record the VT Code commit/build, model/config, benchmark/task and runner versions, pilot seed and
  task IDs, per-task outcomes, elapsed time, and provider usage/cost where available.

### 3. Run a bounded pilot

- Select 5-10 deterministic Lite instances using the official runner's sampling/selection mechanism;
  record their IDs. The pilot checks integration and estimates cost; it is not a leaderboard score.
- Verify the expected repository revision, workspace confinement, patch capture, and official test
  verification for each task.
- Inspect failures by stage (setup, agent, patch capture, verifier) and check all logs/results for secrets.
- Measure actual OpenAI spend, sandbox/compute spend, runtime, timeouts, and per-task usage. Project a full-suite budget as a planning estimate only; do not assume pilot cost scales perfectly or is a hard cap. Stop before the full run if the measured spend or failure rate exceeds the chosen budget.
- Measure OpenAI and sandbox/compute spend, runtime, timeouts, and per-task usage. Treat a full-suite
  projection as an estimate, not a hard cap; pilot costs may not scale linearly. Stop before the full
  run if measured spend or failures exceed the approved budget.

### 4. Run the full Lite evaluation

- After the pilot passes and the budget is approved, run all 300 tasks with the pinned configuration
  and one attempt per task.
- Preserve predictions, outcomes, verifier output, trajectories, and per-task usage in an artifact
  bundle. Exclude secrets and unnecessary sensitive environment data.
- Re-run the official evaluator on saved predictions. Report resolved count/rate, task coverage,
  errors/timeouts, measured API and infrastructure costs, and latency. Clearly label sampled or
  nonstandard runs; do not present them as full official scores.

### 5. Publish and maintain results

- Prepare metadata and artifacts in the format required by `SWE-bench/experiments`. Open the required
  contribution or follow the maintainers' process.
- Identify VT Code, its exact commit, model/settings, runner, benchmark version, and deviations from
  the standard protocol.
- Link the accepted leaderboard entry—or a reproducible public report if declined—from
  `docs/benchmarks/README.md`. Archive raw result metadata without credentials.

## Cost controls

- No reliable total is available before measuring the pilot. API costs depend on model usage and
  agent/tool turns; sandbox/compute charges are separate.
- Use a separate OpenAI project/key. Review its usage controls and alerts, start with sequential
  batches, and check spend after the pilot and each batch. Alerts are monitoring, not proof of a hard
  spend cap.
- Bound retries for transient infrastructure errors. Do not silently retry completed or failed model
  attempts; that changes cost and pass@1 methodology.

## Success criteria

- The official evaluator produces valid pilot and full-run outcomes; patches are not hand-scored.
- Results are reproducible from pinned versions/configuration and task IDs, with actual cost and
  failures reported.
- The benchmark submission is accepted, or a public report explains why it is not an official entry.

## Existing VT Code runner caveat

`scripts/bench_swe_lite_dry.py` prompts VT Code's `ask` command to emit a diff and records whether
one was returned. It does not apply the patch to a task repository or run SWE-bench's official
verifier, so its output must not be reported as a SWE-bench resolution score.
