# EP-001: TUI Runloop and Slash Command Decomposition

**Status**: completed
**Author**: agent
**Created**: 2026-05-15

## Goal

Reduce the largest TUI runloop and slash-command modules to smaller, navigable units without changing slash-command behavior, modal behavior, or session-loop outcomes.

## Context

This plan addresses [TD-005](../TECH_DEBT_TRACKER.md) and the TUI domain in [QUALITY_SCORE.md](../QUALITY_SCORE.md), which now treats large runloop and slash-command modules as active debt again. The current `src/` audit still shows several oversized files concentrated in the TUI session stack, including `src/agent/runloop/slash_commands.rs` (1790 lines), `src/agent/runloop/unified/turn/session/slash_commands/agents.rs` (1925 lines), `src/agent/runloop/unified/turn/session/slash_commands/ui.rs` (1821 lines), `src/agent/runloop/unified/turn/session/slash_commands/share_log.rs` (1735 lines), `src/agent/runloop/unified/turn/session/slash_commands/diagnostics.rs` (1709 lines), and `src/agent/runloop/unified/turn/session/interaction_loop_runner.rs` (1734 lines).

The relevant architectural seam already exists:

- `src/agent/runloop/slash_commands.rs` owns slash-command parsing, normalization, and built-in dispatch.
- `src/agent/runloop/unified/turn/session/slash_command_handler.rs` translates command input into session-loop control flow.
- `src/agent/runloop/unified/turn/session/slash_commands/mod.rs` maps `SlashCommandOutcome` variants to handler functions.
- `src/agent/runloop/unified/turn/session/slash_commands/handlers.rs` is the shared handler hub that already delegates to submodules such as `apps`, `compact`, `diagnostics`, `mcp`, `oauth`, `schedule`, `share_log`, `skills`, `ui`, `update`, and `workspace`.

The work should preserve existing public behavior while making the next round of TUI work easier to review, test, and continue across sessions.

## Steps

- [x] Step 1: Freeze the current hotspot inventory and define decomposition boundaries for the TUI slash-command surface. Outcome: the initial slash-command boundary map is now explicit: `src/agent/runloop/slash_commands.rs` is the orchestration root, `slash_commands/models.rs` owns command models, `slash_commands/dispatch.rs` owns slash-command and skill routing, `slash_commands/builtins.rs` owns built-in command dispatch, and `slash_commands/tests.rs` preserves the legacy coverage block. Verify: `find src -name '*.rs' -type f -exec wc -l {} + | sort -nr | head -n 20` and a manual file map covering `src/agent/runloop/slash_commands.rs`, `src/agent/runloop/unified/turn/session/slash_command_handler.rs`, `src/agent/runloop/unified/turn/session/slash_commands/mod.rs`, and `src/agent/runloop/unified/turn/session/slash_commands/handlers.rs`.
- [x] Step 2: Split `src/agent/runloop/slash_commands.rs` into smaller command-model, parsing, and built-in-dispatch modules while keeping `handle_slash_command` and `execute_command_skill_by_name` behavior stable. Outcome: the root file is now a 27-line orchestration layer, with command models moved into `slash_commands/models.rs`, routing into `slash_commands/dispatch.rs`, built-in command execution into `slash_commands/builtins.rs`, and test coverage into `slash_commands/tests.rs`. Verify: `cargo test -p vtcode slash_commands` and `cargo check -p vtcode`.
- [x] Step 3: Decompose unified slash-command outcome routing in `src/agent/runloop/unified/turn/session/slash_commands/mod.rs` and `handlers.rs` by grouping related outcomes into smaller modules. Outcome: `slash_commands/mod.rs` now delegates outcome fan-out through `slash_commands/outcome_router.rs`, while `handlers.rs` is a thin hub over extracted `control.rs` and `rewind.rs` modules plus the existing feature handlers. Verify: `cargo check -p vtcode` and `cargo test -p vtcode slash_commands`.
- [x] Step 4: Break the oversized UI-heavy slash-command modules into feature-oriented submodules, starting with `src/agent/runloop/unified/turn/session/slash_commands/ui.rs`. Outcome: the root `ui.rs` file now stays focused on shared modal helpers plus theme/session/history/model flows, while status line setup moved into `ui/statusline.rs` and terminal-title setup moved into `ui/terminal_title.rs`; `ui.rs` dropped to 407 lines. Verify: `cargo test -p vtcode slash_commands`, `cargo test -p vtcode --bin vtcode inline_events::tests`, and `wc -l src/agent/runloop/unified/turn/session/slash_commands/ui.rs`.
- [x] Step 5: Decompose the highest-risk slash-command feature modules that remain oversized after the routing split, prioritizing `src/agent/runloop/unified/turn/session/slash_commands/agents.rs`, `src/agent/runloop/unified/turn/session/slash_commands/share_log.rs`, and `src/agent/runloop/unified/turn/session/slash_commands/diagnostics.rs`. Outcome: `agents.rs` is now reduced to the catalog and definition-management surface, with delegated-thread and background-subprocess runtime flows extracted into `agents/runtime.rs`; `share_log.rs` is now a 283-line export orchestrator with the timeline export/HTML pipeline and focused tests moved into `share_log/timeline.rs`; `diagnostics.rs` is now a 208-line status/doctor root with memory controls, prompt helpers, config persistence, and focused tests moved into `diagnostics/memory.rs`. Verify: `cargo check -p vtcode` and `cargo test -p vtcode slash_commands`.
- [x] Step 6: Reduce the surrounding session-loop complexity where command handling still fans out through large orchestration files such as `src/agent/runloop/unified/turn/session/interaction_loop_runner.rs`. Outcome: `interaction_loop_runner.rs` is now a 634-line orchestration root, with follow-up recovery helpers, transcript review flows, user-message preparation, IDE-context refresh helpers, and focused tests moved into `interaction_loop_runner/support.rs`; the inline-action modal/resume match now routes through a dedicated resolver, making loop continuation easier to trace from input to outcome. Verify: `cargo check -p vtcode`, `cargo test -p vtcode interaction_loop_runner`, and `cargo test -p vtcode --bin vtcode inline_events::tests`.
- [x] Step 7: Refresh the harness evidence after the code split. Outcome: [TECH_DEBT_TRACKER.md](../TECH_DEBT_TRACKER.md) and [QUALITY_SCORE.md](../QUALITY_SCORE.md) now reflect the updated hotspot audit, the reduced `interaction_loop_runner.rs` root, and the next highest-priority TUI gaps (`src/agent/runloop/unified/turn/session_loop_runner/mod.rs` and `src/agent/runloop/unified/session_setup/ui.rs`). Verify: `find src -name '*.rs' -type f -exec wc -l {} + | sort -nr | head -n 20`, `python3 scripts/check_docs_links.py`, and `./scripts/check-dev.sh --test`.

## Decision Log

- 2026-05-15: Opened this plan immediately after revalidating TD-005 because the harness tracker and quality score now agree the debt is active, and the repo had no existing exec-plan files despite the documented `docs/harness/exec-plans/` workflow.
- 2026-05-15: Scoped the plan to TUI runloop and slash-command surfaces instead of all oversized `src/` files because the quality-score priority action is specifically about runloop/slash-command decomposition plus modal-flow integration coverage.
- 2026-05-15: Chose to phase the work around existing seams (`slash_commands.rs`, `slash_command_handler.rs`, `slash_commands/mod.rs`, and handler submodules) so behavior-preserving extraction can happen incrementally rather than via a single large rewrite.
- 2026-05-15: Completed the first slash-command split by extracting `models.rs`, `dispatch.rs`, `builtins.rs`, and `tests.rs`, while intentionally leaving `flow.rs`, `management.rs`, `parsing.rs`, and `rendering.rs` untouched to minimize behavior risk and keep the existing crate-level API stable.
- 2026-05-15: Completed the unified outcome-routing split by moving control-state handlers into `control.rs`, rewind/checkpoint flows into `rewind.rs`, and central outcome matching into `outcome_router.rs`, while keeping `handlers.rs` as the compatibility hub for nested handler modules.
- 2026-05-15: Reduced `src/agent/runloop/unified/turn/session/slash_commands/ui.rs` below the 500-line target by extracting statusline and terminal-title workflows into `ui/statusline.rs` and `ui/terminal_title.rs`, including their focused tests, instead of leaving those long-lived modal flows embedded in the root UI handler.
- 2026-05-16: Started Step 5 with the largest remaining slash-command hotspot by extracting delegated-thread inspectors, background subprocess inspectors, runtime status rendering, and subprocess action helpers from `src/agent/runloop/unified/turn/session/slash_commands/agents.rs` into `src/agent/runloop/unified/turn/session/slash_commands/agents/runtime.rs`, reducing the root file to 749 lines while preserving the existing tests through the parent module surface.
- 2026-05-16: Completed the remaining Step 5 slash-command splits by extracting the share-log timeline export and self-contained HTML renderer into `src/agent/runloop/unified/turn/session/slash_commands/share_log/timeline.rs` and the diagnostics memory-control surface into `src/agent/runloop/unified/turn/session/slash_commands/diagnostics/memory.rs`, leaving both roots as thin orchestration modules and keeping their focused tests with the moved code.
- 2026-05-16: Completed Step 6 by extracting the interaction-loop helper cluster into `src/agent/runloop/unified/turn/session/interaction_loop_runner/support.rs`, reducing `interaction_loop_runner.rs` from 1734 lines to 634 lines and collapsing the inline-action modal/resume match behind `resolve_inline_loop_action`.
- 2026-05-16: Completed Step 7 by refreshing the TUI hotspot audit after the split; `interaction_loop_runner.rs` dropped out of the top hotspot list, while the next highest-priority TUI gaps are now `src/agent/runloop/unified/turn/session_loop_runner/mod.rs` (1737 lines) and `src/agent/runloop/unified/session_setup/ui.rs` (1699 lines).

## Retrospective

### What went well

-

### What didn't

-

### What to change

-
