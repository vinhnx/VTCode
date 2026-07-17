# Loop Engineering in vtcode

This document explains how vtcode implements the loop engineering pattern described by [Addy Osmani](https://addyosmani.com/blog/loop-engineering/), and maps each primitive to the module that implements it.

## The Five Primitives

| # | Primitive | vtcode Module | Purpose |
|---|-----------|--------------|---------|
| 1 | **Automations** | `crates/codegen/vtcode-config/src/core/automation.rs` | `LoopEngineConfig` controls `enabled`, `max_iterations`, `reconcile_on_complete`, `preload_skills`. The `loop_engine_enabled()` function gates the loop with an env-var override (`VTCODE_DISABLE_LOOP_ENGINE`). |
| 2 | **Worktrees** | `crates/codegen/vtcode-core/src/git/worktree.rs` | `WorktreeManager` wraps `git worktree add/list/remove` to give each loop iteration an isolated working tree under `.vtcode/worktrees/`. Parallel loops never collide on the working tree. |
| 3 | **Skills** | `crates/codegen/vtcode-core/src/skills/manager.rs` | `SkillsManager::loop_skills()` preloads named skills into the agent's context. `resolve_skill_by_name()` resolves a single skill on demand. Skills become a structured runtime input, not just a file listing. |
| 4 | **Sub-agents** | `crates/codegen/vtcode-core/src/subagents/mod.rs` (+ `controller_spawn_run.rs` / `controller_verify.rs`) | `SubagentController` spawns child agents with `spawn_with_spec()`. When `isolation == "worktree"`, the child runs in a git worktree. After the child finishes, `run_worktree_reconciliation()` runs a verify-then-merge cycle. |
| 5 | **Memory** | `crates/codegen/vtcode-core/src/loop_memory.rs` | `LoopMemoryStore` trait with `MarkdownLoopMemory` (default) and `SqliteLoopMemory` (behind `sqlite` feature). Stores loop-level notes and decisions that survive across invocations. |

## Reconcile Flow

```
External scheduler
  -> vtcode (harness invocation)
    -> SubagentController::launch_child()
      -> child runs in worktree
      -> child_loop completes
      -> run_worktree_reconciliation()
        -> WorktreeReconciler::reconcile()
          -> git diff
          -> verify closure (heuristic check)
          -> if approved: git merge + cleanup
          -> if rejected: skip merge
```

The verify closure runs synchronously inside `spawn_blocking` to avoid `Send` constraint issues with the recursive spawn chain. The full verifier sub-agent path (`verify_proposed_change`) is available for non-reconciliation verification flows.

## Configuration

```toml
[automation.loop_engine]
enabled = true
max_iterations = 100
reconcile_on_complete = true
preload_skills = ["my-skill", "another-skill"]
```

## Feature Flags

- `sqlite` — enables `SqliteLoopMemory` backed by `rusqlite` (bundled SQLite). Without this flag, only `MarkdownLoopMemory` is available.
