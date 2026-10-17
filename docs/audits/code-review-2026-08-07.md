# Code Review + Fixes — 2026-08-07

Scope: vtcode-core, vtcode-llm. Everything green: `./scripts/check-dev.sh` (fmt + clippy + check) and
`cargo nextest run --workspace --profile quick` (8635 tests pass). Transport tests under `--features copilot` (8/8
pass).

## Completed fixes

| Finding                                                | Root cause                                                                                                                                                           | Fix                                                                                                                                                                                        | Files                                                                            | Regression tests                                                                                                                                              |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| acp_client `send_prompt_update` deadlock               | `active_prompt: StdMutex<Option<ActivePrompt>>`; let-chain kept the std Mutex guard alive while `clear_active_prompt_state` re-locked the same Mutex → self-deadlock | Clone the `UnboundedSender` under the lock, drop the guard, then send and clear outside                                                                                                    | `vtcode-llm/…/acp_client.rs`                                                     | 1 (deadlock no longer panics)                                                                                                                                 |
| transport pending-map leak                             | `StdioTransport::call` removes pending entry on response dispatch, but not on timeout path                                                                           | `map_err` closure now removes the pending entry                                                                                                                                            | `vtcode-llm/…/transport.rs`                                                      | `timed_out_call_clears_pending_entry`                                                                                                                         |
| compaction `coherence_tool_call_pairs` dead force-keep | Force-keep filtered args from a single source instead of accumulating the union across all history entries                                                           | Emit from union of `keep` set across all indices, drop orphaned Tool results                                                                                                               | `compaction/mod.rs`                                                              | 2 (distributed-batch + orphaned-tool cases)                                                                                                                   |
| web_fetch O(n²) UTF-8 truncation                       | Byte-by-byte decrement over multibyte chars                                                                                                                          | `err.valid_up_to()` for O(1) boundary; Content-Length precheck + streaming `bytes_stream()` capped at `MAX_BODY_BYTES`                                                                     | `tools/web_fetch/mod.rs`                                                         | 3 (multibyte truncation, cap-at-limit, oversize-body)                                                                                                         |
| read_file / file_ops unbounded line reads              | `read_until` on unbounded reader could over-allocate                                                                                                                 | New `read_bounded_line` in `file_ops/mod.rs` via `reader.take(MAX_LINE_READ_BYTES + 1).read_until`; drain uses chunked `take(4096).read_until`                                             | `tools/file_ops/mod.rs`, `tools/handlers/read_file.rs`, `tools/file_ops/read.rs` | 1 regression (oversized-line bails early)                                                                                                                     |
| execution_facade `mutated_target_paths`                | Duplicated and missed `destination`/`destination_path`                                                                                                               | Delegates to `apply_patch::mutation_target_paths`; added `invalidate_all_reads` + `is_pathless_mutating_command` fallback                                                                  | `execution_facade.rs`, `execution_history.rs`                                    | 1 (`invalidate_all_reads_drops_read_records_only`)                                                                                                            |
| loop_detector identical-call false positives           | Write/edit tools' normalized hash nulls content → distinct edits collided; exec_command's `cmd` key ignored; `::read` suffix only                                    | `is_write_tool_name` exemption; `read_target_for_tool_call` now handles `file_operation` `action: "read"` and `__read__:path` normalization; `is_verification_or_read_command` reads `cmd` | `core/loop_detector.rs` → `core/loop_detector/`                                  | 6 (`write_edit_repeat`, `apply_patch_repeat`, `exec_cmd_verification`, `exec_cmd_without_read_target_breaks_streak`, `reset_clears_window`, `checkpoint_324`) |
| loop_detector navigation streak                        | Read-only streak not reset by mutating shell tools                                                                                                                   | `is_mutating` branch now matches `is_command_tool_name && current_read_target.is_none()`                                                                                                   | `core/loop_detector/mod.rs`                                                      | included above                                                                                                                                                |
| loop_detector navigation streak                        | Read-only streak not reset by exec_command (production shell tool)                                                                                                   | Same branch now handles `tools::UNIFIED_EXEC` when `current_read_target.is_none()`                                                                                                         | `core/loop_detector/mod.rs`                                                      | included above                                                                                                                                                |
| loop_detector sliding window                           | Non-consecutive repeats A→B→A were missed                                                                                                                            | Sliding window of composite `(tool_name, args_hash)` over `DETECTION_WINDOW`; trailing identical entries skipped via `skip_while`                                                          | `core/loop_detector/mod.rs`                                                      | existing `test_immediate_repetition_detection` + `test_command_tools_skip_identical_hard_stop`                                                                |

## Decomposition: `core/loop_detector.rs` → `core/loop_detector/mod.rs` + `normalization.rs`

- Root (`mod.rs`): `ToolCallRecord`, `LoopDetector` impl, constants, tests. Public surface unchanged: `LoopDetector`,
  `ToolCallRecord`, `MAX_TOTAL_READONLY_CALLS`.
- Submodule (`normalization.rs`): pure helpers — `base_tool_name`, `is_command_tool_name`, `is_write_tool_name`,
  `canonicalize_command_for_detection`, `hash_normalized_args`, `hash_normalized_code_search_args`,
  `normalize_args_for_detection`, `read_target_for_tool_call`, `is_file_operation_read`. Visibility `pub(super)`;
  re-exported via `use self::normalization::*;` so all internal call sites and tests keep working unchanged.
- Old `loop_detector.rs` deleted. No external caller referenced the moved helpers (verified across `vtcode-core/src`).
- 62/62 loop_detector tests pass; 8635 workspace tests pass.

## Intentionally not changed

- Readonly budget off-by-one (#12). Existing tests assert `HARD STOP` fires at exactly `MAX_TOTAL_READONLY_CALLS` (30),
  so `>= readonly_budget` is the documented intent.

## Verification summary

```text
./scripts/check-dev.sh            # fmt + clippy + check → PASS
cargo nextest run -p vtcode-core -E 'test(loop_detector)'   # 62/62
./scripts/check-dev.sh --changed --test                    # 5480/5480
cargo nextest run -p vtcode-llm --features copilot -E 'test(transport)'  # 8/8
cargo nextest run --workspace --profile quick              # 8635/8635
```

## Status

Everything in the action plan is implemented, green, and ready to commit.
