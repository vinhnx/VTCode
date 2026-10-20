<!-- markdownlint-disable MD013 -->

# Execution-policy matching analysis

## References and applicability

[uv PR #22651](https://github.com/astral-sh/uv/pull/22651) demonstrates direct simplified-cover extraction from decision diagrams. Its roughly 12,000× speedup refers to uv's conflict-dependent formatting benchmark. VT Code's measurements are reported separately below.

[DNF](https://en.wikipedia.org/wiki/Disjunctive_normal_form) expresses an OR of ANDs. Converting arbitrary factored expressions into explicit clauses can grow exponentially. VT Code's prefix policies are ordered token conjunctions with first-match decisions, rather than a Boolean formula requiring DNF conversion.

[uv PR #22651](https://github.com/astral-sh/uv/pull/22651), reviewed at commit `551c17cbc663b67f7e0b72636f5ea8c6e6732408`, replaces path enumeration and subsequent clause comparisons with direct decision-diagram cover extraction. In [`marker/algebra/isop.rs`](https://github.com/astral-sh/uv/blob/551c17cbc663b67f7e0b72636f5ea8c6e6732408/crates/uv-pep508/src/marker/algebra/isop.rs), recursive calls cache `(lower, upper)` pairs and return an intermediate function satisfying `lower => I => upper`, together with a shared cover. Boolean branches use the Minato–Morreale ISOP recurrence; range branches partition matching cofactors and group equivalent suffixes. Covers expand into paths only at final DNF rendering. Final output size can still be large; the implementation explicitly does not guarantee prime, irredundant covers over arbitrary range unions. Diagram variable ordering remains a required invariant.

Inspection searched Rust sources for DNF/CNF, ISOP, decision diagrams, Cartesian expansion, clauses, and Boolean simplification, then inspected policy evaluation and shell approval code.

| Lesson | Local evidence | Application |
| --- | --- | --- |
| Avoid path enumeration before simplification | No matching decision-diagram or DNF conversion machinery found in `src/` or `crates/` | No ISOP or Boolean-language implementation added |
| Reuse equality results rather than pairwise comparisons | `Policy::prepend_layer` repeatedly compared incoming patterns with accepted and existing rules | Use a temporary hash set for membership, preserving vector order |
| Preserve semantics across alternate representations | Policy evaluation uses ordered first-match decisions; decisions cannot be combined as unordered Boolean clauses | Compare deduplication with the original pairwise algorithm and test restrictive batch aggregation |
| State conjunction boundary behavior explicitly | Empty token vectors previously matched every command | Reject empty insertion/loading; make direct/deserialized empty rules and trusted amendments nonmatching |
| Memoize repeated recursive subproblems | `CommandPolicyEvaluator` already compiles patterns and caches verdicts; shell approval performs direct token checks | No new runtime cache without a measured recurring subproblem |

The empty-prefix insertion invariant also appears in OpenAI Codex's [`codex-rs/execpolicy/src/policy.rs`](https://github.com/openai/codex/blob/091008729d5572712ac08964a9602eb71cf31428/codex-rs/execpolicy/src/policy.rs), pinned at `091008729d5572712ac08964a9602eb71cf31428` (Apache-2.0, © 2025 OpenAI). VT Code independently enforces that boundary through its existing fallible API; no upstream implementation was copied.

## Contracts and compatibility

Zero tokens means an empty pattern. A nonempty vector containing an empty-string argument retains exact token-prefix semantics. Public signatures and serialized shapes stay unchanged. Simple, TOML, and JSON loaders validate each rule through `Policy::add_prefix_rule`, with filename and one-based rule-index error context. Validation happens before replacing the active policy, including layered reloads.

Layer deduplication preserves the existing contract: an existing pattern suppresses incoming duplicates; otherwise the first incoming occurrence wins. Existing duplicate rules remain intact. Incoming survivors keep their original order before existing rules. A hash set holds borrowed token slices only during the merge, and never determines evaluation order. For `n` incoming and `m` existing rules, membership work changes from up to `O(n² + nm)` pattern comparisons to expected `O(n + m)` hash operations, plus token hashing and vector movement. Incoming survivors move into the policy without cloning their strings. Temporary storage is linear. Adversarial hash collisions do not affect correctness; the standard randomized hasher is retained.

This work hardens and optimizes the execution-policy library. It does not establish that the rule-file manager is wired into the main shell-execution path, or measure end-to-end agent latency.

## Validation and measurement

Regression coverage includes empty direct/deserialized rules and amendments, empty/nonempty commands, rejected insertion without mutation, invalid patterns in all three formats, valid empty rule files, atomic single/layer reload failure, exact token boundaries, order-sensitive first-match decisions, and restrictive batch aggregation. Layer equivalence compares against the old pairwise algorithm over asymmetric overlapping patterns and decisions. Matching is compared with an independent slice-prefix oracle and structured core property tests.

The `execution_policy` Criterion benchmark uses the optimized Cargo `bench` profile (`opt-level = 3`), 20 samples per layer case, one-second warmup, and two-second measurement. Setup clones inputs outside the timed merge. Timed work includes merge allocations and disposal of discarded duplicates; final policy destruction occurs outside the timed region. Unique and duplicate-heavy layers contain 16, 512, or 4,096 two-token rules; duplicate-heavy layers repeat 16 patterns. The matching case measures a successful two-token prefix with one trailing argument. These synthetic cases characterize library scaling, not production rule-file distributions. The shipped release profile uses `opt-level = "z"`, so these results do not measure shipped-binary code generation.

```sh
cargo bench --locked -p vtcode-safety --bench execution_policy -- --save-baseline before
# Run the same benchmark after changing the implementation:
cargo bench --locked -p vtcode-safety --bench execution_policy -- --baseline before
```

Measured on the same macOS host with Rust 1.98.1 before and after the production changes (October 20, 2026):

| Case | Before | After | Ratio (before/after) |
| --- | ---: | ---: | ---: |
| 16 unique rules | 943 ns | 724 ns | 1.30× |
| 16 rules, repeating 16 patterns | 947 ns | 736 ns | 1.29× |
| 512 unique rules | 450 µs | 23.8 µs | 18.9× |
| 512 rules, repeating 16 patterns | 38.9 µs | 17.3 µs | 2.25× |
| 4,096 unique rules | 21.4 ms | 203 µs | 105× |
| 4,096 rules, repeating 16 patterns | 310 µs | 136 µs | 2.28× |
| Successful nonempty prefix match | 4.32 ns | 3.37 ns | 1.28× |

An initial hash-set implementation cloned retained rules and regressed the 16-rule case. Moving survivors after recording membership removed that regression. Timing ratios are rounded and describe these synthetic merges only. The hash-set change addresses quadratic deduplication, not uv's exponential DNF expansion problem.

Validation on macOS: 324 selected nextest tests passed (308 safety tests and all 16 `core_fuzz_props` tests); 36 other core test binaries were excluded. The focused prefix selection also passed all five matching tests. Safety Clippy with all targets and warnings denied, `./scripts/check-dev.sh` (default members, including all-target/all-feature Clippy and locked compilation), formatting, affected Markdown, and documentation-link checks passed. The development gate reported shellcheck warnings in unchanged scripts; the broader test build emitted a macOS compact-unwind linker warning. Linux/Windows-specific launch behavior and end-to-end agent performance were not tested.
