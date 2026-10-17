# Session orchestration ownership

The binary keeps one session loop in
`agent/runloop/unified/turn/session_loop_runner/orchestration/mod.rs`.
Private helpers prepare inputs or complete bounded lifecycle phases; they do
not dispatch turns or create another session runner.

| Owner | Responsibility | Ordering boundary |
| --- | --- | --- |
| Orchestration loop | Metadata/config policy, thread activation, UI setup, input/turn coordination, and session transitions | Activates the prepared thread and checkpoints startup before UI initialization |
| `orchestration/session_bootstrap.rs` | Thread/archive preparation, summarized-fork history, primary-agent persistence, plan-selection failure tails, and completion classification | Constructs a provider only for summarized forks; prepares identity/archive before summarizing and activating the thread |
| `session_setup` | Critical/UI setup, registry completion, deferred hydration, and session-start hooks | Typeable shell comes first; hooks follow hydration |
| `turn_tail` | Per-turn metrics and persistence | Turn outcome and checkpoint metadata remain loop-owned inputs |
| `orchestration/session_teardown.rs` | Bounded drains, completed-artifact cleanup, persistent-memory kickoff, and subagent shutdown | The loop emits the canonical terminal event before draining; completed results survive the shared deadline independently of optional maintenance |

Thread preparation returns a named record containing identity, bootstrap history,
and optional archive. The loop supplies history policy and reserved identity;
the helper retains the existing archive and archive-less resume/fork adapters.
In-place resume keeps the source identity. Forks use the reserved or generated
new identity and preserve parent metadata and compatible prompt-cache lineage.
Fresh archive-less startup generates identity without creating archive storage.

Both archive policies use one summarized-fork history call with the same source,
target, history, workspace, and budget-continuation inputs. Provider initialization
errors still occur before archive preparation; compaction errors still propagate
before thread activation. The loop retains runtime archive-ID publication,
startup checkpointing, and canonical harness initialization/finalization.

The existing bootstrap module also owns the config-reload polling helper used
before finalization and at new-session handoff. Poll/debounce policy remains
watcher-owned. Reloads preserve runtime provider/CLI model overrides, rejected
reloads retain the last valid configuration and show one warning, and renderer
errors propagate. The loop keeps both call sites and their ordering.

The focused bootstrap tests cover fresh archive-less startup, in-place resume
under both archive policies, archive-less full-copy forks, and provider failure
ordering. Existing core archived-session and binary summarized-fork tests cover
archive-backed forks and successful compaction separately. Live provider
startup and the full interactive new-session/resume loop remain separate checks.

```sh
cargo nextest run --locked -p vtcode -p vtcode-core -E 'test(session_loop_runner) | test(summarized_fork_history) | test(core::threads::tests::prepare_archived_session)'
```
