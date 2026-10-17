# Tool Summary Display

VT Code has two independent tool-display settings:

- `ui.tool_output_mode` controls how command and tool result bodies are truncated or spooled.
- `ui.tool_display_mode` controls the transition summaries shown before those bodies. It defaults to `compact`;
  contiguous successful command/PTY calls share a compact activity row while successful `task_tracker` updates show
  header plus the single current-task row (`» …`, bold theme `primary` accent) in compact transcript mode and header
  plus truncated tree (max 30 rows + `… N more`) in expanded mode (the full tree stays on the TODO panel body). Task
  status surfaces through text styling only: done rows strike through, italicize, and dim; blocked rows use the
  `warning` token. Each visible row is bounded to one short description (`TRACKER_ROW_DESCRIPTION_MAX_BYTES`, `…` when
  longer) so long step bodies do not wrap; the untruncated text stays in the structured checklist payload.

```toml
[ui]
tool_display_mode = "compact"
```

Compact mode keeps one concise activity row for each contiguous run of successful command/PTY calls. A single command
can show its command and hidden line count, for example `• Ran cargo check · … +12 lines`; consecutive calls collapse to
`• Ran 2 commands`. The review suffix includes the configured shortcut and a styled `click to expand` affordance. In
expanded mode, running PTY blocks show a bounded live tail; after completion, successful bodies collapse to the activity
row. Failures, non-zero exits, cancellations, warnings, stderr diagnostics, meaningful diffs, and useful artifacts
retain bounded inline context. Complete output is available in the session-local Transcript Review opened with the
configured review shortcut, including outside fullscreen, where it is ordered with user messages, assistant responses,
reasoning, and other status entries. Rich review rendering is the default; `r` switches to ANSI-free raw text. Plan
updates, non-command tools, and explicit result bodies end a command group. The review hint follows the configured
primary review binding and is omitted when that action is unbound. Successful file mutations also end a command group
and remain visible as a glanceable `• Edited path (+N -M)` (or created/deleted) row followed by the numbered diff
preview; the complete tool result remains available to the agent and Transcript Review.

Failure-like results (hard failures, timeouts, and non-zero command exits) also show a bounded `Info` diagnosis with
`Observed`, `Likely cause`, and `Next action`. It is derived from the existing bounded preview, while complete raw
output remains available in Transcript Review. Deterministic guidance is used for policy, permission, authentication,
circuit-breaker, sandbox, resource, and preflight failures; provider-native reasoning settings do not expose raw
diagnosis-provider reasoning.

When the UI collapses or bounds a tool result, every provider/model receives this exact disclosure after the tool-result
user message:

```text
Only you see that command's output — the user's terminal shows at most a few lines of it. If the user needs to read any of it, put it in your reply.
```

Anthropic wire routes whose selected provider/model capability supports it use the native
`clear_at: "next_user_message"` field and the required `mid-conversation-system-clear-at-2026-08-21` beta.
Anthropic-shaped routes without that capability promote the same text to their top-level system prompt. All remaining
provider routes receive it through their native system, history, instructions, or transcript mapping, without the
Anthropic-only `clear_at` field; those routes receive only the latest copy. VT Code keeps one typed marker per user turn
in canonical session history and never moves or deletes a copy that was already sent, so provider switching and replay
preserve the disclosure and every request stays append-only. This tells the model when it must quote or summarize output
for the user; it does not expose raw provider reasoning or replace the complete output retained by Transcript Review.

In compact mode, PTY commands keep their complete capture and grouped completion row without emitting a transient live
PTY block. Progress remains available through the active status/spinner, while warnings, failures, diffs, stderr, and
meaningful artifacts stay inline. Expanded mode preserves the bounded live tail.

Model-facing progress guidance complements these UI summaries: for non-trivial tool work, the model updates only on
findings, direction changes, or blockers, and ends with an outcome-first summary. Structured tool-call events are the
authoritative status signal. The model should summarize material findings instead of rerunning a command whose
successful body is available through Transcript Review.

The runtime mode can be changed for the current session with `Alt+T`. This action is rebindable through the existing
keybinding configuration. Use `/config` to cycle `ui.tool_display_mode` and persist the choice to `vtcode.toml`.

Explicit `expanded` mode preserves the existing per-call summary and live-output layout.

## Exec-session calls (`write_stdin` and the session readers)

Exec-session calls repeat on every poll or wait of a long command, so their transcript rows stay minimal: the session
identity plus an explicit wait deadline is the only parameter row shown (`└ Session run-2d5752f2 · wait up to 600s`), and the
generic stream label is dropped next to it so the header reads `• Wait for command session`, `• Read command session`, or
`• Send command input` (action-aware: pure waits no longer misrender as sends) instead of appending
`Use write_stdin output`. Output-token caps, yield windows, and the raw stdin payload are not rendered; the model still
receives the full arguments and result. A still-running session ends with a human hint
(`Still running — check again for more output.` / `More output available — read the next chunk.`) instead of the
model-facing `Reuse next_continue_args` jargon.

The captured stdin/stdout body is capped at 10 visible rows, taken from the tail (streaming polls keep the newest output
— head+tail excerpts are reserved for finished command previews). Overflow shows a trailing
`… +N lines · click to expand` notice instead of the command-preview share hint. The action phrase is underlined so it
is a click target: activating it opens the Tool Output Viewer on that call's recorded complete capture. A session body
is terminal text, so it renders plain in the theme's PTY body color plus `DIMMED`, matching the design-system dim tier
used by reasoning: git-diff detection and `LS_COLORS` per-line styling are skipped, because both misfire on build logs
(`PASS … .rs` picked up file-type colors). Stdin echo rows (`$ …`) sit on the same dim tier. Headers
(`• Wait for command session`, `• Read command session`, `• Send command input`, `└ Session …`) keep normal tool brightness so the call stays identifiable. The spooled branch
already bounds to six rows (three head, three tail). Complete output stays in the tool-output viewer, Transcript Review,
and the spool file referenced in the spool message; only the rendered preview is bounded.

The redundant stream label is only dropped for the generic capture label (`output`), which repeats the body already
shown below the row. Diagnostic labels (`error`, `stderr`, `stdout`, `stdio`) survive so a failed session read does not
lose its failure signal on the header.

`• Ran` headers for long commands head-truncate at a word boundary with a single trailing ellipsis, sharing
`preview_command` between the summary headline and the command line so a path is never cut in half (`…crates/…onfig/…`).

Command launches (`exec_command`, `unified_exec` action `run`, `run_pty_cmd`) are unaffected: they keep the `• Ran …`
header and the three-line head plus three-line tail command preview.

## Model-visible tool output budget

Tool-result previews copied into provider-facing history share an aggregate budget per turn (64 KiB execution, 96 KiB
planning). The existing per-result spool limit still applies; when the aggregate budget is exhausted, later results
expose only bounded metadata such as the tool name, spool path, byte count, completion state, and a short note. Complete
output remains in the internal spool and current-session tool-output viewer, so this budget affects recovery diagnostics
rather than Transcript Review.

Static, bounded reads of `.vtcode/context/tool_outputs/` using `cat`, `sed`, `tail`, or bounded `rg` pipelines are
marked `no_spool` before generic output processing. Dynamic paths, redirects, writes, in-place edits, and malformed
commands remain fail-closed; they cannot opt out of normal spooling.

## Guardrails quick reference

| Signal                          | Expected behavior                                                                                                                                                             | Where it is defined                                                                                    |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| `spool_line_count`              | Completed spool's exact physical line count, including an unterminated last line; read missing ranges within this extent and stop at EOF. Pending spools have no final count. | `tools/output_spooler.rs`, shared pipe/PTY integrity metadata                                          |
| Encoded task updates in `items` | Reject without changing progress; update a step directly or send a complete checklist with descriptions and statuses.                                                         | Shared `handlers/task_tracking.rs` validation                                                          |
| `preview_budget_exhausted`      | Trust preserved metadata, run one `&&` verifier, then synthesize; never repeat an equivalent call                                                                             | `RUNTIME_GUIDANCE_SECTION` in `crates/codegen/vtcode-core/src/prompts/runtime_guidance.rs`             |
| `turn.blocked`                  | Resumable stop with streak and counter metadata; one tool-free synthesis, then checkpoint resume                                                                              | `ThreadEvent::TurnBlocked` in `crates/common/vtcode-exec-events`, `docs/guides/agent-loop-contract.md` |
| Mid-execution replan            | Keep scopes, add falsifiers, continue the run on existing `plan.delta` events                                                                                                 | `docs/guides/planning-workflow.md`                                                                     |

The combined developer narrative lives in
[Preview Budget, Blocked Turns, and Replans](./preview-budget-blocked-replan.md). The executable regression for all
three rows is `crates/codegen/vtcode-eval/evals/preview-budget-blocked-replan.json`.
