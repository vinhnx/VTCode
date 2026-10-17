# Markdown Quality

Run `python3 scripts/check_markdown.py` locally and in CI. It uses
`markdownlint-cli2@0.23.3` and requires Node.js 22 or newer. Use `--list` to inspect
the selected paths, or `--fix` to apply supported formatting fixes.

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
Fix generated output at its generator so regeneration remains lint-clean.

| Check | Command |
| --- | --- |
| Maintained Markdown | `python3 scripts/check_markdown.py` |
| Selection regressions | `python3 scripts/tests/test_check_markdown.py` |
| Core documentation links | `python3 scripts/check_docs_links.py` |
