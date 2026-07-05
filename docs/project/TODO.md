- Test-harness gap recorded in memory: cargo test -p vtcode --lib runs 0 tests because src/lib.rs has no mod agent. Use cargo test -p vtcode --bin vtcode to run the ~1848 inline agent tests.
- **FIXED**: em-dash guidance string (`--` → `—` in loop detection note)
- **FIXED**: 5 compaction test failures (Session Memory Envelope missing, retained user message count, targeted compacted_len). Root cause: `collect_retained_user_messages` in `vtcode-core/src/compaction/mod.rs` scored tool messages higher than user messages, causing zero user messages to be retained. Fixed by prioritizing user message retention in two-phase selection.
- 2 pre-existing baseline failures remain open: `reused_recent_result` not appearing in `repeated_identical_readonly_call_in_same_turn_reuses_recent_result` and `repeated_read_only_guard_dedups_plan_file_in_planning_mode`. Pre-existing, unrelated to compaction/subagent changes.
- Findings recorded in .vtcode/memory/gotchas.md and issues.md for cross-session continuity.

---

CRITICAL: auto setup to auto ignore

.vtcode/logs
.vtcode/history/

on running workspace setup, the .vtcode/logs and .vtcode/history/ directories are created. These should be added to .gitignore to avoid committing logs and history files to version control. on install. maybe gitignore should be auto updated to include these directories. This is a critical step to prevent sensitive information from being accidentally committed to the repository.
