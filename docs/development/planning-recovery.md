# Planning recovery contract

Planning recovery lives in the existing planning session state. The configuration split and read-only planning policy
are unchanged. The runtime owns `researching`, `synthesizing`, `repairing`, `awaiting_input`, and `awaiting_approval`;
model claims and human diagnostic wording cannot select these outcomes.

## Admission and persistence

`plan.delta` optionally carries a complete boxed recovery checkpoint. Older events omit it. The canonical session drain
acknowledges checkpoint writes before model admission; every retry dispatch consumes the same allowance, including
provider history/continuation repair and transport fallback. A cycle allows three requests: one synthesis and two
corrections. Automatic continuations and resume never reset that count. Event cap rewrites retain the latest complete
checkpoint, so evicting old turns cannot replenish recovery. Older sessions without a recovery checkpoint retain
public progress and ask for input before another request; they cannot prove an unused allowance.

Headless checkpoints carry the task ID and a fingerprint of the full prompt and instructions. Replaying the same input
retains its allowance and saved draft. An explicit resume follow-up under the same task ID starts a correction cycle
using saved progress with research closed; a different task in the same session starts its own planning request.
Legacy checkpoints without an ID match the bounded request
text before replay, so another task cannot silently receive an unrelated approval draft.
Resumed synthesis restores the saved public evidence, draft, and validator report into the fresh conversation;
the saved request counter alone is insufficient context for a corrective request. Synthesis explicitly disables tools.

Public tool evidence and drafts are bounded and redacted. Private reasoning is classified only by presence. Draft
ranking retains the structurally strongest available plan; approval-only commentary cannot overwrite it. Requests
repair this retained draft against validator-owned defects. Artifact validation and persistence precede approval.

## Input and model choice

The runtime creates the recovery interview without requiring another successful model response. Retry and clarification
start an explicit cycle using saved evidence with research closed. Model choice opens the existing picker; cancellation
leaves the plan idle. No option is submitted automatically, and none is plan approval. Recovered plans require a
separate explicit approval even when confirmation bypass is configured. Headless sessions receive the
same labeled options in a structured diagnostic. Persistence and queue errors stop automatic work while preserving
in-memory evidence; cancellation never grants another request.

A clarification can supply missing facts directly. The separate explicit “Research missing facts” choice permits
one targeted read-only research batch for a nonempty clarification, subject to the existing tool, time, and
monetary budgets. It never restarts broad research. Retry, model switching, and diagnostic expansion keep research
closed. Existing read-only and approval boundaries remain authoritative.

This follows the blocking Plan-mode input pattern verified directly in
[`request_user_input.rs` at `e1b9c9457ceca75a99b034a5df39ac48c3998d15`](https://github.com/openai/codex/blob/e1b9c9457ceca75a99b034a5df39ac48c3998d15/codex-rs/core/src/tools/handlers/request_user_input.rs)
and the bounded-failure human intervention guidance in OpenAI's
[practical guide to agents](https://openai.com/business/guides-and-resources/a-practical-guide-to-building-ai-agents/).
The integration reuses VT Code's interview and canonical persistence APIs; no upstream code was copied.

## Verification scope

Deterministic fixtures cover the reported empty output, forbidden native tools, rejected scope review, and approval-only
repair. Provider payload tests cover streaming and non-streaming paths. Runtime tests check mixed failures, no research
after synthesis starts, retained draft repair, persisted approval, cancellation, headless input, resume, and checkpoint
admission failure. A live provider replay is supplementary; payload evidence alone does not prove why earlier responses
were empty.

See [diagnostics](diagnostics.md) for human summaries and retained agent details, and
[planning validation](planning-validation.md) for the scope-review/content-check distinction.
