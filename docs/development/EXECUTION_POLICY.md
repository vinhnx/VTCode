# VT Code Execution Policy

This document describes what commands and operations the VT Code agent can execute without prompting, and which require
confirmation.

## Summary

The execution policy is designed to allow engineers typical software development workflows while maintaining safety
boundaries. Common development tools work automatically, while dangerous operations require confirmation.

## Auto-Allowed Commands

### Version Control (Git)

- **Read operations**: `status`, `log`, `show`, `diff`, `branch`, `tag`, `remote`
- **Tree inspection**: `ls-tree`, `ls-files`, `cat-file`, `rev-parse`, `describe`
- **Additional inspection**: `blame`, `grep`, `shortlog`, `format-patch`
- **Safe writes**: `add`, `commit`, `reset`, `checkout`, `switch`, `restore`, `merge`, `stash` (select ops)
- **Blocked**: `push --force`, `clean`, `rebase`, `cherry-pick`, `filter-branch`

### Build Tools (Cargo)

- **Safe operations**: `build`, `check`, `test`, `doc`, `clippy`, `fmt`, `run`, `bench`
- **Additional safe**: `tree`, `metadata`, `search`, `cache`, `expand`
- **Blocked**: `clean`, `install`, `uninstall`, `publish`, `yank`

### Languages

- **Python** (`python`, `python3`): Script execution, module runs with `-m`
- **Node.js** (`node`): Script execution
- **NPM** (`npm`): Install, test, build, run, start, list, view, search
- **Blocked for NPM**: `publish`, `unpublish`

### File Operations

- **Read**: `cat`, `head`, `tail`, `ls`, `grep`, `find`, `rg` (ripgrep)
- **Write**: `sed` (with workspace validation)
- **Copy**: `cp` (with workspace validation)
- **Count**: `wc`

### System Info

- `pwd`, `whoami`, `hostname`, `uname`, `date`, `echo`, `which`, `printenv`

## Tool Policies

Canonical public tool names in the default profile are `exec_command`, `write_stdin`, and `apply_patch`. The advanced VT
Code profile may also expose `code_search` for bounded code search. Plain text search and file inspection use shell
commands inside `exec_command.cmd`.

| Tool                 | Policy     | Notes                                |
| -------------------- | ---------- | ------------------------------------ |
| `exec_command`       | **Prompt** | Public shell command surface         |
| `write_stdin`        | **Prompt** | Live-session continuation surface    |
| `apply_patch`        | **Prompt** | Patch edits                          |
| `code_search`        | Allow      | Advanced-profile bounded code search |
| `request_user_input` | Allow      | Interactive clarification surface    |

### Recovery After Policy Denial

When `exec_command` is denied, treat that denial as a routing signal, not a retryable transient:

1. Do not repeat the same shell inspection.
2. If the task needs syntax-aware search and the advanced profile is available, use `code_search`.
3. If the task still requires shell access, state that the change was **not applied** and recommend a user-approved mode
   change such as `/mode auto`.

Example pivot:

- Denied: `exec_command` running `rg -n "foo" README.md`
- Recovery for a focused code query: advanced `code_search` with `query` and narrower filters
- Final fallback when shell access remains necessary:
  `Not applied; exec was denied by policy; next action: switch to /mode auto or approve the command.`

## Key Safety Features

1. **Workspace Boundary**: All file operations are confined to `WORKSPACE_DIR`; cannot escape or touch system files
2. **Command Whitelisting**: Only specific commands are allowed; unknown commands are blocked
3. **Argument Validation**: Common flags are validated (e.g., git force-push is blocked)
4. **Confirmation Required**: Destructive operations still require user confirmation
5. **Two-Layer Control**:
   - Tool-level: Which tools can be used
   - Command-level: Which specific commands and flags are permitted

## Dangerous Operations (Blocked)

- `rm -rf` (recursively remove)
- `sudo` (privilege escalation)
- `kubectl` (Kubernetes operations)
- `chmod`, `chown` (permission changes)
- Git force-push, clean, rebase, cherry-pick
- Cargo install, publish, clean
- NPM publish, unpublish
- File deletion (`delete_file` requires confirmation)
- Apply complex patches (`apply_patch` requires confirmation)

## Use Cases

### Typical Engineer Workflow

```text
 git status, git diff, git log, git checkout
 cargo test, cargo build, cargo check
 npm install, npm test, npm run build
 python scripts/setup.py
 Editing files, reading logs, viewing diffs
```

### Blocked Without Confirmation

```text
 Deleting files (requires confirmation)
 Applying complex patches (requires confirmation)
 Git force-push or history rewrites
 Publishing to registries
```

## Configuration

Policies are defined in:

- **Core defaults**: `crates/codegen/vtcode-config/src/core/tools.rs` (tool policies)
- **Command validation**: `crates/codegen/vtcode-core/src/exec_policy/mod.rs` (command whitelisting)
- **User overrides**: `vtcode.toml` in project root or the canonical user config directory

Override examples in `vtcode.toml`:

```toml
[tools.policies]
apply_patch = "allow"  # Allow patches without prompt
exec_command = "ask"    # Prompt before shell commands
write_stdin = "ask"     # Prompt before live-session input
code_search = "allow"   # Allow advanced bounded literal search
```

## Cache-Friendly Execution Guidance

Token efficiency is a correctness concern, not just a cost concern: every token of harness payload is context the model
cannot spend on the task. The defaults below are designed to keep the first-request overhead low and per-turn growth
bounded.

### Defaults that keep the prefix small

- **MCP tools defer by default.** Any MCP tool in the catalog is flagged `defer_loading` rather than sent eagerly,
  regardless of tool count. MCP schemas are the dominant source of token inflation; the model discovers them on demand.
- **Client-local deferral is the default.** Providers without a hosted tool search (e.g. Gemini) omit deferred schemas
  from the wire payload and append a compact, cache-stable discoverability summary to the system prompt. Set
  `tools.client_tool_search = false` to opt back into the eager catalog.
- **Subagents use a lightweight profile.** A delegated child agent defaults to `system_prompt_mode = minimal` and
  `tool_documentation_mode = minimal`, and does not inherit the parent's MCP servers unless explicitly requested. This
  prevents replaying the full parent bootstrap on every child turn.
- **Tool-result clearing is on by default.** Old tool results are stripped from context once it grows past
  `trigger_tokens` (default 40k), protecting the most recent `keep_tool_uses` (default 2) results. Requests that will
  not carry native context edits get the request-only local rewrite (`clear_old_tool_results_with_recent_batches`):
  non-Anthropic providers,
  Anthropic routes whose model capability profile lacks context edits, and headless (`vtcode exec`) Anthropic runs,
  which never attach native context management. The local rewrite stubs older tool-result bodies while preserving
  `tool_call_id` pairing. The local trigger is capped at 60% of the history budget: the resolved provider/session
  context limit minus reserved output tokens and system/tool-schema overhead. Lower configured triggers still win;
  unknown capacity preserves the configured trigger. Set `enabled = false` to opt out. Local retention extends
  to every result in each protected result's originating assistant tool-call batch, even with reordered completions
  or intervening system directives. It additionally protects the newest `keep_recent_tool_batches` assistant
  tool-call messages (default 2), by originating message order. The union protects 7 results for rounds of 1/4/3.
  Set `keep_recent_tool_batches = 0` to restore count-based batch retention; native Anthropic edits ignore this
  local-only setting. Small results remain unchanged when a placeholder would not save tokens.
  Durable session history and `ThreadEvent`s keep the full payload. Headless automatic compaction checks the
  normalized, cleared request projection first; if pressure remains high, the existing compaction engine receives
  full history and the request projection is rebuilt afterward.
- **Local summary requests reserve their prefix.** Flat and hierarchical summaries share a history-budget resolver
  that intersects provider/session limits and reserves parent system/tool-schema overhead, any explicit summary
  output limit, and existing safety headroom. Impossible fixed inputs fail before a summary call; unknown capacity
  keeps the existing fallback. See [the compaction contract](../guides/agent-loop-contract.md#unified-auto-compaction).
- **Builtin tool count is capped.** The number of LLM-exposed builtin tools stays within a small cap; new tools must
  consolidate, defer, or deliberately raise the cap. Builtin tool schemas in `progressive` mode fit in a **2,000-token**
  envelope (measured ~1,793 on the Codex-4 default profile). The always-eager set is the Codex baseline plus structured
  search (`exec_command`, `write_stdin`, `apply_patch` when supported, `search_tools`, `code_search`, `grep_file`).
  Deferred structured search pushed models to shell out via `exec_command` and pay in huge tool outputs (session data
  2026-09-28); the extra schema tokens are cheaper. Planner, skills, and agent tools defer until `search_tools` surfaces
  them (planner tools stay eager while planning is active).
- **Tool-result clearing defaults** (`agent.harness.tool_result_clearing`): `trigger_tokens: 40000`,
  `keep_tool_uses: 2`. Research/audit turns were observed at ~1M input tokens/turn with the old 100k trigger — tool
  results piled up long before any clearing. Durable history is unchanged. `clear_tool_inputs` defaults to `true`, so
  paired `apply_patch` / `write_file` arguments are replaced with a JSON placeholder when their results are actually
  stubbed, without increasing projected tokens. Retained results keep their arguments and signatures;
  set it `false` to keep full tool-call inputs on the wire.
- **Tool-output economy.** Interactive and headless paths share `reduce_tool_result` hard caps (32 KiB / 2000 lines on
  read/exec bodies). `list_files` `mode=tree` emits at most 200 nodes and 50 children per directory and sets
  `tree_truncated` when it stops. Failure diagnosis is memoized per `(tool, evidence)` with at most three model-backed
  calls per turn; repeats reuse the first diagnosis or the deterministic fallback.
- **Startup token-overhead warnings.** At session start (unless `--quiet`), VT Code logs non-fatal `tracing::warn!`
  messages when the config is likely to inflate per-request cost: more than 8 configured MCP servers,
  `tools.client_tool_search = false` while MCP servers are configured, `system_prompt_mode = "specialized"`,
  `tool_documentation_mode = "full"`, `tool_result_clearing` disabled, `auto_compaction_enabled` disabled, or
  `max_system_prompt_tokens` set very low (< 4000). The actual composed system prompt is also measured at startup and
  flagged if it exceeds the configured budget. These surface the "what am I actually sending" question before you pay
  for it.

### Authoring guidance

- Keep instruction files (AGENTS.md / CLAUDE.md) focused; they ride on every request.
- Put universal user-facing behavior in the compiled runtime-guidance section; keep authored instruction files for
  project-specific maps and maintainer workflows. Authored files are context, not a security boundary.
- Prefer delegating large searches to subagents with a narrow, explicit tool set rather than fanning out broad
  orchestration.
- When adding a tool, keep its description between 40 and 1500 characters and include a verb cue and, for side-effect
  tools, a concrete constraint cue (`max`, `session`, `timeout`, `requires approval`, ...) so the model selects it
  accurately without padding the prompt. Prohibition phrasing such as `Do NOT` does not satisfy the description contract
  test.
- Pin the cache breakpoint: keep the system prefix stable across turns. Dynamic per-turn content (timestamps, volatile
  workspace state) belongs in trailing sections, not the cached prefix.

### Auditing token cost

A first-request budget guard rail is enforced by tests (lean harness defaults):

| Budget                                 | Cap     |
| -------------------------------------- | ------- |
| Progressive builtin tool-schema tokens | ≤ 2,000 |
| First request (no MCP)                 | ≤ 6,000 |
| First request (MCP growth ceiling)     | ≤ 8,000 |

These are **intentional lean caps** (HarnessTax). Do not raise them without new session/benchmark evidence that the
headroom is insufficient.

- `crates/codegen/vtcode-core/src/tools/registry/builtins.rs::emitted_model_tool_schema_fits_within_first_request_budget`
  asserts builtin tool schemas stay within the budget in `progressive` mode.
- `first_request_total_token_budget_within_limit` asserts the effective-default system prompt (Minimal) plus
  schemas/appendix/addendum fit the 6k/8k ceilings.
- `crates/codegen/vtcode-core/src/tools/handlers/session_tool_catalog.rs` tests assert MCP tools defer (small or large
  catalog) and that the client-local policy defers small MCP catalogs.

Run them with:

```bash
cargo nextest run -p vtcode-core emitted_model_tool_schema_fits_within_first_request_budget
cargo nextest run -p vtcode-core -E 'test(session_tool_catalog)'
```

### Harness tax (first-call fixed overhead)

HarnessTax (Pan et al., 2026 — <https://harnesstax.github.io/>) shows the same model can cost up to 5x more under a
different coding-agent harness at essentially the same success rate; the gap is mostly **first-call fixed overhead**
(instructions + tool schemas), not more turns. VT Code surfaces that tax so it can be measured, not guessed:

- Every assembled request logs `token_budget_breakdown` with `system_prompt_tokens`, `tool_schema_tokens`,
  `message_history_tokens`, `on_wire_tools`, `first_call`, and `fixed_overhead_tokens` (`system + tool schemas` — the
  per-call harness tax before the task prompt).
- The session's first assembled request is captured once into interactive `SessionStats::first_call_composition` and
  shown on exit as `First-call overhead N (system S + tools T)`.
- Eval reports (`vtcode-eval`) include a `Cost efficiency` line: `cost/solve`, mean cost per priced attempt, mean tokens
  per attempt, and mean turns per attempt. `cost_per_solve` is `None` when any attempt is unpriced (unknown cost is not
  free) or nothing passed.

`agent.system_prompt_mode` defaults to **minimal** (~500 tokens). Keep new tools and instruction files lean: Progressive
schemas and the first-request budgets above are the enforcement side of the same trade-off.

### Model pricing and fallback policy

`vtcode-llm::usage_cost` owns normalized provider usage and the shared cost calculation. Raw cost prices all input
tokens at the input rate and is used for USD budget enforcement; cache-aware effective cost is for display. Each turn is
priced under the route that served it and added to the session total; switching to a cheaper model never reprices
earlier spend. A turn without usage or pricing leaves the complete total unknown. Cache creation can make effective cost
higher than raw cost. Missing, negative, or non-finite pricing is not a zero-price estimate. A configured USD budget
blocks an unpriced route before inference or automatic compaction, with a `turn.blocked` diagnostic and recoverable
failed outcome. Explicitly removing the USD budget permits an unpriced route; the reported cost remains unknown.

Model capability tier and lightweight fallback come from `docs/models.json` fields `is_pro` and `lightweight_model`.
Fallback stays within the selected provider and never returns the same model. Missing metadata gives no tier assertion
or speculative fallback. Legacy built-in variants without matching catalog rows currently include `CopilotGPT52Codex`,
`CopilotGPT54`, `EvolinkDeepseekV4Pro`, `MoonshotKimiK3`, and `MoonshotKimiK27Code`. These need explicit catalog
metadata before they can participate in automatic fallback. Pricing is route-specific: OpenAI and OpenRouter Astra
catalog routes are priced; Merge Gateway's Astra route currently has no pricing and therefore blocks with a USD cap.

Deterministic cross-family normalization and route-pricing checks run without paid calls:
`cargo nextest run -p vtcode-llm -E 'test(usage_cost)'`.

### Terminal input and redraw ownership

The core TUI derives a single input owner: overlay, composer, or runtime. Activity changes while an overlay is open
update the state restored on close; busy runtime states cannot accidentally re-enable the composer. Transcript cache
validity is explicit, so revision zero and resize invalidation remain distinct.

Terminal redraw ticks coalesce to one pending notification. Keyboard events remain ordered and PTY data continues
through its existing ordered command channel; redraw coalescing never drops output bytes. Built-in theme checks cover
every semantic foreground against its background at WCAG AA 4.5:1, including inherited tool output. Reasoning uses
italics without terminal dimming, whose contrast varies by terminal.
