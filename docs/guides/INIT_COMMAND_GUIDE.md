# Agent Initialization Guide

## Overview

The `/init` command prepares a repository for VT Code guidance and memory. It still generates a root `AGENTS.md` that complies with the open specification published at [agents.md](https://agents.md/), and it now also scaffolds the repository rule and persistent-memory layout that VT Code uses at runtime.

## Key Features

- **Specification alignment** – follows the section structure encouraged by agents.md and produces Markdown that other tooling can parse without customization.
- **Repository analysis** – detects languages, build tools, dependency manifests, documentation, and CI artifacts to tailor instructions.
- **Focused guidance** – surfaces the most relevant commands and conventions within the recommended 200–400 word budget.
- **Workspace scaffolding** – creates `.vtcode/rules/README.md` and initializes the per-repository memory directory layout used by VT Code.
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

The assistant will analyze the repository, synthesize the relevant guidance, and scaffold the workspace instruction layout. By default that includes `AGENTS.md` at the workspace root, `.vtcode/rules/README.md`, and the repository memory directory layout.

## Generated Content Structure

The resulting workspace scaffold includes:

- `AGENTS.md` – root project guidance file generated from repository analysis.
- `.vtcode/rules/README.md` – starter documentation for modular workspace rules.
- Persistent memory files under the repository memory directory:
  - `memory_summary.md`
  - `MEMORY.md`
  - `preferences.md`
  - `repository-facts.md`
  - `rollout_summaries/`

The generated `AGENTS.md` always includes the following sections when data is available:

- `# AGENTS.md` – top-level heading for compatibility.
- `## Project overview` – high-level summary of languages, directories, and automation.
- `## Setup commands` – environment preparation commands grouped by detected tooling.
- `## Code style` – formatter and naming expectations for each language.
- `## Testing instructions` – how to execute local checks and match CI requirements.
- `## PR instructions` – commit hygiene and review guidelines.
- `## Additional context` – optional section with documentation pointers and highlighted dependencies.

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

## Project overview

- Primary languages: Rust
- Key directories: `src/`, `tests/`
- Application entrypoints live under the source directories above.
- Continuous integration workflows detected; review `.github/workflows/` for required checks.
- Docker artifacts detected; container workflows may be required for local testing.

## Setup commands

- Install the Rust toolchain via `rustup` and warm the cache with `cargo fetch`.
- Container workflows available; use `docker compose up --build` when services are required.

## Code style

- Rust: 4-space indentation, snake_case functions, PascalCase types, run `cargo fmt` and `cargo clippy`.

## Testing instructions

- Run Rust tests with `cargo test` and address clippy warnings.
- Match CI expectations; replicate workflows from `.github/workflows` when possible.

## PR instructions

- Use Conventional Commits (`type(scope): subject`) and keep summaries under 72 characters.
- Reference issues with `Fixes #123` or `Closes #123` when applicable.
- Run linters and test suites before opening a pull request; attach logs for failures.
- Keep pull requests focused; split large features into reviewable chunks.

## Additional context

- Additional documentation available in: README.md.
- Rust (Cargo) dependencies include anyhow, serde, tokio (see manifest for more).
```

Regenerate the root guidance whenever the build, testing, or review process changes so future contributors and agents stay aligned. For more on runtime guidance loading and persistent-memory behavior, see [Guidance and Persistent Memory for VT Code](./memory-management.md).
