# Zen Alignment for VT Code

Full mapping of all 19 Zen of Python principles to VT Code controls, checks, and operating behavior.

Last reviewed: 2026-10-29

## Scope

- Strategy: additive. This document complements `CORE_BELIEFS.md` and `ARCHITECTURAL_INVARIANTS.md`.
- Coverage: all 19 principles.
- Rollout: staged (`warn` first, selective promotion to `enforce`).

## Full Principle Mapping (All 19)

| # | Zen Principle | VT Code Interpretation | Control / Check | Status |
| --- | --- | --- | --- | --- |
| 1 | Beautiful is better than ugly. | Prefer coherent, legible output and docs structure. | Markdown lint + harness document structure | Active |
| 2 | Explicit is better than implicit. | Keep links, policies, and constraints explicit. | `python3 scripts/check_docs_links.py` | Active |
| 3 | Simple is better than complex. | Keep files decomplected and focused. | Soft: `check_rust_file_length.py --mode warn --max-lines 500`; hard ceiling: `check_file_length.py --mode enforce --max-lines 1000` | Hard ceiling active |
| 4 | Complex is better than complicated. | Accept necessary complexity, reject avoidable complication. | Layering invariants + focused scripts | Active |
| 5 | Flat is better than nested. | Keep docs and modules discoverable and shallow where possible. | `python3 scripts/check_markdown_location.py` | Active |
| 6 | Sparse is better than dense. | Prefer concise structures over dense prose in governance output. | Structured tables and remediation sections in checks | Active |
| 7 | Readability counts. | Diagnostics must be readable and actionable. | Remediation-oriented messages across custom checks | Active |
| 8 | Special cases aren't special enough to break the rules. | Exceptions must be limited and audited. | `python3 scripts/check_zen_allowlist.py --mode warn` | Added |
| 9 | Although practicality beats purity. | Use staged rollout over disruptive hard-fail adoption. | Warn-first baselines (`./scripts/check.sh zen`) | Active |
| 10 | Errors should never pass silently. | Checks must surface failures with details. | Non-zero exits in enforce mode + explicit findings | Active |
| 11 | Unless explicitly silenced. | Every allowlisted exception must state rationale. | `check_zen_allowlist.py` requires `\| rationale` | Added |
| 12 | In the face of ambiguity, refuse the temptation to guess. | Require explicit mode (`warn`/`enforce`) in governance checks. | Script CLI mode flags | Active |
| 13 | There should be one-- and preferably only one --obvious way to do it. | One local governance path for developers. | `./scripts/check.sh zen` | Active |
| 14 | Although that way may not be obvious at first unless you're Dutch. | Document the path clearly so discoverability is fast. | `docs/harness/INDEX.md` + this file | Active |
| 15 | Now is better than never. | Fix drift as soon as detected. | Link/allowlist cleanup applied | Active |
| 16 | Although never is often better than right now. | Delay strict gating until baseline is understood. | Warn-mode baselines before enforce mode | Active |
| 17 | If the implementation is hard to explain, it's a bad idea. | Keep governance tools single-purpose and small. | Focused scripts under `scripts/` | Active |
| 18 | If the implementation is easy to explain, it may be a good idea. | Favor checks with clear, teachable remediation. | Remediation blocks in scripts and docs | Active |
| 19 | Namespaces are one honking great idea -- let's do more of those! | Keep boundaries explicit by domain and path. | Docs domain folders + crate/module boundaries | Active |

## Baseline Metrics (2026-03-03)

- Rust file length check (`--max-lines 500`, warn):
- `total_files=1380`, `>500=188`, `>1000=39`, `>1500=7`
- unwrap/expect production scan (warn, with allowlist):
- `scanned_files=1237`, `findings=121`

## Hard-Ceiling Metrics (2026-10-29)

- All-files line ceiling (`--max-lines 1000`, enforce):
- `scanned_text=4551`, `skipped_binary=26`, `>1000_unallowed=0`, `>1000_allowed=29`, `stale_allowlist=0`
- Allowed outliers: 8 vendored (`patches/**`), 10 generated/data (lockfile, changelog, notices, model registry,
  docs map, producer audit, dated snapshots, locale data, sample skill asset), 11 first-party source under
  temporary split-plan caps in `scripts/file_length_allowlist.txt`.

## Rollout

1. Phase 0 (completed)

- Repair broken core docs links.
- Resolve docs top-level and large-file allowlist drift.

1. Phase 1 (active)

- Run Zen governance in warn mode in CI and `check.sh`.
- Include allowlist hygiene for explicit exception silencing.

1. Phase 2 (partially complete)

- Promoted the 1000-line all-files ceiling to enforce (`check_file_length.py`, gated by `check-dev.sh`,
  `check.sh file-length`, and the `setup.sh --with-hooks` pre-commit hook). Caps only ratchet down; stale
  allowlist entries fail the gate.
- Still planned: promote `check_zen_allowlist.py` and the `unwrap/expect` production scan to enforce.

## Verification Commands

```bash
python3 scripts/check_docs_links.py
python3 scripts/check_markdown_location.py
python3 scripts/check_large_files.py 400000
python3 scripts/check_rust_file_length.py --mode warn --max-lines 500
python3 scripts/check_file_length.py --mode enforce --max-lines 1000 --quiet
python3 scripts/check_no_unwrap_expect_prod.py --mode warn --allowlist scripts/zen_allowlist.txt
python3 scripts/check_zen_allowlist.py --mode warn --allowlist scripts/zen_allowlist.txt
```

## Notes

- This alignment does not replace `ARCHITECTURAL_INVARIANTS.md`; it maps principles to controls.
- Debt and quality tracking remain canonical in:
- `docs/harness/TECH_DEBT_TRACKER.md`
- `docs/harness/QUALITY_SCORE.md`
