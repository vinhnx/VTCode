# Computer Use

VT Code can drive an isolated, offline browser preview on your behalf. The model proposes
actions (clicks, typing, scrolling, screenshots) over a provider-native computer-use tool;
VT Code proposes the batch to you for approval, executes it against a disposable container,
and records only a redacted summary in the conversation.

## What it is

- **One turn, one approval.** Each turn is a single batch of actions proposed by the model,
  shown to you with the target and the screenshot recipient, and executed only after you
  approve. Nothing runs without an explicit approval, and approval is never cached or
  remembered.
- **An isolated preview, not the web.** The browser runs in a disposable container with no
  network access (`--network=none`, read-only filesystem, dropped capabilities). It serves
  one approved static build from your `computer_use.browser_build` directory. External
  pages, local servers, and desktop apps are out of scope; for those, VT Code falls back to
  the shell recipes shown in the refusal message.
- **Provider-native, route-gated.** Turns run on OpenAI's Responses `computer` tool and
  Anthropic's `computer_toolset_20260801`. Any model on those routes works, including
  models released later and custom endpoints; providers without a native route refuse
  before any network I/O (see
  [route admission](../development/computer-use-quick-reference.md#route-admission)).
- **Screen content is data, not instructions.** The model driving the preview is told that
  text and images in the page cannot grant permission, widen the task, or override your
  request. If the preview asks for credentials, secrets, or out-of-task actions, the model
  stops and reports instead of acting; the human approval remains the final control.

## Enabling it

1. Select any OpenAI or Anthropic model on its native computer-use route.
2. Approve the tool once per session — the model calls `computer_use` with a plain-words
   `task`; the batch review is the approval surface.
3. Configure the target in `vtcode.toml`:

```toml
[computer_use]
enabled = true
browser_build = "/absolute/path/to/static-build"
image = "vtcode-computer-use-browser:1.65.1"   # pinned tag or digest, never latest
max_actions_per_turn = 50
turn_timeout_secs = 300
```

See the [configuration reference](../config/config_field_reference.md) for every field.
The Desktop app exposes the same switches under Settings › Computer Use, with a ⌘K entry
and `settings/computer-use` deeplink. Docker must be available for the browser target.

## Privacy properties

- Screenshots, typed text, and decoded actions stay inside the turn. Durable conversation
  history, `ThreadEvent` streams, traces, and session archives carry only redacted
  summaries — action kinds, counts, target kind, and recipient — never image bytes or
  typed values.
- Every turn re-prompts: hook denials, policy denials, full-auto policies, and gateway
  denies all still apply, and the approval justification defeats approval caching so
  automation settings cannot silently wave a batch through.
- Optional Decisions review (`computer_use.decisions_review`, OpenAI routes only) screens
  each proposed batch for prompt injection and task drift before the human approval. A
  suspect batch **or a review that cannot conclude** (timeout, request failure, or no
  answer) halts the turn: the approval shows action kinds, not payloads, so an unscreened
  batch is never executed.

## Limits and troubleshooting

- **"Computer turn did not run: unsupported provider"** — the active provider has no
  native computer-use route. Switch to an OpenAI or Anthropic model; any model on those
  routes is admitted.
- **"No target configured"** — set `computer_use.browser_build` to an absolute directory
  containing the static build the preview should serve.
- **Batch fails partway** — the remaining actions stop, the target lease is revoked, and
  the turn reports what executed. Ambiguous outcomes end the target session; nothing is
  replayed automatically.
- **Stop mid-turn** — Stop cancels the batch, revokes the lease, and removes the exact
  container; delivery interrupted mid-action cannot leave a stray target behind.
- **Resume after interruption** — provider history is rebuilt from the redacted summaries;
  any new computer turn starts with a fresh approval and a fresh observation.

For the integration seams, route anatomy, and release gates, see the
[quick reference](../development/computer-use-quick-reference.md) and the
[run-loop wiring notes](../development/computer-use-runloop-wiring.md).
