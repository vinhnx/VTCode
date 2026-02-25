# Agent Initialization Guide

## Overview

The `/init` command generates an `AGENTS.md` file that complies with the open specification published at [agents.md](https://agents.md/). The generated document gives coding agents a predictable place to find setup steps, code style conventions, testing workflows, and pull-request expectations for any repository.

## Key Features

- **Specification alignment** – follows the section structure encouraged by agents.md and produces Markdown that other tooling can parse without customization.
- **Repository analysis** – detects languages, build tools, dependency manifests, documentation, and CI artifacts to tailor instructions.
- **Focused guidance** – surfaces the most relevant commands and conventions within the recommended 200–400 word budget.
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

The assistant will analyze the repository, synthesize the relevant guidance, and write (or overwrite) `AGENTS.md` at the workspace root.

## Generated Content Structure

The resulting document always includes the following sections when data is available:

- `# AGENTS.md` – top-level heading for compatibility.
- `## Project overview` – high-level summary of languages, directories, and automation.
- `## Setup commands` – environment preparation commands grouped by detected tooling.
- `## Code style` – formatter and naming expectations for each language.
- `## Testing instructions` – how to execute local checks and match CI requirements.
- `## PR instructions` – commit hygiene and review guidelines.
- `## Additional context` – optional section with documentation pointers and highlighted dependencies.

Empty sections are replaced with actionable placeholders so maintainers know where to add project-specific details.

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

Regenerate the file whenever the build, testing, or review process changes so future contributors and agents stay aligned.
