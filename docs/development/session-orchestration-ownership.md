# Session orchestration ownership

The binary keeps one session loop in
`agent/runloop/unified/turn/session_loop_runner/orchestration/mod.rs`. The
facade owns the unified `loop` and the state that outlives one session; one
iteration — critical setup and interaction — runs in
`orchestration/session_iteration.rs`, and its session tail runs in
`orchestration/session_teardown.rs`. Private helpers prepare inputs or complete
bounded lifecycle phases; they do not dispatch turns or create another session
runner.

| Owner | Responsibility | Ordering boundary |
| --- | --- | --- |
| Orchestration loop (`orchestration/mod.rs`) | Cross-iteration state (config, resume, config watcher, vt config, pending start trigger and primary-agent override, steering receiver) and restart dispatch | Borrows that state into one iteration; `ControlFlow::Continue` restarts for resume/`/new`, `ControlFlow::Break` ends the loop |
| `orchestration/session_iteration.rs` | One session: critical setup, session-owned guards, initialization dispatch, and the interaction loop | Guard declarations stay in one scope and retain their ordering; the tail receives borrowed state (plus the two values `finalize_session` consumes owned) |
| `orchestration/session_iteration/initialization.rs` | Registry completion, deferred runtime hydration, post-hydration UI refresh, and session-start hooks | Borrows live session/UI state after first paint; cancellation returns `ControlFlow::Break`, errors return to the caller's canonical finalization, and the caller retains the progress guard |
| `orchestration/session_iteration/resume.rs` | Durable follow-up restoration and interrupted-prompt/tracker continuation | Restore durable intents first; preserve prompt identity/provenance, require an actual resume before consuming trailing input, and retain tracker adoption, stop, planning, and continuation-budget gates |
| `orchestration/session_bootstrap.rs` | Thread/archive preparation, summarized-fork history, primary-agent persistence, plan-selection failure tails, and completion classification | Constructs a provider only for summarized forks; prepares identity/archive before summarizing and activating the thread |
| `session_setup` | Critical/UI setup, registry completion, deferred hydration, and session-start hooks | Typeable shell comes first; hooks follow hydration |
| `turn_loop/compaction.rs` | Request-boundary automatic/recovery compaction, scoped progress, and history-index rebasing | The turn loop validates pricing first and owns the stop decision; compaction preserves pending steering and bounded recovery semantics |
| `turn_loop/finalization.rs` | Terminal turn events, bounded exec-session termination, undispatched tool-item closure, and notifications | Publish status, perform cancellation/exit cleanup, emit canonical outcomes, close remaining streamed items, then notify |
| `turn_tail` | Per-turn metrics and persistence | Turn outcome and checkpoint metadata remain loop-owned inputs |
| `orchestration/session_teardown.rs` | The session exit tail (canonical terminal event, one bounded maintenance group, terminal/TUI restoration, exit summary, and the resume/`/new` restart decision) over bounded drains, completed-artifact cleanup, persistent-memory kickoff, and subagent shutdown | Runs the tail in the order the iteration used inline: terminal event first, then the shared deadline for independent branches, and only then restoration and postamble; completed results survive the deadline independently of optional maintenance |

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
errors propagate. The session tail keeps both call sites and their ordering.

The focused bootstrap tests cover fresh archive-less startup, in-place resume
under both archive policies, archive-less full-copy forks, and provider failure
ordering. Existing core archived-session and binary summarized-fork tests cover
archive-backed forks and successful compaction separately. Live provider
startup and the full interactive new-session/resume loop remain separate checks.

Follow-up restoration tests cover fresh versus resumed inputs, user versus
synthetic provenance and stable identity, durable FIFO ordering and applied-ID
deduplication, and tracker continuation with adoption and stop requests.

Headless execution keeps task lifecycle, execution budgets, and cleanup in
`vtcode-core/src/core/agent/runner/execute.rs`. Its sibling `runtime_prompt.rs`
owns prompt-bundle construction, catalog alignment, and refresh. A catalog-only
version change preserves the existing frozen envelope when provider-visible
prompt and tool identities are unchanged.
The sibling `completion.rs` owns evaluator acceptance and the shared pre/post
verification assessment path; verification admission, budgets, and terminal
task lifecycle remain in `execute.rs`.

```sh
cargo nextest run --locked -p vtcode -p vtcode-core -E 'test(session_loop_runner) | test(summarized_fork_history) | test(core::threads::tests::prepare_archived_session)'
```
