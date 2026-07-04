https://huggingface.co/docs/hub/agents-overview#register-your-agent-harness

---

Notes for the team

- Test-harness gap recorded in memory: cargo test -p vtcode --lib runs 0 tests because src/lib.rs has no mod agent. Use cargo test -p vtcode --bin vtcode to run the ~1848 inline agent tests.
- 8 pre-existing baseline failures remain open (em-dash — vs -- in canonical guidance strings, [Session Memory Envelope] missing, reused_recent_result not appearing). These are unrelated to this fix and should be addressed separately.
- Findings recorded in .vtcode/memory/gotchas.md and issues.md for cross-session continuity.

---

fix all cargo clippy/check warnings and errors.
