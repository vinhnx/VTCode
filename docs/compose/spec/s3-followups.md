---
feature: s3-followups
status: delivered
updated: 2026-09-28
branch: feat/s3-followups
commits: bcc463d45..9ad769395
---

# S3 Follow-ups (session-efficiency out-of-scope)

## Report

**What was built** — Four of the five `session-efficiency` out-of-scope items landed; budgets stay lean by explicit
decision. (1) **Compaction**: `DEFAULT_COMPACTION_TRIGGER_RATIO` (0.75) is now applied in
`resolve_compaction_threshold_with_reserve` — the real auto-compaction path — so long research turns compact at 75% of
the prompt budget instead of only at full capacity. The ratio previously fed a dead `CompactionConfig.trigger_threshold`
field. (2) **Registry**: dead-tool sweep found **no unused registrations or `*_ID` constants** (prior deliveries already
removed them). (3) **Budgets**: 2k/6k/8k unchanged; EXECUTION_POLICY records they must not be raised without new
evidence. (4) **Auth flake**: `--print` with empty text skips provider auth only when stdin is a TTY (no piped prompt);
CLI tests set `MERGE_GATEWAY_API_KEY` so `print_mode_requires_prompt_or_stdin` is green without credentials. (5)
**Model-picker**: already landed in `8778f177a`; no further change.

**Verification** — `./scripts/check-dev.sh` PASS. `cargo nextest run -p vtcode-core -p vtcode-config --lib` **4221
passed**. Compaction tests 91/91 + `default_compaction_trigger_uses_ratio_of_prompt_budget` PASS. StartupPolicy tests +
`print_mode_requires_prompt_or_stdin` **PASS** (was PRE-EXISTING fail). First review **request-changes** (dead ratio,
empty Report, piped-stdin auth hole) → fixed in `9ad769395`.

### Journey log

- `CompactionConfig.trigger_threshold` was a dead field — always trace the real call path
  (`effective_compaction_threshold_with_reserve`) before claiming a constant change does something.
- `StartupPolicy` runs before stdin is read; auth-skip based on `--print` text alone cannot see piped prompts. Gate on
  TTY, or accept a later failure.
- Default OpenAI route's auth env is `MERGE_GATEWAY_API_KEY`, not `OPENAI_API_KEY` — CLI tests must set both.
- Integer `75/100` percent avoids `clippy::cast_sign_loss`; pin it to the shared ratio constant in tests.

## [S1] Problem

`session-efficiency` closed empty-session spam, tool-result clearing, search drift, and exit cost. Its [S3] left five
items open. Grilled scope (2026-09-28): implement all five, **but keep lean budgets** (no schema/first-request raise).

1. **Compaction** — `DEFAULT_COMPACTION_TRIGGER_RATIO = 0.90` waited until 90% of the window; research runs at ~1M
   tokens/turn sat in the expensive zone too long. Worse, the constant fed a dead field and never affected runtime
   timing.
2. **Registry** — prior audit found no dead _registered_ tools; leftover dead `*_ID` constants were already removed.
   Need a fresh check and any remaining dead registration.
3. **Budgets** — explicit non-change: keep 2k/6k/8k (HarnessTax lean). Document only.
4. **Auth-env flake** — `cli_harness_failures::print_mode_requires_prompt_or_stdin` failed because `--print` with **no
   prompt** still required provider auth, so the test died on “Authentication not found” instead of “No prompt
   provided”.
5. **Model-picker WIP** — already landed (`8778f177a`); no further code unless a leftover is found.

## [S2] Design

### S2-compaction: trigger earlier (wired)

- `DEFAULT_COMPACTION_TRIGGER_RATIO` **0.90 → 0.75** and applied in `resolve_compaction_threshold_with_reserve`: default
  trigger = 75% of (context − output reserve). Explicit `auto_compaction_threshold_tokens` still wins, capped at the
  prompt budget.
- Keep `DEFAULT_COMPACTION_TARGET_THRESHOLD` at 0.50 and `keep_last_messages` at 10.
- Do not change compaction prompt text.

### S2-registry: dead-tool sweep

- Re-audit `BUILTIN_TOOLS` registrations and workspace-wide tool-ID constants for zero-reference dead code; remove only
  demonstrably unused items.
- **Result: none found.** Prior `budget-retune-dead-tools` and this sweep both found no unused registrations; `*_ID`
  constants were already removed.

### S2-budgets: keep lean (no code)

- Schema cap stays **2,000**; first-request stays **6,000 / 8,000**. No raise. EXECUTION_POLICY records they are
  intentional lean caps.

### S2-auth: `--print` without prompt fails before auth (TTY only)

- When `--print` is present with empty/whitespace prompt text **and stdin is a TTY** (no piped prompt possible),
  `allow_missing_provider_auth` is **true**.
- Piped stdin may still carry a prompt, so empty `--print` with non-TTY stdin still requires auth.
- `build_print_prompt` still returns `No prompt provided…` when there is no piped stdin and no inline text.
- `--print <text>` still requires provider auth (unchanged).
- CLI tests set both `OPENAI_API_KEY` and `MERGE_GATEWAY_API_KEY`.

### S2-model-picker: no-op

- Confirmed committed (`8778f177a`). No further change.

## [S3] Out of Scope

- Raising schema/first-request budgets (explicitly rejected).
- Compaction prompt prose rewrites.
- Changing auth priority (keyring vs ChatGPT vs env).
- Windows PTY SIGKILL tests (no Windows CI).

## Tasks

- [x] T1: compaction trigger ratio 0.75 wired into real threshold path — acceptance:
      `DEFAULT_COMPACTION_TRIGGER_RATIO == 0.75` and `resolve_compaction_threshold_with_reserve` applies 75% of prompt
      budget; compaction tests pass (covers: S2-compaction)
- [x] T2: dead-tool sweep — acceptance: any zero-reference tool IDs/registrations removed, **or Report records none
      found** (covers: S2-registry) → **none found**
- [x] T3: `--print` empty prompt skips auth on TTY — acceptance: `print_mode_requires_prompt_or_stdin` passes without
      credentials; `--print hello` still requires auth; unit tests cover TTY/non-TTY (covers: S2-auth)
- [x] T4: docs note budgets stay lean — acceptance: EXECUTION_POLICY records 2k/6k/8k unchanged (covers: S2-budgets)
