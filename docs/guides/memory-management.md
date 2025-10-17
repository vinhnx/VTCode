# Instruction Memory Management for VT Code

VT Code keeps project guidance in sync by merging `AGENTS.md` files from several scopes. Understanding how those files are
discovered and combined lets you fine-tune what the agent remembers at startup and when navigating a repository.

## Instruction Sources and Precedence

VT Code reads instruction files from the most general scope to the most specific. Later entries override or augment the
previous ones because they are appended to the combined prompt.

| Load order | Scope | Location | Purpose |
| --- | --- | --- | --- |
| 1 | Personal (global) | `~/AGENTS.md`, `~/.vtcode/AGENTS.md`, `~/.config/vtcode/AGENTS.md` | Persistent preferences that apply to every project you open. |
| 2 | Custom patterns | Paths or globs listed in `agent.instruction_files` inside `vtcode.toml` | Extra documentation (for example, runbooks in `docs/`) that should be injected for every session. |
| 3 | Workspace root | `<repo>/AGENTS.md` | Shared rules for the repository. Regenerate with `/init` to refresh the boilerplate. |
| 4 | Nested worktree | Any `AGENTS.md` encountered between the repository root and the active working directory | Directory-specific overrides for subsystems, packages, or apps. |

## Discovery Algorithm

When VT Code builds its instruction bundle, it:

1. Canonicalizes the project root and current working directory.
2. Loads every available personal `AGENTS.md` file from your home directory.
3. Expands any custom `instruction_files` patterns into absolute paths and includes the matches.
4. Walks from the repository root toward the current directory, loading each `AGENTS.md` it finds along the way.
5. Truncates the combined content if it exceeds `agent.instruction_max_bytes` (16 KiB by default) to keep prompts performant.

This process runs each time VT Code refreshes context, so editing or adding a scoped `AGENTS.md` immediately influences future
turns.

## Maintaining Effective AGENTS.md Files

- Keep guidance concise and action oriented - bullet lists and short paragraphs are easier for the agent to honor.
- Link out to full documentation instead of pasting long tutorials; VT Code only guarantees the first `instruction_max_bytes`
  bytes will be loaded.
- Prefer nested `AGENTS.md` files over massive root documents when teams own distinct subsystems.
- Store secrets or personal API keys in environment variables or `vtcode.toml`, never in `AGENTS.md`.
- Regenerate the root file with `/init` after large refactors, then customize the scaffolded sections with project specifics.

By organizing your instruction hierarchy with these practices, VT Code consistently honors organizational policy while leaving
room for team- and developer-level preferences.
