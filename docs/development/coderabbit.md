# CodeRabbit Setup

CodeRabbit reviews two surfaces for this repository, and they are configured differently:

- **Hosted pull-request reviews** — the GitHub App, configured in the web app and by `.coderabbit.yaml`.
- **Local CLI reviews** — `scripts/coderabbit-review.sh`, used in the self-verification loop before opening a PR.

`.coderabbit.yaml` is the source of truth for both. The web app only holds settings that cannot live in the file
(plan toggles, seat assignment, repository enablement).

## What is configured today

`.coderabbit.yaml` at the repository root is validated against
`https://www.coderabbit.ai/integrations/schema.v2.json`. Verify a change with:

```bash
coderabbit config --agent          # inspect: shows the active file and hash
```

Do not use `coderabbit config --agent --generate`: it fails on CodeRabbit's own schema
(`strict mode: unknown keyword: "x-undocumented"`), which is a CLI bug, not a problem with this file.

The file follows one design rule from [architectural invariants](../harness/architectural_invariants.md#enforcement):
**code review is the last line of defense, not the primary enforcement mechanism.** Nearly every VT Code rule already
has a deterministic gate (`clippy`, `rustfmt`, `check-dev.sh`, `check.sh`). Path instructions therefore tell the
reviewer what *not* to report, and reserve findings for what a machine cannot decide:

| Review-only rule | Why it needs a reviewer |
| --- | --- |
| Crate layer direction is a strict DAG | No gate parses the dependency graph for reverse imports |
| `ThreadEvent` is the single runtime event contract | A duplicate event type compiles fine |
| `Provider::name_matches` instead of string literals | A hand-rolled matcher compiles fine |
| `#[expect]` over `#[allow]` for lint debt | Both compile; only `expect` self-expires |
| Native model additions must mirror into Merge Gateway and OpenRouter | A native-only entry compiles and runs |
| Bound model-output diagnostics | An unbounded raw response is legal code |
| Env-mutating tests need `EnvGuard` + `serial` | Races pass on a quiet machine |

## Repository settings walkthrough

Open the repository settings page and set these. Section names match the CodeRabbit UI.

### Reviews > General

| Setting | Value | Reason |
| --- | --- | --- |
| Profile | **Chill** | Balanced signal; assertive produces nitpicks a large Rust workspace does not need |
| Path filters | `!**/*.snap`, `!**/fixtures/**`, `!docs/modules/vtcode_docs_map.md` | Insta snapshots and generated docs map cannot be acted on in review. `patches/**` is deliberately **not** filtered: those are maintained forks that must still compile |
| High level summary | On | Useful on contributor PRs |
| Sequence diagrams | Off | Noise for a CLI |
| Poem | Off | Noise |

### Reviews > Auto review

| Setting | Value | Reason |
| --- | --- | --- |
| Automatic review | On | |
| Drafts | Off | Draft PRs are not ready |
| Incremental review | On | |
| Ignore title keywords | `WIP`, `DO NOT MERGE` | Matches local `auto_review` |

### Reviews > Pre-merge checks

Built-in checks need Essentials or higher; custom checks need Team or higher. Keep them in `warning` mode and treat them
as advisory:

| Check | Mode |
| --- | --- |
| Title | Warning — Conventional Commits `type(scope): subject` |
| Description | Warning |
| Issue assessment | Warning |

**Do not encode a repository policy only here.** A plan downgrade silently disables it. The out-of-scope MCP provider
docs rule is enforced by `scripts/check_mcp_provider_docs.py` in `check-dev.sh` for exactly that reason.

### Anti-Slop

| Setting | Value | Reason |
| --- | --- | --- |
| Enabled | On | This is what flagged PR #806 as promotional |
| Include all authors | Off | Owners/members are exempt by default, which is the intended scope |
| Label | `slop` | Created in the repository so flagged PRs can be filtered |

Slop detection is an early-access feature. It is enabled for the `vinhnx` organization through the web UI, so
`early_access: true` is **not** set in `.coderabbit.yaml`: that flag would opt the repository into every early-access
feature, not just this one.

### Configuration source

| Setting | Value | Reason |
| --- | --- | --- |
| Use organization settings | Off | The repository file is the source of truth |
| Configuration source | Default branch | A pull request must not be able to change how it is reviewed |
| Allow configuration changes from trusted PRs | Off | Same reason: prevents a PR from weakening its own review |

## Verify (not configured)

CodeRabbit Verify (agentic QA) is **not** set up for this repository, and there is deliberately no
`.coderabbit/verify/` directory. Two facts make a committed Verify setup inert:

- Verify has no organization-wide switch and no `.coderabbit.yaml` settings. Its environment setup and startup scripts
  are pasted into the CodeRabbit web form, not read from the repository.
- CodeRabbit only reads `.coderabbit/instructions/` and `.coderabbit/pre-merge-checks/` from a `.coderabbit/`
  directory. Any other subfolder, and any `README.md`, is ignored.

VT Code is also a CLI/TUI with no HTTP server or browser UI, so the "start the app and open a URL" model Verify
assumes does not apply. If Verify is ever adopted, put the scripts in the web form and keep them out of the repository
unless something actually reads them.

## Local CLI review

`scripts/coderabbit-review.sh` wraps `coderabbit review --agent --gate <severity>`:

```bash
./scripts/coderabbit-review.sh                    # current diff, gate at major
./scripts/coderabbit-review.sh --committed --base main
./scripts/coderabbit-review.sh --gate minor --dir crates/codegen
```

Exit codes: `0` gate passed or nothing to review, `1` incomplete or unavailable, `2` blocked by a finding at or above
the threshold. The wrapper skips the call when the selected scope is empty and fails closed when the CLI is missing or
unauthenticated, so it can never report a false pass.

Run it through the gate when you want it in one command:

```bash
./scripts/check-dev.sh --review
```

It is opt-in because a review is network-bound, takes minutes, and draws on a per-plan hourly allowance. Gated reviews
always cover the full selected diff, so they cost more than an incremental rerun; a review the server reuses counts as
*incomplete*, never as a pass.

## Verifying the setup

1. `coderabbit config --agent` reports `"ok": true` and the expected `activeConfig`.
2. `coderabbit doctor` passes all checks.
3. On a real pull request, comment `@coderabbitai configuration` and confirm the resolved settings show the repository
   file as the layer, not organization defaults.
4. Confirm a `.snap` or fixture-only change is not reviewed.

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| `coderabbit config --generate` fails on `x-undocumented` | CodeRabbit CLI schema bug; use `--agent` inspect mode |
| `coderabbit review` returns an auth error while `auth status` is authenticated | Server-side session lookup; retry later. The wrapper correctly exits `1` |
| `coderabbit usage` reports `lookup_failed` | Same server-side lookup issue |
| A policy setting stops being enforced after a plan change | It lived in `pre_merge_checks`; move it to a deterministic script |
