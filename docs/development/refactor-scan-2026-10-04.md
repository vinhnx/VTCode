# Large-file and reusable-component refactor plan

Scan date: 2026-10-04. Checkout: `4513b8883`, plus existing owner changes.
Scan scope: tracked Rust and application/script source; analysis and planning.
Implementation progress is recorded below. `docs/project/TODO.md` was not read or modified.

## Inventory and interpretation

- 2,392 tracked Rust files, 829,793 physical lines, including tests and vendored patches.
- 175 Rust files have at least 1,000 lines after excluding `patches/`.
- Crate totals including tests: core 264,492; binary `src/` 212,299;
  LLM 85,201; UI 82,034; config 34,345; commons 21,555.
- The accompanying [CSV inventory](refactor-scan-2026-10-04.csv) lists every
  tracked Rust file with physical line count and a path-based classification.
  Inline tests remain included in source-file counts.
- Size is a review signal, not defect evidence. No complexity, performance,
  or security defect is established by these counts.
- Largest files include dedicated test suites: turn-loop tests (3,726),
  OpenAI provider tests (3,660), subagent tests (3,337), and tool-outcome
  helper tests (2,912). Split these by behavior only when navigation warrants it;
  do not prioritize them as production monoliths.

The duplication probe compared trimmed 18-line windows with a minimum text
length. Matches were inspected manually: repeated imports and test fixtures
were excluded from production recommendations. This is a candidate scan,
not an exhaustive semantic clone analysis or compiler reachability analysis.

## Production responsibility hotspots

Paths below are relative to the repository; line counts include inline tests.

| Priority | File | Lines | Proposed responsibility boundary | Risk |
| --- | --- | ---: | --- | --- |
| P1 | `crates/codegen/vtcode-llm/src/providers/shared/mod.rs` | 1,832 | Cache lineage/usage, function-output conversion, compacted history, stream aggregation, UTF-8/SSE framing | Medium |
| P1 | `crates/codegen/vtcode-llm/src/providers/common.rs` | 1,766 | HTTP/error-body helpers, chat wire mapping, reasoning replay, token counting, cache policy | Medium |
| P1 | `crates/codegen/vtcode-core/src/tools/handlers/planning_workflow/artifacts.rs` | 2,071 | Embedded tracker codec, plan section/step parsing, verification classification, validation/reporting | Medium |
| P1 | `extensions/vscode-extension/src/extension.ts` | 3,534 | Activation wiring, quick-action views, config/settings commands, process/terminal launch | Medium |
| P1 | `apps/webmcp/src/main.ts` | 1,846 | Evidence dialog, settings/connection UI, workspace drafts, review/approval workflow | Medium |
| P2 | `crates/codegen/vtcode-core/src/tools/registry/execution_facade.rs` | 1,984 | Reentrancy guard, admission/safety adapters, execution pipeline, result/error processing | High |
| P2 | `src/agent/runloop/unified/turn/turn_processing/llm_request/copilot_runtime.rs` | 2,139 | Permission adapter, local terminal bridge, observed-tool presentation, stream conversion | High |
| P3 | `src/agent/runloop/unified/turn/session_loop_runner/orchestration/mod.rs` | 2,664 | Session bootstrap, input/turn coordination, turn persistence, ordered shutdown/postamble | High |
| P2 | `scripts/release.sh` | 1,675 | Changelog generation, Homebrew publishing, release orchestration | High |

These are proposed local modules, not new crates or universal utility layers.
Provider helpers remain in `vtcode-llm`; planning stays in core; runtime
coordination stays in the binary; view controllers stay in their apps.
Keep existing facades/re-exports while moving implementations.

## Concrete deduplication opportunities

### 1. OpenCode model normalization and validation

Evidence: `providers/opencode_go.rs:202` and `opencode_zen.rs:228` share
supported-model construction and the clone-only-when-normalization-is-needed
validation branch. They already have `providers/opencode_shared.rs`.

Extend that existing module with a narrow normalized-model validation helper.
Pass provider identity, supported models, and resolved model explicitly.
Keep protocol selection, credentials, constructor contracts, and route-specific
capabilities in each provider. Avoid a new provider trait or macro framework.
Test empty/default model, prefixed model, unsupported model, and already
normalized model for both providers.

### 2. UI constants with an existing dependency direction

Evidence: `vtcode-config/src/constants/ui.rs:6` and
`vtcode-ui/src/tui/config/constants/ui.rs:5` duplicate slash palette and modal
constants. UI already depends on config; config does not depend on UI.

Inventory public constant names and values first. Re-export identical stable
defaults from config where appropriate; retain UI-only rendering calculations
in UI. Do not blindly replace the whole file: transcript bottom padding is
explicitly documented as a compatibility duplicate, and UI owns its viewport
clamping function. Keep the existing product-name parity test.

### 3. ACP/Copilot stdio transport primitives

Evidence: `vtcode-acp/src/transport.rs` and
`vtcode-llm/src/copilot/transport.rs` both implement bounded write queues,
pending-call guards, JSON-RPC dispatch, bounded reads, and child teardown.
Their matching notification tests are supporting evidence; production code
was also inspected.

Start with bounded framing in an existing commons infrastructure module.
ACP's `read_bounded_line` at line 369 excludes the newline from retained
content/cap accounting; Copilot's reader at line 391 includes it and reuses a
caller buffer. Establish and test an explicit delimiter policy before sharing.
ACP supports string/numeric incoming IDs and configurable JSON-RPC version;
Copilot uses numeric IDs. Preserve those adapter contracts and error types.
Only consider moving the full transport after framing reuse proves useful.
Do not add an LLM dependency on ACP or move domain errors into commons.

### 4. Two live UI command/handle protocols

Evidence: `vtcode-ui/src/tui/core_tui/types/protocol.rs:266` and
`app/types/protocol.rs:401` duplicate handle send/append/replace operations.
Both modules are declared and exported through their respective type facades.
The app handle additionally owns deferred input and transient activity.

Map callers and enum differences before selecting one canonical command
vocabulary. Extract shared payload types first; preserve facade imports.
Keep app event ownership and deferred/transient state explicit. A full handle
merge requires migration tests for both paths, not a mechanical text move.

### Rejected clone candidates

- Repeated controller imports and allow attributes are not reusable behavior.
- Agent preview formatting in slash-command runtime is marked `cfg(test)`;
  its overlap with startup UI is not a second production implementation.
- Runloop and core agent infrastructure are different layers. Their aggregate
  sizes do not justify deduplicating one against the other.
- Existing deferred validation-type work and the AsyncMcpManager actor rewrite
  are not reopened by this scan.

## Incremental execution plan

1. **Small deduplication:** OpenCode validation helper, then audited constant
   re-exports. Separate commits; no unrelated provider changes.
2. **Provider decomposition:** move cohesive groups from `common.rs` and
   `shared/mod.rs` verbatim; preserve visibility/re-exports. Then remove
   duplication only where multiple callers share the same contract.
3. **Planning decomposition:** split tracker codec, section/step parsing, and
   verification classification. Keep `validate_plan_content` as the facade.
   Preserve acceptance rules and useful repair feedback.
4. **Application decomposition:** extract WebMCP evidence/settings controllers
   using the existing evidence/persistence/editor modules; extract VS Code
   view and terminal/config services using its existing command registry and
   backend integration. Inject narrow state/callbacks instead of copying globals.
5. **Infrastructure reuse:** characterize ACP/Copilot framing and cancellation;
   share framing first. Independently migrate shared UI protocol payloads.
6. **Execution facade:** move the reentrancy implementation, then extract
   contiguous execution stages. Preserve safety/admission order, prevalidation,
   settlement mode, snapshots, timeout annotation, and structured error shape.
7. **Copilot/runtime orchestration:** map state ownership and lifecycle exits
   before extraction. Reuse existing `session_setup`, `session_teardown`,
   orchestration `turn_tail`, and interaction-loop helpers. Keep one authoritative
   state machine; do not introduce a parallel runner or a giant context bag.
8. **Release script:** move pure changelog functions first, using established
   release libraries. Validate in fixtures/dry-run paths; never run publication
   as a refactor check.

Each step should be reviewable and independently revertible. Prefer cohesive
modules of a few hundred lines; this is a navigation goal, not a hard line cap.
Success means fewer implementation owners for shared behavior and clearer
state ownership, rather than simply more files. Every extraction must be wired
into live callers; do not retain unused parallel implementations.

## Verification required during implementation

- Rust: `cargo fmt --all -- --check`, `./scripts/check-dev.sh`, focused
  `cargo nextest run --locked -p <affected-crate>`, and relevant warnings-denied
  `cargo check --locked`. Broaden tests when interfaces or multiple consumers change.
- Provider splits: history replay, tool-result pairing, reasoning ordering,
  cache usage, split UTF-8, SSE boundaries, finish reasons, and cancellation.
- Planning: existing artifacts/verification tests, quoted metadata, shell
  metacharacters, concrete verification, vague steps, and tracker round trips.
- Transport: cap minus/at/plus one, delimiter accounting, oversized-line draining,
  partial EOF, malformed frames, IDs, timeout/drop cleanup, queue pressure,
  secret redaction, and child/task shutdown. Test both adapters.
- UI/runtime: wide/narrow resize, scrolling, selection, links, overlays,
  deferred events, streaming, completion, interrupt, new session, and resume.
  Run the documented PTY/inline-event regression suites for affected paths.
- Apps: existing package lint/type/build/test commands plus rendered interaction
  checks for connection, drafts, approval, and terminal lifecycle changes.
- Release: Bash syntax, available shell lint, and fixture-based changelog and
  packaging checks with publication disabled.
- Update module guidance after significant boundary/API changes; update docs
  when behavior changes. No new production dependencies are planned.

## Validation of this scan

Ran tracked-file inventory, line counting, clone-candidate comparison, targeted
source reads, module-export/dependency inspection, and Git status checks.
No builds or tests were run for the initial scan; implementation checks appear below.
The recommendations above are maintainability proposals, not verified bugs.

## Implementation progress

Steps 1 and 2 were committed together at the owner's request as `3f8fcb607`
(`refactor(providers): extract shared wire helpers and deduplicate defaults`).

### Step 1: small deduplication

- OpenCode Go and Zen delegate normalized request validation to one helper in
  `providers/opencode_shared.rs`. Provider-specific normalization, supported
  models, error context, empty-model acceptance, and request immutability remain
  unchanged. The normalized-model branch retains clone-on-demand behavior.
- Added four dispatcher regressions covering accepted/prefixed/blank models,
  unknown models, separate provider allowlists, and message validation.
- Re-exported 19 identical viewport, slash palette, and modal defaults from
  config through the existing UI paths. No new config constants or dependencies.
  UI-specific rendering values and transcript-padding calculation remain local.
- Audited UI module guidance and documented shared-default re-exports.
- Focused validation: seven OpenCode tests and 200 UI modal/slash/constants/
  viewport tests passed. Independently compared all UI constant declarations
  against the pre-change version to verify names, types, and expressions.
- `./scripts/check-dev.sh --quiet` passed formatting, warnings-denied Clippy,
  compilation, and shell lint. `cargo check --locked -p vtcode-llm -p vtcode-ui`
  passed. Scoped `git diff --check` passed; no full workspace test run.
- Next step: provider helper decomposition, using verbatim moves and existing
  re-exports before any additional semantic deduplication.

### Step 2: provider helper decomposition

- Reduced `providers/common.rs` from 1,766 lines to a 65-line facade over
  private chat, HTTP, request, reasoning, prompt-cache, streaming, and token-count
  modules. Existing caller imports and public visibility remain stable.
  Its existing tests now live in `common/tests.rs`.
- Reduced `providers/shared/mod.rs` from 1,832 lines to a 39-line facade.
  Stream assembly, tool-output/compacted-history conversion, prompt lineage,
  cache-token usage, and UTF-8 decoding now have dedicated modules.
  SSE payload extraction and boundary detection joined the existing SSE module.
- Moved private-helper tests alongside their implementations. Retained existing
  Responses adapters and sanitizer modules. No new dependencies or runtime
  behavior changes; semantic deduplication remains separate from extraction.
- Structural comparison with `ast-grep` verified all 181 function bodies
  (including tests), six structs, four enums, one trait, eight constants, and
  one macro across the moved surfaces. Comparison permits module-local visibility,
  indentation changes, and the required HTTP error-handler path qualification;
  string literals remain protected from whitespace normalization.
- `cargo nextest run --locked -p vtcode-llm`: 1,147 passed, zero skipped.
- `./scripts/check-dev.sh --quiet` passed formatting, warnings-denied Clippy,
  compilation, and shell lint. Scoped `git diff --check` and moved-file whitespace
  checks passed. Verified all 15 original public declarations retain facade exports.
- Final `cargo check --locked -p vtcode-llm` passed after the extraction stabilized.
- Updated the LLM crate's module map and facade/visibility guidance.
- Next step: planning artifact decomposition, preserving validation and repair
  feedback through the existing `validate_plan_content` facade.

### Step 3: planning artifact decomposition

- Reduced `planning_workflow/artifacts.rs` from 2,071 lines to a 25-line facade
  over section parsing, quote-aware step parsing, tracker conversion, validation,
  and verification classification. Existing caller imports and public APIs remain
  stable; shared internal helpers have module-scoped visibility.
- Kept validator-owned repair feedback, acceptance rules, embedded tracker markers,
  and path pairing unchanged. Parsing serves both validation and tracker conversion
  without circular module dependencies. Existing agentic-testing cases now live in
  a dedicated test module; no new dependencies or behavior changes.
- Structural comparison with `ast-grep` verified all 75 function bodies, three
  structs, one enum, the report implementation, and 17 constants. Comparison permits
  module-local visibility, indentation, and optional trailing commas while preserving
  string literals. Verified all 10 public declarations retain facade exports.
- `cargo nextest run --locked -p vtcode-core -E 'test(planning_workflow) | test(planning_task_tracker)'`:
  122 passed, 3,929 skipped. This covers planning artifacts, tracker persistence,
  metadata parsing, command checks, policy transitions, and planning runtime callers.
- `./scripts/check-dev.sh --quiet` passed formatting, warnings-denied Clippy,
  compilation, and shell lint. Final `cargo check --locked -p vtcode-core`, scoped
  whitespace checks, and `git diff --check` passed. No full workspace test run.
- Audited core module guidance and recorded the new artifact responsibility map.
- Next step: application decomposition, starting with the WebMCP controllers and
  their existing evidence, persistence, and editor boundaries.

### Step 4a: WebMCP evidence and settings controllers

- Reduced `apps/webmcp/src/main.ts` from 1,846 to 1,533 lines. Extracted the typed
  required-element lookup to `app-elements.ts`, evidence presentation/observation
  to `evidence-controller.ts`, and bridge settings/setup presentation to
  `settings-controller.ts`. Event wiring and editor/proposal/backend ownership stay
  in the application entry point.
- Controllers consume narrow callbacks and current connection facts. Settings read
  the live backend on each render, preserving pairing/disconnection updates.
  Evidence uses the existing bounded, sanitized recorder; failures remain isolated
  from the instrumented tool's result or original error. No dependencies added.
- TypeScript syntax-tree comparison verified 22 moved and 69 retained function
  bodies. The only body adaptations are current-backend access in settings and
  discovery-name setter calls in registration. Existing strings, dialog guards,
  setup command quoting, and approval logic remain unchanged.
- Added six controller tests for post-call editor state, result/error identity,
  failing evidence capture, discovery reset, literal display, backend replacement,
  modal guards/focus, setup command quoting, and credential-free persistence with
  storage recovery. `bun run test`: 66 passed, zero skipped.
- `bun run typecheck`, `bun run build`, `bun audit --audit-level=high`,
  `./scripts/check-dev.sh --quiet`, and `git diff --check` passed. Vite reports
  a JavaScript chunk larger than 500 kB; this extraction does not split bundles.
- Rendered browser checks could not run: the browser-use runtime was unavailable
  and the computer-use browser inventory was empty. DOM tests do not establish
  real dialog geometry, keyboard behavior, or connected-browser integration.
- Added app-local module guidance after auditing the new ownership boundaries.
- Next increment: VS Code views and terminal/config services within step 4;
  rendered WebMCP checks remain an explicit validation gap.

### Step 4b: VS Code quick-action and workspace-status views

- Reduced `extensions/vscode-extension/src/extension.ts` from 3,534 to 3,037
  lines. Quick-action descriptions and their tree adapter now live in
  `views/quickActions.ts`; workspace-status descriptions and their adapter live in
  `views/workspaceInsights.ts`. Activation, command wiring, trust, configuration
  updates, and terminal ownership remain in the entry point.
- Passed the existing executable-path getter and status-tooltip builder as narrow
  services to workspace insights. Restricted-workspace rendering still avoids
  reading executable configuration. Tree providers keep reading current state
  through the existing callbacks. No new dependencies or runtime behavior changes.
- TypeScript syntax-tree comparison verified all 75 original function bodies,
  five classes, and 15 interfaces across the entry point and extracted modules;
  only service injection and its activation call were adapted.
- Added five Node tests covering restricted/trusted command lists, CLI availability,
  HITL/full-auto/provider branches, parse feedback, shared tooltip arguments,
  live state, refresh notifications, and tree-item metadata. They compile the real
  view modules with TypeScript and load an isolated VS Code API fixture.
- Added real `test:views` and `typecheck:views` scripts plus a focused TS config.
  The existing compile/typecheck/lint/test scripts only print skip messages.
  Updated the development guide and added app-local ownership/verification guidance.
- `npm run test:views`: five passed, zero skipped. `npm run typecheck:views`,
  direct view lint through the existing ESLint config, and
  `npm run bundle -- --production` passed. Rebuilt the local esbuild dependency
  after detecting an installed-binary platform mismatch; manifests/dependency
  versions and lockfile remain unchanged apart from the new verification scripts.
- `npm audit --omit=dev --audit-level=high` found zero production dependency
  vulnerabilities. The entry-point typecheck retains the same eight diagnostics
  as the baseline; full-project typechecking also fails on legacy tests. No full
  extension test pass or rendered Extension Development Host validation is claimed.
- `./scripts/check-dev.sh --quiet`, scoped Markdown lint, and staged
  `git diff --check` passed.
- Next increment: terminal/config services within step 4. Existing typecheck
  diagnostics and rendered app/extension checks remain explicit validation gaps.

### Step 4c: shared CLI process execution and configuration commands

- Reduced `extensions/vscode-extension/src/extension.ts` from 3,037 to 2,618
  lines and `utils/vtcodeRunner.ts` from 194 to 125 lines. Both CLI runners now
  use `services/processExecution.ts` for spawning, streaming, progress,
  cancellation, and completion. Their existing trust checks and caller-specific
  config/context/environment preparation remain in their respective wrappers.
- Removed duplicate executable-path and logging helpers from the entry point;
  its config-argument adapter delegates to the existing URI-based helper.
  Kept different workspace-root and environment-selection contracts separate.
- Moved the four HITL/tool-policy/MCP configuration registrations into
  `commands/configurationCommands.ts`. The entry point retains subscription
  ownership and supplies live summary, trust, output, error, and guide services.
  Summary lookup still occurs after awaiting the config picker; command IDs,
  registration order, trust gates, feedback, and TOML mutation APIs are preserved.
- Structural comparison verified 70 retained/delegated function bodies, all four
  moved callbacks, both original process implementations, caller preflight, and
  remaining activation wiring. Removed one unused catch binding identified by
  lint; its error-handling behavior is unchanged. No new production dependencies.
- Added 11 service tests and reused one compilation/fixture helper with the five
  existing view tests. The combined run passed all 16 tests without skips.
  Coverage includes literal argv, a real Node child process with shell
  metacharacters and explicit environment values, streaming, cancellation,
  spawn errors, live progress environment, trust rejection, picker-time config
  changes, stale summary reload, failed updates, provider state, and disposal.
- Added real `test:services` and `typecheck:services` scripts and a focused config.
  Service typecheck/lint, production bundle, production dependency audit,
  `./scripts/check-dev.sh --quiet`, and scoped diff checks passed. The direct
  entry-point typecheck retains exactly the same eight baseline diagnostics.
- Updated module guidance and development instructions. Tests mock VS Code and
  configuration APIs; actual CLI execution, filesystem edits, full activation,
  and rendered/interactive terminal checks remain unverified.
- Next increment: interactive terminal ownership/lifecycle extraction within
  step 4, then the transport/UI infrastructure reuse phase.

### Step 4d: interactive terminal ownership and launch lifecycle

- Reduced the entry point from 2,618 to 2,552 lines by extracting
  `services/interactiveTerminal.ts`; the entry point supplies live
  environment/config/context/trust/error callbacks and owns service disposal.
  Retained terminal name, cwd, icon, reuse, delayed launch, and command formatting.
- Fixed delayed work surviving a terminal close or extension shutdown. Session
  identity prevents an in-flight context flush from sending into a closed terminal
  or affecting its replacement. Trust is checked before and after the flush;
  flush failures reach the command error handler instead of becoming unhandled
  promise rejections. Disposal cancels the timer and closes the terminal once.
- Added nine focused lifecycle tests. All 25 combined view/service/terminal tests
  pass. Tests use mock terminal APIs and a fake clock; real host profiles and
  shell interaction remain unverified. Existing shell formatting is preserved;
  a separate shell-launch hardening increment needs profile-specific coverage.
- Service typecheck, service lint, production bundle, repository fast gate,
  Markdown lint, and scoped diff checks passed. AST comparison confirmed 68
  retained or moved functions; the entry-point typecheck and lint retain their
  eight and 21 baseline diagnostics respectively.
- Next increment: assess shell-safe interactive launching, then step 5 transport
  framing and UI protocol reuse. No production dependencies added.

### Step 4e: native interactive CLI launch

- Removed shell command interpolation and the activation timer from the terminal
  service. Executable and config paths containing shell operators are now forwarded
  as `TerminalOptions.shellPath` and an argument array as `shellArgs`.
- The command awaits IDE context flush before creation. Concurrent requests share
  the pending session; trust revocation and shutdown cancel pending creation,
  failures release the session for retry, and close/reopen ownership is preserved.
- Documented the intentional environment change: the CLI is the terminal process,
  so shell profiles, aliases, and virtual-environment activation commands are not
  evaluated first. VS Code's inherited/configured terminal environment still applies;
  exiting VT Code ends the terminal process instead of returning to a shell prompt.
- All 28 combined Node tests pass, including 12 terminal cases. Adversarial cases
  assert executable and argv values containing shell operators, substitutions,
  quotes, newlines, Unicode, and empty arguments. A real Node process independently
  verifies forwarded arguments. No shell-text fallback exists.
- Service typecheck/lint, production dependency audit, production bundle,
  repository fast gate, scoped Markdown lint, and staged diff checks passed.
  Retained entry-point bodies and baseline typecheck/lint diagnostics were compared;
  the existing eight typecheck and 21 lint diagnostics remain unchanged.
- Native VS Code terminal behavior, Windows quoting, and full activation require
  host checks and are not claimed tested. No production dependencies added.
- Next increment: step 5 transport framing and UI protocol reuse.

### Step 5a: shared bounded subprocess line framing

- Added `vtcode-commons::line_framing` as the single bounded byte-framing
  implementation. ACP and Copilot retain their local adapters and error shapes;
  ACP explicitly excludes LF from the cap, while Copilot includes it. CR remains
  content. Copilot server-client headers also use the shared reader through the
  existing Copilot adapter. No production dependencies or crate boundaries changed.
- Retained drain-to-LF/EOF behavior, zero-cap and empty-line handling, final partial
  lines, byte-oriented truncation, and caller-buffer reuse. Pending-call guards,
  write-queue backpressure, routing, JSON-RPC IDs, and child teardown are untouched.
- Added four commons tests and two adapter regressions. An independent full-input
  oracle checks both policies across five inputs, nine caps, and nine chunk sizes;
  explicit cases cover exact caps, CRLF, oversized final lines, and partial-read
  errors. All 43 focused framing/transport/Copilot-client tests passed with the
  Copilot feature enabled, including timeout, EOF, and prompt cancellation cases.
- Warnings-denied locked checks for all three affected crates, the repository
  fast gate, formatting, scoped Markdown lint, and staged diff checks passed.
  Updated affected module guidance and documented the delimiter/error/cancellation
  contracts in `docs/development/stdio-line-framing.md` and the async guide.
- The reader is not cancellation-safe; its public contract requires callers to
  terminate on read failure rather than assuming a fresh frame boundary. Existing
  transport reader ownership is preserved. Live external ACP/Copilot subprocesses
  were not exercised; verification used unit and in-memory transport fixtures.
- Next increment: independently map and extract shared UI protocol payloads.

### Step 5b: shared UI text-command payloads and send methods

- Mapped the core and app protocols. Styled segments, link ranges, message kinds,
  and submitted input already have shared owners; app captures, palettes, and
  transient/deferred input remain app-owned. A full handle merge is unnecessary.
- Centralized the four identical text-command variant definitions and five send
  methods in private `types/message_commands.rs` macros. This preserves public
  facade imports, existing struct-variant construction/match syntax, field types,
  variant order, method signatures, and each handle's local send/state ownership.
  Named tuple payloads would require migrating many existing match sites without
  changing behavior, so this increment shares the definitions directly instead.
- Added independent payload checks for both public handles, including ordered
  styled segments, supplied pasted-line metadata, asymmetric replacement counts,
  empty rows, link byte ranges/targets, and `None` versus `Some(empty)` links.
  All ten focused protocol tests passed, followed by all 1,479 UI tests with no
  skips. Existing transient/deferred input and evidence-navigation tests pass.
- Reconstructed both protocol sources from the shared definitions and compared
  normalized Rust tokens against HEAD, preserving literals/comments and ignoring
  whitespace and optional trailing argument commas. Retained logic and expanded
  variant order match; only unused imports and macro wiring change outside the
  extraction. No production dependencies or runtime state added.
- Repository fast gate, scoped Markdown lint, formatting, and staged diff checks
  passed. Updated UI module guidance and added the shared message-protocol guide.
  Live terminal visual interaction was not exercised.
- Preserved the unrelated Merge Gateway changes present before this increment.
- Next increment: assess remaining common control commands before the execution
  facade phase; keep app-only state and event ownership explicit.

### Step 5c: common UI control-method forwarding

- Used AST function matches to compare core/app handle methods and extracted 36
  identical methods into private `types/control_commands.rs`. Both handles invoke
  the shared macro and retain their original channel/send implementation.
- Kept control variant definitions in the local enums to preserve their existing
  interleaving with app-only commands. Kept message-label setters local because
  the app setter updates Unicode display-width state, styled-placeholder helpers
  because visibility differs, and overlay/session methods because ownership differs.
- Added nine tests across both public handles for lifecycle order, both toggle
  values, asymmetric statuses/queues, draft attachments and batching metadata,
  plus the app's label-width/reset side effects. All 19 focused protocol tests
  passed, followed by all 1,488 UI tests with no skips.
- Verified all 36 generated method bodies/signatures against both AST-captured
  originals. Normalized retained protocol tokens and enum order match HEAD;
  comparison preserves literals/comments and ignores whitespace and optional
  trailing argument commas. No public API, runtime state, or dependency changes.
- Repository fast gate, formatting, scoped Markdown lint, and staged diff checks
  passed. Updated the shared-protocol guide and UI module guidance. Live terminal
  visual interaction was not exercised.
- Next increment: step 6 execution-facade reentrancy extraction, preserving
  admission/safety order and structured error contracts.

### Step 6a: execution reentrancy ownership

- Moved recursion tracking and frame cleanup into private registry
  `reentrancy.rs`, reducing `execution_facade.rs` from 1,984 to 1,835 lines.
  Kept the guard's execution call site, prevalidated parallel-safe allowance,
  admission/safety ordering, error payloads, and execution history unchanged.
- Compared the extracted guard and retained facade against HEAD after
  normalizing extraction-only imports, private visibility, standard Result
  qualification, and formatting. Implementation and retained execution logic
  match. No public API or dependency changes.
- Added eight guard tests covering ordered cycle traces, the 64-frame boundary
  for parallel siblings and distinct tools, out-of-order drops, thread unwind,
  isolation between tasks on one thread, moved task guards, and cancellation.
  All 94 selected guard and registry tests passed, including both existing
  public recursive-tool regressions; 3,965 unrelated tests were excluded.
- Updated core module guidance and added the tool-reentrancy development guide.
- Repository fast gate (formatting, warnings-denied Clippy, compilation, shell
  lint) and scoped Markdown lint passed. Tests ran separately through nextest.
- Next increment: extract contiguous execution stages while retaining one
  authoritative admission and settlement path.

### Step 6b: structured-request attempt lifecycle

- Moved request safety admission, denial decoration, retry scheduling, and
  outcome construction into private registry `execution_attempts.rs`. The
  execution facade is now 1,576 lines, down from 1,835 after step 6a.
- Public request wrappers retain their signatures and snapshot construction.
  The prepared wrapper still requires fresh safety admission. Harness dispatch
  stays in the facade with sibling-only visibility; its routing authority,
  prevalidation, settlement mode, snapshots, timeout annotations, and handler
  history remain unchanged. No new execution path or dependency was introduced.
- Captured all five moved functions with ast-grep and compared their token
  streams against the originals, preserving literals and comments. Retained
  facade logic also matches after extraction-only visibility/import/format
  normalization.
- Added seven tests for first-attempt output, transient recovery, exact retry
  exhaustion, dispatch failure, denial category/retry hints, safety rejection
  before handler execution, and approval-required shell requests even under
  prevalidated flags. All 489 selected registry tests passed; 3,577 unrelated
  tests were excluded. Existing timeout, policy, and reentrancy regressions pass.
- Updated core module guidance and added the execution-pipeline ownership guide.
- Repository fast gate, final locked warnings-denied Clippy for the standard
  default members, final formatting, and scoped Markdown lint passed.
  The seven focused tests passed again after changing denial fixtures to named
  cases. Standalone core-only all-target/all-feature warnings-denied Clippy
  reports 15 `large_futures` findings in `compaction/tests.rs`; the identical
  command reproduces all 15 at baseline `db9071fe9` with matching local Cargo
  settings. These pre-existing findings are outside this extraction's scope.
- Next increment: assess the remaining execution preparation and routing stages
  against existing kernel/stage helpers before extracting another coherent block.

### Step 6c: shared execution preparation and base routing

- Wired the facade to the existing canonical/display-name and base-route
  helpers in `execution_stages.rs`, removing their duplicated implementations.
  Added a private typed argument-preparation result for normalized handler
  arguments, preview budget, and verification classification. The facade is
  now 1,529 lines, down from 1,576 after step 6b.
- Name resolution still precedes canonical hot-cache lookup. Normalization,
  verification classification, and preview-budget resolution still precede
  metadata stripping and facade preflight. Base route resolution stays after policy
  constraints; awaited MCP discovery, errors, history, and PTY ownership remain
  facade-owned. Metadata-aware handlers retain borrowed or owned normalized
  payloads without an extra clone. Public APIs and dependencies are unchanged.
- Compared all four existing public stage-method token streams against their
  originals. Retained facade tokens match after excluding only the three stage
  call sites and the parser import change; literals and comments were preserved.
- Replaced two tests that only constructed result structs with eight tests of
  registered aliases, standard/PTY/canonical MCP routing, canonical cache keys,
  handler payloads, borrowed/owned normalization, planning/verifier budgets,
  malformed metadata rejection, and normalization without execution. All eight
  focused tests passed, then all 499 selected registry/output-limit tests passed;
  3,573 unrelated tests were excluded.
- Updated core module guidance and the execution-pipeline ownership guide.
- Repository fast gate, formatting, scoped Markdown lint, and staged diff
  checks passed. Tests ran separately through locked nextest.
- Next increment: review the remaining awaited MCP discovery and route-error
  adaptation as a coherent stage, retaining its policy and history boundaries.

### Step 6d: awaited MCP route discovery

- Moved awaited MCP lookup into the existing execution stages module, with a
  private typed result carrying route metadata and the original lookup error.
  The facade is now 1,494 lines, down from 1,529 after step 6c.
- Discovery remains after policy constraints and before unknown-tool handling,
  circuit-breaker checks, and PTY acquisition. Canonical MCP routes skip remote
  discovery. Negative or failed discovery preserves registered standard tools.
  Alias normalization, lookup logging, and provider lookup retain their order.
- Error payloads, history, timeout metadata, policy admission, and execution
  remain facade-owned. Public APIs and dependencies are unchanged.
- Compared moved discovery tokens after identifier adaptation and retained
  facade tokens after the call-site/import changes; both match the originals.
- Added four regressions covering missing/disabled clients, disconnected lookup
  failures, standard public execution after remote failure, unknown-tool error
  payload/history, and canonical MCP bypass. Live connected-provider discovery
  is not exercised by these new tests.
- All 499 selected registry tests passed; 3,577 unrelated tests were excluded.
  The final repository fast gate, scoped Markdown lint, and diff checks passed.
- Updated core module guidance and the pipeline ownership guide.
- Next increment: map handler execution and settlement ownership before choosing
  another extraction; keep PTY permits, snapshot lifetime, and history together.

### Step 6e: registered-handler dispatch

- Mapped execution ownership and extracted registered-handler dispatch into
  private `execution_handlers.rs`. The facade is now 1,458 lines, down from
  1,494 after step 6d; the new production module is 63 lines.
- The helper owns deprecation warnings, function-handler dispatch with optional
  memory-pool retrieval, and trait-object cache selection/insertion. It executes
  inside the original outer timeout and PTY permit lifetime. It grants no
  admission and does not normalize output or record history.
- MCP and noninteractive command routing, settlement mode, middleware, fresh-read
  nonce, snapshots, timeout/breaker handling, output processing, and history stay
  in the facade. Public APIs and dependencies are unchanged.
- Moved handler tokens match after cache-reference and canonical-name ownership
  adaptations; retained facade tokens match after the call-site/import changes.
- Added six regressions with distinct cached/registered instances, both
  optimization/pool settings, canonical cache keys, raw error chains, public
  alias payload/history, and cancellation followed by cached retry. Cancellation
  verifies handler drop, PTY permit release, and absence of a completed history
  record before retry. It uses instance-owned signals and counters.
- All 505 selected registry tests passed; 3,577 unrelated tests were excluded.
  Final fast gate, scoped Markdown lint, and diff checks passed. The cancellation
  fixture verifies permit accounting without launching a host PTY subprocess.
- Updated core module guidance and the execution-pipeline ownership guide.
- Next increment: review success-result processing and history invalidation for
  shared, focused boundaries before moving any broader settlement state.

### Step 6f: completed-output preparation and read invalidation

- Extracted output preparation and mutation-target invalidation into private
  `execution_results.rs`. The facade is now 1,382 lines, down from 1,458 after
  step 6e; the new production module is 118 lines.
- Output preparation retains the object/scalar/array spool-inspection guard,
  existing output processor, normalization, code-search response shape, and
  structured-error interpretation. A private typed result carries normalized
  output and error evidence together without a broad execution context object.
- Mutation invalidation still runs only for calls classified as mutating, after
  output preparation and before history recording. Targeted mutations retain
  unrelated read records; pathless command mutations clear file reads only.
  Structured failures returned inside successful handler futures retain the
  same invalidation and failure-history behavior.
- Breakers, latency/adaptive timeout accounting, middleware, history recording,
  and execution/PTY lifetime remain facade-owned. APIs and dependencies are
  unchanged.
- Original helper tokens, moved output/invalidation tokens, and retained facade
  tokens match after argument ownership and call-site/import adaptations.
- Six focused regressions passed, covering success and structured error shapes,
  code-search errors, nested-spool prevention with a real-spool control,
  source/destination invalidation, pathless-command versus unrelated mutation,
  and public structured-failure history. Updated core guidance and pipeline docs.
- All 511 selected registry tests passed; 3,577 unrelated tests were excluded.
  Final fast gate, scoped Markdown lint, and diff checks passed. New spool tests
  exercise predicate/output storage behavior without launching shell commands.
- Next increment: assess the remaining facade admission and settlement blocks
  against the scan plan; avoid moving tightly coupled state just to reduce lines.

### Step 6 closure: retained admission and settlement ownership

- Reviewed the remaining facade blocks after step 6f. Admission depends on the
  same classified intent, prevalidation/dispatch authority, snapshots, recovery
  state, policy constraints, and history closure. Settlement shares PTY lifetime,
  middleware request state, timeout/breaker metadata, and history recording.
- Keep these blocks together for now; splitting them would require moving a
  broad execution context or distributing ordering-sensitive error recording.
  Phase 6 is complete at this maintainable boundary, with the facade reduced
  from 1,984 to 1,382 lines across its validated increments. This is a scoped
  conclusion, not a claim that every future refactor opportunity is exhausted.

### Step 7a: Copilot prompt-stream adapter

- Mapped runtime host, permission/admission, observed-call presentation, local
  terminal sessions, prompt streaming, and request-renderer lifecycle ownership
  in the [Copilot runtime guide](copilot-runtime-ownership.md).
- Extracted prompt-session conversion into private
  `copilot_runtime/streaming.rs`, retaining a synchronous facade entrypoint and
  unchanged return shape. Runtime host source is now 1,984 lines, down from
  2,139; the new production adapter is 174 lines.
- Prompt cancellation guard, channel-close handling, queued-update draining,
  reasoning accumulation, and completion mapping moved verbatim. Permission,
  budget, verification, tool/terminal execution, and harness-event ownership
  remain in the host; runtime requests stay on their separate receiver.
- Moved stream/helper tokens and retained host tokens match after module/import
  and entrypoint delegation changes. No APIs or dependencies were added.
- Relocated three existing conversion tests into the private child suite and
  added three tests of Unicode/paragraph/punctuation boundaries, asymmetric
  chunk accumulation, whitespace, and unknown finish reasons.
- All 24 focused binary runtime tests passed. The broader locked selection
  passed all 90 runtime, provider cancel-handle, and inline-events tests; 4,634
  unrelated tests were excluded. Fast gate, scoped Markdown lint, and diff
  checks passed. Direct prompt-stream cancellation/completion races and a live
  Copilot provider remain outside this validation.
- Corrected the root guideline to use test(inline_events): the former binary
  filter matched no binary and aborted before running tests.
- Next increment: map local terminal task/association/exit helpers before
  extracting them; preserve host drop, release/kill/wait, and event ordering.

### Step 7b: Copilot local terminal lifecycle

- Mapped terminal create/output/release/kill/wait, monitor polling, session state,
  observed-call binding, inline stream lifetime, and host-drop paths. Extracted
  them into private `copilot_runtime/terminal.rs`; the host is now 1,474 lines,
  down from 1,984 after step 7a. The terminal module is 568 production lines.
- The host retains the session map, request dispatcher, observed-call event
  emission, permission/budget/verification gates, and its original drop loop.
  Session state/task fields stay private. Only parent-used handles, binding
  fields, and methods are sibling-visible; no public API/dependency was added.
- Kept registry harness launch/output/close/termination paths, literal argv/env
  mapping, polling cadence, output-byte limits, release notification/task abort,
  and incomplete-terminal presentation. Promoted output, completion, and stream
  setup tuples to named private records without changing their contents/order.
- Compared all six moved sections against the mechanical extraction with private
  visibility changes; retained host tokens match after module/import wiring and
  import ordering. Reviewed the three named-record adapters separately.
- Seven focused tests passed: UTF-8 cap boundaries including zero/unbounded,
  late binding after exit, association enrichment without identity replacement,
  accumulated output/completion ordering, one-time completion, preexisting and
  pending wait notification, release/abort monitor drop, exit-code range checks,
  and literal metacharacters in argv. Existing host real-PTY coverage is retained.
- All 96 selected binary runtime/inline-events tests passed; 3,434 unrelated
  tests were excluded. Final fast gate, scoped Markdown lint, and diff checks
  passed. The existing real-PTY test ran on this host; live Copilot CLI/provider
  behavior and other host platforms remain outside this validation.
- Updated the binary module guidance and runtime ownership guide.
- Next increment: inspect observed-call presentation ownership and remaining
  Copilot adapters before moving to session orchestration.

### Step 7c: shared Copilot inline PTY presentation

- Replaced the duplicate observed-call and local-terminal presentation wrappers
  with private `copilot_runtime/presentation.rs`. Both live paths use the same
  prepared reporter, spinner setup, output callback, and scheduled shutdown.
  The host source is now 1,429 lines, down from 1,474; the terminal source is
  518 lines, and the shared helper contains 63 production lines.
- Observed calls retain their state, Unicode output deltas, event ordering, and
  status mapping. Local terminals retain progress initialization, the elapsed
  guard, monitor cadence, and warning presentation when no exit is observed.
  Shared finish stops the spinner and drops the callback before scheduling
  progress completion and PTY shutdown. No public API or dependency was added.
- Shared setup/shutdown tokens match the original observed implementation after
  member-name and final-color adapters. Host methods/drop, terminal handlers and
  state, and terminal argument/output/exit helpers match their original tokens.
  Reviewed monitor/setup wiring and caller-owned status mapping separately.
- Three focused presentation tests passed. They verify ordered Unicode chunks
  queued before finish, success/error/warning colors, prepared progress through
  completion, bounded tails, empty output, deferred status restoration, and
  handle release after both completion and dropping a started PTY worker.
- All 99 selected binary runtime/inline-events tests passed; 3,434 unrelated
  tests were excluded. Final fast gate, scoped Markdown lint, and diff checks
  passed. The retained real-PTY host regression ran; live Copilot provider
  behavior and other host platforms remain outside this validation.
- Updated the binary module guidance and runtime ownership guide.
- Next increment: inspect the remaining observed-call adapter boundary and
  session orchestration ownership before choosing the next extraction.

### Step 7d: private Copilot observed-call state

- Moved observed-call transitions, display extraction, UTF-8 output deltas, and
  status colors into private `copilot_runtime/observed.rs`. State fields stay
  private; the host uses a name accessor and update method that borrows PTY
  configuration rather than the complete registry. The host is now 1,282 lines,
  down from 1,429; the observed adapter has 159 production lines.
- Named the returned cumulative output `output_snapshot` to distinguish harness
  event payloads from the incremental delta sent to shared PTY presentation.
  Local terminal association, host event order, failure accounting, permissions,
  budgets, and request dispatch remain with their existing owners.
- Transition and command/status/UTF-8 helper tokens match the original after
  receiver/config/result-field adapters and rustfmt trailing-comma normalization.
- Four new tests passed: Unicode append/rewrite/shrink boundaries, canonical
  command aliases and display fallbacks, placeholder-name enrichment, blank and
  repeated output suppression, full snapshots versus one-time PTY deltas, and
  one-shot success/failure presentation. Public host entrypoint tests remain.
- All 103 selected binary runtime/inline-events tests passed; 3,434 unrelated
  tests were excluded. Final fast gate, scoped Markdown lint, and diff checks
  passed. Live Copilot provider behavior and other platforms remain untested.
- Copilot decomposition closes at this boundary. Remaining host work shares
  permissions, budgets, hooks, tool execution, and event ownership; no new host
  or broad execution context is warranted. Next: reuse the existing session
  bootstrap module for thread/archive preparation in the orchestration loop.

### Step 7e: session thread/archive preparation

- Moved fresh/resume/fork thread preparation into the existing private
  `orchestration/session_bootstrap.rs`, returning a named identity/bootstrap/
  optional-archive record. The loop retains metadata/history policy, thread
  activation, runtime-ID publication, startup checkpointing, and UI/harness
  lifecycle. Orchestration source is now 2,599 lines, down from 2,664; the
  bootstrap helper contains 284 production lines.
- Replaced two summarized-fork history calls with one common call after identity
  and archive preparation. Preserve provider construction only for summarized
  forks, source/target/history and budget-continuation arguments, archive policy,
  reserved identity handling, and error propagation before thread activation.
- Both original summary argument sets match the shared call. The orchestration
  prefix/tail outside preparation and existing bootstrap helpers match original
  tokens after the pure thread-manager construction relocation.
- Four new bootstrap tests cover reserved/generated archive-less identity,
  resume identity/history/cache lineage/continuation under both archive policies,
  persisted startup checkpoint contents, archive-less full-copy fork metadata,
  and provider errors before archive preparation. Fixtures use owned temporary
  paths and explicit policy inputs; no global environment/state is changed.
- The first compile exposed a mistaken test module path, and the first focused
  run exposed a mistaken generated-ID prefix in the fixture. Corrected both from
  the actual module wiring and workspace-label contract. All 201 selected
  lifecycle, summarized-fork, archived-session, Copilot runtime, and inline-event
  tests then passed; 7,430 unrelated tests were excluded. Final fast gate, scoped
  Markdown lint, and diff checks passed.
- Added the [session ownership guide](session-orchestration-ownership.md) and
  updated binary module guidance. Successful compaction and archive-backed forks
  are covered by their existing owner tests separately; live provider startup
  and interactive new-session/resume behavior remain outside this validation.
- Next increment: inspect turn/input coordination and ordered finalization for
  useful existing-helper boundaries, then proceed to release changelog helpers.

### Step 7f: shared reload polling and orchestration closure

- Replaced the two identical config-reload blocks in session finalization and
  new-session handoff with the existing bootstrap module's private polling
  helper. Preserve watcher/debounce behavior, runtime provider and CLI model
  overrides, rejected-config warnings, renderer errors, both call positions,
  and their distinct diagnostic messages. Orchestration source is 2,587 lines;
  the bootstrap source is 310 lines including its test declaration.
- Reversing only the two helper calls reproduces the original orchestration
  source exactly. No state machine, public API, dependency, or transition
  policy was added. Tests use owned config files and distinct explicit mtimes.
- Two new tests verify unchanged polls, rejected reload retaining prior UI
  configuration with one warning, correction recovery, and CLI override
  preservation. All six bootstrap tests passed; all 199 selected binary
  lifecycle, summarized-fork, runtime, and inline-event tests passed, with 3,344
  unrelated tests excluded. Final fast gate, Markdown lint, and diff checks
  passed. Interactive session handoff and other hosts remain untested.
- Reviewed the remaining input/turn coordination and session tail. They share
  runtime/steering history, primary-agent handoffs, persistence outcomes,
  recovery counters, and ordered shutdown. Existing interaction, turn-tail,
  bootstrap, and teardown helpers already own separable phases. Retain the
  remaining loop ownership rather than introducing a broad context bag or a
  second runner. Phase 7 closes at this boundary; further splits need a concrete
  independent responsibility and lifecycle evidence.
- Next: phase 8, extract release changelog formatting and share only helpers
  whose canonical and legacy contracts match. Keep publication entrypoints out
  of fixture checks.

### Step 8a: release changelog formatting and shared classification

- Extracted canonical author mapping/tags, category formatting, grouped notes,
  and contributors into sourceable `scripts/release-changelog.sh` (296 lines).
  The release entrypoint is now 1,359 lines. Shared conventional type parsing
  and subject exclusion live in `release-changelog-common.sh` (24 lines), used
  by both the canonical formatter and existing legacy release library.
- Preserve distinct canonical/legacy titles, username mapping, category layout,
  and CI-marker cleanup rather than normalizing incompatible contracts. File
  updates, versioning, packaging, tags, publishing, and Homebrew remain in their
  existing owners. No new dependency was added.
- A real Git fixture exposed a final-record bug: `--pretty=format` omits the
  last newline, so canonical commit, contributor, and author-mapping readers
  skipped the oldest record. Fixed all three readers to accept partial final
  records; a single-commit range now produces notes and its contributor/tag.
- Moved function bodies match the originals after exclusion delegation and
  these three EOF fixes. The remaining entrypoint is byte-identical after
  source wiring. Existing release scripts have no new ShellCheck diagnostics;
  their one/eight prior diagnostics remain outside this increment.
- All 26 owned Git-fixture checks passed: exact category/history order and
  output, aliases, contributor deduplication, bot exclusions, subject filtering,
  empty ranges, final-record author mapping, distinct legacy contracts, live
  legacy classifier wiring, and safe entrypoint help. HEAD/worktree/tags remain
  unchanged by formatting, and publication commands are instrumented to fail.
- Bash syntax, strict ShellCheck for the new modules/test, formatting, scoped
  Markdown lint, diff checks, and the existing asset/signing fixture suites
  passed. Full release dry-run orchestration and
  publication were not run. Rust verification was completed in step 7f; this
  script/docs increment does not require another Rust rebuild.
- Added the [changelog ownership guide](release-changelog-ownership.md) and
  linked it from the CI/release guide. Next: share the identical file-insertion
  helper with fixture checks before closing the release decomposition boundary.

### Step 8b: shared changelog artifact insertion

- Removed the byte-identical `insert_changelog_entry` implementations from both
  release owners. Their existing shared changelog module now owns classification
  and insertion (56 lines), keeping the same function name and cwd/file contract.
  The main release script is 1,329 lines; the legacy library is 747 lines.
- An insertion fixture exposed macOS `head -n 0` failure for a headerless file
  whose first version starts on line 1. Skip the empty prefix in that case;
  retain header/no-version handling, literal `%s` entry writes, newest-first
  version ordering, and existing content. No publication operation was added.
- Both release owners match their previous source after only insertion removal.
  The shared implementation matches its original body after the empty-prefix
  fix. No dependency, wrapper, separate publishing runner, or API was added.
- All 31 changelog checks passed, adding empty/header-only/headerless insertion,
  repeated newest-first updates without splitting previous bodies, nested-heading
  preservation, literal percent/shell-like content, and live legacy-caller wiring.
  Bash syntax, strict new-module/test ShellCheck, formatting, scoped Markdown
  lint, and diff checks passed. Publication and full release orchestration were
  not exercised; this increment only updates owned temporary fixture artifacts.
- Updated the ownership guide. Phase 8 closes at shared changelog formatting,
  classification, and insertion. Keep version/git-cliff orchestration, packaging,
  signing, upload, and Homebrew side effects with existing owners: those paths
  require their release contracts and target matrix, not clone-window similarity,
  to justify further sharing. Existing asset/signing helpers already own those
  separable infrastructure contracts.
- Next: audit the completed eight-phase plan against the current checkout and
  identify any remaining implementation or verification requirements before
  claiming the overall refactor objective is complete.

## Eight-phase implementation audit (2026-10-05)

All eight phases have implemented, committed increments. The audit confirms
that their extracted owners are called through the existing facades or app
entrypoints. The original CSV remains a dated scan baseline. Current line counts
at this audit snapshot include module declarations and tests; they measure navigation changes,
not removed behavior or reduced runtime cost.

| Phase | Current ownership and live wiring | Implementation boundary |
| ----- | --------------------------------- | ----------------------- |
| 1 | Both OpenCode adapters call `opencode_shared::validate_normalized_request`; UI constants re-export config defaults. | Shared behavior has one existing owner; provider allowlists and UI-only values stay local. |
| 2 | `providers/common.rs` (1,766 → 66 lines) and `shared/mod.rs` (1,832 → 39) declare private helpers and retain their facade exports. | HTTP, wire mapping, reasoning, cache, replay, stream assembly, SSE, and UTF-8 helpers have cohesive owners. |
| 3 | `planning_workflow/artifacts.rs` (2,071 → 26) declares parser/tracker/validation modules and re-exports the original entrypoints. | `validate_plan_content` still owns acceptance and repair feedback through its existing facade. |
| 4 | WebMCP `main.ts` (1,846 → 1,533) constructs both controllers; VS Code `extension.ts` (3,534 → 2,556) registers extracted views/config commands and constructs the native terminal service. | Activation, live backend replacement, trust, proposal authority, and disposal stay with their original owners. |
| 5 | ACP and Copilot adapters call commons `read_bounded_line` with their respective LF policies; both UI handles invoke the shared message/control macros. | Transport lifecycle, adapter errors, local channel state, and app-only side effects stay local. |
| 6 | `execution_facade.rs` (1,984 → 1,382) invokes reentrancy, request attempts, argument preparation, route discovery, handler dispatch, output preparation, and read invalidation. | Coupled admission/settlement ordering stays in one facade; no second execution path. |
| 7 | `copilot_runtime.rs` (2,139 → 1,282) uses private stream/terminal/presentation/observed owners; orchestration (2,664 → 2,587) calls thread preparation and shared reload polling. | Permission/event accounting and the remaining history, steering, persistence, and ordered shutdown stay with one host/loop. |
| 8 | `release.sh` (1,675 → 1,329) sources canonical changelog helpers; both release owners source shared classification/insertion. | Distinct legacy formatting and publication/version/package/signing side effects retain their existing owners. |

### Audit correction and current validation

- Moved the external test declarations in the provider common and planning
  artifact facades after all production items, applying the root test-layout
  rule to earlier extractions. Their sources match HEAD after removing only
  those declarations; test files, public APIs, visibility, and behavior are unchanged.
- Checked 14 external test declarations in the affected provider, planning,
  execution, Copilot, and orchestration areas: all end their owning module.
  Root guidance remains 149 lines; audited crate/app guidance stays below 30.
  All 69 local links across the nine affected instruction files resolve.
  Existing guidance covers this correction; no additional module rule is needed.
- Current locked nextest selection for provider common, planning workflow, and
  planning tracker passed 144 tests; 5,145 unrelated tests were excluded.
  Earlier per-increment broader crate/lifecycle evidence remains recorded above.
- Final `./scripts/check-dev.sh --quiet` passed formatting, warnings-denied
  Clippy, compilation, and shell lint. Scoped Markdown lint and diff checks passed.
- Re-ran WebMCP typecheck, all 66 tests, and production build; all passed.
  Vite retains its existing bundle-size warning. Re-ran both VS Code focused
  typechecks, all 28 view/service/terminal tests, and production bundle; all passed.
  These commands compile real modules but use mock VS Code APIs.
- Re-ran all 31 changelog checks, release-asset fixtures, and all 24 macOS
  signing fixtures; all passed with publication/signing operations stubbed.

### Remaining acceptance checks

Implementation closure and overall acceptance are separate. The following
originally planned host checks retain explicit verification limits:

- **WebMCP rendered behavior:** settings/evidence dialogs, keyboard focus,
  draft review, connection/reconnection, and terminal-authoritative approval.
  The current Browser Use runtime has no `agent` binding and Computer Use
  reports no browser providers. No rendered browser result is claimed.
- **VS Code host behavior:** activation and rendered view refresh, native
  terminal launch/reuse/close/shutdown, trusted/untrusted transitions, and
  literal argv through the actual host. Mock fixtures and a real Node child
  establish service contracts, not VS Code terminal integration or Windows quoting.
  An isolated macOS VS Code 1.140.0 development host activated the production
  bundle in an owned temporary workspace. Its output log reported that the
  extension's workspace-trust request accesses the unavailable `workspaceTrust`
  API proposal. This is a confirmed follow-up finding, not a rendered pass.
  Native automation selected the existing editor instance rather than this host;
  the owner then requested skipping VS Code Computer Use. Further rendered VS
  Code checks are deferred at the owner's request; keep this limit explicit.
- **TUI host behavior:** live wide/narrow terminal interactions for the shared
  protocol consumers. The recorded UI/PTY/inline-event suites cover automated
  regressions; they do not establish visual acceptance in a real session.

The existing eight entry-point typecheck and 21 lint diagnostics in the VS Code
extension, standalone core-only Clippy findings, live provider/subprocess races,
and release target-matrix/publication checks retain their documented boundaries.
No full workspace test pass or full release execution is claimed. Further
extraction of the retained coupled runtime/release blocks requires new evidence
of an independent responsibility rather than a file-length target.

Next: fix the confirmed workspace-trust proposal access in source with focused
service regressions, without VS Code Computer Use. Browser/TUI rendered checks
remain open when an appropriate isolated test surface becomes available.
The eight-phase implementation plan is delivered; overall acceptance remains open.

### Follow-up: shared stable workspace-trust flow

- Replaced both optional proposed-API request implementations and repeated
  manual management logic with `services/workspaceTrust.ts` (23 lines).
  Activation, command admission, and the modular trust command now use the
  existing stable dialogs and `workbench.action.manageTrust`, then read
  `workspace.isTrusted`. Settings navigation and command return values never
  establish approval. Activation retains its one-time prompt state and logging;
  UI/management failures propagate to existing caller error handling.
- The registered trust command inherited `BaseCommand.canExecute`, which
  rejects restricted workspaces before the trust flow runs. Override that gate
  only for trust management; ordinary execution commands retain their gate.
  Retain the command signatures with intentionally unused context parameters.
  No execution authority, API proposal, dependency, or automatic trust grant was added.
- The extension entrypoint is now 2,494 lines. The trust command is 40 lines;
  its existing IDs, manual-management messages, and granted-state feedback stay
  with the command. Added owner guidance and development/command documentation.
- Eight new regressions pass with a proposed-API getter that throws if accessed:
  already-trusted state, literal warning prompts, actual grant versus settings
  navigation, dismissal, delayed host-state changes, original error identity,
  registered trust admission versus blocked execution, and no false grant feedback.
  All 36 view/service/terminal/trust tests passed without skips, alongside both
  focused typechecks, changed-module lint, and the production bundle.
- Direct entrypoint typecheck improved from eight existing diagnostics to seven;
  no diagnostic was added. An isolated baseline source copy reproduces the same
  21 entrypoint lint diagnostics as the final source. The full-project checks
  remain outside this increment's passed checks. Scoped Markdown/diff checks
  passed. Rust sources are unchanged since the audit's successful fast gate.
- VS Code Computer Use remains skipped at the owner's request. The source-level
  finding is fixed and covered by service/registered-command regressions; no
  post-fix rendered host, terminal, or Windows result is claimed.

Next: remaining acceptance is browser/TUI rendered validation when an isolated
surface is available. Keep VS Code Computer Use deferred unless the owner changes
that instruction; no further coupled-block extraction is justified by this audit.

### Follow-up: core modal painting and isolated protocol fixture

- Added `vtcode-ui/examples/inline_protocol_smoke.rs`, exercising both public
  handles and event enums without providers or command execution. The shared
  fixture uses distinct Unicode rows, streaming, literal multiline paste,
  ordered replacement, links, and modal dismissal. Stop/resume/start follows
  the existing external-editor lifecycle contract.
- The fixture exposed a blank core frame at both 120×40 and 44×18: the shared
  modal renderer cleared the viewport before checking for an active overlay.
  Guard painting with the authoritative overlay state and clear only stale hit
  areas when absent. Both new full-session regressions failed before the fix
  and passed afterward, including transcript/input/status restoration after Esc.
- Moved the existing 15 modal-renderer tests into a private child module and
  added the two frame regressions. Production source shrank from 746 to 582
  lines without widening visibility, changing APIs, or adding dependencies.
  Recorded the unique rendering invariant in crate guidance and documented the
  fixture in the message-protocol guide.
- The complete UI suite passed: 1,490 tests, none skipped. All 17 focused
  modal-renderer tests passed, with 1,473 excluded by that filter. Example build
  and warnings-denied Clippy passed. Post-fix core PTYs at 120×40 and 44×18,
  plus app at 44×18, processed fixture commands and restored transcript output
  after Esc; all exited zero with terminal teardown sequences. The fast gate,
  locked UI check with `RUSTFLAGS='-D warnings'`, and scoped Markdown/diff checks
  passed. The fast gate did not run tests; the separate UI suite did.
- Native Computer Use refused Ghostty access for safety reasons; Browser Use
  has no agent runtime and Computer Use has no browser provider. PTY bytes and
  buffered frames do not establish rendered host acceptance, mouse selection,
  live resize, or browser integration. Those checks remain open. VS Code
  Computer Use remains skipped at the owner's request.

### Follow-up: registered command context and real extension checks

- Fixed a missing runtime context field: activation now supplies its output
  channel to `CommandRegistry`, and each registered invocation forwards it to
  admission and execution. This restores modular command logging and IDE-context
  warning output. No-argument callers retain a registry-owned fallback channel;
  supplied channels remain activation-owned. Editor/selection/terminal state is
  still read for each invocation, and workspace-trust gates remain intact.
- Shared tool/participant contribution lookup through a small validated manifest
  helper instead of two untyped blocks. Invalid root/group/entry shapes do not
  enable integrations; valid literal identifiers preserve the existing decisions.
  Marked four intentionally unused parameters while retaining their signatures.
  The unused backend context slot now carries `unknown`, and disposal avoids
  returning VS Code's untyped result from a callback.
- The shipped entrypoint import graph now typechecks with the repository's strict
  compiler options: seven prior diagnostics are resolved. Default
  `npm run typecheck` and `npm test` now run the explicit entrypoint graph check and all
  isolated Node fixtures. Full-project legacy tests and unshipped modules remain
  outside that passed typecheck; the full project reports 570 diagnostics, 58 in
  non-test files outside the shipped graph. No full-source pass is claimed.
- All 42 Node tests passed without skips, including six new manifest/registry
  regressions. Registry regressions failed before the correction. Both focused
  typechecks, changed-module lint, production bundle, and production dependency
  audit passed (zero vulnerabilities). Entrypoint lint improves from 21 to seven
  existing errors, with no new diagnostic in a source-level baseline comparison.
- The owner deprioritized further VS Code extension optimization. Close this
  already-started correction and return to Rust-side improvements. Computer Use
  for VS Code remains skipped; native/browser rendered acceptance stays open.

### Follow-up: configuration model and bootstrap ownership

- Returned to Rust maintainability after the owner deprioritized VS Code work.
  The config root mixed serde/schema definitions and validation with scaffold
  writes and a 648-line static sample. Moved the bootstrap methods into the
  existing feature-gated `loader/bootstrap.rs` owner and embedded the sample from
  `data/default_config.toml`. The root shrank from 1,382 to 467 lines; bootstrap
  is 239 lines, with its tests in a private 188-line child module.
- Kept all three public bootstrap/sample entrypoints, defaults-provider path
  selection, write order, force/preserve rules, permissions, and I/O error context.
  Config definitions, defaults, validation, and provider helpers match their
  prior source after removing only the extracted code/imports and formatting.
  Template bytes match the prior literal exactly: 19,307 bytes, SHA256
  `374b3dc289d297925be4940ecf95f0c8b31b18330482e81568c0e2c8c512f8f3`.
  Cargo's package listing includes the asset. No dependency or public API was added.
- Preserved four existing bootstrap tests and added three regressions for all
  seven scaffold files on both sides of the force boundary, public sample
  generation and write-error context, and canonical home versus workspace fallback
  with a custom filename. The Unix path test verifies the private 0700 directory.
- The first broad run exposed a prompt-cache guard that searched the old Rust
  source for template text. Updated that guard to read the actual asset and
  assert parsed cache settings, retaining the documented retention comment check.
  Module guidance records this test-scope lesson and the new ownership map.
- All 482 config tests passed without skips. The focused bootstrap/template
  selection passed eight tests with 474 excluded. The core-facing config-loader
  integration passed its one test, with 36 unrelated binaries excluded. Locked
  config checks with warnings denied passed for all features and without default
  features. This verifies the feature-gated API and the existing public consumer;
  the fast development gate and scoped Markdown/diff checks also passed.
  No full-workspace test or native host acceptance result is claimed.
