# Enhanced Init Command Implementation

## Overview

The enhanced `/init` workflow now targets the open [agents.md](https://agents.md/) specification. Rather than generating a generic "Repository Guidelines" document, the command crafts an `AGENTS.md` file that mirrors the canonical sections agents expect: setup commands, code style, testing instructions, pull-request guidance, and optional project context. The implementation remains repository-agnostic so maintainers can apply it across any workspace.

## Specification Alignment

1. **Title and headings** – emits `# AGENTS.md` with secondary headings that match the examples promoted on agents.md.
2. **Section ordering** – presents setup, style, testing, and review guidance in a predictable order so downstream agents can parse precedence.
3. **Fallback messaging** – fills empty sections with prompts reminding maintainers to add project-specific rules, ensuring the file never ships blank guidance.
4. **Hierarchical compatibility** – the generated file integrates with vtcode's instruction loader, which merges global rules, configured extras, and nested `AGENTS.md` files in scope order.

## Analysis Enhancements

- **Language detection** – inspects manifests and source directories to identify Rust, JavaScript/TypeScript, Python, Go, Java/Kotlin, and other ecosystems.
- **Build and runtime tooling** – records Cargo, npm/yarn/pnpm, pip/poetry, Go modules, Maven/Gradle, and Docker artifacts to drive setup/test sections.
- **Documentation discovery** – captures README, CHANGELOG, CONTRIBUTING, and similar files to reference in the Additional context section.
- **Dependency surfacing** – highlights notable dependencies per ecosystem so maintainers can quickly spot major libraries.
- **Commit heuristic** – samples recent git history to determine whether Conventional Commits are in use and updates PR guidance accordingly.

## Generated Section Logic

```rust
fn generate_agents_md(analysis: &ProjectAnalysis) -> Result<String> {
    // 1. # AGENTS.md header
    // 2. Project overview (languages, directories, automation hints)
    // 3. Setup commands mapped from detected build systems
    // 4. Code style bullets per language
    // 5. Testing instructions per build tool + CI reminders
    // 6. PR instructions derived from commit analysis
    // 7. Additional context (docs + dependencies)
}
```

Each helper returns `None` when no meaningful data exists; the formatting layer inserts a placeholder bullet to nudge maintainers toward filling the gap.

## Usage Patterns

- **Monorepos** – run `/init` at the root and within major packages to produce scoped `AGENTS.md` files. vtcode's loader reads the closest file when editing.
- **Existing repositories** – rerun `/init` whenever you introduce new tooling (for example adding Docker or enabling CI workflows).
- **Fresh projects** – use the generated file as a scaffold; edit sections directly to document organization-specific policies.

## Example Comparison

Previous behavior (legacy):

```markdown
# Repository Guidelines

## Project Structure & Module Organization
- `src/` - Source code
```

New behavior (agents.md compliant):

```markdown
# AGENTS.md

## Setup commands
- Install the Rust toolchain via `rustup` and warm the cache with `cargo fetch`.

## Code style
- Rust: 4-space indentation, snake_case functions, PascalCase types, run `cargo fmt` and `cargo clippy`.
```

The reworked output is shorter, directly actionable, and consistent with the language other agent platforms already consume.

## Maintenance Tips

- Keep the generator in sync with updates to [agents.md](https://agents.md/); new sections can be slotted into the helper functions without disrupting existing output.
- When vtcode gains richer analysis signals (for example static analysis results), extend the Additional context section rather than inventing new headings.
- Encourage contributors to treat `AGENTS.md` as living documentation—re-run `/init` for scaffolding, then commit manual edits that capture nuanced workflows.
