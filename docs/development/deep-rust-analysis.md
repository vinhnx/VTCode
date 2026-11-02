# Deep Rust Analysis with Charon

VT Code's Rust support is intentionally layered:

- `code_search` reports **recognised declarations** and **exact syntactic usages**. A usage can belong
  to a different symbol with the same spelling, so it is *not* a semantically resolved reference
  (see [Language Support](../protocols/LANGUAGE_SUPPORT.md)).
- `ast-grep` covers structural/AST queries over source shape.
- The shell covers builds, tests, and toolchain probes.

None of these answer questions about what the compiler *computes*: which item an identifier truly
resolves to, where values are dropped, what layout a type is given, which generic instantiation runs.
For those, use [Charon](https://github.com/AeneasVerif/charon), which extracts a whole crate (and its
dependencies) into a stable, uniform JSON representation of what rustc sees, including simplified MIR.

The operational workflow lives in the dedicated skill at
[`charon-rust-analysis`](../../.agents/skills/charon-rust-analysis/SKILL.md). This guide records *why*
the integration is shaped the way it is and where its boundaries are.

## What Charon gives you

Running `charon cargo` in a crate emits `crate_name.llbc`: a JSON file with `type_decls`, `fun_decls`,
`global_decls`, `trait_decls`, `trait_impls`, simplified MIR bodies, and source information. It also
pretty-prints a Rust-like rendering that makes implicit behavior explicit — `⚡` marks control flow
taken during unwinding, and statements such as `storage_live`, `conditional_drop`, and
`unwind_continue` reveal where values are dropped. `--precise-drops` reflects drop-elaborated MIR when
drop placement is the question. It supports all Rust features except `async`.

## Design lessons VT Code adopts

Charon is a useful external reference for principles the harness already leans on; when extending
Rust tooling, keep these aligned:

| Lesson from Charon | VT Code counterpart |
| --- | --- |
| A **stable facade** absorbs a volatile, complex interface (rustc internals) so consumers never couple to it | `vtcode-llm` provider facade, `vtcode-mcp`, the sandbox boundary; `ThreadEvent` as the single runtime contract |
| **Uniform extraction shape** — every item of a kind exposes the same fields — so consumers stay small | shape-suffix naming, tagged tool-result rows, `CodeSearchResponse` filters/results split |
| Make **implicit semantics explicit** (drops, unwinding, layout, stability guarantees) | execution explanations, source-inspection ledger, runtime guidance |
| **Batch analysis is not an interactive hot path** | background index rebuilds and output spooling stay off the interactive path |

## Boundaries and non-goals

- **Opt-in only.** Charon is not a workspace dependency; it is invoked when present for a semantic
  question. There is no lock-in and no build-time coupling.
- **Beta and nightly.** Charon pins a nightly toolchain and still has planned API breaking changes, so
  treat `.llbc` output as evidence, not a frozen contract, and label inferred claims.
- **Batch, not low-latency.** Unlike rust-analyzer, Charon is not designed for fast incremental
  updates; never wire it into per-keystroke or per-turn paths.
- **Prefer the cheaper tool first.** Text, path, and structural queries belong to `code_search` and
  `ast-grep`; reach for Charon only when compiler semantics are the question.
