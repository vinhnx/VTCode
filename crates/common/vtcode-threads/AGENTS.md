<!-- markdownlint-disable MD013 -->
# vtcode-threads

[Root AGENTS.md](../../../AGENTS.md) | Thread identifiers extracted from `vtcode-core`.

## Modules

- `lib.rs` — `ThreadId`, `SubmissionId` (StringNewtype).

## Rules

- Preserve `StringNewtype` derive and `v4` UUID generation for `SubmissionId`.
- Keep `vtcode-core/src/core/threads.rs` facade: `pub use vtcode_threads::{ThreadId, SubmissionId}`.

See root [AGENTS.md](../../../AGENTS.md) for shared crates guidance.
