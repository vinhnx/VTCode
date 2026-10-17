---
feature: session-stdio-dimmer
status: delivered
updated: 2026-09-25
branch: feat/session-stdio-dimmer
commits: 21c2ab545..15474018d
---

# Session Stdin/Stdout Dimmer and Expand

## Report

**What was built** — Exec-session stdin/stdout bodies (`write_stdin`, `send_pty_input`, `read_pty_session`,
`unified_exec` follow-ups) now render on the design-system dim tier: theme `pty_output`/`tool_detail` plus
`Effects::DIMMED`, while headers stay at normal tool brightness. The existing 10-row tail cap is unchanged; overflow now
shows `… +N lines · click to expand` (underlined action) instead of the `/share html` share hint. The complete capture
is recorded as a tool-output viewer record whose id is carried on the notice row, so clicking the action opens the Tool
Output Viewer focused on that call. CLI sinks get the plain notice (no SGR leak). A latent `inline_style_from_ratatui`
gap that dropped `UNDERLINED`/`DIM` from ANSI-parsed segments was fixed so the click target's hit-region signal
survives.

### Verification

- `./scripts/check-dev.sh` — PASS
- `cargo clippy -p vtcode-ui -p vtcode-core -p vtcode --tests --locked -- -D warnings` — PASS
<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode -E 'test(exec_session) or test(hidden_lines) or test(trim_to_tail) or test(session_body) or test(write_stdin) or test(prepare_summary) or test(is_exec_session)'`
  — PASS (31)
- `cargo nextest run -p vtcode -E 'test(tool_output)'` — PASS (256)
<!-- markdownlint-disable-next-line MD013 -->
- `cargo nextest run -p vtcode-ui -E 'test(expand_action) or test(compact_activity) or test(tool_output) or test(diff_review) or test(tool_output_is_not_dimmed)'`
  — PASS (37)
- `cargo nextest run -p vtcode-core -E 'test(convert_plain) or test(prepare_markdown) or test(line_function)'` — PASS
  (8)

### Journey log

- Screenshots 2026-09-24 were the pre-`cac29a333` state (row-per-param, unbounded body). That commit already shipped the
  10-line cap and summary folding; this work layers dim + expand on top rather than redoing it.
- `find_expand_action_text_region` must walk buffer cells, not UTF-8 bytes: `…` and `·` in the notice are multi-byte, so
  a byte offset lands the hit target several columns right of `click to expand`.
- `inline_style_from_ratatui` only mapped BOLD/ITALIC — ANSI underline was silently dropped, so the click target never
  registered as underlined. Mapping UNDERLINE/DIM was required for hit regions to work.
- Emitting SGR in the notice must be gated on `supports_inline_ui()`: the CLI writer path prints text raw and would leak
  escapes when color is off.
- The expand notice must force-update `anchor_line` on its tool-output block; a live-PTY header can otherwise claim the
  anchor first and hide the notice from hit-region scan.

## [S1] Problem

Exec-session stdin/stdout handling (`write_stdin`, `send_pty_input`, `read_pty_session`, and `unified_exec` follow-ups)
re-renders the session's captured terminal text on every poll or wait. Screenshots from 2026-09-24 (11.38.38, 11.40.47)
show the pre-`cac29a333` failure mode: a stuttering header (`Send command input Use Write stdin output`), one transcript
row per plumbing argument, and an unbounded bright stdout dump that overwhelms the transcript.

`cac29a333` already folded the summary rows and capped the body at 10 tail lines. Two user-visible gaps remain:

1. **Brightness** — the captured body still competes with assistant text. It should recede into the design system's
   secondary tier so the transcript stays scannable.
2. **No expand** — the truncation notice reads `… +N lines (/share html for full transcript)`, which is a share hint,
   not an expand affordance. The user needs a clear, clickable way to open the complete capture without leaving the TUI.

## [S2] Design

### S2.1 Truncation research (settled)

| Technique               | Where it applies                                                                              | Why                                                                                                                                |
| ----------------------- | --------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| **Tail-trim to N rows** | Exec-session stdin/stdout bodies (this feature)                                               | Session follow-ups are streaming polls of a live log; the newest rows carry the signal (progress, errors, summary). Head is stale. |
| **Head + tail excerpt** | Static command previews (`RUN_COMMAND_HEAD/TAIL_PREVIEW_LINES = 3`) and `condense_text_bytes` | Finished logs put the outcome at the end and the invocation at the start.                                                          |
| **Token budget**        | Model-facing tool results (~25k tokens)                                                       | Context-window cost, not UI cost.                                                                                                  |
| **Byte budget**         | `ShellHandler` preview (`max_output_tokens` × 4 chars)                                        | Bounds a single tool call before it reaches the model.                                                                             |
| **Line-count cap**      | UI surfaces (`INLINE_STREAM_MAX_LINES = 30`, `MAX_CODE_LINES = 30`)                           | Prevents TUI lag from pathological output.                                                                                         |

**Decision:** keep `EXEC_SESSION_OUTPUT_MAX_LINES = 10` (already landed). Do not change command-launch previews. Do not
invent a summarizer; the tail already includes tool-emitted summaries (`Summary …`, `✓ done`).

### S2.2 Dimming (design system)

Exec-session stdin/stdout bodies render in the theme's PTY body token **plus** `Effects::DIMMED`, matching the
`reasoning` dim pattern already in `ThemeStyles`. This is the lowest visual tier: below `tool_detail`, below response
text.

- Body lines: `MessageStyle::ToolOutput` (`styles.pty_output`) with `Effects::DIMMED` via `render_preview_line`
  `override_style`.
- Stdin echo rows (`$ …`): `MessageStyle::ToolDetail` with `Effects::DIMMED`.
- Headers (`• Send command input`, `└ Session …`) stay at normal tool brightness so the call remains identifiable.
- No new hardcoded colors. No change to `THEME_PTY_OUTPUT_MIX_RATIO` (global PTY contrast is out of scope). WCAG AA
  contrast of `pty_output` is already enforced by `theme` tests; DIM is a modifier on top of that token.

### S2.3 Condensed body + expand

When an exec-session body exceeds 10 rows:

1. Keep the existing tail-trim to 10 rows.
2. Replace the share-hint notice with a design-system expand notice:

   ```text
   … +N lines · click to expand
   ```

   Styled like the compact-activity hint: dim separator/body, accent + underline on `click to expand` so hit-region
   detection can find it.

3. Record a tool-output viewer capture (`handle.record_tool_output`) for the complete session body and carry its
   `ToolOutputId` on the notice row (`set_next_tool_output_anchor`).
4. Click on the underlined `click to expand` opens the Tool Output Viewer focused on that capture — the same expand path
   compact command activities already use (`compact_activity_review_anchor_at` → `open_tool_output_viewer`).

Command launches, file diffs, and non-session tools keep their existing notices (`/share html`, `review full diff`,
etc.).

### S2.4 Contracts

- `ThreadEvent` is untouched.
- Model-facing tool results are untouched (full capture still reaches the model).
- `EXEC_SESSION_OUTPUT_MAX_LINES` stays 10.
- Only exec-session tools (`is_exec_session_call`) receive the dim + expand treatment.

## [S3] Out of Scope

- Changing `THEME_PTY_OUTPUT_MIX_RATIO` or global PTY brightness.
- Summarizing session output into a synthetic one-line digest.
- New keybindings for expand (mouse notice + existing Open Tool Output action).
- Command-launch preview layout (`• Ran …` head/tail 3+3).
- CLI/non-TUI rendering beyond keeping it readable.

## Tasks

- [x] T1: Dim exec-session body and stdin rows — acceptance: session body lines and `$ stdin` rows carry
      `Effects::DIMMED` on `pty_output`/`tool_detail`; headers stay undimmed; command-launch previews unchanged (covers:
      S2.2)
- [x] T2: Expand notice + viewer capture for truncated session bodies — acceptance: overflow shows
      `… +N lines · click to expand` with underlined action; clicking opens Tool Output Viewer at that capture;
      within-10-line bodies show no notice (covers: S2.3; depends: T1)
- [x] T3: Regression tests — acceptance:
      `cargo nextest run -p vtcode -E 'test(exec_session) or test(session_body) or test(hidden_lines) or test(trim_to_tail)'`
      green, including dim-style and expand-notice assertions (covers: S2.2, S2.3)
- [x] T4: Docs — acceptance: `docs/development/tool-summary-display.md` exec-session section reflects dim + expand; no
      stale `/share html` claim for session bodies (covers: S2.2, S2.3; depends: T2)
