# Full Automation

`--full-auto` lets VT Code run non-interactively on explicitly allow-listed tools. It is an execution and permission
layer on top of the active primary agent, not a primary agent of its own. Use it only when you fully trust the workspace
configuration and have reviewed the safeguards below.

`--full-auto` is intentionally separate from normal session permissions:

- Normal sessions use primary agents plus granular permission rules.
- Agent specs use `permissions.default` plus `allow`, `ask`, `auto`, and `deny` rule buckets.
- The `permissions.auto` bucket sends matching calls to classifier-backed review instead of treating them as
  unrestricted.
- `--full-auto` uses the explicit `[automation.full_auto]` allow-list as a hard gate. Tools outside the allow-list are
  denied; promptable outcomes inside the allow-list are routed through automatic permission review after explicit deny
  and policy checks instead of asking. Workflow-coordination tools (`task_tracker`, `start_planning`,
  `request_user_input`) stay available in every mode even when omitted from the allow-list.
- `--dangerously-skip-permissions` auto-approves promptable actions while still respecting explicit denies and policy
  blocks.

Primary-agent selection still works normally. If you explicitly select or configure a primary agent, including `duck`,
full-auto runs on top of that agent. If no primary agent is explicitly selected or configured, VT Code selects the
effective `auto` primary agent. If full-auto needs that defaulted `auto` agent and no effective `auto` exists, startup
fails fast.

When an approved plan enters execution, Auto is a confirmation policy choice, not an authority upgrade. Build and Auto
expose the same tools and safety gates; command/path policy, verification requirements, blocked-call fuses, budgets, and
recovery limits remain unchanged.

## Activation Checklist

1. **Update `vtcode.toml`**
   - Enable the feature: `automation.full_auto.enabled = true`.
   - Configure the tool allow-list to match your risk tolerance.
   - Keep `require_profile_ack = true` so a profile file is required.
2. **Create the acknowledgement profile**
   - Place the file referenced by `automation.full_auto.profile_path` in your workspace.
   - Document acceptable behaviour, escalation procedures, and any workspace-specific hazards.
3. **Review tool policies**
   - Full automation still honours existing tool policies; denied tools remain blocked.
   - Tools not included in the allow-list will be rejected automatically.
   - Allow-listed promptable actions use automatic permission review after deny and policy checks.
4. **Launch the agent**
   - Run `vtcode --full-auto` with any other CLI flags you need.

## Runtime Behaviour

- VT Code displays the active allow-list at session start.
- Full-auto does not grant tools outside `[automation.full_auto].allowed_tools`, except workflow-coordination tools
  (`task_tracker`, `start_planning`, `request_user_input`), which stay available in every mode.
- Explicit denies and policy blocks are honoured before full-auto review.
- Promptable allow-listed actions are reviewed automatically instead of interrupting for user input.
- Tool-loop and session tool-call limit increases are granted automatically (same per-grant increments and hard caps as
  manual approvals) instead of showing an interactive prompt. Session auto-grants share a bounded `2000`-call headroom
  per session before failing closed. Set `auto_grant_tool_limits = false` to restore the prompts.
- Informational or read-only requests (summarize, explain, compare, explore — no imperative action clause) end the run
  once answered instead of triggering another autonomous continuation provider call. Action requests remain on the
  continuation path.
- Non allow-listed tools are rejected before execution, and their attempts are logged.
- If the acknowledgement profile is missing while required, the CLI aborts before launching.

## Experimental Decisions Probe

```toml
[permissions.auto]
use_decisions_probe = false
```

Enable this default-off experiment in `/settings` → **Approvals & Security** to classify tool output with OpenAI
Decisions. It applies only to the built-in OpenAI provider using an API key and the standard
`https://api.openai.com/v1` endpoint. Ordinary interactive TUI sessions can use it with either Build or Auto selected,
without full-auto or its acknowledgement profile. ChatGPT subscription access, custom providers, gateways, other
providers, and custom endpoints send no additional probes in normal TUI sessions. Full-auto retains its existing
generation probe when Decisions is disabled or unsupported. The setting persists when you change providers and
eligibility is reevaluated using the current provider and loaded settings for each dispatch.

The probe remains advisory: `SUSPECT` queues the existing prompt-injection warning; `SAFE` queues none. Permission
approval, failure diagnosis, summaries, and completion judges are unchanged. Inactive planning, nonempty tool output,
cancellation checks, and the three-dispatch per-turn budget still gate probing. Noninteractive sessions require
full-auto. Evidence remains limited to the last
two user messages (240 characters each) and 2,400 characters of tool output, so attacks outside that window can be missed.

Eligible sessions send one text-only `tool_output_injection` choice question (`SAFE`/`SUSPECT`) to
`POST /v1/decisions` with `gpt-6-luna`. Decisions has four seconds; an inconclusive answer, refusal, HTTP error, or timeout
allows one generation fallback using `permissions.auto.probe_model` and the existing lightweight route. Both attempts
share the original eight-second deadline. After Decisions, there is no further main-model retry. Cancellation stops
the dispatch and prevents fallback. V1 uses a validated choice without a confidence threshold; two inconclusive
attempts retain the existing probe-failure behavior and execution continues.

Both attempts contribute to session usage and cost, separately from prompt-cache health. Missing usage makes the
complete cost unknown. The bounded standard-endpoint Decisions estimate uses $0.10 per million input tokens and no
output charge. Regional and long-context premiums may apply to other usage. For example, 10,000 requests of 1,000 billed
input tokens cost $1 before premiums; generation fallback adds its own cost.
See [OpenAI Decisions documentation and pricing](https://developers.openai.com/api/docs/guides/decisions#pricing-and-availability).

An eligible interactive session shows one local suggestion at an idle boundary while the option is disabled.
It sends no API request and does not enable the option. Headless sessions, planning, and active turns suppress the
suggestion. Admitted TUI probes show transient **Checking tool output...** feedback through both attempts; completion
restores the preceding status, while cancellation or exit clears activity. Parallel tool batches retain their spinner
ownership. Probe logs record session/turn identifiers, endpoint, outcome, elapsed time, and usage availability without
evidence, credentials, or response bodies.

This support is experimental. No live classification or cost comparison has been established; use the
[evaluation guide](../development/decisions-probe.md) before considering wider adoption.

## Customising The Allow-List

```toml
[automation.full_auto]
enabled = true
require_profile_ack = true
profile_path = "automation/full_auto_profile.toml"
auto_grant_tool_limits = true
allowed_tools = [
    "exec_command",
    "write_stdin",
    "apply_patch",
    "code_search",
    "task_tracker",
    "start_planning",
    "request_user_input",
]
```

Tips:

- Use the constants listed in `vtcode_core::config::constants::tools` to avoid typos.
- Include `"*"` only when the workspace is fully isolated.
- Combine with granular agent permissions if you need per-tool constraints in normal interactive sessions.
- Treat the list as a hard execution boundary for full-auto: outside the list is denied, and inside the list still
  passes deny and policy checks before automatic review.

## Propose/Verify Sub-agent

When `automation.full_auto.verify_mutations` is enabled, every call to `SubagentController::verify_proposed_change()`
spawns a fresh read-only verifier sub-agent that re-reads the affected files and approves or rejects the change before
it is committed. The verifier runs with no shared context from the proposer, preventing confirmation bias.

```toml
[automation.full_auto]
enabled = true
verify_mutations = true
```

The verifier is gated behind this flag because it roughly doubles the token cost on mutating calls. Default: `false`.

When enabled, the propose/verify cycle works as follows:

1. The proposer sub-agent makes a change (file edit, shell command, etc.).
2. The caller invokes `verify_proposed_change()` with a diff description and affected file paths.
3. A verifier sub-agent (falling back to the `explorer` agent if no `verifier` agent is defined) reads the files and
   returns `VerificationResult { approved, issues, reasoning }`.
4. If rejected, the caller can retry the mutation up to N times.

See [Loop Engineering](../loop-engineering.md) for the full design.

## Orchestrated Harness

For longer unattended builds, prefer enabling the planner/evaluator harness instead of relying on a single uninterrupted
build loop:

```toml
[agent.harness]
orchestration_mode = "plan_build_evaluate"
max_revision_rounds = 2
```

When enabled, `vtcode exec --full-auto` writes a small set of working artefacts under `.vtcode/tasks/`:

- `current_spec.md`: high-level execution spec
- `current_contract.md`: observable done criteria and verification contract
- `current_task.md`: tracker state
- `current_evaluation.md`: evaluator output after a completion attempt

This keeps long-running work resumable and makes evaluator-driven revision rounds explicit instead of relying on the
generator to judge itself.

Each revision round follows a fixed LLM-call lifecycle:

1. **Planner** expands the task into a spec, contract, feature list, and tracker.
2. **Build** executes the tracker (may issue multiple turns, including tool calls, until it signals completion).
3. **Evaluator** judges the candidate and returns a verdict.
4. On rejection, a **Replanner** rewrites the spec/contract/tracker from evaluator feedback.
5. The **Build** phase runs again (its own completion-signaling turn) before the **Evaluator** re-runs.

A round is exhausted once `max_revision_rounds` replans have been attempted; the run then writes a blocked-handoff
artifact instead of silently accepting the candidate. Because the build phase must complete (produce a
completion-signaling response) before re-evaluation, every revision round consumes at least one additional build turn.
The eval test harness uses a role-aware mock (`RoleQueuedProvider`) that matches responses to the calling sub-agent by
system prompt and returns a completion default for any build turn it did not explicitly queue, so tests declare _what
each role says_ rather than the exact wire-order of calls.

## Regression Testing

For long-running automation, use the `vtcode-eval` crate to build a regression eval suite that verifies the agent still
handles previously-passing tasks after model or harness changes.

### Running Eval Suites

Use the `vtcode exec eval` CLI command to run an eval suite:

```bash
vtcode exec eval --suite my-suite.json --output report.md
```

The suite JSON file contains an `EvalSuite` with tasks, each specifying a prompt, setup/verify commands, and category
(capability or regression). The runner executes each task through the agent with a bounded default concurrency of two,
verifies outcomes with environment probes (command exit codes), computes combinatorial pass@k and independent pass^k
metrics per task, and outputs a deterministically ordered markdown report. Known per-attempt cost is aggregated; unknown
pricing is reported separately rather than treated as free.

See [vtcode-eval](../../crates/codegen/vtcode-eval/) for the framework and metric definitions.

## Profile File Recommendations

The profile file is a simple acknowledgement document. Suggested content:

- Operator name and timestamp approving unattended execution.
- Workspace-specific limitations, such as directories that must not be modified.
- Contact or escalation details if automation encounters unexpected failures.
- Rollback procedures or monitoring steps to follow afterwards.

Keeping this file under version control provides a clear audit trail for when full automation was used and under which
guardrails.
