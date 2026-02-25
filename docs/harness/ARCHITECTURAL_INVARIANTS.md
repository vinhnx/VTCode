# Architectural Invariants

Mechanical enforcement rules for VT Code. These are not suggestions — they are invariants that must hold at all times. Violations should be caught by CI, not code review.

Each invariant includes a **remediation** instruction so agents can fix violations without asking for help.

---

## 1. Layer Dependency Rules

VT Code modules form a strict dependency DAG. No reverse imports.

```
types / commons
    ↓
  config
    ↓
   core
    ↓
  tools
    ↓
  agent (runloop, subagents)
    ↓
   TUI (src/)
```

Side crates with no upstream dependents:

- `vtcode-bash-runner` — used by core/exec
- `vtcode-markdown-store` — used by core
- `vtcode-indexer` — used for workspace file indexing
- `vtcode-exec-events` — event definitions, used by core
- `vtcode-acp-client` — Zed integration, used by agent
- `vtcode-process-hardening` — pre-main, no deps on workspace crates
- `vtcode-file-search` — used by core/tools

**Violation**: a lower-layer crate imports from a higher-layer crate.
**Remediation**: move the shared type/function down to the lowest common layer (usually `vtcode-commons` or `vtcode-config`). Never add a reverse dependency.

---

## 2. File Size Limits

Each Rust source file should be ≤500 lines. Files exceeding this limit should be split into focused submodules.

**Violation**: `wc -l` > 500 on a `.rs` file.
**Remediation**: extract logical sections into submodules within the same directory. Use `mod.rs` to re-export public items. Preserve the public API surface.

---

## 3. Naming Conventions

Enforced mechanically:

| Element   | Convention             | Example           |
| --------- | ---------------------- | ----------------- |
| Functions | `snake_case`           | `execute_tool`    |
| Variables | `snake_case`           | `provider_name`   |
| Types     | `PascalCase`           | `ToolRegistry`    |
| Structs   | `PascalCase`           | `McpProvider`     |
| Enums     | `PascalCase`           | `SafetyDecision`  |
| Constants | `SCREAMING_SNAKE_CASE` | `DEFAULT_TIMEOUT` |
| Modules   | `snake_case`           | `golden_path`     |
| Crates    | `kebab-case`           | `vtcode-core`     |

**Violation**: naming does not match the convention for its element type.
**Remediation**: rename the item. Use your editor's rename refactoring to update all references. If it's a public API, check for downstream usage first.

---

## 4. Structured Logging

All log statements must use the `tracing` crate with structured fields. No `println!` or `eprintln!` in library code (TUI binary `src/` may use `eprintln!` for fatal startup errors only).

```rust
// Correct
tracing::info!(provider = %name, model = %model_id, "Sending LLM request");

// Incorrect
println!("Sending request to {} with model {}", name, model_id);
```

**Violation**: `println!` or `eprintln!` in any crate except `src/` startup code.
**Remediation**: replace with `tracing::info!`, `tracing::warn!`, `tracing::error!`, or `tracing::debug!` using structured fields.

---

## 5. No `unwrap()`

Never use `.unwrap()` or `.expect()` in production code. Use `anyhow::Result<T>` with `.with_context()`.

```rust
// Correct
let config = tokio::fs::read_to_string(path)
    .await
    .with_context(|| format!("Failed to read config at {}", path))?;

// Incorrect
let config = tokio::fs::read_to_string(path).await.unwrap();
```

Exception: test code (`#[cfg(test)]` modules) may use `.unwrap()` when the test should panic on failure.

**Violation**: `.unwrap()` or `.expect()` outside of `#[cfg(test)]`.
**Remediation**: replace with `.with_context(|| "descriptive message")?`. The context message should describe what was being attempted, not just what failed.

---

## 6. No Hardcoded Model IDs

Model identifiers change frequently. All model references must come from `docs/models.json` or `vtcode-core/src/config/constants.rs`.

```rust
// Correct
use vtcode_core::config::constants::DEFAULT_MODEL_ID;

// Incorrect
let model = "gpt-4o-mini";
```

**Violation**: string literal matching a known model ID pattern (e.g., `"gpt-"`, `"claude-"`, `"gemini-"`) in non-test code.
**Remediation**: add the model to `docs/models.json` and reference it via constants. If it's a default, add it to `vtcode-core/src/config/constants.rs`.

---

## 7. Documentation Location

All `.md` documentation files go in `docs/`. The only exceptions in repository root are approved governance files (`README.md`, `AGENTS.md`, `CLAUDE.md`, `CONTRIBUTING.md`, `CHANGELOG.md`).

Within `docs/`, top-level `docs/*.md` is reserved for stable entrypoint docs. New one-off implementation notes, phase reports, and fix summaries must go to a domain folder (for example `docs/features/`) or archive path (for example `docs/archive/`).

**Violation**:

- a `.md` file in repository root outside the approved list.
- a `docs/*.md` file that is not listed in `scripts/docs_top_level_allowlist.txt`.

**Remediation**:

1. Move the file to the appropriate `docs/<domain>/` path or `docs/archive/`.
2. Update links that referenced the old path.
3. Add to `scripts/docs_top_level_allowlist.txt` only when the file is intentionally a long-lived top-level entrypoint.

---

## 8. Workspace Boundary Enforcement

All file operations (read, write, list, search) must validate that paths are within the workspace root. No file tool should access paths outside the workspace without explicit user approval.

```rust
// At the tool boundary, before any file operation:
let canonical = path.canonicalize()
    .with_context(|| format!("Failed to resolve path: {}", path.display()))?;
if !canonical.starts_with(&workspace_root) {
    anyhow::bail!("Path {} is outside workspace boundary", path.display());
}
```

**Violation**: file operation without workspace boundary check.
**Remediation**: add path validation at the tool's entry point (the API boundary), not deep inside helper functions. Use the existing `validate_path` utility if available.

---

## 9. Parse at Boundaries

Validate and parse inputs where they enter the system — at API boundaries, config loading, and tool argument parsing. Internal functions should receive validated types, not raw strings.

```rust
// At the boundary (tool argument parsing):
let line_number: usize = args["line"]
    .as_u64()
    .with_context(|| "line must be a positive integer")?
    as usize;

// Internal function receives validated type:
fn process_line(content: &str, line: usize) -> Result<String> { ... }
```

**Violation**: raw string parsing or validation deep inside business logic.
**Remediation**: move validation to the boundary function. Define typed structs for parsed inputs. Internal functions should receive these typed structs.

---

## 10. Lint Error Messages

Custom lint rules, Clippy configurations, and CI checks must include remediation instructions in their error messages. An agent reading the error should know how to fix it without searching.

```
// Good error message:
error: File exceeds 500-line limit (623 lines).
  Remediation: split into submodules. Extract logical sections into
  separate files and re-export from mod.rs.

// Bad error message:
error: File too long.
```

**Violation**: CI check or lint rule that produces an error without remediation guidance.
**Remediation**: update the check's error message to include a "Remediation:" section with specific instructions.

---

## 11. Agent Legibility

Operational data and status reports must be presented in structured formats (tables, YAML, or consistent headers) to ensure high parseability by agents and clarity for humans.

**Violation**: multi-file or multi-component status reported in long prose blocks without structure.
**Remediation**: convert the status report into a markdown table or structured list. Follow the examples in `docs/harness/AGENT_LEGIBILITY_GUIDE.md`.

---

## 12. Documentation Link Integrity

Core documentation entrypoints must not contain broken local markdown links.

**Violation**: a local markdown link target in `AGENTS.md`, `README.md`, `docs/README.md`, `docs/INDEX.md`, or harness docs does not exist.
**Remediation**:

1. Fix or remove broken references.
2. Keep links relative to the source markdown file when possible.
3. Re-run `python3 scripts/check_docs_links.py`.

## 13. Pre-flight Environment Checks

Before modifying code in any workspace, the agent must identify the project's build system, test runner, and module structure. Structural errors (missing `__init__.py`, broken `mod.rs` declarations, wrong test runner) cause more failures than incorrect logic.

**Violation**: Agent modifies code without first checking `Cargo.toml`, `package.json`, `pyproject.toml`, or equivalent project manifests.
**Remediation**: Before any code changes, run at least one of: `ls *.toml *.json Makefile`, read `AGENTS.md`, or use `list_files` on the project root. Identify the build/test commands and module convention before editing.

---

## 14. Verification-First Autonomy

Agent output must be verifiable before deployment. Every agent action that produces or modifies code must be followed by at least one verification step (test, type-check, or lint).

**Blind Editing**: Making consecutive code changes without intermediate testing or verification is strictly forbidden. This leads to compounding errors and brittle implementations.

**Violation**: Agent declares a task complete or moves to a next major phase without executing a verification tool (e.g., `cargo check`, `cargo test`, `npx tsc`).
**Remediation**: Run the appropriate verification command. Analyze the output. If it fails, fix and re-verify. Never rely on internal reasoning as proof of correctness ("hallucination of verification").

---

## 15. Error Mode Diagnosis

Before modifying code in response to a shell/command failure, the agent must verify if the failure is environmental or logical.

**Violation**: Agent modifies code to "fix" an error that is actually caused by a missing dependency, port conflict, incorrect file path, or permission issue.
**Remediation**: Use `ls`, `cat /etc/*release`, `which <cmd>`, or `ps` to diagnose the environment state first. Proactively document environment findings in `<analysis>`. If the environment is broken, fix the environment (if possible) or report it to the user rather than editing code.

---

## 16. Regression Verification

Every intentional "fix" for an observed error must be followed by running at least one related existing test to prevent introducing regressions. Research shows agents break existing code in 12-30% of cases when focusing purely on a new feature or fix.

**Hallucination of Verification Warning**: Avoid declaring success purely through internal reasoning. If you claim a regression check passed, you MUST show the tool output that proves it.

---

## Enforcement

These invariants should be enforced by:

1. **Clippy lints** — configured in workspace `Cargo.toml` under `[workspace.lints]`.
2. **CI checks** — `cargo clippy`, `cargo fmt --check`, custom scripts.
3. **Pre-commit hooks** — optional but recommended for file size and naming.
4. **Code review** — last line of defense, not the primary enforcement mechanism.

When adding a new invariant:

1. Add it to this document with violation description and remediation.
2. Implement automated enforcement (Clippy lint, CI script, or pre-commit hook).
3. Fix all existing violations before merging.
4. Add a tech debt item (`docs/harness/TECH_DEBT_TRACKER.md`) if existing violations cannot be fixed immediately.

## Known Violations

Not all invariants are fully enforced yet. Known violations are tracked in `docs/harness/TECH_DEBT_TRACKER.md`:

| Invariant                 | Debt Item | Status                                                                        |
| ------------------------- | --------- | ----------------------------------------------------------------------------- |
| #2 File Size Limits       | TD-005    | TUI event handler modules likely exceed 500 lines                             |
| #4 Structured Logging     | TD-013    | Not yet audited; some legacy `println!` may exist                             |
| #10 Lint Error Messages   | TD-014    | Custom lints with remediation not yet implemented                             |
| #7 Documentation Location | TD-001    | Top-level docs sprawl now gated by allowlist; consolidation still in-progress |

Adding CI enforcement for invariants is itself tracked as future work. Until enforcement exists, these invariants are enforced by code review and agent discipline.
