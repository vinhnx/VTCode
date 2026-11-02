# VT Code Companion private pilot

For installation, host support, examples, and troubleshooting, start with the
[user guide](../user-guide/vtcode-companion.md). The packager's `--marketplace`
option generates a fresh machine-specific local catalog using the same validated
components as ZIP export; it never installs or changes trust. The optional
`apps/webmcp/companion/` information page ships alongside the browser editor.
Public submission remains deferred.

## Local distribution milestone (2026-10-17)

The repository README and user guide now describe Companion as an experimental
local integration. The Vite build includes `companion/index.html` beside the
unchanged editor route, with the supplied branding banner and links to GitHub
documentation and Issues. No site deployment or registry submission was run.

Validation for this increment:

- Python packaging/marketplace suite: **13 passed**. New cases check catalog-relative
  plugin resolution, exact fixed launch arguments, paths containing spaces,
  preservation of an existing catalog, and rejection before writing invalid input.
- Ruff lint/format and scoped Markdown/whitespace checks passed.
- Browser TypeScript checking, **67 tests**, and production build passed. Vite
  reported its large editor chunk warning. Both built pages and their referenced
  local assets returned HTTP 200. Visual inspection remains unverified because
  this session had no available browser surface.
- Generated a fresh local marketplace and invoked its launcher directly: MCP
  initialization, all five tools, workspace README fetch, read-only planning,
  and EOF cleanup passed. Task `1cbace4c-2806-4b70-8293-532cd72075c9` completed
  with a 1,616-byte final response, `evidence_truncated=false`, no recorded
  changes, and `verification=unverified` (no verifier requested). The README
  content hash remained unchanged; the service exited zero on disconnect.
- This directly exercises the generated connection, not installation or tool
  attachment inside a fresh Codex/ChatGPT host. No installed cache, host settings,
  trust, or credentials were changed. No Rust source changed in this increment;
  Rust runtime suites were not rerun.

The local probe report is stored at
`target/vtcode-companion-local-setup-validation.json`. Existing implementation
evaluation remains unresolved. A later ChatGPT Desktop Local Work planning task
was reported successful; full review coverage and implementation remain unverified.

## GitHub sharing and installation paths

The repository includes a GitHub marketplace catalog and portable MCP
configuration. This repository source is the shareable package; recipients
install it from GitHub and configure their own VT Code binary and workspace.
Run `vtcode --workspace <path> companion setup --binary <path>`
to persist the exact executable and one canonical workspace in VT Code's
per-user config. The launcher reads those fixed paths; MCP arguments and host
working directory cannot override them. The config follows the platform's
existing VT Code config directory and is written using its private atomic-file
helper. Direct GitHub installation steps are in the
[user guide](../user-guide/vtcode-companion.md). Host-side GitHub installation
and plugin process startup still require fresh Codex and ChatGPT Desktop checks.
Codex marketplace names are unique; if a same-named marketplace points at an
older source or ref, remove that entry before adding the GitHub source. Do not
run the plugin-add command after a failed marketplace-add command, since Codex
may install from the already-registered source instead.
The `0.179.2` release tag includes the Companion source and CLI setup command,
but it predates the later merged task-failure fix. Use the repository's `main`
ref to test that fix privately. After publishing a VT Code release that contains
the fix, update installation instructions to pin that release tag. Keep the
plugin source and VT Code binary on compatible revisions.

Do not create a ZIP to share this local-only integration. The packager's local
`--local --marketplace` and `--local --output` modes generate per-user packages
containing the creator's absolute executable and workspace paths. Keep those
generated artifacts local and untracked. Share the GitHub repository
marketplace instead; each recipient must build or install a compatible VT Code
binary, then run `companion setup` for their own binary and workspace.

The repository's `scripts/install.sh` installs the latest published VT Code
release; it does not install the Companion plugin. Until a release includes
`webmcp stdio --companion` and `companion setup`, build the executable from the
same source checkout as the plugin package. `scripts/setup.sh` is a contributor environment script,
not a runtime installer. It updates Rust, installs development components and
nextest, and runs a locked Cargo check. Its test guidance consistently uses
nextest, as required by the repository testing workflow.

VT Code Companion exposes coding tasks to a local MCP client while execution
remains on the user's machine. The local plugin uses stdio, opens no listener,
and needs no tunnel, domain, or OAuth account. Public submission is deferred.
The optional HTTP transport remains available for a separately configured deployment.

## Local connection

Use a rebuilt VT Code binary. First configure the selected workspace through VT
Code, explicitly persist full-auto workspace trust, and enable
`automation.full_auto.enabled` with the intended tool allowlist. Companion never
grants trust or disables permission checks. Each child sets
`VTCODE_TRUST_WORKSPACE=deny`: an existing persisted trust record still works,
but environment and dotenv values cannot grant a new one.

The MCP client launches this command with stdin/stdout pipes:

```sh
/absolute/path/to/vtcode --workspace /absolute/path/to/workspace --quiet \
  webmcp stdio --companion
```

Stdout contains only MCP messages. Closing the input pipe ends the service and
cancels/reaps owned tasks. Without `--companion`, stdio exposes only `search` and
`fetch`. Stdio access belongs to the local process owner; coding tasks still use
the same persisted trust, allowlists, explicit denies, and exec sandbox checks.
Tool calls cannot select another workspace. It is intended for local desktop/CLI
hosts; web and mobile clients cannot launch this process.

Build the binary and create a machine-specific local ZIP:

```sh
cargo build --locked --bin vtcode
python3 scripts/package-vtcode-companion.py --local \
  --binary "$PWD/target/debug/vtcode" --workspace "$PWD" \
  --output "$PWD/target/vtcode-companion-local-0.1.1.zip"
```

The archive contains `plugin.json`, `mcp.json`, the workflow skill, a small
Python 3 launcher, and the referenced square icon and branding banner. Portable
manifests require a command name and a package-relative
working directory, so the launcher replaces itself with the exact VT Code binary
using `execv`. Its configuration records canonical absolute executable and
workspace paths as separate arguments, with no shell expansion. It contains no
provider credentials, proxy tokens, or binary.
Rebuild it for another machine/workspace; do not upload this local archive to the
public directory. The packager validates paths and archive contents; actual
runtime discovery must be tested separately. Import/install it using the local
host's supported plugin flow. Packaging does not install or enable the plugin.

### Branding update 0.1.1

The display name is **VT Code Companion**; `vtcode-companion` remains the package
identifier and `vtcode` denotes the CLI. Vinh Nguyen is the creator-selected
author, using the public `vinhnx` profile; Platform identity verification remains
separate. The creator confirmed free/open-source operation without plugin
payments and availability in all supported countries.

The logo and composer use an unchanged copy of
`resources/icons/vtcode-app-dynamic-512.png` (512×512, 4,730 bytes) in both appearances.
The gallery uses the supplied `resources/logo/vtcode_5.png` CLI branding banner
(1774×887, 8,043 bytes), not a runtime screenshot. The manifest includes refined
descriptions, three starter prompts, release notes, and five positive plus three
negative draft review cases. Legal URLs, a demo link, and verified organization
eligibility remain absent rather than fabricated.

The local artifact `target/vtcode-companion-local-0.1.1.zip` contains exactly six
files: the four runtime/skill components above plus `assets/icon.png` and
`assets/banner.png`. SHA-256:
`8158cf74c6187392cc9238a47c831a12f135b4748ed5e039ccb5ae910a92882b`.
Archive inspection checked PNG source-byte equality, dimensions, referenced
files, metadata/prompt limits, review case fields/counts, and credential markers.
The packaging suite passed 11 tests, including missing/escaping/symlinked assets
and nonsquare logo rejection. Ruff lint/format, Markdown lint, and scoped
whitespace checks passed. No Rust runtime changed or runtime suites were rerun
for this branding increment; previous runtime validation is recorded below.

The prepared local marketplace source was refreshed to 0.1.1. Reinstall/update
through the host's plugin flow and start a new session to load its new metadata;
the installed cache was not edited. This remains a private machine-specific ZIP,
not a public installer or a registry submission. See
[current submission readiness](vtcode-companion-submission.md#can-this-be-submitted-now).

## ChatGPT Desktop: tool attachment and partial planning evidence

Use a **local Work conversation** with VT Code Companion enabled. Loading the
skill or seeing a connected badge does not prove that the MCP tool definitions
reached that conversation. The workflow skill first uses host tool discovery,
when available, because tools can be deferred and names can carry server prefixes.
It must not diagnose a disabled server solely from missing model-visible tools.

1. In Settings > MCP servers, or `/mcp`, inspect the discovered tools. Expect
   `search`, `fetch`, `start_task`, `get_task`, and `cancel_task`.
2. If only `search` and `fetch` appear, check the launch command: it needs
   `webmcp stdio --companion`, with the fixed workspace configured by the host.
3. If all five appear but the model cannot call them, restart the MCP connection
   and start a new local Work conversation with Companion enabled. Ask it to
   discover the Companion task tools before reporting them unavailable. Capture
   the discovery result or exact connection error if that still fails.
4. Refresh/reinstall an updated plugin from its local marketplace source using
   the host's supported flow. Do not edit its installed cache. A skill update
   cannot repair a host that never exposes the connected MCP tools.

The [official MCP guide](https://learn.chatgpt.com/docs/extend/mcp) documents local
stdio setup and restarting connections. OpenAI's
[tool-search guide](https://developers.openai.com/api/docs/guides/tools-tool-search)
documents deferred MCP/plugin tools; that is a discovery possibility, not proof
of the failure's cause in a particular ChatGPT thread. Local stdio itself needs
no tunnel or public submission. A direct MCP protocol check verifies the launch
configuration, not model-side discovery inside ChatGPT Desktop. The official
shared-configuration guidance is scoped to the same Codex host; it must not be
treated as proof that a particular Work conversation received the tools.

An earlier follow-up Work response reported **neither Companion tools nor tool
search**, and fetched the public README through GitHub instead. That was not a
Companion call or access to the local dirty worktree. A later user-reported
ChatGPT Desktop Local Work task completed a Companion read-only plan for
`crates/codegen/vtcode-webmcp/src/remote_mcp.rs` and the loop detector changes:

- Task: `f9f84f19-349d-4eef-8c1d-1f5d91d98c4c`; no changes recorded.
- Reported inspection: `remote_mcp.rs`, loop detector, its runner caller and
  harness tests, WebMCP filesystem/library files, WebMCP manifest, `src/cli/webmcp.rs`
  wiring, plus working-tree status and Git history.
- Truncation: task metadata said evidence was not truncated. The report still
  described partial review coverage: some diff/test tails and
  `src/cli/companion.rs` were not fully inspected, and filesystem-helper callers
  were not enumerated.
- Verification: none; correctly reported as unverified for a read-only plan.

This confirms Companion tool attachment and one planning run in ChatGPT Desktop.
It does not establish a complete review, a passing test run, or implementation
reliability. Current status: Codex/local-protocol planning is verified; one
ChatGPT Desktop planning task is user-reported and partial; implementation and
the full cross-client workflow remain unverified. Desktop UI inspection from
this session remains unavailable, so host details rely on the user's report.

On 2026-10-17, a reported Local/connected ChatGPT thread could load the skill but
reported missing tools. The installed launch configuration independently passed
initialization, all five discovered tools, a harmless `README.md` fetch, and EOF
shutdown (exit zero). Desktop UI inspection was blocked, so the host-side cause
is unverified. The skill now attempts available tool discovery and avoids
inferring server failure from absent model-visible tools. Ten package tests,
Markdown lint, and scoped whitespace checks passed; no Rust code changed.

The updated private artifact is
`target/vtcode-companion-local-discovery-0.1.0.zip`, SHA-256
`f38ca65fddc58cef8ad571f2a965f9ab76e3c72791d8e970211ddb8f6635b13e`.
Its four components were reopened and checked, and the skill matches source.
The prepared local marketplace source was refreshed; the installed cache was
not edited and no plugin was installed or submitted by this diagnostic.
The follow-up provenance/host-blocker guidance is packaged in
`target/vtcode-companion-local-host-diagnostic-0.1.0.zip` (SHA-256
`c5a92e9ab3767b814e89587d1de09dc9276031042143ad76f78d0e01b89803dd`).
Its four components and exact skill contents were checked; the prepared
marketplace source was refreshed. This changes diagnostics only and does not
establish a working ChatGPT Work connection.

## Optional remote transport (deferred)

Configure the existing remote MCP transport and start it with an additional opt-in:

```sh
vtcode webmcp serve --mcp --companion \
  --allowed-root /absolute/path/to/workspace \
  --mcp-public-url https://YOUR-MCP-HOST/sse/ \
  --mcp-authorization-server https://YOUR-AUTHORIZATION-SERVER \
  --mcp-proxy-token-env VTCODE_WEBMCP_MCP_PROXY_TOKEN
```

The uppercase hosts are configuration placeholders, not deployed services.
Supply the internal proxy token through a secure environment. Never put it in
the command line, plugin manifest, repository, or task prompt. The listener binds
only to loopback. Omitting `--companion` preserves the two read-only tools.
Companion requires `--mcp` explicitly even if remote MCP is enabled in config.

## Task contract

Headless planning completes on a nonempty, tool-free terminal response, using
the normal planning completion gate. The canonical completion result retains
the model's plan instead of replacing it with the operational task summary.
Completion does not imply implementation approval or successful verification.

Shell loop detection compares normalized actions rather than treating every
`exec_command` use as the same action. Distinct inspection ranges can make
progress; identical reads, repeated action sequences, explicit operator limits,
navigation budgets, and harness turn/tool/wall-clock limits still apply. A hard
loop stop is recorded explicitly, rather than inferred from aggregate tool-name
usage when a soft warning is emitted.

| Tool | Input | Outcome |
| --- | --- | --- |
| `start_task` | `prompt`, optional `mode` (defaults to `plan`), optional `plan_task_id` | Starts one task and returns its opaque ID and running snapshot |
| `get_task` | `task_id` | Returns lifecycle, public response, recorded changes/diff, and verification |
| `cancel_task` | `task_id` | Requests cancellation; poll `get_task` until cleanup is terminal |

Planning runs `vtcode exec --json --dry-run`; implementation runs
`vtcode exec --json`. The binary fixes the executable, workspace, arguments, and
environment policy. Untrusted prompts are delivered through stdin and never parsed as arguments; tools
cannot override the launch. Child exec reuses the selected absolute session config path and the operator's
inline config, permission rules, security level, execution limits, and explicit
provider/model/agent choices through the existing CLI loader. The task caller
cannot supply these flags. Workspace config, sandboxing, explicit denies, and
canonical session persistence remain authoritative.

Implementation requires a completed, untruncated plan with a nonempty result
from the same service instance. Its prompt includes that plan and the new user
request. The packaged skill requires explicit user authorization; the plan ID
is only a workflow prerequisite, not cryptographic proof of user approval.
The workspace can change between tasks; implementation must inspect current
evidence, and existing edit conflict checks remain authoritative.

Only one task may run at a time. The service retains up to 64 task snapshots;
restart after reaching the cap. IDs do not survive restart. Canonical exec
history remains under the workspace's `.vtcode/sessions/`; planning may write
runtime session artifacts but uses read-only policy for coding tools.
Each task has a 30-minute deadline and a two-second post-exit output-drain limit. Prompt input is limited to 16 KiB; event
records are limited to 1 MiB. Public responses and diffs are each limited to
16 KiB, with at most 128 recorded changed paths. Truncation is explicit.

`completed` requires process exit zero and a successful canonical thread outcome.
Missing completion, malformed events, process failure, or unconfirmed cleanup
produce `failed`. No recorded verification means `unverified`, even after exit
zero. `verified` requires a runtime verification-passed event with a command and
exit zero. Later tool activity invalidates that proof conservatively. `failed`
verification is distinct from task failure. A failed or cancelled task retains
an observed failed check, discards positive proof, and marks its evidence incomplete. Changes derive from successful
canonical file-change events, not a whole-workspace Git diff; shell changes may
therefore lack attribution. Diffs without safe relative public paths are withheld.
Public text uses the shared secret sanitizer; raw stderr and private reasoning
are excluded. Sanitization is best effort, so only connect a trusted private client.

### Structured command evidence

`command_results` adds historical command observations to `start_task` and
`get_task` responses. The legacy `verification`, `verification_command`, and
`verification_exit_code` fields retain their current freshness/invalidation
rules and describe the latest runtime-recorded verifier only. A passing Cargo
check does not establish byte equality or cleanup of a smoke file.

| Entry field | Meaning |
| --- | --- |
| `source` | Canonical `tool_output` or `command_execution` event |
| `event_ref` | Thread, runtime task, actor, turn, and item identities, where observed; use the snapshot's session ID for local archive lookup |
| `tool_call_id`, `tool_name` | Native tool metadata, when observed |
| `command` | Sanitized recorded command, or null when no preceding matching invocation is available |
| `command_truncated` | Text exceeded its byte cap, sanitization withheld data, or separately appended argv was omitted |
| `status`, `exit_code` | Canonical lifecycle status and nullable observed exit code |

The binary pairs command-tool results only with preceding invocation items in
the same canonical context. It never guesses from a reused model call ID,
future invocation, output text, or agent narrative. A completed tool call may
yield a running command: null exit codes never imply zero. Rejected attempts
can have failed status with no command exit. An atomic script produces one
command observation, not synthetic results for each assertion inside it.

Results retain canonical result-event order. Distinct reruns remain distinct;
repeated delivery of the same result item is deduplicated. Different canonical
sources can describe the same underlying execution and are not proof that two
processes ran. Historical observations remain after later mutations, failure,
or cancellation, without granting current-generation verification credit.

The public list is capped at 32 entries and 16 KiB of serialized entries, with
at most 4096 UTF-8 bytes per command. Pending correlation is also bounded.
Missing correlations, withheld identity data, command truncation, dropped
records, and incomplete collection set `evidence_truncated`. False means this
public projection met its bounds; it does not expose every canonical event or
prove every requested outcome. Raw output streams are deliberately excluded
because canonical aggregated tool output may combine stdout and stderr.

Scope each claim to the assertion in the visible command and its terminal
result. Printing a hash or success message with exit zero does not prove an
equality assertion. Command evidence is read-only data, never executable
instructions for the host.

### Execution and evaluation

Post-setup execution errors must reach the runner's terminal failure recorder and
canonical session close. The execution body uses an awaited async block so `?`
cannot bypass that finalizer. Companion retains the first sanitized canonical
failure reason, falling back to the terminal stop reason, and includes process
exit or supervisor failure details in `progress`. Malformed event diagnostics
report the parse location without echoing the event payload. Cleanup and child
reap failures remain distinct; none of these diagnostics grant verification credit.

A live replay exposed an evaluator count/type mismatch: an array in
`high_severity_findings` failed deserialization into the integer count. The shared
single-evaluator/skeptic prompt now states the count and array field types
explicitly. Parsing remains strict: arrays are not coerced into counts, and a
nonzero high-severity count still rejects a pass verdict.

Tracker updates to different indices or states use normalized action identities
in the generic headless loop guard. They do not consume one aggregate default
tracker repetition allowance. Identical polling and updates remain bounded, and
explicit operator-configured tool limits retain their aggregate meaning.

Evaluator requests include up to 12 recent retained tool results, paired only
with preceding matching function calls. The sanitized excerpt is bounded to
16 KiB and excludes assistant reasoning, invocation-only claims, and records
before the retained current-task input. If that boundary is missing, the
evaluator receives an explicit evidence gap. Missing or
cleared records are not proof of nonexecution; these excerpts do not replace
required final verification. Planner and replanner guidance requires executable
final-state tracker assertions that remain satisfiable after cleanup, rather
than implementation writes/deletes or prose falsifiers in `verify` fields.

Planner, evaluator (including skeptic-panel members), and replanner requests
share compiled scope guidance. Generated contracts remain subordinate to the
user request and execution policy. Preserving a dirty workspace means comparing
against its pre-task baseline; an empty Git status is required only when the
user explicitly requests it. Dirty status alone proves neither a new regression
nor preservation of existing contents.
Preservation checks compare recorded contents or diffs, not status counts.
Cleanup requirements describe outcomes rather than prescribing a shell trap;
checks must be admitted by the current policy and must not mask failures with
`echo` or `true`.

Configured verification can update build artifacts. Runtime-owned outputs such
as `.vtcode/` metadata and `memories/progress.md` must be disclosed separately
from requested file edits. They are not evidence that no other files were
touched. Explicit user restrictions still apply: a conflict with required
runtime behavior must be reported before implementation, without silently
expanding scope or disabling mandatory checks.

Evaluator generalization notes retain their scope, evidence, and falsifier in
the contract as observations and limitations. Replanning no longer copies raw
falsifiers into executable tracker fields or automatically makes each note a
new acceptance criterion. A structured replan may add positive executable checks
for unmet requirements within the original scope; failure or rejected scope
changes leave notes as evidence. Existing mandatory checks and generation-based
freshness gates remain enforced.

Evaluator responses have a 4096-token allowance. JSON-only sub-role requests
permit one tool-free concise rewrite when the provider reports output-length
truncation; a second truncated response fails explicitly. Other parse errors
retain finish-reason and output-budget context. The implementation is not
replayed by this response repair, and malformed verdicts remain rejected.

The failure-reason comparison used OpenAI Codex's
[`codex-rs/exec/src/event_processor_with_jsonl_output.rs`](https://github.com/openai/codex/blob/ce0323f2f2d3b25e82806cd555237da3db4e1897/codex-rs/exec/src/event_processor_with_jsonl_output.rs#L539)
at commit `ce0323f2f2d3b25e82806cd555237da3db4e1897`
(Apache-2.0, copyright 2025 OpenAI). VT Code reuses its own `ThreadEvent` contract
and finalizer; no upstream implementation was copied.

The preservation comparison also checked
[`codex-rs/core/templates/agents/orchestrator.md`](https://github.com/openai/codex/blob/ce0323f2f2d3b25e82806cd555237da3db4e1897/codex-rs/core/templates/agents/orchestrator.md)
at that commit: unrelated user changes must be preserved. This is guidance
comparison, not an imported implementation or a guarantee of model compliance.

Cancellation requests a process-group interrupt, allows a short cleanup window,
then terminates remaining owned processes and reaps the child. Cancellation is
not rollback. Listener shutdown cancels and joins task supervisors before return.
The transport crate owns only task interfaces; launch/supervision belongs to the binary.

Windows process cleanup uses an owned synchronization handle to distinguish a
missing or exited process from permission and termination failures. A process
that exits during termination is also successful cleanup. Both forced and
graceful `taskkill` helpers capture stdout/stderr; helper diagnostics must never
enter the stdio MCP protocol. The Windows-only `windows-sys` dependency supplies
the native handle and wait APIs, reusing the version already in the lockfile.
The handle is closed by RAII on every return path.

This lifetime check follows the owned-handle principle inspected in
[`codex-rs/utils/pty/src/win/job.rs`](https://github.com/openai/codex/blob/8c076942cb70fba8e5c4732c23f1faabc4aed484/codex-rs/utils/pty/src/win/job.rs)
(Apache-2.0, copyright 2025 OpenAI). No upstream code was copied and no Job Object
containment is claimed. Unix managed commands use the separate lifetime
mechanism described below because they detach into their own process groups.

Windows regression sources cover exited/repeated cleanup, live-child
termination, and invalid PID rejection. Cross-compilation checks those sources;
it does not establish native Windows runtime behavior.

### Unix managed-command ownership

Each active task owns a private Unix socket in an exclusively created directory
with mode `0700`. The exec runner, pipe launcher, and both PTY launch sites use
the shared native helper in `vtcode-bash-runner::process_owner`. The binary
dispatches this mode before agent-specific environment hardening, worker
threads, or the async runtime. It
preserves the admitted program, literal argv, `argv[0]`, working directory,
environment filtering, and sandbox launcher. An absent or closed endpoint fails
before target execution. Command approval remains with the existing launchers.
Normal agent startup still applies process hardening; managed commands retain
their admitted environment, including explicit unsandboxed loader overrides.

After endpoint acknowledgement, the helper double-forks a group-local watchdog
and replaces itself with the admitted command. The watchdog pins its own group
identity, closes standard streams, and waits for owner EOF. EOF or an I/O error
causes it to kill its own group. Double-forking keeps shell `wait` independent of
the watchdog. Fork-child paths use documented async-signal-safe operations;
ownership and readiness sockets close before the target executes. Cleanup does
not maintain a registry of stale numeric tool process IDs.

One service guard kills the root exec group, closes ownership connections, and
joins the private cleanup worker before publishing a terminal result. Its drop
path also runs on supervisor abort. Admission is capped at 256 live connections;
startup acknowledgement and shutdown peer draining each have a two-second bound.
A cleanup failure remains an error on repeated shutdown. The production task
deadline remains thirty minutes; tests use the same private supervision path
with a shorter budget.

The regression sources exercise the shipped helper with real pipe and PTY
groups, output closure, literal arguments, `argv[0]`, missing endpoints,
descriptor closure, unrelated-process survival, and repeated shutdown. Service
fixtures launch real detached helper groups and cover cancellation, collector
failure, runner SIGKILL, deadline, and supervisor abort. This is a managed-group
lifetime contract: descendants that deliberately create another process group
or session are outside it. Native Linux and Windows behavior requires separate
host validation; cross-compilation alone does not establish runtime behavior.

### Local validation (2026-10-17)

The installed local plugin's stdio launcher completed MCP initialization, tool
discovery, search/fetch, planning, and approved implementation using the rebuilt
debug binary. Plan `f748cd96-bd39-4ae2-89c0-e7263e875fcd` and implementation
`74cee6f9-3782-41da-b6de-0e7a47ef7856` completed with
`evidence_truncated: false`. Canonical tool events recorded exit zero for exact
`cmp` against the literal smoke-test text, removal, and the final file/symlink
absence assertion. The snapshot's separate mandatory verifier was
`cargo check --locked`, exit zero. The canonical manifest closed with
`in_turn: false`; service shutdown exited zero and the tracked diff was unchanged.
An earlier failed replay generated `memories/progress.md`; that extra artifact
was removed after validation.

The focused runner, evaluator, loop-detector, Companion, prompt, stdio, and inline
event suites passed 456 tests. The development gate, locked binary build, scoped
Markdown lint, and diff whitespace check passed. This validates the installed
launcher directly; an already-running Codex connection must restart to load the
rebuilt executable.

The follow-up scope/replanning increment passed 339 focused runner and prompt
tests plus the development gate and locked binary build. Installed-launcher
plan `e389c505-3f1e-4726-8914-01804a733d9b` and implementation
`0035b907-2939-4189-a29f-aeb4d68821b4` completed with
`evidence_truncated: false`; implementation verification was `verified`
(`cargo check --locked`, exit zero). Full canonical events retain the Python
pre-existing-path guard, exclusive creation, literal byte comparison, and
`finally` cleanup, with exit zero. Final absence, closed canonical persistence,
service shutdown, and preservation of the tracked diff were independently
checked. Runtime metadata and configured-check outputs were explicitly allowed.
The evaluator's internal command excerpt remained bounded; the full canonical
command was inspected separately. An earlier trap-based replay was rejected for
unsupported guard/cleanup claims and cancelled at the client's five-minute
limit; its leftover smoke file was checked and removed separately. That earlier
attempt is not counted as a successful task.

The structured-command increment passed 416 focused Companion, MCP, stdio,
runner, and prompt tests, the development gate, and the locked binary build.
Plan `322b3c64-c665-4efd-804d-a00372cf2d8d` and implementation
`ea1f9483-bd9f-46fc-9e1f-054474d2219c` completed with
`evidence_truncated: false`. The implementation snapshot retained three full
`command_results`, independently matched to canonical invocation/result events:
the atomic 29-byte equality/cleanup script, final file/symlink absence assertion,
and `cargo check --locked`, each with exit zero. Overall `verification` remained
`unverified`: the runtime ended with `stopped_no_action` after idle turns and
emitted no harness verification outcome. These historical commands therefore
validate the new projection, not task-wide verified completion. Service shutdown
exited zero; the smoke path was absent; the tracked diff and new untracked
projection source were unchanged. The installed launcher used the rebuilt
binary; the updated plugin skill was not reinstalled into an existing host.

An earlier replay exposed false secret redaction of canonical `task-<UUID>` IDs;
the projection now preserves that exact validated format, with regression
coverage that malformed secret-bearing identities still receive filtering.
Another replay ran the broader nextest suite and failed
`plugin_providers_surface_through_manager_config` (two providers found, one
expected). It then stalled during unrelated inspection and was stopped; shutdown
exited zero and preserved the tracked diff. The final replay explicitly scoped
verification to smoke assertions and configured Cargo verification, with the
development gate and focused tests validated separately. This does not establish
a passing full workspace suite.

## Authentication and connection

The existing contract is documented in [WebMCP development](webmcp.md#remote-mcp-transport).
VT Code validates a private internal bearer token. An external proxy must:

1. Terminate HTTPS and validate the user's OAuth token, issuer, audience/resource,
   expiration, and applicable permissions.
2. Strip incoming authorization and any caller-supplied proxy identity headers.
3. Inject the configured internal bearer token before forwarding to loopback.
4. Preserve MCP HTTP methods and content negotiation, and publish the protected
   resource metadata route as JSON without a login-page redirect.

VT Code advertises an authorization server; it does not implement OAuth login,
token issuance, JWKS, or a tunnel. Provider/model credentials used by VT Code are
separate from companion-client authentication. See the
[official plugin authentication guide](https://developers.openai.com/plugins/build/auth).
A reachable tunnel alone does not satisfy authentication.

First test with a local MCP client and an internal bearer supplied through that
client's secure configuration. Then configure a stable authenticated HTTPS
tunnel, test OAuth discovery/login, and repeat the workflow from ChatGPT.
Client installation and account configuration are explicit setup steps; building
the source does not install a plugin in Codex or ChatGPT.

Connection acceptance: unauthenticated calls return 401; authenticated
`initialize` works; `tools/list` advertises all five tools; `search`/`fetch` remain
bounded; planning changes no source files; explicitly requested implementation
returns recorded evidence; cancellation reaches a terminal state. Repeat from
both clients before calling the private pilot connected.

## Remote packaging (deferred)

The source is in `plugins/vtcode-companion`. The local packager generates its
stdio `mcp.json` from explicit machine paths. Remote packaging instead requires
an actual endpoint to be verified. Set a short-lived external OAuth
access token in `VTCODE_COMPANION_ACCESS_TOKEN` through your secure environment,
then run:

```sh
python3 scripts/package-vtcode-companion.py \
  --mcp-url https://YOUR-MCP-HOST/mcp \
  --output /absolute/path/outside/plugin/vtcode-companion-0.1.0.zip
```

The packager checks public resource metadata, rejected unauthenticated access,
authenticated initialization, all five tools, and conservative action hints.
It writes the verified OAuth resource into portable `mcp.json`, excludes tokens,
and reopens the ZIP to inspect its exact contents. Existing artifacts are never
overwritten. These checks prove discovery, not model-provider execution or login
from both clients. Do not represent a source-only package or fixture test as a
connected plugin. Upload, installation, submission, and publication are separate.
Public review preparation, draft acceptance cases, and demo recording steps are
tracked in [submission preparation](vtcode-companion-submission.md).

## Research and verification

DeepWiki discovery returned client-side MCP paths rather than a comparable task
server. Direct inspection pinned to OpenAI Codex commit
`e16b631d08cdf82773831f9bbfb793da687d5299` verified conservative approval behavior in
[`codex-rs/core/src/mcp_tool_call_tests.rs`](https://github.com/openai/codex/blob/e16b631d08cdf82773831f9bbfb793da687d5299/codex-rs/core/src/mcp_tool_call_tests.rs):
destructive tools require approval even if marked read-only. The companion marks
start and cancellation as non-read-only, destructive actions. The same pinned source's
[`codex-rs/codex-mcp/src/binding.rs`](https://github.com/openai/codex/blob/e16b631d08cdf82773831f9bbfb793da687d5299/codex-rs/codex-mcp/src/binding.rs)
retains immutable permission authority in `PreparedMcpCall`. VT Code preserves
operator-selected child launch policy using its existing CLI and session-config
loader rather than copying that implementation. No upstream code was copied.
OpenAI Codex is Apache-2.0, copyright 2025 OpenAI. No verified upstream
task-server implementation is claimed; execution ownership follows local VT Code code.

Focused checks:

```sh
cargo nextest run --locked -p vtcode -p vtcode-webmcp \
  -E 'test(cli::companion) | test(remote_mcp)'
python3 -m unittest discover -s scripts/tests -p 'test_package_vtcode_companion.py'
cargo clippy --locked -p vtcode -p vtcode-core -p vtcode-webmcp --all-targets -- -D warnings
cargo fmt --all -- --check
./scripts/check-dev.sh
```

Process fixtures exercise real child supervision without a paid provider.
Live provider, Codex, ChatGPT, tunnel, and OAuth checks require the actual configured
services and must be reported separately. macOS process tests do not establish
Linux or Windows behavior.

For local stdio, `tests/companion_stdio.rs` starts the real binary with isolated
configuration and checks discovery, exact fixture reads, sensitive/path-escape
rejection, denied trust escalation, and EOF shutdown without a provider request.
The handler uses the same tool router for local and HTTP transports. DeepWiki
again returned client-side launch code rather than server startup; pinned Codex
`e16b631d08cdf82773831f9bbfb793da687d5299`,
`codex-rs/rmcp-client/tests/process_group_cleanup.rs`, directly verifies explicit
shutdown with in-flight work and process-group cleanup. No source was copied.
The server-side stdio path follows the installed rmcp transport API; no upstream
server implementation is claimed.

## Current local validation

On 2026-10-17, the affected locked nextest selection passed **87 tests**, with
zero failures and 8233 tests outside the selection. It included all WebMCP crate
tests and the new real-process stdio tests. The Python package suite passed
**10 tests**. The local archive validated as one skill and one MCP server.
The final stdio-only rerun passed both tests after test-helper cleanup. Affected
strict Clippy (`--all-targets -- -D warnings`), Rust formatting, Ruff lint/format,
Markdown lint, scoped diff checks, and `check-dev.sh --quiet` passed. The development
gate completed in 1m 20s and did not run tests. The broader core test build emitted
a nonfatal macOS unwind-table size linker warning; its test run exited zero.

An independent smoke test extracted the ZIP and launched its actual Python
entrypoint. It verified initialization, all five tools, an actual README fetch,
rejection of implementation without a plan, overlap rejection, task cancellation,
and EOF shutdown with exit zero. The temporary extraction was removed. This is
direct protocol validation, not proof of installation or invocation inside Codex.
No plugin was installed, uploaded, or submitted.

Earlier live-provider runs did change a disposable fixture and pass its check,
but failed at harness evaluator-response parsing; the task correctly returned
failed/unverified. That error remains unresolved. These new local transport and
packaging checks do not establish successful model-driven implementation. See
the retained [validation evidence](vtcode-companion-submission.md#observed-validation-on-2026-10-17).

The local ZIP contains exactly four files:

```text
plugin.json
mcp.json
skills/vtcode-companion/SKILL.md
server/launch.py
```

Its SHA-256 is
`74e02b2e59b15ea63f5070963e5292f012e049141ab065d65dc4993b46e83136`.

## Planning loop regression (2026-10-17)

Codex session `01a148af-11e8-7900-a4cd-fe45c3902a4e` invoked the installed local
Companion twice. Tasks `df3445e9-14ff-488c-b2fa-cb5a50a9327d` and
`e2385be1-4ea6-4045-b671-3eb6ce6ea24c` map to canonical VT Code sessions
`session-vtcode-20261017T070732Z_815633-75293` and
`session-vtcode-20261017T070930Z_965152-75910`. Both ended with `loop_detected`.
Their event histories show different shell inspections being classified as the
oscillation `[exec_command -> exec_command]`, followed by a hard stop inferred
from aggregate tool-name counts. `evidence_truncated` marked incomplete failed
execution; it was not evidence that a completed plan exceeded the output cap.

The repair counts normalized command actions, records explicit hard stops, and
keeps custom operator limits aggregate. A terminal, nonempty, tool-free planning
response enters the existing completion assessment without needing prose
completion keywords. Successful planning emits the actual plan as the final
agent message and canonical completion result. Permission and verification
gates remain in place; failed and cancelled tasks still report incomplete evidence.

A later read-only Companion replay exposed another false positive: fuzzy
oscillation detection reduced every repeated `code_search` call to the same tool
name, even when each call searched for different evidence. Fuzzy oscillation now
requires at least two distinct tool names in the repeated pattern. Exact
normalized-action repetition, navigation budgets, and alternating multi-tool
pattern detection remain active.

The last configured planning turn is reserved for tool-free synthesis, so lengthy
inspection cannot consume every turn before attempting an answer. The model must
state evidence gaps. Native or textual tool calls on that final turn are rejected
without execution and retain a failed turn-limit outcome; no extra turn is granted.
Provider output-token truncation also triggers one tool-free rewrite into a shorter
complete plan, using the same output cap and remaining turn budget. Completion
keywords in a truncated draft cannot count as success. If the rewrite is still
truncated, the task fails explicitly instead of repeating the draft through idle recovery.
Headless planning uses a 4096-token fallback output allowance, matching the existing
skill-synthesis allowance and compaction output reserve, rather than the generic
2000-token task default. Explicit provider sampling limits keep precedence. Replay
exposed the smaller default truncating both the initial draft and its rewrite.

DeepWiki discovery was checked against pinned OpenAI Codex commit
`e16b631d08cdf82773831f9bbfb793da687d5299`,
[`codex-rs/core/src/stream_events_utils.rs`](https://github.com/openai/codex/blob/e16b631d08cdf82773831f9bbfb793da687d5299/codex-rs/core/src/stream_events_utils.rs)
and its
[`stream_events_utils_tests.rs`](https://github.com/openai/codex/blob/e16b631d08cdf82773831f9bbfb793da687d5299/codex-rs/core/src/stream_events_utils_tests.rs):
visible assistant text in a final or unphased message is treated as terminal,
while nonterminal text does not close the turn. This supports terminal-phase
completion rather than keyword matching; it does not establish VT Code's loop
thresholds. No upstream code was copied (Apache-2.0, copyright 2025 OpenAI).

After rebuilding `target/debug/vtcode`, restart Codex to replace an already
running MCP process. The installed local launcher points directly to that binary;
these runtime fixes do not require changes to the plugin ZIP or cached manifest.
Nextest builds test executables and must not substitute for
`cargo build --locked --bin vtcode` when validating this launch path.

### Observed regression validation

The final locked nextest selection passed **300 tests**, zero failed, 8026 outside
the selection. It covered the loop detector, all headless-runner tests, Companion,
remote/local MCP routing, planning prompt presence/size, skill execution, and the
autonomous executor. New regressions cover 18 distinct real shell inspections
followed by a plain plan, actual repeated-action hard stops, explicit operator
limits, final-turn tool rejection (native and supported textual forms), blank
answers, and one bounded truncated-plan rewrite. A second truncated draft fails
without accepting completion keywords. Affected strict Clippy passed with
`--all-targets -- -D warnings`; the package suite passed 10 Python tests. The
macOS test linker emitted its nonfatal unwind-table size warning.
Final Rust formatting, Markdown lint, and scoped whitespace checks passed.
`./scripts/check-dev.sh --quiet` passed in 36 seconds (formatting, default-member
Clippy/compilation, and shell lint; tests were run separately with nextest).

After an explicit binary rebuild, the exact first archived Codex request was
replayed through the **installed** `vtcode-local/vtcode-companion/0.1.0` Python
launcher over MCP stdio. Canonical session
`session-vtcode-20261017T074302Z_404837-23278` returned `success` after 20 completed
tool calls. `get_task` returned `completed`, the actual **9699-byte** plan,
`evidence_truncated=false`, no attributed changes, and `verification=unverified`
(correct for planning without implementation/checks). The tracked source diff
remained unchanged, excluding the private TODO path from inspection; EOF shutdown
exited zero. This verifies the live provider and installed launch configuration
at protocol level. Restarting the user's existing Codex MCP process and invoking
it again from the host remain user-side steps; ChatGPT and remote transport were
not exercised.
