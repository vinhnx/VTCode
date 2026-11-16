# Markdown Quality

Run `python3 scripts/check_markdown.py` locally and in CI. It uses
`markdownlint-cli2@0.23.3` and requires Node.js 22 or newer. Run `npm install`
once so the pinned linter is available locally (a `devDependencies` entry);
without the local install the script falls back to a pinned `npx` fetch, which
needs network access. If `npx` is not on PATH the script reports a toolchain
error and exits 2 (distinct from a lint failure, exit 1). Use `--list` to
inspect the selected paths, or `--fix` to apply supported formatting fixes.
Pass maintained paths (`python3 scripts/check_markdown.py README.md`) to lint
only those files; unknown or excluded paths fail closed instead of silently
linting nothing.

Normal checks report the selected file count, diagnostics, and exit status without
repeating the entire path list. Use `--list` when the full file inventory is needed.

The selector includes tracked, maintained first-party Markdown. It excludes
vendored patches, test fixtures and snapshots, embedded generated copies,
historical session reviews, symbolic links, and the owner-only project TODO.
Ignored session state, dependencies, and build output never enter the scan.

Correctness rules remain enabled, including fragments, references, and table
column counts. Prose wraps at 120 columns; code blocks and tables retain their
natural widths. HTML is limited to the elements used by badges, contributor
avatars, disclosure sections, and keyboard labels.

Narrow inline exceptions document intentional historical headings, long
generated avatar markup, and compact instruction files with fixed line budgets.
The generated `CHANGELOG.md` carries a file-level `markdownlint-disable` for the
subject-dependent rules (see [CHANGELOG_GENERATION](changelog_generation.md)). Fix
generated output at its generator so regeneration remains lint-clean.

| Check | Command |
| --- | --- |
| Maintained Markdown | `python3 scripts/check_markdown.py` |
| Selection regressions | `python3 scripts/tests/test_check_markdown.py` |
| Links in all maintained Markdown (also in `check-dev.sh`) | `python3 scripts/check_docs_links.py` |

## File naming and location

GitHub recognizes a small set of special filenames case-insensitively in the repository root, `docs/`, or
`.github/`: `README.md`, `LICENSE`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `ACCESSIBILITY.md`,
`CHANGELOG.md`, `SUPPORT.md`, `CITATION.cff`, `FUNDING.yml`, and `CODEOWNERS`. Keep those names as written.

Every other Markdown file uses lowercase, separator-style filenames (`command_security_model.md`,
`desktop-workspace.md`). When renaming a doc, rewrite every relative link in the same change, update hardcoded
paths in `scripts/` (notably `scripts/check_docs_links.py` entrypoints and generator outputs), and keep the
generated-file owner in sync — a renamed generator target must be regenerated from the updated script.

Place new documents in the domain folder that owns them (`guides/`, `development/`, `user-guide/`, `modules/`,
`security/`, …) rather than at `docs/` root. `docs/index.md` is the repository-wide index and `docs/README.md`
the hub; the owner-only `docs/project/TODO.md` is never renamed or edited by agents.
