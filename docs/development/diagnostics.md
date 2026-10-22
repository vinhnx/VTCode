# Runtime diagnostics

Operational failures use `Diagnostic`, the canonical payload carried by an existing
`ThreadEvent` reasoning item with stage `diagnosis`. This stage contains operational
evidence, never private model reasoning. Event schema 0.19.0 introduced an optional boxed
`diagnostic`; old sessions without it remain readable. ATIF exports preserve the
complete payload in step metadata.

Event schema 0.20.0 adds optional `tool_call_id` fields to permission requested/resolved
events so concurrent prompts can be correlated with their tool invocation. Older event
payloads without the fields remain readable.

Tool failure diagnostics include the provider `tool_call_id` in their incident identity
when available, so concurrent identical failures remain separate. Calls without an ID
retain the legacy identity shape for replay compatibility.

Web-fetch policy diagnostics distinguish a tool-level denial from a URL or domain safety
denial. Tool-level failures point to the active `web_fetch` permission policy; URL-level
failures point to the allowed-domain and network-safety settings. Both retain the
fail-closed instruction and never suggest bypassing safeguards.

| Field | Contract |
| --- | --- |
| `severity` | `info`, `warning`, or `error`; retains severity colors |
| `code` | Stable machine-readable category, for example `planning.awaiting_input` |
| `summary` | Concise human message, always visible |
| `agent_details` | Operational evidence, defect report, attempts, and recovery instructions |
| `correlation_id` | Incident identity; repeated updates replace its transcript source |
| `actions` | Available actions with stable identifiers and human labels; display never submits them |

Call `vtcode_commons::diagnostics::redact_diagnostic` before emitting or persisting
the event. `AnsiRenderer::diagnostic` applies redaction again before calling its canonical
persistence callback and before display. Callback errors propagate after the
summary is shown with a redacted persistence-failure detail;
the runtime installs this callback once the thread event sink exists.
Sanitize all fields, including action labels and correlation identity. Do not put
credentials or private reasoning into details in the first place.

The transcript retains one source per incident. The summary and available actions
remain visible while details are collapsed. Click its Details control or press F8 to toggle the latest diagnostic in place.
F8 owns the latest incident that carries details; an incident whose payload the
transcript does not disclose is skipped rather than retargeting an older one.
Summary and evidence clicks preserve text selection and link interaction. Expansion affects presentation only;
it emits no input, changes no model history, and grants no plan approval.
Transcript Review always retains the complete details, including when the main
transcript is collapsed. Resume replays the latest canonical payload per incident
through `restore_diagnostic`, without emitting duplicate events; Local Agents
also retains operational details rather than labeling diagnostics as thinking.
The existing Markdown renderer handles code, paths, plan
wrapper removal, wrapping, and resizing. Headless text includes the summary,
available actions, and a labeled `Details:` section.

## Transcript presentation

| Element            | Presentation                                                                                                             |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| Summary            | Incident severity color; the loudest element on the row.                                                                 |
| Disclosure control | Muted chrome: the theme foreground receded toward the background, never the severity color, never a faint/dim attribute. |
| Fenced evidence    | The code surface: syntax highlighted, receded toward the background, with a neutral line-number gutter.                  |
| Prose evidence     | The incident severity color, as prose.                                                                                   |

Machine-readable payloads (a JSON status object, for example) are noise in an
informational notice: the transcript keeps the incident summary, discloses no
payload, and offers no disclosure control for it; Transcript Review retains the
complete payload and renders it on the code surface. A warning or an error is
actionable, so its payload stays disclosed in the transcript behind the control.
Prose evidence is always disclosed in place.

Evidence code keeps the syntax palette that matches the terminal theme. The
`syntax_highlighting.enabled` and `syntax_highlighting.enabled_languages`
settings govern it (an explicit opt-out disables evidence highlighting), while
`syntax_highlighting.theme` governs highlighted tool output.

Legacy `AnsiRenderer` Error, ToolError, and Warning output passes through the
shared adapter. Direct inline-handle Error/Warning appends and pasted/batched
messages also pass through a session-local observer shared by handle clones.
Bootstrap installs the same canonical callback on the renderer and handle;
renderer presentation and resume replay bypass the observer to avoid re-emitting.
Void append APIs keep summaries visible and log sanitized persistence failures;
fallible renderer APIs additionally return the error. The first line supplies the human summary; remaining text stays
available as details. Long first lines use a concise generic summary and retain the full original message in details. Error
summaries remain visible and recorded regardless of the old debug diagnostics
switch. Ordinary Info output remains progress/help text. Early startup stderr
failures use the same redaction and summary/details adapter before a thread sink
exists; they are not falsely represented as persisted thread events.

The machine-readable [producer audit](diagnostics/producer-audit.json) records each
production severity reference and terminal output producer, its adapter or
progress/help classification, and its source location. Regenerate after moving or
adding producers:

```sh
python3 scripts/audit_diagnostic_producers.py
python3 scripts/audit_diagnostic_producers.py --check
```

Prefer explicit stable incident codes and identities for new diagnostics. The
legacy adapter assigns a fresh identity to each emission so repeated command
failures remain visible at their current transcript position. Explicit incidents
update attempt counts without appending duplicate warnings; use the explicit
payload and a retained correlation ID for retries with changing details.

## Producer review

The inventory records every shipped-runtime severity reference and terminal
producer. Build scripts, release tooling, examples, tests, and doc examples are
outside the runtime audit. The domain review uses these boundaries:

| Domain | Diagnostic transition | Ordinary progress/help |
| --- | --- | --- |
| Startup and early errors | Shared sanitized stderr adapter before a thread exists | Onboarding prompts, setup instructions, terminal recovery hints |
| Providers and retries | Explicit `provider.retrying`, `provider.retry_succeeded`, and `provider.retry_stopped`, correlated by turn and step; billing advisory has its own incident | Retry spinner and backoff progress |
| Tools and execution | Error/Warning renderer adapter plus canonical sink | Tool output, command/status summaries, result counts |
| Policy and permissions | Error/Warning renderer adapter; approval remains separate | Permission choices, policy explanations and help |
| Planning | Explicit planning incident codes and retained-state evidence | Research progress and valid plan content |
| Verification | Error/Warning renderer adapter; saved verifier evidence remains authoritative | Verifier queue/admission progress and status summaries |
| MCP | Explicit catalog failure/ready transitions retain phase and redacted error under one incident identity | `/mcp` inspection output and connection instructions |
| Persistence and session recovery | Canonical renderer callback returns failure after showing the summary; failed persistence is never silently reported as saved | Resume/reload notices and historical transcript content |
| UI and rendering | Severity references classify presentation styles; canonical command owns payload | Colors, borders, rendering tests and progress counters |

Recent identical incident states are persisted once within a bounded renderer
cache. Explicit retry and MCP payloads update the same transcript source as their
state changes. This does not suppress the human summary when persistence fails.

The audit reviews failure-like Info contexts separately from severity/style
references. `INFO_CONTEXT_REVIEWS` records the reason those remaining lines are
progress, requested data, or recovery instructions; a new failure-like context
without a review stops audit generation. `STATUS_RESULT_REVIEWS` explicitly
classifies auth status, validation/audit reports, policy listings, migration
counts, and historical metrics as requested results rather than new incidents.
Hook warnings, interactive setup fallbacks, provider/tool failures, and failed
queue admission retain warning/error severity. Headless runner incidents emit
the same complete payload through `ExecEventRecorder::diagnostic`; their ordinary
progress printer redacts before display. Human exec output renders updated
canonical incidents with severity labels and full labeled details.
