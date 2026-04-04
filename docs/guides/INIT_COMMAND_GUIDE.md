# Agent Initialization Guide

## Overview

The `/init` command prepares a repository for VT Code guidance and memory. It generates a root `AGENTS.md` that complies with the open specification published at [agents.md](https://agents.md/), scaffolds the repository rule and persistent-memory layout that VT Code uses at runtime, and now runs a guided AGENTS setup when key guidance is ambiguous.

## Key Features

- **Specification alignment** – follows the section structure encouraged by agents.md and produces Markdown that other tooling can parse without customization.
- **Repository analysis** – inspects manifests, scripts, docs, CI workflows, and recent git history to tailor instructions.
- **Targeted questions** – asks up to three high-value questions only when verification commands, orientation docs, or one critical repo rule are not obvious from the codebase.
- **Focused guidance** – surfaces the most relevant commands and conventions within the recommended 200–400 word budget.
- **Workspace scaffolding** – creates `.vtcode/README.md` and initializes the per-repository memory directory layout used by VT Code.
- **Portable output** – works for any project layout; update the file as conventions evolve and regenerate when new components are added.

## Usage

1. Navigate to the target repository:

   ```bash
   cd /path/to/project
   ```

2. Launch vtcode:

   ```bash
   ./run.sh
   ```

3. Run the initialization command from chat:

   ```
   /init
   ```

The assistant will analyze the repository, synthesize the relevant guidance, ask targeted questions when needed, and scaffold the workspace instruction layout. By default that includes `AGENTS.md` at the workspace root, `.vtcode/README.md`, and the repository memory directory layout.

You can run the same flow from the CLI with:

```bash
vtcode init
```

Use `vtcode init --force` or `/init --force` to overwrite an existing `AGENTS.md` without an overwrite confirmation.

## Generated Content Structure

The resulting workspace scaffold includes:

- `AGENTS.md` – root project guidance file generated from repository analysis.
- `.vtcode/README.md` – starter documentation for workspace prompt files and rule placement.
- Persistent memory files under the repository memory directory:
  - `memory_summary.md`
  - `MEMORY.md`
  - `preferences.md`
  - `repository-facts.md`
  - `rollout_summaries/`

The generated `AGENTS.md` always includes the following sections when data is available:

- `# AGENTS.md` – top-level heading for compatibility.
- `## Quick start` – environment preparation commands and the default verification command when selected.
- `## Architecture & layout` – high-level summary of languages, directories, entrypoints, and the preferred orientation doc when selected.
- `## Important instructions` – optional repo-wide rule captured from guided setup.
- `## Code style` – formatter and naming expectations for each language.
- `## Testing` – how to execute local checks and match CI requirements.
- `## PR guidelines` – commit hygiene and review guidelines.
- `## Additional guidance` – optional section with documentation pointers and highlighted dependencies.

Empty sections are replaced with actionable placeholders so maintainers know where to add project-specific details.

## Relationship to Rules and Memory

`/init` sets up the three main VT Code guidance surfaces:

- `AGENTS.md` for shared, human-authored project instructions
- `.vtcode/rules/` for modular always-on or path-scoped rules
- the per-repository memory directory for learned persistent memory

After initialization:

- use `AGENTS.md` for project-wide guidance
- add focused rule files under `.vtcode/rules/` when only some paths need extra instructions
- use `/memory` or `/config memory` to inspect and tune persistent-memory behavior

## Example Output

For a Rust service with Docker support and conventional commits:

```markdown
# AGENTS.md

## Quick start

- Default verification command: `./scripts/check.sh` before calling work complete.
- Build with `cargo check` (preferred) or `cargo build --release`.

## Architecture & layout

- Start with `docs/ARCHITECTURE.md` when you need repo orientation or architectural context.
- Primary languages: Rust.
- Key source directories: `src/`, `tests/`.

## Important instructions

- Use Conventional Commits (`type(scope): subject`).

## Code style

- Rust code uses 4-space indentation, snake_case functions, PascalCase types, and `anyhow::Result<T>` with `.with_context()` for fallible paths.

## Testing

- Default verification command: `./scripts/check.sh`.
- Rust suite: `cargo nextest run` for speed, or `cargo test` for targeted fallback.

## PR guidelines

- Use Conventional Commits (`type(scope): subject`) and keep summaries under 72 characters.
- Reference issues with `Fixes #123` or `Closes #123` when applicable.
- Keep pull requests focused and include test evidence for non-trivial changes.

## Additional guidance

- Preferred orientation doc: `docs/ARCHITECTURE.md`.
- Repository docs spotted: README.md, docs/ARCHITECTURE.md.
```

Regenerate the root guidance whenever the build, testing, or review process changes so future contributors and agents stay aligned. For more on runtime guidance loading and persistent-memory behavior, see [Guidance and Persistent Memory for VT Code](./memory-management.md).
