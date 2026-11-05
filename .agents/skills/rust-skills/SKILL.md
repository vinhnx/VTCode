---
name: rust-skills
description: >
  193 Rust coding rules across 14 categories. Use when writing, reviewing,
  or refactoring Rust code. Covers ownership, errors, async, API design,
  memory, performance, testing, and anti-patterns.
license: MIT
metadata:
  author: leonardomso
  version: "1.1.0"
  sources:
    - Rust API Guidelines
    - Rust Performance Book
    - Google Rust Style Guide
    - Microsoft Pragmatic Rust Guidelines
    - ripgrep, tokio, serde, polars codebases
---

# Rust Best Practices

Comprehensive guide for writing high-quality, idiomatic, and highly optimized Rust code. Contains 193 rules across 14 categories, prioritized by impact to guide LLMs in code generation and refactoring. Quick reference below; the reasoning behind each rule family lives in the per-source digests under [`references/`](references/) (adapted from [BeautifulRust](https://github.com/morganlinton/BeautifulRust), MIT).

## When to Apply

Reference these guidelines when:
- Writing new Rust functions, structs, or modules
- Implementing error handling or async code
- Designing public APIs for libraries
- Reviewing code for ownership/borrowing issues
- Optimizing memory usage or reducing allocations
- Tuning performance for hot paths
- Refactoring existing Rust code

## Rule Categories by Priority

| Priority | Category | Impact | Prefix | Rules |
|----------|----------|--------|--------|-------|
| 1 | Ownership & Borrowing | CRITICAL | `own-` | 14 |
| 2 | Error Handling | CRITICAL | `err-` | 14 |
| 3 | Memory Optimization | CRITICAL | `mem-` | 15 |
| 4 | API Design | HIGH | `api-` | 17 |
| 5 | Async/Await | HIGH | `async-` | 16 |
| 6 | Compiler Optimization | HIGH | `opt-` | 12 |
| 7 | Naming Conventions | MEDIUM | `name-` | 17 |
| 8 | Type Safety | MEDIUM | `type-` | 11 |
| 9 | Testing | MEDIUM | `test-` | 15 |
| 10 | Documentation | MEDIUM | `doc-` | 11 |
| 11 | Performance Patterns | MEDIUM | `perf-` | 11 |
| 12 | Project Structure | LOW | `proj-` | 12 |
| 13 | Clippy & Linting | LOW | `lint-` | 12 |
| 14 | Anti-patterns | REFERENCE | `anti-` | 16 |

---

## Quick Reference

### 1. Ownership & Borrowing (CRITICAL)

- [`own-borrow-over-clone`](rules/own-borrow-over-clone.md) - Prefer `&T` borrowing over `.clone()`
- [`own-slice-over-vec`](rules/own-slice-over-vec.md) - Accept `&[T]` not `&Vec<T>`, `&str` not `&String`
- [`own-cow-conditional`](rules/own-cow-conditional.md) - Use `Cow<'a, T>` only for real conditional ownership
- [`own-arc-shared`](rules/own-arc-shared.md) - Use `Arc<T>` for thread-safe shared ownership
- [`own-rc-single-thread`](rules/own-rc-single-thread.md) - Use `Rc<T>` for single-threaded sharing
- [`own-refcell-interior`](rules/own-refcell-interior.md) - Use `RefCell<T>` for interior mutability (single-thread)
- [`own-mutex-interior`](rules/own-mutex-interior.md) - Use `Mutex<T>` for interior mutability (multi-thread)
- [`own-rwlock-readers`](rules/own-rwlock-readers.md) - Use `RwLock<T>` when reads dominate writes
- [`own-copy-small`](rules/own-copy-small.md) - Derive `Copy` for small, trivial types
- [`own-clone-explicit`](rules/own-clone-explicit.md) - Make `Clone` explicit, avoid implicit copies
- [`own-move-large`](rules/own-move-large.md) - Move large data instead of cloning
- [`own-lifetime-elision`](rules/own-lifetime-elision.md) - Rely on lifetime elision when possible
- [`own-borrow-counter-ladder`](rules/own-borrow-counter-ladder.md) - Fix borrow errors by moving data: `mem::take`, struct splitting, typed intermediates
- [`own-drop-contract`](rules/own-drop-contract.md) - Destructors never fail, panic, or block; bind guards to names

### 2. Error Handling (CRITICAL)

- [`err-thiserror-lib`](rules/err-thiserror-lib.md) - Use `thiserror` for library error types
- [`err-anyhow-app`](rules/err-anyhow-app.md) - Use `anyhow` for application error handling
- [`err-result-over-panic`](rules/err-result-over-panic.md) - Return `Result`, don't panic on expected errors
- [`err-context-chain`](rules/err-context-chain.md) - Add context with `.context()` or `.with_context()`
- [`err-no-unwrap-prod`](rules/err-no-unwrap-prod.md) - Never use `.unwrap()` in production code
- [`err-expect-bugs-only`](rules/err-expect-bugs-only.md) - Use `.expect()` only for programming errors
- [`err-question-mark`](rules/err-question-mark.md) - Use `?` operator for clean propagation
- [`err-from-impl`](rules/err-from-impl.md) - Use `#[from]` for automatic error conversion
- [`err-source-chain`](rules/err-source-chain.md) - Use `#[source]` to chain underlying errors
- [`err-lowercase-msg`](rules/err-lowercase-msg.md) - Error messages: lowercase, no trailing punctuation
- [`err-doc-errors`](rules/err-doc-errors.md) - Document errors with `# Errors` section
- [`err-custom-type`](rules/err-custom-type.md) - Create custom error types, not `Box<dyn Error>`
- [`err-panic-policy`](rules/err-panic-policy.md) - Panics are not exceptions; poisoned locks are fatal; `catch_unwind` is last resort
- [`err-app-lib-boundary`](rules/err-app-lib-boundary.md) - One app error type; libraries never expose `anyhow`; return consumed args in errors

### 3. Memory Optimization (CRITICAL)

- [`mem-with-capacity`](rules/mem-with-capacity.md) - Use `with_capacity()` when size is known
- [`mem-smallvec`](rules/mem-smallvec.md) - Use `SmallVec` for usually-small collections
- [`mem-arrayvec`](rules/mem-arrayvec.md) - Use `ArrayVec` for bounded-size collections
- [`mem-box-large-variant`](rules/mem-box-large-variant.md) - Box large enum variants to reduce type size
- [`mem-boxed-slice`](rules/mem-boxed-slice.md) - Use `Box<[T]>` instead of `Vec<T>` when fixed
- [`mem-thinvec`](rules/mem-thinvec.md) - Use `ThinVec` for often-empty vectors
- [`mem-clone-from`](rules/mem-clone-from.md) - Use `clone_from()` to reuse allocations
- [`mem-reuse-collections`](rules/mem-reuse-collections.md) - Reuse collections with `clear()` in loops
- [`mem-avoid-format`](rules/mem-avoid-format.md) - Avoid `format!()` when string literals work
- [`mem-write-over-format`](rules/mem-write-over-format.md) - Use `write!()` instead of `format!()`
- [`mem-arena-allocator`](rules/mem-arena-allocator.md) - Use arena allocators for batch allocations
- [`mem-zero-copy`](rules/mem-zero-copy.md) - Use zero-copy patterns with slices and `Bytes`
- [`mem-compact-string`](rules/mem-compact-string.md) - Use `CompactString` for small string optimization
- [`mem-smaller-integers`](rules/mem-smaller-integers.md) - Use smallest integer type that fits
- [`mem-assert-type-size`](rules/mem-assert-type-size.md) - Assert hot type sizes to prevent regressions

### 4. API Design (HIGH)

- [`api-builder-pattern`](rules/api-builder-pattern.md) - Use Builder pattern for complex construction
- [`api-builder-must-use`](rules/api-builder-must-use.md) - Add `#[must_use]` to builder types
- [`api-newtype-safety`](rules/api-newtype-safety.md) - Use newtypes for type-safe distinctions
- [`api-typestate`](rules/api-typestate.md) - Use typestate for compile-time state machines
- [`api-sealed-trait`](rules/api-sealed-trait.md) - Seal traits to prevent external implementations
- [`api-extension-trait`](rules/api-extension-trait.md) - Use extension traits to add methods to foreign types
- [`api-parse-dont-validate`](rules/api-parse-dont-validate.md) - Parse into validated types at boundaries
- [`api-impl-into`](rules/api-impl-into.md) - Accept `impl Into<T>` for flexible string inputs
- [`api-impl-asref`](rules/api-impl-asref.md) - Accept `impl AsRef<T>` for borrowed inputs
- [`api-must-use`](rules/api-must-use.md) - Add `#[must_use]` to `Result` returning functions
- [`api-non-exhaustive`](rules/api-non-exhaustive.md) - Use `#[non_exhaustive]` for future-proof enums/structs
- [`api-from-not-into`](rules/api-from-not-into.md) - Implement `From`, not `Into` (auto-derived)
- [`api-default-impl`](rules/api-default-impl.md) - Implement `Default` for sensible defaults
- [`api-common-traits`](rules/api-common-traits.md) - Implement `Debug`, `Clone`, `PartialEq` eagerly
- [`api-serde-optional`](rules/api-serde-optional.md) - Gate `Serialize`/`Deserialize` behind feature flag
- [`api-no-bool-params`](rules/api-no-bool-params.md) - No `bool` parameters in public signatures - enum or two methods
- [`api-ffi-boundary`](rules/api-ffi-boundary.md) - FFI: `repr(C)`, same-side alloc/free, RAII pointer wrappers, `foo-sys`/`bindgen`

### 5. Async/Await (HIGH)

- [`async-tokio-runtime`](rules/async-tokio-runtime.md) - Use Tokio for production async runtime
- [`async-no-lock-await`](rules/async-no-lock-await.md) - Never hold `Mutex`/`RwLock` across `.await`
- [`async-spawn-blocking`](rules/async-spawn-blocking.md) - Use `spawn_blocking` for CPU-intensive work
- [`async-tokio-fs`](rules/async-tokio-fs.md) - Use `tokio::fs` not `std::fs` in async code
- [`async-cancellation-token`](rules/async-cancellation-token.md) - Use `CancellationToken` for graceful shutdown
- [`async-join-parallel`](rules/async-join-parallel.md) - Use `tokio::join!` for parallel operations
- [`async-try-join`](rules/async-try-join.md) - Use `tokio::try_join!` for fallible parallel ops
- [`async-select-racing`](rules/async-select-racing.md) - Use `tokio::select!` for racing/timeouts
- [`async-bounded-channel`](rules/async-bounded-channel.md) - Use bounded channels for backpressure
- [`async-mpsc-queue`](rules/async-mpsc-queue.md) - Use `mpsc` for work queues
- [`async-broadcast-pubsub`](rules/async-broadcast-pubsub.md) - Use `broadcast` for pub/sub patterns
- [`async-watch-latest`](rules/async-watch-latest.md) - Use `watch` for latest-value sharing
- [`async-oneshot-response`](rules/async-oneshot-response.md) - Use `oneshot` for request/response
- [`async-joinset-structured`](rules/async-joinset-structured.md) - Use `JoinSet` for dynamic task groups
- [`async-clone-before-await`](rules/async-clone-before-await.md) - Clone data before await, release locks
- [`async-send-sync-assert`](rules/async-send-sync-assert.md) - Pin `Send`/`Sync` with compile-time `assert_send` checks

### 6. Compiler Optimization (HIGH)

- [`opt-inline-small`](rules/opt-inline-small.md) - Use `#[inline]` for small hot functions
- [`opt-inline-always-rare`](rules/opt-inline-always-rare.md) - Use `#[inline(always)]` sparingly
- [`opt-inline-never-cold`](rules/opt-inline-never-cold.md) - Use `#[inline(never)]` for cold paths
- [`opt-cold-unlikely`](rules/opt-cold-unlikely.md) - Use `#[cold]` for error/unlikely paths
- [`opt-likely-hint`](rules/opt-likely-hint.md) - Use `likely()`/`unlikely()` for branch hints
- [`opt-lto-release`](rules/opt-lto-release.md) - Enable LTO in release builds
- [`opt-codegen-units`](rules/opt-codegen-units.md) - Use `codegen-units = 1` for max optimization
- [`opt-pgo-profile`](rules/opt-pgo-profile.md) - Use PGO for production builds
- [`opt-target-cpu`](rules/opt-target-cpu.md) - Set `target-cpu=native` for local builds
- [`opt-bounds-check`](rules/opt-bounds-check.md) - Use iterators to avoid bounds checks
- [`opt-simd-portable`](rules/opt-simd-portable.md) - Use portable SIMD for data-parallel ops
- [`opt-cache-friendly`](rules/opt-cache-friendly.md) - Design cache-friendly data layouts (SoA)

### 7. Naming Conventions (MEDIUM)

- [`name-types-camel`](rules/name-types-camel.md) - Use `UpperCamelCase` for types, traits, enums
- [`name-variants-camel`](rules/name-variants-camel.md) - Use `UpperCamelCase` for enum variants
- [`name-funcs-snake`](rules/name-funcs-snake.md) - Use `snake_case` for functions, methods, modules
- [`name-consts-screaming`](rules/name-consts-screaming.md) - Use `SCREAMING_SNAKE_CASE` for constants/statics
- [`name-lifetime-short`](rules/name-lifetime-short.md) - Use short lowercase lifetimes: `'a`, `'de`, `'src`
- [`name-type-param-single`](rules/name-type-param-single.md) - Use single uppercase for type params: `T`, `E`, `K`, `V`
- [`name-as-free`](rules/name-as-free.md) - `as_` prefix: free reference conversion
- [`name-to-expensive`](rules/name-to-expensive.md) - `to_` prefix: expensive conversion
- [`name-into-ownership`](rules/name-into-ownership.md) - `into_` prefix: ownership transfer
- [`name-no-get-prefix`](rules/name-no-get-prefix.md) - No `get_` prefix for simple getters
- [`name-is-has-bool`](rules/name-is-has-bool.md) - Use `is_`, `has_`, `can_` for boolean methods
- [`name-iter-convention`](rules/name-iter-convention.md) - Use `iter`/`iter_mut`/`into_iter` for iterators
- [`name-iter-method`](rules/name-iter-method.md) - Name iterator methods consistently
- [`name-iter-type-match`](rules/name-iter-type-match.md) - Iterator type names match method
- [`name-acronym-word`](rules/name-acronym-word.md) - Treat acronyms as words: `Uuid` not `UUID`
- [`name-crate-no-rs`](rules/name-crate-no-rs.md) - Crate names: no `-rs` suffix
- [`name-weasel-types`](rules/name-weasel-types.md) - No `Service`/`Manager`/`Factory`/`Helper`/`Util`; don't repeat the module name in the type

### 8. Type Safety (MEDIUM)

- [`type-newtype-ids`](rules/type-newtype-ids.md) - Wrap IDs in newtypes: `UserId(u64)`
- [`type-newtype-validated`](rules/type-newtype-validated.md) - Newtypes for validated data: `Email`, `Url`
- [`type-enum-states`](rules/type-enum-states.md) - Use enums for mutually exclusive states
- [`type-option-nullable`](rules/type-option-nullable.md) - Use `Option<T>` for nullable values
- [`type-result-fallible`](rules/type-result-fallible.md) - Use `Result<T, E>` for fallible operations
- [`type-phantom-marker`](rules/type-phantom-marker.md) - Use `PhantomData<T>` for type-level markers
- [`type-never-diverge`](rules/type-never-diverge.md) - Use `!` type for functions that never return
- [`type-generic-bounds`](rules/type-generic-bounds.md) - Add trait bounds only where needed
- [`type-no-stringly`](rules/type-no-stringly.md) - Avoid stringly-typed APIs, use enums/newtypes
- [`type-repr-transparent`](rules/type-repr-transparent.md) - Use `#[repr(transparent)]` for FFI newtypes
- [`type-strongest-std`](rules/type-strongest-std.md) - Strongest std types: `PathBuf`, `Duration`, `NonZeroU32` internal-only, `bitflags` vs enum

### 9. Testing (MEDIUM)

- [`test-cfg-test-module`](rules/test-cfg-test-module.md) - Use `#[cfg(test)] mod tests { }`
- [`test-use-super`](rules/test-use-super.md) - Use `use super::*;` in test modules
- [`test-integration-dir`](rules/test-integration-dir.md) - Put integration tests in `tests/` directory
- [`test-descriptive-names`](rules/test-descriptive-names.md) - Use descriptive test names
- [`test-arrange-act-assert`](rules/test-arrange-act-assert.md) - Structure tests as arrange/act/assert
- [`test-proptest-properties`](rules/test-proptest-properties.md) - Use `proptest` for property-based testing
- [`test-mockall-mocking`](rules/test-mockall-mocking.md) - Use `mockall` for trait mocking
- [`test-mock-traits`](rules/test-mock-traits.md) - Use traits for dependencies to enable mocking
- [`test-fixture-raii`](rules/test-fixture-raii.md) - Use RAII pattern (Drop) for test cleanup
- [`test-tokio-async`](rules/test-tokio-async.md) - Use `#[tokio::test]` for async tests
- [`test-should-panic`](rules/test-should-panic.md) - Use `#[should_panic]` for panic tests
- [`test-criterion-bench`](rules/test-criterion-bench.md) - Use `criterion` for benchmarking
- [`test-doctest-examples`](rules/test-doctest-examples.md) - Keep doc examples as executable tests
- [`test-refactor-pin-behavior`](rules/test-refactor-pin-behavior.md) - Pin current behavior with tests before refactoring; list signature changes
- [`test-miri-fuzz`](rules/test-miri-fuzz.md) - Miri over `unsafe`; `cargo fuzz` untrusted-input parsers

### 10. Documentation (MEDIUM)

- [`doc-all-public`](rules/doc-all-public.md) - Document all public items with `///`
- [`doc-module-inner`](rules/doc-module-inner.md) - Use `//!` for module-level documentation
- [`doc-examples-section`](rules/doc-examples-section.md) - Include `# Examples` with runnable code
- [`doc-errors-section`](rules/doc-errors-section.md) - Include `# Errors` for fallible functions
- [`doc-panics-section`](rules/doc-panics-section.md) - Include `# Panics` for panicking functions
- [`doc-safety-section`](rules/doc-safety-section.md) - Include `# Safety` for unsafe functions
- [`doc-question-mark`](rules/doc-question-mark.md) - Use `?` in examples, not `.unwrap()`
- [`doc-hidden-setup`](rules/doc-hidden-setup.md) - Use `# ` prefix to hide example setup code
- [`doc-intra-links`](rules/doc-intra-links.md) - Use intra-doc links: `[Vec]`
- [`doc-link-types`](rules/doc-link-types.md) - Link related types and functions in docs
- [`doc-cargo-metadata`](rules/doc-cargo-metadata.md) - Fill `Cargo.toml` metadata

### 11. Performance Patterns (MEDIUM)

- [`perf-iter-over-index`](rules/perf-iter-over-index.md) - Prefer iterators over manual indexing
- [`perf-iter-lazy`](rules/perf-iter-lazy.md) - Keep iterators lazy, collect() only when needed
- [`perf-collect-once`](rules/perf-collect-once.md) - Don't `collect()` intermediate iterators
- [`perf-entry-api`](rules/perf-entry-api.md) - Use `entry()` API for map insert-or-update
- [`perf-drain-reuse`](rules/perf-drain-reuse.md) - Use `drain()` to reuse allocations
- [`perf-extend-batch`](rules/perf-extend-batch.md) - Use `extend()` for batch insertions
- [`perf-chain-avoid`](rules/perf-chain-avoid.md) - Avoid `chain()` in hot loops
- [`perf-collect-into`](rules/perf-collect-into.md) - Use `collect_into()` for reusing containers
- [`perf-black-box-bench`](rules/perf-black-box-bench.md) - Use `black_box()` in benchmarks
- [`perf-release-profile`](rules/perf-release-profile.md) - Optimize release profile settings
- [`perf-profile-first`](rules/perf-profile-first.md) - Profile before optimizing

### 12. Project Structure (LOW)

- [`proj-lib-main-split`](rules/proj-lib-main-split.md) - Keep `main.rs` minimal, logic in `lib.rs`
- [`proj-mod-by-feature`](rules/proj-mod-by-feature.md) - Organize modules by feature, not type
- [`proj-flat-small`](rules/proj-flat-small.md) - Keep small projects flat
- [`proj-mod-rs-dir`](rules/proj-mod-rs-dir.md) - Use `mod.rs` for multi-file modules
- [`proj-pub-crate-internal`](rules/proj-pub-crate-internal.md) - Use `pub(crate)` for internal APIs
- [`proj-pub-super-parent`](rules/proj-pub-super-parent.md) - Use `pub(super)` for parent-only visibility
- [`proj-pub-use-reexport`](rules/proj-pub-use-reexport.md) - Use `pub use` for clean public API
- [`proj-prelude-module`](rules/proj-prelude-module.md) - Create `prelude` module for common imports
- [`proj-bin-dir`](rules/proj-bin-dir.md) - Put multiple binaries in `src/bin/`
- [`proj-workspace-large`](rules/proj-workspace-large.md) - Use workspaces for large projects
- [`proj-workspace-deps`](rules/proj-workspace-deps.md) - Use workspace dependency inheritance
- [`proj-semver-features`](rules/proj-semver-features.md) - Semver is API: additive bare-named features, deprecate-then-remove, deliberate MSRV

### 13. Clippy & Linting (LOW)

- [`lint-deny-correctness`](rules/lint-deny-correctness.md) - `#![deny(clippy::correctness)]`
- [`lint-warn-suspicious`](rules/lint-warn-suspicious.md) - `#![warn(clippy::suspicious)]`
- [`lint-warn-style`](rules/lint-warn-style.md) - `#![warn(clippy::style)]`
- [`lint-warn-complexity`](rules/lint-warn-complexity.md) - `#![warn(clippy::complexity)]`
- [`lint-warn-perf`](rules/lint-warn-perf.md) - `#![warn(clippy::perf)]`
- [`lint-pedantic-selective`](rules/lint-pedantic-selective.md) - Enable `clippy::pedantic` selectively
- [`lint-missing-docs`](rules/lint-missing-docs.md) - `#![warn(missing_docs)]`
- [`lint-unsafe-doc`](rules/lint-unsafe-doc.md) - `#![warn(clippy::undocumented_unsafe_blocks)]`
- [`lint-cargo-metadata`](rules/lint-cargo-metadata.md) - `#![warn(clippy::cargo)]` for published crates
- [`lint-rustfmt-check`](rules/lint-rustfmt-check.md) - Run `cargo fmt --check` in CI
- [`lint-workspace-lints`](rules/lint-workspace-lints.md) - Configure lints at workspace level
- [`lint-expect-over-allow`](rules/lint-expect-over-allow.md) - `#[expect(lint, reason)]` over `#[allow]`; never `#![deny(warnings)]` in source

### 14. Anti-patterns (REFERENCE)

- [`anti-unwrap-abuse`](rules/anti-unwrap-abuse.md) - Don't use `.unwrap()` in production code
- [`anti-expect-lazy`](rules/anti-expect-lazy.md) - Don't use `.expect()` for recoverable errors
- [`anti-clone-excessive`](rules/anti-clone-excessive.md) - Don't clone when borrowing works
- [`anti-lock-across-await`](rules/anti-lock-across-await.md) - Don't hold locks across `.await`
- [`anti-string-for-str`](rules/anti-string-for-str.md) - Don't accept `&String` when `&str` works
- [`anti-vec-for-slice`](rules/anti-vec-for-slice.md) - Don't accept `&Vec<T>` when `&[T]` works
- [`anti-index-over-iter`](rules/anti-index-over-iter.md) - Don't use indexing when iterators work
- [`anti-panic-expected`](rules/anti-panic-expected.md) - Don't panic on expected/recoverable errors
- [`anti-empty-catch`](rules/anti-empty-catch.md) - Don't use empty `if let Err(_) = ...` blocks
- [`anti-over-abstraction`](rules/anti-over-abstraction.md) - Don't over-abstract with excessive generics
- [`anti-premature-optimize`](rules/anti-premature-optimize.md) - Don't optimize before profiling
- [`anti-type-erasure`](rules/anti-type-erasure.md) - Don't use `Box<dyn Trait>` when `impl Trait` works
- [`anti-format-hot-path`](rules/anti-format-hot-path.md) - Don't use `format!()` in hot paths
- [`anti-collect-intermediate`](rules/anti-collect-intermediate.md) - Don't `collect()` intermediate iterators
- [`anti-stringly-typed`](rules/anti-stringly-typed.md) - Don't use strings for structured data
- [`anti-unsound-api`](rules/anti-unsound-api.md) - A safe API that can cause UB through safe calls is unsound

---

## Deep Dive References

When you need the reasoning behind a rule family — or a complete checklist for a task
like publishing a crate or auditing an API — read the per-source digests in
[`references/`](references/) (adapted from [BeautifulRust](https://github.com/morganlinton/BeautifulRust), MIT):

- [`references/rust-api-guidelines.md`](references/rust-api-guidelines.md) - all 54 official C-codes with the full naming, interoperability, and predictability checklist
- [`references/effective-rust.md`](references/effective-rust.md) - 35 items on choosing between alternatives: generics vs `dyn`, `Result` vs panic, iterators vs loops, borrow-checker tactics, dependency/CI policy
- [`references/rust-design-patterns.md`](references/rust-design-patterns.md) - idioms, patterns (builder, RAII, newtype, visitor, `mem::take`), and anti-patterns with code
- [`references/microsoft-rust-guidelines.md`](references/microsoft-rust-guidelines.md) - M-codes for teams: error struct design, panic/unsafe policy, workspace layout, agent-specific guidance
- [`references/google-rust-style-guide.md`](references/google-rust-style-guide.md) - formatting, imports, module layout, accessor naming, and the application-vs-library distinction

Never put `#![deny(warnings)]` in source: new toolchains add warnings that
break your build. Configure lint levels once in `Cargo.toml`
(`[lints.rust]` / `[workspace.lints]`), override narrowly with
`#[expect(lint, reason)]`, and enforce warnings-as-errors in CI with
`cargo clippy -- -D warnings`. See
[`lint-expect-over-allow`](rules/lint-expect-over-allow.md).

---

## Recommended Cargo.toml Settings

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true

[profile.bench]
inherits = "release"
debug = true
strip = false

[profile.dev]
opt-level = 0
debug = true

[profile.dev.package."*"]
opt-level = 3  # Optimize dependencies in dev
```

---

## How to Use

This skill provides rule identifiers for quick reference. When generating or reviewing Rust code:

1. **Check relevant category** based on task type
2. **Apply rules** with matching prefix
3. **Prioritize** CRITICAL > HIGH > MEDIUM > LOW
4. **Read rule files** in `rules/` for detailed examples

### Rule Application by Task

| Task | Primary Categories |
|------|-------------------|
| New function | `own-`, `err-`, `name-` |
| New struct/API | `api-`, `type-`, `doc-` |
| Async code | `async-`, `own-` |
| Error handling | `err-`, `api-` |
| Memory optimization | `mem-`, `own-`, `perf-` |
| Performance tuning | `opt-`, `mem-`, `perf-` |
| Code review | `anti-`, `lint-` |

### PR Code Review

For automated PR bug review and fix loops, use the [`pr-code-review`](../pr-code-review/SKILL.md) skill/agent.
It ranks bugs critical/high/medium/low, eliminates false positives, applies KISS/DRY fixes (main logic only, skip
tests), and re-reviews after every change.

```text
/pr-code-review                    # review current uncommitted diff
/pr-code-review --branch main      # review branch diff against main
/pr-code-review --last-diff        # review the last committed diff
```

---

## Sources

This skill synthesizes best practices from:

- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Google Rust Style Guide](https://google.github.io/styleguide/rustguide.html)
- [Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/)
- [Effective Rust](https://www.lurklurk.org/effective-rust.html) (David Drysdale)
- [Rust Design Patterns](https://rust-unofficial.github.io/patterns/)
- Production codebases: ripgrep, tokio, serde, polars, axum, deno
- Clippy lint documentation
- Community conventions (2024-2026)

v1.1.0 additions adapted from [BeautifulRust](https://github.com/morganlinton/BeautifulRust)
by Morgan Linton (MIT License, © 2025), which synthesizes the Google Rust Style Guide,
the Rust API Guidelines, *Effective Rust*, Rust Design Patterns, and Microsoft's
Pragmatic Rust Guidelines; full per-source digests are in [`references/`](references/).
