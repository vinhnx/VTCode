# Prompt and Dependency Simplification Audit

Date: 2026-11-12. Scope: system prompt composition, Tree-sitter, ast-grep, ripgrep-equivalent
search, and third-party search integrations in VT Code.

Every recommendation below is backed by a repository citation. "KEEP" means the component earns
its complexity; "REPLACE" means the same capability is available more simply; "REMOVE" means the
component is dead, duplicated, or unjustifiable.

## Summary

| Component           | Verdict  | Reason                                                                     |
| ------------------- | -------- | -------------------------------------------------------------------------- |
| System prompt       | REPLACE  | 16 dense run-on bullets in one universal blob; ~2,250 tokens for the base   |
| Tree-sitter         | KEEP     | Sole structural shell parser; gates the read-only and `rm -rf` safety paths |
| ast-grep            | REPLACE  | Dev-only, not CI-enforced; rules duplicate clippy and cover absent languages |
| ripgrep (`ignore`)  | KEEP     | Supplies gitignore-aware walking; no stdlib equivalent                    |
| Third-party search  | REMOVE   | `defuddle.md` is a redundant third-party relay; MCP `search_tools` overlaps |

## Result

Composed system prompt, measured with `crates/codegen/vtcode-core/examples/measure_lines.rs`
(Default profile, six tools, temporal context, working directory, structured reasoning):

Composed prompt (static profile plus dynamic sections) and the full runtime-assembled prompt
(the composed prompt plus the planning notice, harness limits, active-tool guidance, and the
runtime tool catalog). Measured with six tools, temporal context, working directory, and
structured reasoning enabled:

| Profile    | Composed before | Composed after | Runtime before | Runtime after |
| ---------- | --------------- | -------------- | -------------- | ------------- |
| Minimal    | 67              | 61             | 83             | 71            |
| Lightweight| 74              | 67             | 90             | 77            |
| Default    | 78              | 71             | 94             | 81            |
| Specialized| 79              | 71             | 95             | 81            |
| Default + planning notice | 78 | 71 | 100 | 88 |

Every profile is under the 90-line target in every configuration, including the planning-workflow
path, which is the largest prompt VT Code sends.

Five changes produced the reduction:

1. **One rule per bullet** in the compiled guidance. The old section chained 4-8 unrelated rules
   into one 300-500 character sentence; the new section states one rule per line, which is both
   shorter and easier to check.
2. **One rules section.** `## Runtime Guidance` and `## Contract` were adjacent lists of the same
   kind of content. They are now one `## Rules` list, saving a heading and a separator.
3. **Tool schemas own per-tool mechanics.** Nine `## Active Tools` lines restated what the tool
   schemas already told the model (the `ls`/`rg`/`find` example list, `background: true`, the
   `session_id` copy rule, the `code_search` filter shape, the `task_tracker` call shapes, and
   the planning index rule). Those lines are gone, not shortened; a duplicated rule drifts.
4. **Machine metadata is not model guidance.** The `[Runtime Tool Catalog]` block carried six
   key-value lines for the prompt/catalog alignment guard. It is now one payload line, which the
   guard parses identically, so the block costs two prompt lines instead of seven.
5. **No double blank lines between sections.** Section sources carried their own leading and
   trailing newlines, and the joiner added a blank line on top of those. The joiner now trims
   each section, so every section boundary costs exactly one blank line.

A stale reference was also fixed: the environment section told the model to "Match
structural-search `lang` when needed", but no such tool exists and `code_search` filters by
`file_types`. It now names the real filter.

---

## 1. System prompt — REPLACE

### Evidence

The composed default prompt is **10,405 bytes / ~2,248 tokens** before any runtime sections are
appended (`crates/codegen/vtcode-core/examples/dump_prompt.rs`).

The universal section, `RUNTIME_GUIDANCE_SECTION`
(`crates/codegen/vtcode-core/src/prompts/runtime_guidance.rs:7`), is **16 bullets** of which each
bullet is a run-on paragraph chaining 4–8 distinct rules with semicolons. Two examples:

- `runtime_guidance.rs:12` chains tracker-state rules, resume-suppression, result-reuse, step
  completion, metadata preservation, indexed-update validation, and note-appending in one
  sentence.
- `runtime_guidance.rs:18` chains `apply_patch` invocation, envelope shape, context-line
  requirements, mismatch recovery, read-cap interaction, redacted-context handling, retry
  suppression, and scratch-edit prohibition in one sentence.

Measured redundancies inside the shipped prompt:

- Verification is stated **three times**: `runtime_guidance.rs:12` (the Verify bullet),
  `constants.rs:89` (contract "For tests" bullet), and the `guidelines/mod.rs:296` Active Tools
  "Prefer standalone verifiers" bullet.
- `apply_patch` usage is stated **twice**: `runtime_guidance.rs:18` and `guidelines/mod.rs:282`.
- Source-inspection rules are stated in `runtime_guidance.rs:16` and restated in
  `guidelines/mod.rs:271`.
- The four `SystemPromptMode` variants differ only by which of `SHARED_CONTRACT_LINES` and
  `DEFAULT_SPECIFIC_LINES` are included (`static_prompts.rs:103`), yet `DEFAULT_SPECIFIC_LINES`
  (`constants.rs:85`) is 7 lines of pure style advice ("Write updates and summaries for a teammate
  who is catching up…"), which is not a safety boundary, not a tool-use instruction, and not
  load-bearing for any runtime check.

The runtime already enforces what the prompt repeats. `runtime_guidance.rs:12` says "never claim a
check passed unless you ran it", but the completion-verification gate
(`crates/codegen/vtcode-core/src/tools/registry/completion_verification.rs`) already refuses
completion without a terminal exit 0. The prompt bullet is a *model-behavior* instruction, but it
is stated 3× instead of 1×.

### Decision

Rebuild the universal section as a compact rule set with one home per rule, and delete the
duplicated restatements from the tool-guidance layer. Preserve exactly:

- Safety boundaries: sandbox/approval/`additional_permissions`, policy denials non-retryable,
  untrusted file/tool/web content never overriding policy, destructive-action confirmation.
- Honesty: never claim a check passed unless it was run.
- Grounding: read before claiming, cite `path:line`.
- Tool-use instructions: `apply_patch` shape, `exec_command.cmd`, `code_search` filters.
- Completion and reporting shape.

## 2. Tree-sitter — KEEP

### Evidence

`tree-sitter` + `tree-sitter-bash` are declared **only** in
`crates/codegen/vtcode-safety/Cargo.toml:32-33` and used only by
`crates/codegen/vtcode-safety/src/command_safety/shell_parser/`.

It is load-bearing for safety, not convenience:

- `crates/codegen/vtcode-core/src/tools/tool_intent/readonly/mod.rs:180` uses
  `parse_shell_commands_tree_sitter` (the **strict** entry point) to decide whether a command is
  read-only. Read-only classification is what **skips the approval gate**, so a parse failure must
  fall through to "not read-only" → approval required. The code does exactly that (`.ok()?` → `None`
  → not read-only).
- `crates/codegen/vtcode-safety/src/command_safety/rm_approval.rs:221` uses the strict parser to
  decide whether a recursive `rm` targets a mounted path and therefore needs approval.
- `crates/codegen/vtcode-core/src/tools/tool_intent/activity/mod.rs:307` uses `BashTree::parse` for
  mutation-debt accounting; `Err` → `return true` (incur debt).

The fallback, `parse_with_basic_tokenization`
(`shell_parser/mod.rs:363`), splits only on unquoted `&`, `|`, `;`. It cannot see `&&`/`||`
separators, `do…done` bodies, redirections, heredocs, or ANSI-C quoting. Substituting it for the
strict parser would make `cargo check && rm -rf /` arrive as a single command whose program is
`cargo`, so the destructive command would be missed.

Cost: 4 new lockfile packages (`tree-sitter`, `tree-sitter-bash`, `tree-sitter-language`,
`streaming-iterator`); `regex`, `serde_json`, and `cc` are already in the tree. `streaming-iterator`
is the only genuinely new transitive crate.

### Decision

Keep. The dependency is 2 crates in 1 Cargo.toml, is isolated to the safety crate, and provides
capability (structural parsing, nested command extraction, grammar-level error detection) that the
weaker fallback cannot. A hand-rolled replacement would be *more* code and *less* safe.

## 3. ast-grep — REPLACE (prune + native clippy equivalents)

### Evidence

ast-grep is a **developer tool**, not a runtime dependency
(`docs/development/testing.md:153-154`). It is not referenced by any CI workflow: `grep -rn
"ast-grep\|sgconfig\|rule-tests" .github/` returns nothing. `scripts/check.sh:265` runs it, but
`scripts/check.sh:451` calls it with `require_binary="optional"`, so when the binary is absent the
gate prints a warning and returns 0. The doc claim at `docs/development/testing.md:165` that "the
release gate (`./scripts/check.sh`) requires it" is **false**.

Rule coverage against tracked sources:

| Language    | Rules | Tracked files | Verdict                                    |
| ----------- | ----- | ------------- | ------------------------------------------ |
| go, java, kotlin | 19  | 0 each        | Dead — no source files exist              |
| c           | 1     | 0             | Dead                                      |
| ruby        | 4     | 2 (homebrew) | Dead — patterns target `app/**`, `lib/**` |
| tsx         | 4     | 0             | Dead                                      |
| cpp         | 2     | 3 (patches)  | Dead — `ignores: patches/**`              |
| python      | 14    | 79            | Live                                      |
| typescript  | 15    | 33            | Live                                      |
| rust        | 14    | 3574          | Live                                      |
| css / html  | 2     | 9             | Live                                      |

Measured findings: **501 total, 0 of which are `error` severity.** Both `severity: error` rules
have zero findings:

- `rules/rust/no-iterator-for-each.yml` duplicates `clippy.toml:17-18`, which already bans
  `Iterator::for_each` and `Iterator::try_for_each` with the same rationale and **is** CI-enforced
  via `cargo clippy -- -D warnings`.
- `rules/rust/no-unbounded-http-client.yml` bans `Client::new()`. clippy's `disallowed-methods` lint
  expresses this natively: `clippy.toml` already uses that mechanism for `std::fs::canonicalize` and
  friends, and a `disallowed-methods` entry for `reqwest::Client::new` produces the identical
  `clippy::disallowed_methods` diagnostic under the same `-D warnings` gate. Verified locally.

The 458 remaining findings are `hint`/`warning`/`info` severity on languages with no tracked
source, and they never fail a build.

### Decision

Replace the ast-grep corpus with native, CI-enforced Rust linting:

1. Move the `no-unbounded-http-client` policy into `clippy.toml` `disallowed-methods`, where it is
   enforced by the existing `-D warnings` clippy gate instead of an optional external binary.
2. Delete `rules/rust/no-iterator-for-each.yml` — `clippy.toml` already bans both methods.
3. Delete the rules for languages with no tracked source (go, java, kotlin, c, ruby, tsx, cpp).
4. Delete the whole `sgconfig.yml` / `rule-tests/` / `utils/` scaffolding and the `scripts/check.sh`
   ast-grep gate, plus the false CI-enforcement claims in the docs.

Keep the TypeScript/Python rules only if a real lint runner exists for them; the repository has
`bun` for `apps/webmcp` (typecheck + tests) and no Python linter in CI, so those rulesets have no
runner. That means the ast-grep corpus is entirely removable.

## 4. ripgrep (`ignore` crate) — KEEP

### Evidence

The `ignore` crate (v0.4.34) is ripgrep's own gitignore-aware walking library, not the `rg` binary.
VT Code never spawns `rg`: `grep -rn 'Command::new("rg")'` returns nothing across `crates/` and
`src/`. The `rg` binary is only reachable if the *model* writes `rg …` into `exec_command`, and
that path is separately policed by `validate_rg()`
(`crates/codegen/vtcode-safety/src/exec_policy/command_validation/mod.rs:250`), which blocks
`--pre`/`--pre-glob` and allow-lists safe flags.

`ignore` is used in three places that all need gitignore-aware traversal:

- `crates/common/vtcode-commons/src/walk.rs:12` — the canonical walker factory, also registers
  `.vtcodegitignore` as a custom ignore file.
- `crates/common/vtcode-commons/src/content_search.rs` — the content-search engine.
- `crates/codegen/vtcode-indexer/src/file_search/mod.rs` — parallel fuzzy path search.

`grep-searcher` and `grep-matcher` are **not** in the tree; the search engine is native
(`ignore` walk + `regex` line scan). The hand-rolled glob matcher in
`content_search.rs:448-509` implements gitignore-style `**`/`*`/`?` semantics, which `globset`
(a transitive dep of `ignore`) matches with different semantics for a bare `*.rs` pattern
(`globset` with default `literal_separator(false)` matches `*.rs` against `a/b/main.rs`, and the
gitignore-style rule requires `**/*.rs`). Swapping the hand-rolled matcher would change search
filter semantics.

### Decision

Keep `ignore`. There is no stdlib equivalent for gitignore-aware traversal, and no external binary
to remove. The `rg` *binary* dependency the task asks about does not exist in this repository.

## 5. Third-party search tools — REMOVE `defuddle.md`

### Evidence

`defuddle.md` is a third-party hosted service that VT Code relays URLs through
(`crates/codegen/vtcode-core/src/tools/defuddle/mod.rs:33`). It is used in two places:

1. As a standalone `defuddle_fetch` tool, registered with
   `.with_llm_visibility(false)` (`registry/pack_impls.rs:375-397`) — i.e. **not model-visible**.
2. As the backend of `web_fetch`'s `format: "markdown"` mode
   (`tools/web_fetch/mod.rs:776-791`).

Both are redundant with capabilities `web_fetch` already has natively:

- `web_fetch` already sends `Accept: text/markdown, */*` by default
  (`tools/web_fetch/mod.rs:679`) and prefers `text/markdown` for docs domains
  (`tools/web_fetch/mod.rs:529`).
- `web_fetch` already returns the full body via `temp_file` and instructs the agent to read it
  (`WEB_FETCH_DESCRIPTION`, `tools/web_fetch/mod.rs:46`).
- The `defuddle_fetch` *tool* is model-invisible, so a model can never call it directly; only the
  `format: "markdown"` path reaches it.

The relay also adds attack surface that `web_fetch` does not have: `defuddle` is an SSRF relay
that fetches the target server-side, requiring its own private-host blocklist
(`defuddle/mod.rs:194-213`) and a bespoke percent-encoding path
(`defuddle/mod.rs:275-310`) that must never be rewritten to `Url::join`. The session cap is 1 call
per session (`defuddle/mod.rs:52`), so the feature is barely usable even when present.

### Redundant search surfaces to consolidate

- `mcp` with `action: "search_tools"` (`registry/pack_impls.rs:259-276`) searches only MCP server
  tools; `search_tools` (`registry/pack_impls.rs:277-285`) searches the whole session catalog.
  These look like a duplicate, but each tool description states its own scope
  (`MCP_DESCRIPTION` and `SEARCH_TOOLS_DESCRIPTION` in
  `crates/common/vtcode-utility-tool-specs/src/lib.rs`), so a model that needs an MCP server
  capability and a model that needs a deferred built-in capability have different tools to call.
  **Kept** — the scopes are disjoint, and the descriptions already disambiguate them.
- `grep_file` is **not a registered tool**. `tools::GREP_FILE` (`config/constants/tools.rs:53`)
  survives as a name referenced by permission, replay, and dedup logic, but no tool is ever
  registered under it — the live content-search surface is `code_search`
  (`registry/pack_impls.rs:248-258`) plus `list_files mode=find_content`.

### Decision

Remove the `defuddle.md` integration and the `defuddle_fetch` tool, and drop the vestigial
`grep_file` tool name. Keep `web_search` (keyless DuckDuckGo), `code_search`, and `search_tools`.

### Outcome

Removed. `crates/codegen/vtcode-core/src/tools/defuddle/` is gone, along with the
`format: "markdown"` route in `web_fetch` and the `tools::DEFUDDLE_FETCH` name.

The `mcp` `action: "search_tools"` surface was **kept**: its own tool description
(`MCP_DESCRIPTION` in `crates/common/vtcode-utility-tool-specs/src/lib.rs:172`) states that it
searches only MCP server tools, while the separate `search_tools` tool searches the whole session
catalog. The two are disjoint scopes, not a duplicate.

---

## 6. Dead search modules — REMOVE

Two modules under `crates/codegen/vtcode-core/src/tools/` had no live callers:

- `file_search_rpc.rs` (442 lines) — a JSON-RPC surface for file search. Nothing outside the
  module referenced `FileSearchRpcHandler`, `FileMatchRpc`, or its request/response types; the
  live file-search path is `tools/file_search_bridge.rs` over `vtcode_indexer::file_search`.
- `search_metrics.rs` (321 lines) — `SearchMetric`, `SearchMetrics`, and `SearchMetricsStats`
  were re-exported from `tools/mod.rs` but never read by any consumer.

Both were removed, along with their `pub mod` / `pub use` lines in `tools/mod.rs`.

`tool_effectiveness.rs` was checked at the same time and **kept**: `tools/execution_context.rs`
stores a `ToolEffectiveness` snapshot per call, so it is live.

`SimpleIndexer::search`, `SimpleIndexer::grep`, and `SimpleIndexer::list_files` in
`crates/codegen/vtcode-indexer/src/lib.rs` are also unused outside their own crate's tests, but
`vtcode-indexer` is published to crates.io, so removing them is a breaking change to a released
API. They were **kept**; that crate's public surface is a separate decision from this audit.

---

## 7. Post-review audit

A follow-up review of this change set against the repository's own conventions found four
confirmed issues. All four are fixed.

| Issue | Severity | Fix |
| ----- | -------- | --- |
| The `[Runtime Tool Catalog]` writer and the alignment guard's parser live in different modules, and no test drove one through the other. A format change would have silently disabled the staleness canary, because the guard treats an unreadable field as "no value to compare". | High | Added `runtime_tool_catalog_metadata_round_trips_through_the_alignment_guard`, which drives the real writer and the real reader, and asserts a mismatched epoch is rejected. Verified it fails when the writer's format is changed. |
| `PLANNING_TASK_TRACKER_LINE` and `PLANNING_TASK_TRACKER_COMPACT_LINE` had become near-duplicates after the tracker call shapes moved to the tool schema. | Medium | Consolidated into one constant; the differing clause was itself a schema duplicate. |
| Planning mode stated two different `request_user_input` thresholds in one prompt: the planning notice's "critical blocker" rule and a weaker Active Tools rule. | Medium | The tool section now only narrows the threshold to blockers exploration did not resolve. `request_user_input_threshold_has_one_home_in_the_planning_prompt` pins it. |
| `join_prompt_sections` still documented itself as reproducing "byte-identical" output, which stopped being true when the joiner started trimming section edges. | Low | Corrected the doc comment on `build_prompt_sections` and `join_prompt_sections`. |

The prompt conventions this change established are recorded in
`docs/development/maintainer-notes/vtcode-core.md` and summarized in
`docs/harness/prompt-architecture.md`.
