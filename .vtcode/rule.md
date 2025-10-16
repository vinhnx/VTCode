# VTCode Project Rules

## Scope and Precedence
- This workspace now follows the [AGENTS.md specification](https://agents.md/) for instruction discovery.
- Merge global rules from `~/.vtcode/AGENTS.md` (legacy `~/.vtcode/rule.md` remains supported) before applying workspace guidance.
- Within the repository, traverse from the project root down to the active directory and apply each `AGENTS.md` you encounter, letting the deepest scope win when conflicts exist.
- Additional instruction files referenced in `vtcode.toml` (`agent.instruction_files`) participate in the hierarchy after global rules and before directory-scoped files.

## Rule Discovery Protocol
1. Before executing any task, enumerate all `AGENTS.md` files (and legacy `.vtcode/rule.md` shims) from the project root to the working directory.
2. Expand any configured `agent.instruction_files` globs and include those files in the discovery list.
3. Record the resolved hierarchy in the analysis channel so users can verify precedence and truncation.

## Execution Rules
- **R1 – Fidelity**: Never ignore or shortcut documented rules. If unsure, stop and clarify before proceeding.
- **R2 – Hierarchy**: Resolve conflicts by preferring the most specific scope (deepest path) and retain non-conflicting guidance from broader scopes.
- **R3 – Traceability**: Reference the relevant rule identifier (e.g., `R1`) when explaining reasoning in the final summary.

## Repository-Specific Requirements
- **Build Discipline**: Follow the project build instructions (`cargo fmt`, `cargo clippy`, `cargo test`, `cargo check`) whenever code changes are made or when requested.
- **Documentation Boundaries**: Do not place general documentation outside of `./docs/`. Configuration rule files inside `.vtcode/` are exempt from this restriction.
- **Configuration Integrity**: Avoid hardcoding values that belong in `vtcode.toml` or `vtcode-core/src/config/constants.rs`. Respect existing configuration patterns and reference constants.
- **Error Handling**: Use `anyhow::Result<T>` with `.with_context()` for fallible Rust functions within this repository.
- **Testing Expectations**: Prefer `cargo nextest run` for exhaustive suites when applicable; otherwise, `cargo test` is acceptable. Document any skipped checks with justification.

## Change Management
- Keep this rule file up to date when repository conventions evolve. Any modification to the rule hierarchy must preserve the multi-tier structure (global + optional shared instructions + directory scope).
- Do not delete or relocate `.vtcode/rule.md`; it serves as a compatibility shim for legacy workflows.
