# proj-semver-features

> Semver is API: additive features, explicit deprecations, deliberate MSRV

## Why It Matters

Every public item is a promise; breaking it silently strands every downstream crate on
an unpinned semver. Cargo happily keeps semver-incompatible versions of a crate
coexisting in one build (never two compatible versions), so a "harmless" breaking
change fragments builds into two copies of your library - with types and trait impls
that no longer unify.

## Breaking Changes (the real list)

- Removing or renaming a public item
- Adding a variant to a matchable (non-`#[non_exhaustive]`) enum
- Adding a field to a publicly constructible struct (fixable with `#[non_exhaustive]` or private fields + builder)
- Adding a trait bound, or making a trait non-object-safe
- Adding a blanket impl (orphan-rule conflicts downstream)
- Changing the license or the default feature set (both are API)
- Removing `const` from a public function

## Deprecated-then-removed policy

Deprecate in a minor release (`#[deprecated(note = "use X")]` + docs + changelog
entry), remove in the next major. Never a silent removal.

## Bad

```toml
[features]
default = ["no-std"]   # a feature that REMOVES functionality / changes semantics
serialize = ["dep:serde"] # vague; is this `serde` support or the act of serializing?
```

## Good

```toml
[features]
default = ["std"]
# Features are always additive: any combination compiles, and a feature
# never removes or changes a public item.
std = ["dep:std-util"]
serde = ["dep:serde"]
test-util = ["dep:mock-ctrl"]  # mocks, fakes, fake data generators live here
```

```rust
// Never feature-gate a struct field or trait method - it breaks additive
// assumptions downstream. Feature-gate impls and constructors instead.
#[cfg_attr(feature = "serde", derive(serde::Serialize))] // fine: additive derive
pub struct Metrics { pub requests: u64 }                 // never `#[cfg(feature)] pub extra: f64`
```

## Feature Naming

Feature names share a namespace with crate names, and Cargo unifies features across
the whole graph. Use bare nouns (`std`, `serde`, `alloc`, `test-util`), never verbs or
negations (`use-serde`, `no-std` - a "no-std feature" is always wrong; model
`#![no_std]` crates with an additive `std` feature instead).

## Dependency Hygiene

- Ranges (`"1"`, `"1.4"`), never `"*"`; `"="` pins are for security emergencies and macro companion crates.
- Commit `Cargo.lock` for binaries; libraries don't.
- Audit: `cargo tree --duplicates` (version fragmentation), `cargo audit`, `cargo deny`, `cargo udeps`.
- MSRV: set it at crate creation, raise conservatively in minor releases; declare it in `rust-version`.
- Edition: latest stable edition for new crates (2024 as of 2026).

## See Also

- [api-non-exhaustive](api-non-exhaustive.md) - `#[non_exhaustive]` for future-proof enums
- [api-serde-optional](api-serde-optional.md) - Feature-gating `serde` support
- [lint-cargo-metadata](lint-cargo-metadata.md) - Cargo manifest lints
- [proj-workspace-deps](proj-workspace-deps.md) - Workspace dependency inheritance
