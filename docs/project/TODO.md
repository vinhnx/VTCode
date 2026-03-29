NOTE: use deepwiki mcp to reference from codex https://deepwiki.com/openai/codex

---

extract and open source more components from vtcode-core

---

CODEX plus

main account
kiweuro
writedownapp
humidapp
vtchat.io

---

https://code.claude.com/docs/en/headless

---

hooks

https://developers.openai.com/codex/hooks

https://deepwiki.com/search/how-does-hooks-works-in-codex_68383f0e-ec03-44eb-be92-69a26aa3d1e1?mode=fast

https://code.claude.com/docs/en/hooks

==

plugins and LSP

https://code.claude.com/docs/en/discover-plugins

https://code.claude.com/docs/en/plugins-reference

https://developers.openai.com/codex/plugins

https://deepwiki.com/search/httpsdevelopersopenaicomcodexp_ee8404d4-ca94-48ac-9fad-60e24e3b4f5a?mode=fast

---

High-value Rust codemods to build for VT Code (and the broader ecosystem):

Codemod
Effort
Credit
tokio 1.x → 2.x migration
M
$200
clap v3 → v4 derive API
S
$100
ratatui breaking changes (VT Code uses this)
S–M
$100–200
serde attribute renames / deprecations
S
$100
anyhow / thiserror v1 → v2
S
$100
reqwest breaking changes
S
$100
hyper 0.14 → 1.0 (massive pain point)
L
$400
actix-web v3 → v4
M
$200
sqlx 0.7 → 0.8
M
$200
tree-sitter API changes (VT Code uses this)
S
$100

Being first to publish any quality Rust codemod also positions you for the $2,000 framework adoption tier — e.g., getting ratatui or tokio maintainers to reference your codemod in their upgrade guides.

---

use https://github.com/Uzaaft/libghostty-rs/ replace existing libghostty-vt impl.

---

memory

```
# VT Code Persistent Guidance and Memory

## Summary

- Build a VT Code-native memory system using Claude’s concepts as reference, not Claude file formats.
- Keep `AGENTS.md` and nested `AGENTS.md` as the human-authored guidance surface.
- Add modular VT Code rules under `.vtcode/rules/` and `~/.vtcode/rules/`.
- Add durable per-repo learned memory under the existing user config tree, loaded into every session and maintained automatically.

## Key Changes

- Extend instruction loading in [vtcode-core/src/instructions.rs](/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/instructions.rs) and [vtcode-core/src/project_doc.rs](/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/project_doc.rs).
- Discovery order:
  1. User `AGENTS.md`
  2. User unconditional rules
  3. User matched path rules
  4. `agent.instruction_files`
  5. Workspace root and nested `AGENTS.md` from root to active scope
  6. Workspace unconditional rules
  7. Workspace matched path rules
- Within a scope, later and more specific entries win.
- Add `.vtcode/rules/**/*.md` and `~/.vtcode/rules/**/*.md`.
- Rule files support YAML frontmatter with `paths`; no `paths` means always load.
- Path-scoped rules activate from:
  - active editor file
  - visible editor files
  - current active instruction directory
  - tracked session file activity
- Expand session file activity from modified-only to explicit read/search/edit hits so read-only work can activate rules.
- Rule activation is per-turn; files first seen mid-turn affect the next prompt rebuild.
- Support `@path` imports for `AGENTS.md` and rule files.
- Import rules:
  - max depth `5`
  - relative paths resolve from the containing file
  - imported content inherits the caller’s scope and matching
  - strip block HTML comments before prompt injection
  - canonicalize and dedupe files
  - allow imports only from the workspace or VT Code user config roots
- Keep existing byte-budget behavior; when truncated, render summaries instead of full inline content.

- Add a new main-session persistent memory module, separate from subagent memory.
- Storage path: `get_config_dir()/projects/<project>/memory/`, using the same project name resolution as project-profile config.
- Files:
  - `MEMORY.md` as the startup index
  - topic files such as `debugging.md`, `workflows.md`, or `conventions.md`
- Startup load behavior:
  - inject authored guidance first
  - then inject the first `200` lines or `25 KiB` of `MEMORY.md`
- Auto-memory write pipeline:
  - run after completed turns and on session finalization
  - extract explicit user “remember” / “important” facts, durable preferences, and grounded repo facts
  - use the configured small model only for triage/classification, not for blind freeform rewrites
  - dedupe against existing memory
  - keep `MEMORY.md` concise and move detail into topic files
- Reuse the existing grounded-fact extraction in session compaction where practical instead of inventing a second heuristic stack.

- Add a VT Code `/memory` slash command.
- `/memory` should show:
  - loaded `AGENTS.md` sources
  - matched rules
  - persistent memory status and path
  - open/edit targets for the memory directory
- Extend `/init` to scaffold:
  - root `AGENTS.md` as today
  - `.vtcode/rules/README.md`
  - optional empty per-repo `MEMORY.md` index

## Public Interfaces

- Add `agent.instruction_excludes: Vec<String>` for glob-based instruction/rule exclusion.
- Add `agent.instruction_import_max_depth: usize` defaulting to `5`.
- Add `agent.persistent_memory` config:
  - `enabled = true`
  - `auto_write = true`
  - `directory_override = null`
  - `startup_line_limit = 200`
  - `startup_byte_limit = 25600`
- Only system, user, or project-profile config layers may set `directory_override`; workspace-root `vtcode.toml` must not redirect memory storage.
- Keep subagent `memory = user|project|local` unchanged in this pass.

## Test Plan

- Instruction discovery precedence across user, custom, workspace, nested, and rule sources.
- Rule matching for unconditional rules and `paths` rules using active editor files and tracked file activity.
- Import expansion, dedupe, depth limit, and allowed-root enforcement.
- Comment stripping and truncation-summary behavior.
- Persistent memory path resolution for normal repos, renamed projects, and `.vtcode-project`.
- Startup prompt ordering: authored guidance before persistent memory excerpt.
- Auto-memory dedupe, size limits, topic-file spillover, and no-op sessions.
- `/memory` command coverage and `/init` scaffolding coverage.
- Regression tests confirming existing `AGENTS.md`, fallback filename behavior, and subagent memory continue to work.

## Assumptions

- This pass is VT Code-native only; no `CLAUDE.md` or `.claude/rules/` compatibility work.
- `AGENTS.md` remains the primary human-authored map for VT Code.
- Main-session persistent memory and subagent memory stay separate for v1.
- Path-scoped rules do not hot-inject mid-tool-call; they apply on the next prompt rebuild after the matching path becomes known.

```

reference codex's memory system and reapply and use /rust-skills and enhance implementation. review overall changes again carefully, can you do better? continue with your careful recommendations, proceed with outcome. KISS and DRY but focus on main logic, no need DRY for tests, do repeatly until all done, don't stop

https://deepwiki.com/search/tell-me-about-codexs-memory-sy_b73032c0-8cff-4a21-a23d-1a0e2be7c593?mode=fast

```
make this feature interactive configurable in both /config settings and /memory command
```

---

update /config with light model config (as additional to main model) for memory triage and summarization, with a fallback to the main model if not set or if the light model fails for any reason. this allows users to choose a smaller, cheaper model for the auto-memory write pipeline while still having the robustness of the main model as a backup. also update /memory command to show the configured memory triage model and allow quick editing of that setting.

also update first time wizard to prompt for a memory triage model choice, with an explanation of the trade-offs between using a smaller model for cost savings versus using the main model for potentially better accuracy in memory extraction. provide some recommended models for memory triage based on typical use cases and budgets.
