# lint-expect-over-allow

> `#[expect(..., reason = "...")]` for lint overrides; never `#![deny(warnings)]` in source

## Why It Matters

A `#[allow]` is a silent, unowned promise that the lint will never matter again - and
`#![deny(warnings)]` in source is worse: the next toolchain release adds new warnings
and your build breaks for reasons you didn't choose. `#[expect]` inverts the failure
mode: when the lint stops firing (because the code changed), the *expect* itself emits
a warning, so stale suppressions clean themselves up. CLI flags own enforcing
warnings-as-errors; source owns narrow, reasoned exceptions.

## Bad

```rust
#![deny(warnings)] // breaks on every toolchain that adds a new lint

#[allow(dead_code)] // no reason, no owner, checked by nobody
fn legacy_path() { /* ... */ }
```

## Good

```rust
// Narrow, reasoned, self-expiring.
#[expect(dead_code, reason = "used by feature `fast-path`, removed in #1234")]
fn legacy_path() { /* ... */ }

// Cargo.toml owns lint levels once; source only documents narrow exceptions.
```

```toml
[lints.rust]
unsafe_op_in_unsafe_fn = "warn"
missing_debug_implementations = "warn"

[lints.clippy]
all = { level = "warn", priority = -1 }
# where deliberately tolerated:
undocumented_unsafe_blocks = "allow"
allow_attributes_without_reason = "warn"
```

## When `#[allow]` is still right

- Generated code you don't control (macro output, `build.rs` output checked in)
- Inline attributes inside a macro that emits code (macro expansion can't always use `expect`)

## Don't override a lint when you can fix the code

Read the lint's explanation first. A suppression that hides a real correctness problem
(`clippy::manual_filter_map`) will be found again - by someone debugging at 2am without
closure context. Overriding is: "this lint is *wrong for this one item, because*".

## Use one narrow scope

Attach the `#[expect]` to the smallest item that needs it (one function/range, not the
whole module or crate). Burned scope is a review tax and escalates entitlement.

## See Also

- [lint-workspace-lints](lint-workspace-lints.md) - Workspace-level lint config
- [lint-unsafe-doc](lint-unsafe-doc.md) - `SAFETY` comment linting
- [lint-missing-docs](lint-missing-docs.md) - `missing_docs` enforcement
- [lint-rustfmt-check](lint-rustfmt-check.md) - `cargo fmt --check` in CI
