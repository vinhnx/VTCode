# Rust Error Handling — Reference Notes

Reference artifact for VT Code contributors and coding agents working on error
classification and recovery (`crates/common/vtcode-commons/src/error_category.rs`,
`crates/codegen/vtcode-core/src/retry.rs`).

## Source (not vendored)

- Title: **The Missing Piece in Rust Error Handling**
- Author: Dillon McMahon
- Published: 2026-10-06
- URL: <https://mcmah309.github.io/posts/the-missing-piece-in-rust-error-handling/>

The full text lives at the URL above and is **not reproduced here**. What follows
is an original distillation plus a small number of short attributed observations
for orientation. When in doubt, read the original — it is the authority; this
file is only VT Code's working notes.

## Why this matters to VT Code

VT Code funnels every failure through a small set of classifiers before deciding
whether to retry, compact, ask the user, or stop: `classify_anyhow_error` →
`ErrorCategory` (the single tool-error classifier) and `ErrorCategory::from(&LLMError)`
for provider errors. Those decisions are exactly what the article is about —
**choosing what to do based on the error, not on its prose**. The article's core
thesis is that the type should tell you how to *recover*, the context should tell
you what *happened*, and a deliberate `.context(...)` must not erase the type
that carries the recovery policy.

## Distilled principles (in our own words)

1. **Decide by type, not by text.** A typed error (`LLMError`) is the source of
   truth for its recovery policy. Re-deriving a category by scanning the message
   of a typed error throws away information the compiler already tracks.
2. **Context is additive, never masking.** Attaching `.context(...)` should add
   operational detail ("compaction failed"), not hide the underlying cause. A
   classifier that reads only the outermost string loses the root error.
3. **Read the whole cause chain.** When no typed error survives the `anyhow`
   boundary, classify against the chain (`{err:#}`-style) so an inner marker
   such as `connection reset` is still recognized.
4. **Erase types only where callers merely propagate.** Precise types belong
   where a caller branches on them; a top-level report that only prints the error
   may use the opaque form. VT Code's `anyhow::Result` + `.context()` convention
   already matches this.
5. **Recovery should reduce what can still go wrong.** `Retryability` /
   `is_retryable()` is VT Code's version of this: once a category is handled, the
   caller only needs to reason about what remains.

## How VT Code applies this

- `classify_anyhow_error` (`vtcode-commons`) classifies the outermost context
  first (preserving intentional wording), then prefers a typed `LLMError` that
  survived the boundary via `downcast_ref`, then walks the remaining cause chain.
  This mirrors the existing `detect_misconfiguration_in_anyhow` and
  `is_context_capacity_error`, which already downcast/chain-walk for the same
  reason.
- **Not adopted:** the `eros` open-sum / error-set DSL. VT Code's boundary is
  sealed (`anyhow` in libraries, `VtCodeError` at the top level), so adding a new
  error-set type system would be churn without a caller that recovers differently
  per concrete source type. Revisit only if a call site genuinely needs to branch
  on a *union* of leaf types.

## Regression guard

`crates/common/vtcode-commons/src/error_category.rs` tests cover the four cases:
typed `LLMError` under context, deeper cause chain, outermost-context priority,
and unknown → `ExecutionError`.
